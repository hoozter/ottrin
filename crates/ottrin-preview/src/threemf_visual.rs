use crate::mesh_visual::{MeshBuilder, MeshVisualError, ParsedMesh, render_mesh_preview_png};
use crate::preflight_epub_zip;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use std::path::Path;

pub const MAX_3MF_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_3MF_ENTRIES: usize = 1_024;
pub const MAX_3MF_MODEL_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_3MF_EXPANDED_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_3MF_OBJECTS: usize = 4_096;
pub const MAX_3MF_VERTICES: usize = 200_000;
pub const MAX_3MF_TRIANGLES: usize = 200_000;
pub const MAX_3MF_BUILD_ITEMS: usize = 4_096;
const MAX_3MF_XML_EVENTS: usize = 1_000_000;
const MAX_3MF_ATTRIBUTES: usize = 16;
const MAX_3MF_NAME_BYTES: usize = 1_024;
const MAX_3MF_RATIO: u64 = 200;
const CORE_NAMESPACE: &[u8] = b"http://schemas.microsoft.com/3dmanufacturing/core/2015/02";
const RELATIONSHIPS_NAMESPACE: &[u8] =
    b"http://schemas.openxmlformats.org/package/2006/relationships";
const MODEL_RELATIONSHIP: &[u8] = b"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel";
const MODEL_CONTENT_TYPE: &str = "application/vnd.ms-package.3dmanufacturing-3dmodel+xml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreeMfPreviewSourceGeneration(pub(crate) [u8; 32]);
impl ThreeMfPreviewSourceGeneration {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThreeMfMetadata {
    pub unit: String,
    pub object_count: usize,
    pub build_item_count: usize,
    pub vertex_count: usize,
    pub triangle_count: usize,
    pub dimensions: [f32; 3],
    pub ignored_optional_features: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedThreeMfPreview {
    pub source_generation: ThreeMfPreviewSourceGeneration,
    pub png: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum ThreeMfVisualError {
    #[error("3MF visual previews support only .3mf packages")]
    UnsupportedFormat,
    #[error("could not inspect 3MF input: {0}")]
    InputMetadata(String),
    #[error("3MF package is too large for a bounded preview")]
    InputTooLarge,
    #[error("could not read immutable 3MF package snapshot: {0}")]
    Read(String),
    #[error("3MF package changed while its bounded snapshot was read")]
    InputChanged,
    #[error("3MF package is malformed, ambiguous, or exceeds bounded package limits: {0}")]
    Package(String),
    #[error("3MF model is malformed or outside the truthful supported subset: {0}")]
    Model(String),
    #[error("3MF mesh contains non-finite, subnormal, or unsafe geometry")]
    UnsafeGeometry,
    #[error("3MF mesh has no usable non-flat triangles")]
    EmptyMesh,
    #[error("3MF mesh would exceed bounded raster work")]
    RasterWorkLimit,
    #[error("could not encode 3MF preview PNG")]
    Encode,
}

impl From<MeshVisualError> for ThreeMfVisualError {
    fn from(value: MeshVisualError) -> Self {
        match value {
            MeshVisualError::UnsafeCoordinates => Self::UnsafeGeometry,
            MeshVisualError::EmptyMesh
            | MeshVisualError::DegenerateTriangle
            | MeshVisualError::UnusableBounds => Self::EmptyMesh,
            MeshVisualError::RasterWorkLimit => Self::RasterWorkLimit,
            MeshVisualError::Encode => Self::Encode,
            MeshVisualError::TriangleLimit => Self::Model("triangle cap exceeded".into()),
        }
    }
}

struct Snapshot {
    bytes: Vec<u8>,
    generation: ThreeMfPreviewSourceGeneration,
}
struct ParsedThreeMf {
    mesh: ParsedMesh,
    metadata: ThreeMfMetadata,
    generation: ThreeMfPreviewSourceGeneration,
}
struct Object {
    vertices: Vec<[f32; 3]>,
    triangles: Vec<[usize; 3]>,
}
struct BuildItem {
    object_id: u32,
    transform: [f32; 12],
}
type PackageParts = (Vec<u8>, Vec<u8>, Option<Vec<u8>>);
struct ModelParseState {
    stack: Vec<Vec<u8>>,
    unit: String,
    objects: BTreeMap<u32, Object>,
    current: Option<(u32, Object)>,
    build: Vec<BuildItem>,
    ignored: bool,
    vertex_total: usize,
    triangle_total: usize,
}

pub fn threemf_preview_source_generation(path: &Path) -> Option<ThreeMfPreviewSourceGeneration> {
    read_snapshot(path).ok().map(|snapshot| snapshot.generation)
}
pub fn inspect_3mf_metadata(path: &Path) -> Result<ThreeMfMetadata, ThreeMfVisualError> {
    Ok(parse_snapshot(path)?.metadata)
}
pub fn render_3mf_preview_png(path: &Path, target_dim: u32) -> Result<Vec<u8>, ThreeMfVisualError> {
    Ok(render_3mf_preview_png_with_generation(path, target_dim)?.png)
}
pub fn render_3mf_preview_png_with_generation(
    path: &Path,
    target_dim: u32,
) -> Result<RenderedThreeMfPreview, ThreeMfVisualError> {
    let parsed = parse_snapshot(path)?;
    Ok(RenderedThreeMfPreview {
        source_generation: parsed.generation,
        png: render_mesh_preview_png(&parsed.mesh, target_dim)?,
    })
}

fn parse_snapshot(path: &Path) -> Result<ParsedThreeMf, ThreeMfVisualError> {
    if !crate::is_3mf_path(path) {
        return Err(ThreeMfVisualError::UnsupportedFormat);
    }
    let snapshot = read_snapshot(path)?;
    let (content_types, relationships, model) = preflight_package(&snapshot.bytes)?;
    validate_content_types(&content_types)?;
    let model_path = root_model_target(&relationships)?;
    let model = model.ok_or_else(|| {
        ThreeMfVisualError::Package("root relationship model entry is missing".into())
    })?;
    let (mesh, mut metadata) = parse_model(&model, &model_path)?;
    metadata.dimensions = mesh.dimensions;
    Ok(ParsedThreeMf {
        mesh,
        metadata,
        generation: snapshot.generation,
    })
}

fn read_snapshot(path: &Path) -> Result<Snapshot, ThreeMfVisualError> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| ThreeMfVisualError::InputMetadata(error.to_string()))?;
    let initial_size = file
        .metadata()
        .map_err(|error| ThreeMfVisualError::InputMetadata(error.to_string()))?
        .len();
    if initial_size > MAX_3MF_PACKAGE_BYTES {
        return Err(ThreeMfVisualError::InputTooLarge);
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(initial_size).map_err(|_| ThreeMfVisualError::InputTooLarge)?,
    );
    file.by_ref()
        .take(MAX_3MF_PACKAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| ThreeMfVisualError::Read(error.to_string()))?;
    let final_size = file
        .metadata()
        .map_err(|error| ThreeMfVisualError::Read(error.to_string()))?
        .len();
    if bytes.len() as u64 > MAX_3MF_PACKAGE_BYTES {
        return Err(ThreeMfVisualError::InputTooLarge);
    }
    if bytes.len() as u64 != initial_size || final_size != initial_size {
        return Err(ThreeMfVisualError::InputChanged);
    }
    Ok(Snapshot {
        generation: ThreeMfPreviewSourceGeneration(Sha256::digest(&bytes).into()),
        bytes,
    })
}

fn preflight_package(bytes: &[u8]) -> Result<PackageParts, ThreeMfVisualError> {
    let directory = preflight_epub_zip(bytes).ok_or_else(|| {
        ThreeMfVisualError::Package("ZIP footer or central directory is unsafe".into())
    })?;
    if directory.entry_count > MAX_3MF_ENTRIES {
        return Err(ThreeMfVisualError::Package("entry cap exceeded".into()));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| ThreeMfVisualError::Package("ZIP reader rejected bounded snapshot".into()))?;
    if archive.len() != directory.entry_count {
        return Err(ThreeMfVisualError::Package(
            "directory count disagrees with archive".into(),
        ));
    }
    let mut names = BTreeSet::new();
    let mut expanded = 0u64;
    let mut candidates = BTreeMap::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| ThreeMfVisualError::Package("could not enumerate ZIP entry".into()))?;
        let name = entry.name().to_string();
        if !safe_name(&name) || !names.insert(name.to_ascii_lowercase()) {
            return Err(ThreeMfVisualError::Package(
                "unsafe, duplicate, or case-colliding entry name".into(),
            ));
        }
        if local_header_uses_data_descriptor(bytes, entry.header_start()) {
            return Err(ThreeMfVisualError::Package(
                "data descriptors are unsupported".into(),
            ));
        }
        if entry.encrypted() || entry.is_symlink() || (!entry.is_file() && !entry.is_dir()) {
            return Err(ThreeMfVisualError::Package(
                "encrypted or special ZIP entry".into(),
            ));
        }
        if !entry.is_dir()
            && !matches!(
                entry.compression(),
                zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
            )
        {
            return Err(ThreeMfVisualError::Package(
                "unsupported ZIP compression".into(),
            ));
        }
        let compressed = entry.compressed_size();
        let size = entry.size();
        if size > 0 && (compressed == 0 || size > compressed.saturating_mul(MAX_3MF_RATIO)) {
            return Err(ThreeMfVisualError::Package(
                "suspicious compression ratio".into(),
            ));
        }
        expanded = expanded
            .checked_add(size)
            .ok_or_else(|| ThreeMfVisualError::Package("expanded-size overflow".into()))?;
        if expanded > MAX_3MF_EXPANDED_BYTES {
            return Err(ThreeMfVisualError::Package(
                "aggregate expanded-size cap exceeded".into(),
            ));
        }
        if name.ends_with(".model") {
            candidates.insert(name, ());
        }
    }
    let content_types = read_entry(
        archive
            .by_name("[Content_Types].xml")
            .map_err(|_| ThreeMfVisualError::Package("missing [Content_Types].xml".into()))?,
        MAX_3MF_MODEL_BYTES,
    )?;
    let relationships = read_entry(
        archive.by_name("_rels/.rels").map_err(|_| {
            ThreeMfVisualError::Package("missing package root relationships".into())
        })?,
        MAX_3MF_MODEL_BYTES,
    )?;
    let target = root_model_target(&relationships)?;
    if !candidates.contains_key(&target) || candidates.len() != 1 {
        return Err(ThreeMfVisualError::Package(
            "competing or missing model part".into(),
        ));
    }
    let model = archive
        .by_name(&target)
        .map_err(|_| ThreeMfVisualError::Package("model cannot be opened".into()))?;
    if model.encrypted() || !model.is_file() || model.size() > MAX_3MF_MODEL_BYTES {
        return Err(ThreeMfVisualError::Package(
            "model entry is unsafe or oversized".into(),
        ));
    }
    let model = read_entry(model, MAX_3MF_MODEL_BYTES)?;
    Ok((content_types, relationships, Some(model)))
}

