use crate::mesh_visual::{MeshBuilder, MeshVisualError, ParsedMesh, render_mesh_preview_png};
#[cfg(unix)]
use sha2::{Digest, Sha256};
use std::path::Path;

#[cfg_attr(not(unix), allow(dead_code))]
pub const MAX_PLY_PREVIEW_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_PLY_VERTICES: usize = 200_000;
pub const MAX_PLY_FACES: usize = 200_000;
pub const MAX_PLY_FACE_ARITY: usize = 64;
pub const MAX_PLY_TRIANGLES: usize = 200_000;
const MAX_PLY_HEADER_BYTES: usize = 256 * 1024;
const MAX_PLY_HEADER_LINES: usize = 4_096;
const MAX_PLY_LINE_BYTES: usize = 16 * 1024;
const MAX_PLY_ELEMENTS: usize = 64;
const MAX_PLY_PROPERTIES_PER_ELEMENT: usize = 64;
const MAX_PLY_NAME_BYTES: usize = 256;
const MAX_PLY_COMMENT_BYTES: usize = 4_096;
const MAX_PLY_DECLARED_RECORDS: usize = 400_000;
const MAX_PLY_BODY_TOKENS: usize = 4_000_000;
const MAX_PLY_TOKEN_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlyPreviewSourceGeneration(pub(crate) [u8; 32]);

impl PlyPreviewSourceGeneration {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlyMetadata {
    pub vertex_count: usize,
    pub face_count: usize,
    pub triangle_count: usize,
    pub dimensions: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPlyPreview {
    pub png: Vec<u8>,
    pub source_generation: PlyPreviewSourceGeneration,
}

#[derive(Debug, thiserror::Error)]
pub enum PlyVisualError {
    #[error("PLY visual previews support only .ply files")]
    UnsupportedFormat,
    #[error("could not inspect PLY input: {0}")]
    InputMetadata(String),
    #[error("PLY source is not a regular file")]
    InputNotRegular,
    #[error(
        "PLY previews are disabled on this platform until native safe-file admission is available"
    )]
    PreviewDisabledOnPlatform,
    #[error("PLY input is too large for a bounded preview ({actual} bytes; limit {limit})")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("could not read PLY input: {0}")]
    Read(String),
    #[error("PLY input changed while its bounded snapshot was read")]
    InputChanged,
    #[error("PLY header is invalid or outside bounded preview limits: {0}")]
    Header(String),
    #[error("PLY body is invalid or outside bounded preview limits: {0}")]
    Parse(String),
    #[error("PLY has more than {limit} vertices")]
    VertexLimit { limit: usize },
    #[error("PLY has more than {limit} faces")]
    FaceLimit { limit: usize },
    #[error("PLY face has more than {limit} vertices")]
    FaceArityLimit { limit: usize },
    #[error("PLY has more than {limit} generated triangles")]
    TriangleLimit { limit: usize },
    #[error("PLY contains non-finite, subnormal, or unsafe coordinates")]
    UnsafeCoordinates,
    #[error("PLY contains a degenerate, concave, self-intersecting, or non-planar polygon")]
    InvalidPolygon,
    #[error("PLY has no triangles")]
    EmptyMesh,
    #[error("PLY has unusable or zero-size global bounds")]
    UnusableBounds,
    #[error("PLY mesh would exceed bounded raster work")]
    RasterWorkLimit,
    #[error("could not encode PLY preview PNG: {0}")]
    Encode(String),
}

impl From<MeshVisualError> for PlyVisualError {
    fn from(error: MeshVisualError) -> Self {
        match error {
            MeshVisualError::TriangleLimit => Self::TriangleLimit {
                limit: MAX_PLY_TRIANGLES,
            },
            MeshVisualError::UnsafeCoordinates => Self::UnsafeCoordinates,
            MeshVisualError::DegenerateTriangle => Self::InvalidPolygon,
            MeshVisualError::EmptyMesh => Self::EmptyMesh,
            MeshVisualError::UnusableBounds => Self::UnusableBounds,
            MeshVisualError::RasterWorkLimit => Self::RasterWorkLimit,
            MeshVisualError::Encode => Self::Encode("shared mesh PNG encoder failed".to_string()),
        }
    }
}

#[derive(Debug)]
struct PlySnapshot {
    bytes: Vec<u8>,
    generation: PlyPreviewSourceGeneration,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Ascii,
    LittleEndian,
    BigEndian,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScalarType {
    Char,
    UChar,
    Short,
    UShort,
    Int,
    UInt,
    Float,
    Double,
}

impl ScalarType {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "char" | "int8" => Self::Char,
            "uchar" | "uint8" => Self::UChar,
            "short" | "int16" => Self::Short,
            "ushort" | "uint16" => Self::UShort,
            "int" | "int32" => Self::Int,
            "uint" | "uint32" => Self::UInt,
            "float" | "float32" => Self::Float,
            "double" | "float64" => Self::Double,
            _ => return None,
        })
    }

    fn is_integer(self) -> bool {
        !matches!(self, Self::Float | Self::Double)
    }

    fn byte_len(self) -> usize {
        match self {
            Self::Char | Self::UChar => 1,
            Self::Short | Self::UShort => 2,
            Self::Int | Self::UInt | Self::Float => 4,
            Self::Double => 8,
        }
    }
}

#[derive(Clone)]
enum PropertyKind {
    Scalar(ScalarType),
    List(ScalarType, ScalarType),
}

#[derive(Clone)]
struct Property {
    name: String,
    kind: PropertyKind,
}

#[derive(Clone)]
struct Element {
    name: String,
    count: usize,
    properties: Vec<Property>,
}

struct Header {
    encoding: Encoding,
    elements: Vec<Element>,
    body_offset: usize,
    vertex_index: usize,
    face_index: usize,
}

enum ScalarValue {
    Signed(i64),
    Unsigned(u64),
    Float(f64),
}

impl ScalarValue {
    fn integer(self) -> Result<i128, PlyVisualError> {
        match self {
            Self::Signed(value) => Ok(i128::from(value)),
            Self::Unsigned(value) => Ok(i128::from(value)),
            Self::Float(_) => Err(PlyVisualError::Parse(
                "expected integer list value".to_string(),
            )),
        }
    }

    fn coordinate(self) -> Result<f32, PlyVisualError> {
        let value = match self {
            Self::Float(value) => value,
            Self::Signed(_) | Self::Unsigned(_) => {
                return Err(PlyVisualError::Parse(
                    "vertex coordinate was not floating point".to_string(),
                ));
            }
        };
        let coordinate = value as f32;
        if !value.is_finite()
            || !coordinate.is_finite()
            || (value != 0.0 && (coordinate == 0.0 || coordinate.is_subnormal()))
            || coordinate.abs() > 1.0e12
        {
            return Err(PlyVisualError::UnsafeCoordinates);
        }
        Ok(coordinate)
    }
}

/// Reads one immutable bounded snapshot and returns its content identity.
pub fn ply_preview_source_generation(path: &Path) -> Option<PlyPreviewSourceGeneration> {
    read_ply_snapshot(path)
        .ok()
        .map(|snapshot| snapshot.generation)
}

pub fn inspect_ply_metadata(path: &Path) -> Result<PlyMetadata, PlyVisualError> {
    let (mesh, _, header) = parse_ply_snapshot(path)?;
    Ok(PlyMetadata {
        vertex_count: header.elements[header.vertex_index].count,
        face_count: header.elements[header.face_index].count,
        triangle_count: mesh.triangles.len(),
        dimensions: mesh.dimensions,
    })
}

pub fn render_ply_preview_png(path: &Path, target_dim: u32) -> Result<Vec<u8>, PlyVisualError> {
    Ok(render_ply_preview_png_with_generation(path, target_dim)?.png)
}

pub fn render_ply_preview_png_with_generation(
    path: &Path,
    target_dim: u32,
) -> Result<RenderedPlyPreview, PlyVisualError> {
    let (mesh, generation, _) = parse_ply_snapshot(path)?;
    Ok(RenderedPlyPreview {
        png: render_mesh_preview_png(&mesh, target_dim)?,
        source_generation: generation,
    })
}