fn safe_name(name: &str) -> bool {
    let normalized = name.strip_suffix('/').unwrap_or(name);
    !normalized.is_empty()
        && name.len() <= MAX_3MF_NAME_BYTES
        && !normalized.starts_with('/')
        && !normalized.contains(['\\', ':', '\0'])
        && normalized
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".."))
}
fn local_header_uses_data_descriptor(bytes: &[u8], offset: u64) -> bool {
    let start = usize::try_from(offset).ok();
    start
        .and_then(|start| bytes.get(start..start.checked_add(30)?))
        .and_then(|header| {
            (header.get(..4) == Some(b"PK\x03\x04"))
                .then(|| u16::from_le_bytes([header[6], header[7]]))
        })
        .is_none_or(|flags| flags & 0x0008 != 0)
}
fn read_entry(
    mut entry: zip::read::ZipFile<'_>,
    limit: u64,
) -> Result<Vec<u8>, ThreeMfVisualError> {
    if entry.size() > limit {
        return Err(ThreeMfVisualError::Package(
            "required XML entry exceeds cap".into(),
        ));
    }
    let mut output = Vec::with_capacity(
        usize::try_from(entry.size())
            .map_err(|_| ThreeMfVisualError::Package("entry size does not fit memory".into()))?,
    );
    entry
        .by_ref()
        .take(limit + 1)
        .read_to_end(&mut output)
        .map_err(|_| ThreeMfVisualError::Package("bounded entry decompression failed".into()))?;
    (output.len() as u64 <= limit)
        .then_some(output)
        .ok_or_else(|| ThreeMfVisualError::Package("entry decompression exceeded cap".into()))
}