#[cfg(unix)]
fn read_ply_snapshot(path: &Path) -> Result<PlySnapshot, PlyVisualError> {
    let path_metadata = std::fs::symlink_metadata(path)
        .map_err(|error| PlyVisualError::InputMetadata(error.to_string()))?;
    if !path_metadata.is_file() {
        return Err(PlyVisualError::InputNotRegular);
    }
    let mut file = open_ply_source(path)?;
    let opened_metadata = file
        .metadata()
        .map_err(|error| PlyVisualError::InputMetadata(error.to_string()))?;
    if !opened_metadata.is_file() {
        return Err(PlyVisualError::InputNotRegular);
    }
    if !same_source_identity(&path_metadata, &opened_metadata) {
        return Err(PlyVisualError::InputChanged);
    }
    let input_size = opened_metadata.len();
    if input_size > MAX_PLY_PREVIEW_BYTES {
        return Err(PlyVisualError::InputTooLarge {
            actual: input_size,
            limit: MAX_PLY_PREVIEW_BYTES,
        });
    }
    let capacity = usize::try_from(input_size).map_err(|_| PlyVisualError::InputTooLarge {
        actual: input_size,
        limit: MAX_PLY_PREVIEW_BYTES,
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    use std::io::Read;
    file.by_ref()
        .take(MAX_PLY_PREVIEW_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| PlyVisualError::Read(error.to_string()))?;
    let final_size = file
        .metadata()
        .map_err(|error| PlyVisualError::Read(error.to_string()))?
        .len();
    if bytes.len() as u64 > MAX_PLY_PREVIEW_BYTES {
        return Err(PlyVisualError::InputTooLarge {
            actual: bytes.len() as u64,
            limit: MAX_PLY_PREVIEW_BYTES,
        });
    }
    if bytes.len() as u64 != input_size || final_size != input_size {
        return Err(PlyVisualError::InputChanged);
    }
    let current_metadata =
        std::fs::symlink_metadata(path).map_err(|_| PlyVisualError::InputChanged)?;
    if !current_metadata.is_file() || !same_source_identity(&opened_metadata, &current_metadata) {
        return Err(PlyVisualError::InputChanged);
    }
    Ok(PlySnapshot {
        generation: PlyPreviewSourceGeneration(Sha256::digest(&bytes).into()),
        bytes,
    })
}

#[cfg(not(unix))]
fn read_ply_snapshot(_path: &Path) -> Result<PlySnapshot, PlyVisualError> {
    // The former generic File::open fallback followed Windows reparse points
    // and could block before proving this is a regular file. Do not re-enable
    // PLY until a native opened-handle implementation is tested.
    Err(PlyVisualError::PreviewDisabledOnPlatform)
}

#[cfg(unix)]
fn open_ply_source(path: &Path) -> Result<std::fs::File, PlyVisualError> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| PlyVisualError::InputMetadata(error.to_string()))
}

#[cfg(unix)]
fn same_source_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev() && left.ino() == right.ino()
}

fn parse_ply_snapshot(
    path: &Path,
) -> Result<(ParsedMesh, PlyPreviewSourceGeneration, Header), PlyVisualError> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ply"))
    {
        return Err(PlyVisualError::UnsupportedFormat);
    }
    let snapshot = read_ply_snapshot(path)?;
    let header = parse_ply_header(&snapshot.bytes)?;
    let mesh = parse_ply_body(&snapshot.bytes[header.body_offset..], &header)?;
    Ok((mesh, snapshot.generation, header))
}

fn parse_ply_header(bytes: &[u8]) -> Result<Header, PlyVisualError> {
    let mut offset = 0usize;
    let mut lines = 0usize;
    let mut format = None;
    let mut elements = Vec::new();
    let mut active_element = None;
    let mut saw_magic = false;
    let mut declared_records = 0usize;

    while offset < bytes.len() && offset < MAX_PLY_HEADER_BYTES {
        let Some(newline_relative) = bytes[offset..MAX_PLY_HEADER_BYTES.min(bytes.len())]
            .iter()
            .position(|byte| *byte == b'\n')
        else {
            return Err(PlyVisualError::Header(
                "header line is unterminated or too long".to_string(),
            ));
        };
        let line_end = offset + newline_relative;
        let mut line = &bytes[offset..line_end];
        offset = line_end + 1;
        lines += 1;
        if lines > MAX_PLY_HEADER_LINES || line.len() > MAX_PLY_LINE_BYTES {
            return Err(PlyVisualError::Header(
                "header exceeds line limits".to_string(),
            ));
        }
        if line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        if line.iter().any(|byte| !byte.is_ascii() || *byte == 0) {
            return Err(PlyVisualError::Header(
                "header contains non-ASCII or NUL bytes".to_string(),
            ));
        }
        let line = std::str::from_utf8(line)
            .map_err(|_| PlyVisualError::Header("header is not UTF-8".to_string()))?;
        if !saw_magic {
            if line != "ply" {
                return Err(PlyVisualError::Header(
                    "first line must be exactly `ply`".to_string(),
                ));
            }
            saw_magic = true;
            continue;
        }
        if line == "end_header" {
            let encoding = format
                .ok_or_else(|| PlyVisualError::Header("missing format declaration".to_string()))?;
            let vertex_index = elements
                .iter()
                .position(|element: &Element| element.name == "vertex")
                .ok_or_else(|| PlyVisualError::Header("missing vertex element".to_string()))?;
            let face_index = elements
                .iter()
                .position(|element: &Element| element.name == "face")
                .ok_or_else(|| PlyVisualError::Header("missing face element".to_string()))?;
            if vertex_index >= face_index {
                return Err(PlyVisualError::Header(
                    "vertex element must precede face element".to_string(),
                ));
            }
            validate_mesh_header(&elements, vertex_index, face_index)?;
            return Ok(Header {
                encoding,
                elements,
                body_offset: offset,
                vertex_index,
                face_index,
            });
        }
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let Some(keyword) = fields.first().copied() else {
            return Err(PlyVisualError::Header(
                "blank header line is not permitted".to_string(),
            ));
        };
        match keyword {
            "format" => {
                if format.is_some()
                    || active_element.is_some()
                    || fields.len() != 3
                    || fields[2] != "1.0"
                {
                    return Err(PlyVisualError::Header(
                        "format must occur once before elements and be version 1.0".to_string(),
                    ));
                }
                format = Some(match fields[1] {
                    "ascii" => Encoding::Ascii,
                    "binary_little_endian" => Encoding::LittleEndian,
                    "binary_big_endian" => Encoding::BigEndian,
                    _ => {
                        return Err(PlyVisualError::Header(
                            "unsupported PLY encoding".to_string(),
                        ));
                    }
                });
            }
            "comment" | "obj_info" => {
                if line.len() > MAX_PLY_COMMENT_BYTES {
                    return Err(PlyVisualError::Header(
                        "comment or obj_info exceeds bounds".to_string(),
                    ));
                }
            }
            "element" => {
                if format.is_none() || fields.len() != 3 || !valid_name(fields[1]) {
                    return Err(PlyVisualError::Header(
                        "invalid element declaration".to_string(),
                    ));
                }
                let count = bounded_count(fields[2], "element count")?;
                declared_records = declared_records.checked_add(count).ok_or_else(|| {
                    PlyVisualError::Header("declared record count overflow".to_string())
                })?;
                if declared_records > MAX_PLY_DECLARED_RECORDS
                    || elements.len() >= MAX_PLY_ELEMENTS
                    || elements
                        .iter()
                        .any(|element: &Element| element.name == fields[1])
                {
                    return Err(PlyVisualError::Header(
                        "duplicate or excessive element declarations".to_string(),
                    ));
                }
                elements.push(Element {
                    name: fields[1].to_string(),
                    count,
                    properties: Vec::new(),
                });
                active_element = Some(elements.len() - 1);
            }
            "property" => {
                let Some(index) = active_element else {
                    return Err(PlyVisualError::Header(
                        "property declared before an element".to_string(),
                    ));
                };
                let property = parse_property(&fields)?;
                let element = &mut elements[index];
                if element.properties.len() >= MAX_PLY_PROPERTIES_PER_ELEMENT
                    || element
                        .properties
                        .iter()
                        .any(|known| known.name == property.name)
                {
                    return Err(PlyVisualError::Header(
                        "duplicate or excessive property declarations".to_string(),
                    ));
                }
                element.properties.push(property);
            }
            _ => {
                return Err(PlyVisualError::Header(
                    "unknown header directive".to_string(),
                ));
            }
        }
    }
    Err(PlyVisualError::Header(
        "header exceeds byte limit".to_string(),
    ))
}

fn parse_property(fields: &[&str]) -> Result<Property, PlyVisualError> {
    let (kind, name) = if fields.get(1) == Some(&"list") {
        if fields.len() != 5 {
            return Err(PlyVisualError::Header(
                "invalid list property declaration".to_string(),
            ));
        }
        let count = ScalarType::parse(fields[2])
            .ok_or_else(|| PlyVisualError::Header("unsupported list count type".to_string()))?;
        let value = ScalarType::parse(fields[3])
            .ok_or_else(|| PlyVisualError::Header("unsupported list value type".to_string()))?;
        (PropertyKind::List(count, value), fields[4])
    } else {
        if fields.len() != 3 {
            return Err(PlyVisualError::Header(
                "invalid scalar property declaration".to_string(),
            ));
        }
        let scalar = ScalarType::parse(fields[1]).ok_or_else(|| {
            PlyVisualError::Header("unsupported scalar property type".to_string())
        })?;
        (PropertyKind::Scalar(scalar), fields[2])
    };
    if !valid_name(name) {
        return Err(PlyVisualError::Header("invalid property name".to_string()));
    }
    Ok(Property {
        name: name.to_string(),
        kind,
    })
}

fn validate_mesh_header(
    elements: &[Element],
    vertex_index: usize,
    face_index: usize,
) -> Result<(), PlyVisualError> {
    let vertex = &elements[vertex_index];
    let face = &elements[face_index];
    if vertex.count > MAX_PLY_VERTICES {
        return Err(PlyVisualError::VertexLimit {
            limit: MAX_PLY_VERTICES,
        });
    }
    if face.count > MAX_PLY_FACES {
        return Err(PlyVisualError::FaceLimit {
            limit: MAX_PLY_FACES,
        });
    }
    for name in ["x", "y", "z"] {
        let matches = vertex
            .properties
            .iter()
            .filter(|property| property.name == name)
            .collect::<Vec<_>>();
        if matches.len() != 1
            || !matches!(
                matches[0].kind,
                PropertyKind::Scalar(ScalarType::Float | ScalarType::Double)
            )
        {
            return Err(PlyVisualError::Header(
                "vertex requires exactly one floating-point x, y, and z property".to_string(),
            ));
        }
    }
    if vertex
        .properties
        .iter()
        .any(|property| matches!(property.kind, PropertyKind::List(..)))
    {
        return Err(PlyVisualError::Header(
            "vertex lists are unsupported".to_string(),
        ));
    }
    let index_lists = face
        .properties
        .iter()
        .filter(|property| matches!(property.name.as_str(), "vertex_indices" | "vertex_index"))
        .collect::<Vec<_>>();
    if index_lists.len() != 1
        || !matches!(index_lists[0].kind, PropertyKind::List(count, value) if count.is_integer() && value.is_integer())
    {
        return Err(PlyVisualError::Header(
            "face requires exactly one integer vertex_indices or vertex_index list".to_string(),
        ));
    }
    if face.properties.iter().any(|property| {
        matches!(property.kind, PropertyKind::List(..))
            && !matches!(property.name.as_str(), "vertex_indices" | "vertex_index")
    }) {
        return Err(PlyVisualError::Header(
            "face contains unsupported list properties".to_string(),
        ));
    }
    for element in elements {
        if element.name != "face"
            && element.name != "vertex"
            && element
                .properties
                .iter()
                .any(|property| matches!(property.kind, PropertyKind::List(..)))
        {
            return Err(PlyVisualError::Header(
                "unknown list element cannot be skipped safely".to_string(),
            ));
        }
    }
    Ok(())
}