fn validate_content_types(xml: &[u8]) -> Result<(), ThreeMfVisualError> {
    let text = std::str::from_utf8(xml)
        .map_err(|_| ThreeMfVisualError::Package("content types are not UTF-8 XML".into()))?;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut found = 0usize;
    let mut events = 0usize;
    loop {
        events += 1;
        if events > MAX_3MF_XML_EVENTS {
            return Err(ThreeMfVisualError::Package(
                "content types event cap exceeded".into(),
            ));
        }
        match reader.read_event() {
            Ok(Event::Empty(event)) if event.local_name().as_ref() == b"Default" => {
                let attrs = attrs(&event)?;
                if attrs.get("Extension") == Some(&"model".to_string())
                    && attrs.get("ContentType") == Some(&MODEL_CONTENT_TYPE.to_string())
                {
                    found += 1;
                }
            }
            Ok(Event::DocType(_) | Event::GeneralRef(_) | Event::PI(_) | Event::CData(_)) => {
                return Err(ThreeMfVisualError::Package(
                    "content types use unsupported XML feature".into(),
                ));
            }
            Ok(Event::Eof) => break,
            Err(_) => {
                return Err(ThreeMfVisualError::Package(
                    "content types XML is malformed".into(),
                ));
            }
            _ => {}
        }
    }
    (found == 1).then_some(()).ok_or_else(|| {
        ThreeMfVisualError::Package("expected exactly one 3MF model content type".into())
    })
}