fn parse_ply_body(bytes: &[u8], header: &Header) -> Result<ParsedMesh, PlyVisualError> {
    let mut reader = BodyReader::new(bytes, header.encoding);
    let mut vertices = Vec::with_capacity(header.elements[header.vertex_index].count);
    let mut builder = MeshBuilder::new(MAX_PLY_TRIANGLES);
    for (element_index, element) in header.elements.iter().enumerate() {
        for _ in 0..element.count {
            if element_index == header.vertex_index {
                let mut point = [None; 3];
                for property in &element.properties {
                    let PropertyKind::Scalar(kind) = property.kind else {
                        return Err(PlyVisualError::Parse(
                            "vertex list reached body parser".to_string(),
                        ));
                    };
                    let value = reader.scalar(kind)?;
                    match property.name.as_str() {
                        "x" => point[0] = Some(value.coordinate()?),
                        "y" => point[1] = Some(value.coordinate()?),
                        "z" => point[2] = Some(value.coordinate()?),
                        _ => {}
                    }
                }
                vertices.push([
                    point[0]
                        .ok_or_else(|| PlyVisualError::Parse("missing x coordinate".to_string()))?,
                    point[1]
                        .ok_or_else(|| PlyVisualError::Parse("missing y coordinate".to_string()))?,
                    point[2]
                        .ok_or_else(|| PlyVisualError::Parse("missing z coordinate".to_string()))?,
                ]);
            } else if element_index == header.face_index {
                let mut indices = None;
                for property in &element.properties {
                    match property.kind {
                        PropertyKind::Scalar(kind) => {
                            let _ = reader.scalar(kind)?;
                        }
                        PropertyKind::List(count_kind, value_kind) => {
                            let count = reader.scalar(count_kind)?.integer()?;
                            if count < 3 {
                                return Err(PlyVisualError::InvalidPolygon);
                            }
                            let count = usize::try_from(count).map_err(|_| {
                                PlyVisualError::FaceArityLimit {
                                    limit: MAX_PLY_FACE_ARITY,
                                }
                            })?;
                            if count > MAX_PLY_FACE_ARITY {
                                return Err(PlyVisualError::FaceArityLimit {
                                    limit: MAX_PLY_FACE_ARITY,
                                });
                            }
                            let mut values = Vec::with_capacity(count);
                            for _ in 0..count {
                                let index = reader.scalar(value_kind)?.integer()?;
                                if index < 0 {
                                    return Err(PlyVisualError::Parse(
                                        "face index is negative".to_string(),
                                    ));
                                }
                                let index = usize::try_from(index).map_err(|_| {
                                    PlyVisualError::Parse("face index overflow".to_string())
                                })?;
                                if index >= vertices.len() {
                                    return Err(PlyVisualError::Parse(
                                        "face index is out of range".to_string(),
                                    ));
                                }
                                values.push(index);
                            }
                            indices = Some(values);
                        }
                    }
                }
                triangulate_convex_face(
                    &mut builder,
                    &vertices,
                    &indices
                        .ok_or_else(|| PlyVisualError::Parse("missing face indices".to_string()))?,
                )?;
            } else {
                for property in &element.properties {
                    let PropertyKind::Scalar(kind) = property.kind else {
                        return Err(PlyVisualError::Parse(
                            "unsupported list element reached body parser".to_string(),
                        ));
                    };
                    let _ = reader.scalar(kind)?;
                }
            }
        }
    }
    reader.finish()?;
    builder.finish().map_err(Into::into)
}

fn triangulate_convex_face(
    builder: &mut MeshBuilder,
    vertices: &[[f32; 3]],
    indices: &[usize],
) -> Result<(), PlyVisualError> {
    let polygon = indices
        .iter()
        .map(|index| vertices[*index])
        .collect::<Vec<_>>();
    if !strictly_convex_planar(&polygon) {
        return Err(PlyVisualError::InvalidPolygon);
    }
    for index in 1..polygon.len() - 1 {
        let triangle = [polygon[0], polygon[index], polygon[index + 1]];
        builder.add_triangle(triangle_normal(triangle), triangle)?;
    }
    Ok(())
}

fn strictly_convex_planar(vertices: &[[f32; 3]]) -> bool {
    if vertices.len() < 3 {
        return false;
    }
    let normal = triangle_normal([vertices[0], vertices[1], vertices[2]]);
    let normal_length =
        (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if !normal_length.is_finite() || normal_length <= f32::EPSILON {
        return false;
    }
    let max_extent = vertices
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0_f32, f32::max);
    for vertex in vertices.iter().skip(3) {
        let delta = [
            vertex[0] - vertices[0][0],
            vertex[1] - vertices[0][1],
            vertex[2] - vertices[0][2],
        ];
        if (normal[0] * delta[0] + normal[1] * delta[1] + normal[2] * delta[2]).abs()
            > normal_length * max_extent * 1.0e-5
        {
            return false;
        }
    }
    let axis = if normal[0].abs() >= normal[1].abs() && normal[0].abs() >= normal[2].abs() {
        0
    } else if normal[1].abs() >= normal[2].abs() {
        1
    } else {
        2
    };
    let points = vertices
        .iter()
        .map(|vertex| match axis {
            0 => (vertex[1], vertex[2]),
            1 => (vertex[0], vertex[2]),
            _ => (vertex[0], vertex[1]),
        })
        .collect::<Vec<_>>();
    let mut sign = 0.0_f32;
    for index in 0..points.len() {
        let previous = points[(index + points.len() - 1) % points.len()];
        let current = points[index];
        let next = points[(index + 1) % points.len()];
        let cross = (current.0 - previous.0) * (next.1 - current.1)
            - (current.1 - previous.1) * (next.0 - current.0);
        if !cross.is_finite()
            || cross.abs() <= f32::EPSILON
            || (sign != 0.0 && cross.signum() != sign.signum())
        {
            return false;
        }
        sign = cross;
    }
    true
}

fn triangle_normal(vertices: [[f32; 3]; 3]) -> [f32; 3] {
    let left = [
        vertices[1][0] - vertices[0][0],
        vertices[1][1] - vertices[0][1],
        vertices[1][2] - vertices[0][2],
    ];
    let right = [
        vertices[2][0] - vertices[0][0],
        vertices[2][1] - vertices[0][1],
        vertices[2][2] - vertices[0][2],
    ];
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

struct BodyReader<'a> {
    bytes: &'a [u8],
    position: usize,
    encoding: Encoding,
    tokens: usize,
}

impl<'a> BodyReader<'a> {
    fn new(bytes: &'a [u8], encoding: Encoding) -> Self {
        Self {
            bytes,
            position: 0,
            encoding,
            tokens: 0,
        }
    }

    fn scalar(&mut self, kind: ScalarType) -> Result<ScalarValue, PlyVisualError> {
        match self.encoding {
            Encoding::Ascii => self.ascii_scalar(kind),
            Encoding::LittleEndian | Encoding::BigEndian => self.binary_scalar(kind),
        }
    }