fn root_model_target(xml: &[u8]) -> Result<String, ThreeMfVisualError> {
    let text = std::str::from_utf8(xml)
        .map_err(|_| ThreeMfVisualError::Package("relationships are not UTF-8 XML".into()))?;
    let mut reader = quick_xml::NsReader::from_str(text);
    let mut target = None;
    let mut events = 0usize;
    loop {
        events += 1;
        if events > MAX_3MF_XML_EVENTS {
            return Err(ThreeMfVisualError::Package(
                "relationship event cap exceeded".into(),
            ));
        }
        match reader.read_event() {
            Ok(Event::Empty(event)) => {
                let (ns, local) = reader.resolve_element(event.name());
                if !matches!(ns, ResolveResult::Bound(value) if value.as_ref() == RELATIONSHIPS_NAMESPACE)
                    || local.as_ref() != b"Relationship"
                {
                    continue;
                }
                let attrs = attrs(&event)?;
                if attrs
                    .get("TargetMode")
                    .is_some_and(|value| value.eq_ignore_ascii_case("External"))
                {
                    return Err(ThreeMfVisualError::Package(
                        "external package relationship".into(),
                    ));
                }
                if attrs
                    .get("Type")
                    .is_some_and(|value| value.as_bytes() == MODEL_RELATIONSHIP)
                {
                    let raw = attrs.get("Target").ok_or_else(|| {
                        ThreeMfVisualError::Package("model relationship has no target".into())
                    })?;
                    let normalized = raw.strip_prefix('/').unwrap_or(raw).to_string();
                    if !safe_name(&normalized) || target.replace(normalized).is_some() {
                        return Err(ThreeMfVisualError::Package(
                            "ambiguous or unsafe model target".into(),
                        ));
                    }
                }
            }
            Ok(Event::DocType(_) | Event::GeneralRef(_) | Event::PI(_) | Event::CData(_)) => {
                return Err(ThreeMfVisualError::Package(
                    "relationships use unsupported XML feature".into(),
                ));
            }
            Ok(Event::Eof) => break,
            Err(_) => {
                return Err(ThreeMfVisualError::Package(
                    "relationships XML is malformed".into(),
                ));
            }
            _ => {}
        }
    }
    target.ok_or_else(|| {
        ThreeMfVisualError::Package("expected exactly one 3MF model relationship".into())
    })
}