    fn ascii_scalar(&mut self, kind: ScalarType) -> Result<ScalarValue, PlyVisualError> {
        while self.position < self.bytes.len() && is_ascii_space(self.bytes[self.position]) {
            self.position += 1;
        }
        let start = self.position;
        while self.position < self.bytes.len() && !is_ascii_space(self.bytes[self.position]) {
            self.position += 1;
        }
        if start == self.position
            || self.position - start > MAX_PLY_TOKEN_BYTES
            || self.bytes[start..self.position]
                .iter()
                .any(|byte| !byte.is_ascii() || *byte == 0)
        {
            return Err(PlyVisualError::Parse(
                "invalid ASCII body token".to_string(),
            ));
        }
        self.tokens += 1;
        if self.tokens > MAX_PLY_BODY_TOKENS {
            return Err(PlyVisualError::Parse(
                "ASCII body token limit exceeded".to_string(),
            ));
        }
        let token = std::str::from_utf8(&self.bytes[start..self.position])
            .map_err(|_| PlyVisualError::Parse("ASCII token was not UTF-8".to_string()))?;
        match kind {
            ScalarType::Char => token
                .parse::<i8>()
                .map(|value| ScalarValue::Signed(i64::from(value)))
                .map_err(|_| PlyVisualError::Parse("invalid scalar body value".to_string())),
            ScalarType::UChar => token
                .parse::<u8>()
                .map(|value| ScalarValue::Unsigned(u64::from(value)))
                .map_err(|_| PlyVisualError::Parse("invalid scalar body value".to_string())),
            ScalarType::Short => token
                .parse::<i16>()
                .map(|value| ScalarValue::Signed(i64::from(value)))
                .map_err(|_| PlyVisualError::Parse("invalid scalar body value".to_string())),
            ScalarType::UShort => token
                .parse::<u16>()
                .map(|value| ScalarValue::Unsigned(u64::from(value)))
                .map_err(|_| PlyVisualError::Parse("invalid scalar body value".to_string())),
            ScalarType::Int => token
                .parse::<i32>()
                .map(|value| ScalarValue::Signed(i64::from(value)))
                .map_err(|_| PlyVisualError::Parse("invalid scalar body value".to_string())),
            ScalarType::UInt => token
                .parse::<u32>()
                .map(|value| ScalarValue::Unsigned(u64::from(value)))
                .map_err(|_| PlyVisualError::Parse("invalid scalar body value".to_string())),
            ScalarType::Float => token
                .parse::<f32>()
                .map(|value| ScalarValue::Float(f64::from(value)))
                .map_err(|_| PlyVisualError::Parse("invalid scalar body value".to_string())),
            ScalarType::Double => token
                .parse::<f64>()
                .map(ScalarValue::Float)
                .map_err(|_| PlyVisualError::Parse("invalid scalar body value".to_string())),
        }
    }

    fn binary_scalar(&mut self, kind: ScalarType) -> Result<ScalarValue, PlyVisualError> {
        let end = self
            .position
            .checked_add(kind.byte_len())
            .ok_or_else(|| PlyVisualError::Parse("binary offset overflow".to_string()))?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| PlyVisualError::Parse("truncated binary body".to_string()))?;
        self.position = end;
        let big = self.encoding == Encoding::BigEndian;
        let signed = |bytes: &[u8]| -> i64 {
            match bytes.len() {
                1 => i64::from(i8::from_ne_bytes([bytes[0]])),
                2 => i64::from(if big {
                    i16::from_be_bytes(bytes.try_into().expect("fixed length"))
                } else {
                    i16::from_le_bytes(bytes.try_into().expect("fixed length"))
                }),
                _ => i64::from(if big {
                    i32::from_be_bytes(bytes.try_into().expect("fixed length"))
                } else {
                    i32::from_le_bytes(bytes.try_into().expect("fixed length"))
                }),
            }
        };
        let unsigned = |bytes: &[u8]| -> u64 {
            match bytes.len() {
                1 => u64::from(bytes[0]),
                2 => u64::from(if big {
                    u16::from_be_bytes(bytes.try_into().expect("fixed length"))
                } else {
                    u16::from_le_bytes(bytes.try_into().expect("fixed length"))
                }),
                _ => u64::from(if big {
                    u32::from_be_bytes(bytes.try_into().expect("fixed length"))
                } else {
                    u32::from_le_bytes(bytes.try_into().expect("fixed length"))
                }),
            }
        };
        Ok(match kind {
            ScalarType::Char | ScalarType::Short | ScalarType::Int => {
                ScalarValue::Signed(signed(bytes))
            }
            ScalarType::UChar | ScalarType::UShort | ScalarType::UInt => {
                ScalarValue::Unsigned(unsigned(bytes))
            }
            ScalarType::Float => ScalarValue::Float(f64::from(if big {
                f32::from_be_bytes(bytes.try_into().expect("fixed length"))
            } else {
                f32::from_le_bytes(bytes.try_into().expect("fixed length"))
            })),
            ScalarType::Double => ScalarValue::Float(if big {
                f64::from_be_bytes(bytes.try_into().expect("fixed length"))
            } else {
                f64::from_le_bytes(bytes.try_into().expect("fixed length"))
            }),
        })
    }

    fn finish(self) -> Result<(), PlyVisualError> {
        match self.encoding {
            Encoding::Ascii
                if self.bytes[self.position..]
                    .iter()
                    .all(|byte| is_ascii_space(*byte)) =>
            {
                Ok(())
            }
            Encoding::Ascii => Err(PlyVisualError::Parse(
                "ASCII body has trailing non-whitespace data".to_string(),
            )),
            Encoding::LittleEndian | Encoding::BigEndian if self.position == self.bytes.len() => {
                Ok(())
            }
            Encoding::LittleEndian | Encoding::BigEndian => Err(PlyVisualError::Parse(
                "binary body has trailing bytes".to_string(),
            )),
        }
    }
}