fn parse_model(
    xml: &[u8],
    _model_path: &str,
) -> Result<(ParsedMesh, ThreeMfMetadata), ThreeMfVisualError> {
    let text = std::str::from_utf8(xml)
        .map_err(|_| ThreeMfVisualError::Model("model XML is not UTF-8".into()))?;
    let mut reader = quick_xml::NsReader::from_str(text);
    let mut events = 0usize;
    let mut state = ModelParseState {
        stack: Vec::new(),
        unit: "millimeter".to_string(),
        objects: BTreeMap::new(),
        current: None,
        build: Vec::new(),
        ignored: false,
        vertex_total: 0,
        triangle_total: 0,
    };
    loop {
        events += 1;
        if events > MAX_3MF_XML_EVENTS {
            return Err(ThreeMfVisualError::Model("XML event cap exceeded".into()));
        }
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let local = core_local(&reader, &event)?;
                let attributes = attrs(&event)?;
                model_start(&local, &attributes, &mut state)?;
                state.stack.push(local);
            }
            Ok(Event::Empty(event)) => {
                let local = core_local(&reader, &event)?;
                let attributes = attrs(&event)?;
                model_empty(&local, &attributes, &mut state)?;
            }
            Ok(Event::End(event)) => {
                let local = event.local_name().as_ref().to_vec();
                if state.stack.pop().as_deref() != Some(local.as_slice()) {
                    return Err(ThreeMfVisualError::Model("mismatched XML nesting".into()));
                }
                if local == b"object" {
                    let (id, object) = state.current.take().ok_or_else(|| {
                        ThreeMfVisualError::Model("object closure without object".into())
                    })?;
                    if state.objects.insert(id, object).is_some() {
                        return Err(ThreeMfVisualError::Model("duplicate object id".into()));
                    }
                }
            }
            Ok(Event::Text(event)) => {
                let text = event
                    .decode()
                    .map_err(|_| ThreeMfVisualError::Model("invalid XML text".into()))?;
                if !text.trim().is_empty() && !inside_ignored_optional_feature(&state.stack) {
                    return Err(ThreeMfVisualError::Model(
                        "unexpected model XML content".into(),
                    ));
                }
            }
            Ok(Event::Decl(_) | Event::Comment(_)) => {}
            Ok(Event::Eof) => break,
            Ok(Event::DocType(_) | Event::GeneralRef(_) | Event::PI(_) | Event::CData(_)) => {
                return Err(ThreeMfVisualError::Model(
                    "DTD, entities, processing instructions, or CDATA are unsupported".into(),
                ));
            }
            Err(_) => return Err(ThreeMfVisualError::Model("model XML is malformed".into())),
        }
    }
    if !state.stack.is_empty()
        || state.current.is_some()
        || state.objects.is_empty()
        || state.build.is_empty()
    {
        return Err(ThreeMfVisualError::Model(
            "model must contain closed objects and a nonempty build".into(),
        ));
    }
    let mut mesh = MeshBuilder::new(MAX_3MF_TRIANGLES);
    for item in &state.build {
        let object = state.objects.get(&item.object_id).ok_or_else(|| {
            ThreeMfVisualError::Model("build item references missing object".into())
        })?;
        for triangle in &object.triangles {
            let vertices = [
                validate_coordinate(transform(object.vertices[triangle[0]], item.transform))?,
                validate_coordinate(transform(object.vertices[triangle[1]], item.transform))?,
                validate_coordinate(transform(object.vertices[triangle[2]], item.transform))?,
            ];
            mesh.add_triangle(normal(vertices), vertices)?;
        }
    }
    let mesh = mesh.finish()?;
    Ok((
        mesh,
        ThreeMfMetadata {
            unit: state.unit,
            object_count: state.objects.len(),
            build_item_count: state.build.len(),
            vertex_count: state.vertex_total,
            triangle_count: state.triangle_total,
            dimensions: [0.0; 3],
            ignored_optional_features: state.ignored,
        },
    ))
}