fn is_ascii_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_PLY_NAME_BYTES
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
fn bounded_count(value: &str, label: &str) -> Result<usize, PlyVisualError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(PlyVisualError::Header(format!("invalid {label}")));
    }
    value
        .parse()
        .map_err(|_| PlyVisualError::Header(format!("{label} overflow")))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixListener;
    use std::process::Command;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn temp_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "ottrin-ply-nonregular-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ))
    }

    fn assert_non_regular_ply_rejected(path: &Path) {
        assert!(ply_preview_source_generation(path).is_none());
        for result in [
            inspect_ply_metadata(path).map(|_| ()),
            render_ply_preview_png(path, 128).map(|_| ()),
        ] {
            assert_eq!(
                result
                    .expect_err("non-regular PLY source must fail closed")
                    .to_string(),
                "PLY source is not a regular file"
            );
        }
    }

    #[test]
    fn ply_special_sources_and_symlinks_fail_closed() {
        let root = temp_root();
        std::fs::create_dir(&root).expect("create root");
        let directory = root.join("directory.ply");
        std::fs::create_dir(&directory).expect("create PLY directory");
        let socket = root.join("socket.ply");
        let _listener = UnixListener::bind(&socket).expect("create PLY socket");
        let target = root.join("target.ply");
        std::fs::write(&target, b"ply\nformat ascii 1.0\nend_header\n")
            .expect("write PLY symlink target");
        let link = root.join("link.ply");
        symlink(&target, &link).expect("create PLY symlink");

        for path in [&directory, &socket, &link] {
            assert_non_regular_ply_rejected(path);
        }
        assert!(
            ply_preview_source_generation(Path::new("/dev/null")).is_none(),
            "device sources must be rejected before a bounded snapshot is read"
        );

        drop(_listener);
        std::fs::remove_dir_all(root).expect("remove PLY fixtures");
    }

    #[test]
    fn ply_fifo_is_rejected_without_blocking() {
        const CHILD: &str = "OTTRIN_PLY_FIFO_REJECTION_CHILD";
        const PATH: &str = "OTTRIN_PLY_FIFO_REJECTION_PATH";
        if std::env::var_os(CHILD).as_deref() == Some(OsStr::new("1")) {
            let fifo = std::env::var_os(PATH).expect("FIFO path");
            assert_non_regular_ply_rejected(Path::new(&fifo));
            return;
        }

        let root = temp_root();
        std::fs::create_dir(&root).expect("create root");
        let fifo = root.join("blocked.ply");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .expect("run mkfifo")
                .success()
        );
        let mut child = Command::new(std::env::current_exe().expect("current test binary"))
            .args([
                "--exact",
                "ply_visual::tests::ply_fifo_is_rejected_without_blocking",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .env(PATH, &fifo)
            .spawn()
            .expect("start bounded FIFO child");
        let deadline = Instant::now() + Duration::from_secs(2);
        let status = loop {
            if let Some(status) = child.try_wait().expect("poll FIFO child") {
                break Some(status);
            }
            if Instant::now() >= deadline {
                child.kill().expect("stop blocking FIFO child");
                child.wait().expect("reap blocking FIFO child");
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        std::fs::remove_dir_all(root).expect("remove FIFO fixture");
        assert!(
            status.is_some_and(|status| status.success()),
            "PLY must reject an unread FIFO before blocking"
        );
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "ottrin-ply-windows-admission-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ))
    }

    fn assert_windows_ply_admission_disabled(path: &Path) {
        assert!(ply_preview_source_generation(path).is_none());
        for result in [
            inspect_ply_metadata(path).map(|_| ()),
            render_ply_preview_png(path, 128).map(|_| ()),
        ] {
            assert!(matches!(
                result,
                Err(PlyVisualError::PreviewDisabledOnPlatform)
            ));
        }
    }

    #[test]
    fn ply_windows_source_admission_is_fail_closed_pending_native_handle_validation() {
        let root = temp_root();
        std::fs::create_dir(&root).expect("create root");
        let ordinary = root.join("ordinary.ply");
        std::fs::write(&ordinary, b"ply\nformat ascii 1.0\nend_header\n")
            .expect("write ordinary PLY");
        let oversized = root.join("oversized.ply");
        let oversized_file = std::fs::File::create(&oversized).expect("create oversized PLY");
        oversized_file
            .set_len(MAX_PLY_PREVIEW_BYTES + 1)
            .expect("size oversized PLY");
        drop(oversized_file);
        let truncated = root.join("truncated.ply");
        std::fs::write(&truncated, b"ply\nformat ascii 1.0\n").expect("write truncated PLY");

        for path in [&ordinary, &oversized, &truncated] {
            assert_windows_ply_admission_disabled(path);
        }
        std::fs::remove_dir_all(root).expect("remove PLY fixtures");
    }
}