fn core_local(
    reader: &quick_xml::NsReader<&[u8]>,
    event: &BytesStart<'_>,
) -> Result<Vec<u8>, ThreeMfVisualError> {
    let (namespace, local) = reader.resolve_element(event.name());
    matches!(namespace, ResolveResult::Bound(value) if value.as_ref() == CORE_NAMESPACE)
        .then_some(local.as_ref().to_vec())
        .ok_or_else(|| ThreeMfVisualError::Model("unknown or competing core namespace".into()))
}
fn attrs(event: &BytesStart<'_>) -> Result<BTreeMap<String, String>, ThreeMfVisualError> {
    let mut result = BTreeMap::new();
    for attribute in event.attributes() {
        if result.len() >= MAX_3MF_ATTRIBUTES {
            return Err(ThreeMfVisualError::Model("attribute cap exceeded".into()));
        }
        let attribute =
            attribute.map_err(|_| ThreeMfVisualError::Model("malformed XML attribute".into()))?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|_| ThreeMfVisualError::Model("non-UTF-8 attribute name".into()))?
            .rsplit(':')
            .next()
            .unwrap()
            .to_string();
        let value = attribute
            .unescape_value()
            .map_err(|_| ThreeMfVisualError::Model("invalid XML attribute".into()))?
            .into_owned();
        if key.len() > MAX_3MF_NAME_BYTES
            || value.len() > MAX_3MF_NAME_BYTES
            || result.insert(key, value).is_some()
        {
            return Err(ThreeMfVisualError::Model(
                "duplicate or oversized XML attribute".into(),
            ));
        }
    }
    Ok(result)
}
fn model_start(
    local: &[u8],
    attrs: &BTreeMap<String, String>,
    state: &mut ModelParseState,
) -> Result<(), ThreeMfVisualError> {
    match local {
        b"model" if state.stack.is_empty() => {
            if let Some(value) = attrs.get("unit") {
                if !matches!(
                    value.as_str(),
                    "micron" | "millimeter" | "centimeter" | "inch" | "foot" | "meter"
                ) {
                    return Err(ThreeMfVisualError::Model("unsupported unit".into()));
                }
                state.unit = value.clone();
            }
        }
        b"resources" if state.stack.as_slice() == [b"model".as_slice()] => {}
        b"object"
            if state
                .stack
                .last()
                .is_some_and(|v| v.as_slice() == b"resources") =>
        {
            if state.current.is_some()
                || state.objects.len() >= MAX_3MF_OBJECTS
                || attrs.get("type").is_some_and(|v| v != "model")
            {
                return Err(ThreeMfVisualError::Model(
                    "unsupported or excessive object".into(),
                ));
            }
            let id = parse_u32(attrs, "id")?;
            state.current = Some((
                id,
                Object {
                    vertices: Vec::new(),
                    triangles: Vec::new(),
                },
            ));
        }
        b"mesh" if state.current.is_some() => {}
        b"vertices" | b"triangles" if state.current.is_some() => {}
        b"build" if state.stack.as_slice() == [b"model".as_slice()] => {}
        b"components" => {
            return Err(ThreeMfVisualError::Model(
                "component objects are not supported by this truthful preview".into(),
            ));
        }
        b"metadata"
        | b"basematerials"
        | b"colorgroup"
        | b"texture2d"
        | b"texture2dgroup"
        | b"compositematerials"
        | b"multiproperties" => state.ignored = true,
        _ => {
            return Err(ThreeMfVisualError::Model(
                "unsupported core model element".into(),
            ));
        }
    }
    Ok(())
}
fn model_empty(
    local: &[u8],
    attrs: &BTreeMap<String, String>,
    state: &mut ModelParseState,
) -> Result<(), ThreeMfVisualError> {
    match local {
        b"vertex"
            if state
                .stack
                .last()
                .is_some_and(|v| v.as_slice() == b"vertices") =>
        {
            let object = state
                .current
                .as_mut()
                .ok_or_else(|| ThreeMfVisualError::Model("vertex outside object".into()))?;
            if state.vertex_total >= MAX_3MF_VERTICES {
                return Err(ThreeMfVisualError::Model("vertex cap exceeded".into()));
            }
            object.1.vertices.push(validate_coordinate([
                parse_f32(attrs, "x")?,
                parse_f32(attrs, "y")?,
                parse_f32(attrs, "z")?,
            ])?);
            state.vertex_total += 1;
        }
        b"triangle"
            if state
                .stack
                .last()
                .is_some_and(|v| v.as_slice() == b"triangles") =>
        {
            let object = state
                .current
                .as_mut()
                .ok_or_else(|| ThreeMfVisualError::Model("triangle outside object".into()))?;
            if state.triangle_total >= MAX_3MF_TRIANGLES {
                return Err(ThreeMfVisualError::Model("triangle cap exceeded".into()));
            }
            let indices = [
                parse_usize(attrs, "v1")?,
                parse_usize(attrs, "v2")?,
                parse_usize(attrs, "v3")?,
            ];
            if indices
                .iter()
                .any(|index| *index >= object.1.vertices.len())
            {
                return Err(ThreeMfVisualError::Model(
                    "triangle index is invalid".into(),
                ));
            }
            object.1.triangles.push(indices);
            state.triangle_total += 1;
        }
        b"item" if state.stack.last().is_some_and(|v| v.as_slice() == b"build") => {
            if state.build.len() >= MAX_3MF_BUILD_ITEMS {
                return Err(ThreeMfVisualError::Model("build item cap exceeded".into()));
            }
            state.build.push(BuildItem {
                object_id: parse_u32(attrs, "objectid")?,
                transform: parse_transform(attrs.get("transform"))?,
            });
        }
        b"base" | b"color" | b"tex2coord" | b"composite" | b"multi" => state.ignored = true,
        _ => {
            return Err(ThreeMfVisualError::Model(
                "unsupported empty core model element".into(),
            ));
        }
    }
    Ok(())
}
fn inside_ignored_optional_feature(stack: &[Vec<u8>]) -> bool {
    stack.iter().any(|element| {
        matches!(
            element.as_slice(),
            b"metadata"
                | b"basematerials"
                | b"colorgroup"
                | b"texture2d"
                | b"texture2dgroup"
                | b"compositematerials"
                | b"multiproperties"
        )
    })
}
fn parse_u32(attrs: &BTreeMap<String, String>, name: &str) -> Result<u32, ThreeMfVisualError> {
    attrs
        .get(name)
        .ok_or_else(|| ThreeMfVisualError::Model(format!("missing {name}")))?
        .parse()
        .map_err(|_| ThreeMfVisualError::Model(format!("invalid {name}")))
}
fn parse_usize(attrs: &BTreeMap<String, String>, name: &str) -> Result<usize, ThreeMfVisualError> {
    attrs
        .get(name)
        .ok_or_else(|| ThreeMfVisualError::Model(format!("missing {name}")))?
        .parse()
        .map_err(|_| ThreeMfVisualError::Model(format!("invalid {name}")))
}
fn parse_f32(attrs: &BTreeMap<String, String>, name: &str) -> Result<f32, ThreeMfVisualError> {
    attrs
        .get(name)
        .ok_or_else(|| ThreeMfVisualError::Model(format!("missing {name}")))?
        .parse()
        .map_err(|_| ThreeMfVisualError::Model(format!("invalid {name}")))
}
fn parse_transform(value: Option<&String>) -> Result<[f32; 12], ThreeMfVisualError> {
    let Some(value) = value else {
        return Ok([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    };
    let values = value
        .split_whitespace()
        .map(|part| {
            part.parse::<f32>()
                .map_err(|_| ThreeMfVisualError::Model("invalid transform".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() != 12 {
        return Err(ThreeMfVisualError::Model(
            "transform must have twelve values".into(),
        ));
    }
    let values: [f32; 12] = values
        .try_into()
        .map_err(|_| ThreeMfVisualError::Model("invalid transform".into()))?;
    values
        .into_iter()
        .all(safe_scalar)
        .then_some(values)
        .ok_or(ThreeMfVisualError::UnsafeGeometry)
}
fn validate_coordinate(value: [f32; 3]) -> Result<[f32; 3], ThreeMfVisualError> {
    value
        .into_iter()
        .all(safe_scalar)
        .then_some(value)
        .ok_or(ThreeMfVisualError::UnsafeGeometry)
}
fn safe_scalar(value: f32) -> bool {
    value.is_finite() && (value == 0.0 || value.abs() >= f32::MIN_POSITIVE) && value.abs() <= 1.0e12
}
fn transform(vertex: [f32; 3], m: [f32; 12]) -> [f32; 3] {
    [
        m[0] * vertex[0] + m[1] * vertex[1] + m[2] * vertex[2] + m[3],
        m[4] * vertex[0] + m[5] * vertex[1] + m[6] * vertex[2] + m[7],
        m[8] * vertex[0] + m[9] * vertex[1] + m[10] * vertex[2] + m[11],
    ]
}
fn normal(vertices: [[f32; 3]; 3]) -> [f32; 3] {
    let a = [
        vertices[1][0] - vertices[0][0],
        vertices[1][1] - vertices[0][1],
        vertices[1][2] - vertices[0][2],
    ];
    let b = [
        vertices[2][0] - vertices[0][0],
        vertices[2][1] - vertices[0][1],
        vertices[2][2] - vertices[0][2],
    ];
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const CONTENT_TYPES: &str = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/></Types>"#;
    const RELATIONSHIPS: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rel" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel" Target="/3D/model.model"/></Relationships>"#;
    const MODEL: &str = r##"<?xml version="1.0"?><model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><metadata name="Application">Ottrin fixture</metadata><resources><basematerials id="2"><base name="Inert material" displaycolor="#ffffff"/></basematerials><object id="1" type="model"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="1"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources><build><item objectid="1"/></build></model>"##;

    fn path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ottrin-3mf-{}-{}", std::process::id(), name))
    }
    fn package(path: &Path, model: &str, relations: &str) {
        let file = std::fs::File::create(path).expect("create 3MF fixture");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (name, value) in [
            ("[Content_Types].xml", CONTENT_TYPES),
            ("_rels/.rels", relations),
            ("3D/model.model", model),
        ] {
            zip.start_file(name, options).expect("start 3MF entry");
            zip.write_all(value.as_bytes()).expect("write 3MF entry");
        }
        zip.finish().expect("finish 3MF fixture");
    }

    #[test]
    fn canonical_package_has_truthful_metadata_and_deterministic_bounded_png() {
        let fixture = path("canonical.3mf");
        package(&fixture, MODEL, RELATIONSHIPS);
        let metadata = inspect_3mf_metadata(&fixture).expect("inspect canonical 3MF");
        assert_eq!(metadata.unit, "millimeter");
        assert_eq!(metadata.object_count, 1);
        assert_eq!(metadata.build_item_count, 1);
        assert_eq!(metadata.vertex_count, 3);
        assert_eq!(metadata.triangle_count, 1);
        assert!(metadata.ignored_optional_features);
        let first = render_3mf_preview_png(&fixture, 1).expect("render minimum dimension");
        let second = render_3mf_preview_png(&fixture, 1).expect("repeat render");
        assert_eq!(first, second);
        assert_eq!(&first[..8], b"\x89PNG\r\n\x1a\n");
        let _ = std::fs::remove_file(fixture);
    }

    #[test]
    fn build_transforms_and_invalid_geometry_are_not_silently_omitted() {
        let fixture = path("transforms.3mf");
        let transformed = MODEL.replace(
            "<item objectid=\"1\"/>",
            "<item objectid=\"1\"/><item objectid=\"1\" transform=\"1 0 0 3 0 1 0 0 0 0 1 0\"/>",
        );
        package(&fixture, &transformed, RELATIONSHIPS);
        assert_eq!(
            inspect_3mf_metadata(&fixture)
                .expect("inspect transformed build")
                .build_item_count,
            2
        );
        std::fs::write(&fixture, b"not a package").expect("replace fixture");
        assert!(
            render_3mf_preview_png(&fixture, 128).is_err(),
            "invalid replacement cannot inherit a render"
        );
        let _ = std::fs::remove_file(fixture);
    }

    #[test]
    fn external_or_competing_model_relationships_are_rejected() {
        let fixture = path("external.3mf");
        let external = RELATIONSHIPS.replace(
            "Target=\"/3D/model.model\"",
            "Target=\"https://example.invalid/model.model\" TargetMode=\"External\"",
        );
        package(&fixture, MODEL, &external);
        assert!(inspect_3mf_metadata(&fixture).is_err());
        let competing = RELATIONSHIPS.replace("</Relationships>", "<Relationship Id=\"other\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\" Target=\"/3D/model.model\"/></Relationships>");
        package(&fixture, MODEL, &competing);
        assert!(inspect_3mf_metadata(&fixture).is_err());
        let _ = std::fs::remove_file(fixture);
    }
}
