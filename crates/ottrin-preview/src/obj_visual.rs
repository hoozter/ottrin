use crate::mesh_visual::{MeshBuilder, MeshVisualError, ParsedMesh, render_mesh_preview_png};
use sha2::{Digest, Sha256};
use std::io::{BufReader, Cursor, Read};
use std::path::Path;

pub const MAX_OBJ_PREVIEW_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_OBJ_VERTICES: usize = 200_000;
pub const MAX_OBJ_NORMALS: usize = 200_000;
pub const MAX_OBJ_TEXCOORDS: usize = 200_000;
pub const MAX_OBJ_MODELS: usize = 4_096;
pub const MAX_OBJ_FACES: usize = 200_000;
pub const MAX_OBJ_FACE_ARITY: usize = 64;
pub const MAX_OBJ_TRIANGLES: usize = 200_000;
const MAX_OBJ_LINES: usize = 500_000;
const MAX_OBJ_LINE_BYTES: usize = 16 * 1024;
const MAX_OBJ_NAME_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjPreviewSourceGeneration(pub(crate) [u8; 32]);
impl ObjPreviewSourceGeneration {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjMetadata {
    pub object_count: usize,
    pub group_count: usize,
    pub vertex_count: usize,
    pub face_count: usize,
    pub triangle_count: usize,
    pub dimensions: [f32; 3],
}

/// A rendered OBJ preview tied to the exact immutable snapshot used for both
/// preflight and rasterization. Cache callers must compare this generation
/// before storing or displaying the PNG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedObjPreview {
    pub source_generation: ObjPreviewSourceGeneration,
    pub png: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum ObjVisualError {
    #[error("OBJ visual previews support only .obj files")]
    UnsupportedFormat,
    #[error("could not inspect OBJ input: {0}")]
    InputMetadata(String),
    #[error("OBJ input is too large for a bounded preview ({actual} bytes; limit {limit})")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("could not read OBJ input: {0}")]
    Read(String),
    #[error("OBJ input changed while its bounded snapshot was read")]
    InputChanged,
    #[error("OBJ must be UTF-8 text without NUL bytes")]
    InvalidText,
    #[error("OBJ record exceeds bounded preview limits: {0}")]
    RecordLimit(String),
    #[error("OBJ contains unsupported free-form or non-mesh geometry: {0}")]
    UnsupportedGeometry(String),
    #[error("OBJ has malformed or out-of-range indices: {0}")]
    InvalidIndex(String),
    #[error("could not parse the complete OBJ mesh: {0}")]
    Parse(String),
    #[error("OBJ has no usable triangular faces")]
    EmptyMesh,
    #[error("OBJ contains non-finite, subnormal, or unsafe coordinates")]
    UnsafeCoordinates,
    #[error("OBJ contains a degenerate triangle")]
    DegenerateTriangle,
    #[error("OBJ has unusable or zero-size global bounds")]
    UnusableBounds,
    #[error("OBJ mesh would exceed bounded raster work")]
    RasterWorkLimit,
    #[error("could not encode OBJ preview PNG")]
    Encode,
}

struct ObjSnapshot {
    bytes: Vec<u8>,
    generation: ObjPreviewSourceGeneration,
}
struct ObjPreflight {
    objects: usize,
    groups: usize,
    vertices: usize,
    faces: usize,
    triangles: usize,
}

impl From<MeshVisualError> for ObjVisualError {
    fn from(error: MeshVisualError) -> Self {
        match error {
            MeshVisualError::TriangleLimit => {
                Self::RecordLimit(format!("more than {MAX_OBJ_TRIANGLES} generated triangles"))
            }
            MeshVisualError::UnsafeCoordinates => Self::UnsafeCoordinates,
            MeshVisualError::DegenerateTriangle => Self::DegenerateTriangle,
            MeshVisualError::EmptyMesh => Self::EmptyMesh,
            MeshVisualError::UnusableBounds => Self::UnusableBounds,
            MeshVisualError::RasterWorkLimit => Self::RasterWorkLimit,
            MeshVisualError::Encode => Self::Encode,
        }
    }
}

pub fn obj_preview_source_generation(path: &Path) -> Option<ObjPreviewSourceGeneration> {
    read_obj_snapshot(path)
        .ok()
        .map(|snapshot| snapshot.generation)
}
pub fn inspect_obj_metadata(path: &Path) -> Result<ObjMetadata, ObjVisualError> {
    let (mesh, preflight, _) = parse_obj_snapshot(path)?;
    Ok(ObjMetadata {
        object_count: preflight.objects,
        group_count: preflight.groups,
        vertex_count: preflight.vertices,
        face_count: preflight.faces,
        triangle_count: mesh.triangles.len(),
        dimensions: mesh.dimensions,
    })
}
pub fn render_obj_preview_png(path: &Path, target_dim: u32) -> Result<Vec<u8>, ObjVisualError> {
    Ok(render_obj_preview_png_with_generation(path, target_dim)?.png)
}

pub fn render_obj_preview_png_with_generation(
    path: &Path,
    target_dim: u32,
) -> Result<RenderedObjPreview, ObjVisualError> {
    let (mesh, _, source_generation) = parse_obj_snapshot(path)?;
    Ok(RenderedObjPreview {
        source_generation,
        png: render_mesh_preview_png(&mesh, target_dim)?,
    })
}

fn read_obj_snapshot(path: &Path) -> Result<ObjSnapshot, ObjVisualError> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| ObjVisualError::InputMetadata(error.to_string()))?;
    let input_size = file
        .metadata()
        .map_err(|error| ObjVisualError::InputMetadata(error.to_string()))?
        .len();
    if input_size > MAX_OBJ_PREVIEW_BYTES {
        return Err(ObjVisualError::InputTooLarge {
            actual: input_size,
            limit: MAX_OBJ_PREVIEW_BYTES,
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(input_size).map_err(|_| {
        ObjVisualError::InputTooLarge {
            actual: input_size,
            limit: MAX_OBJ_PREVIEW_BYTES,
        }
    })?);
    file.by_ref()
        .take(MAX_OBJ_PREVIEW_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| ObjVisualError::Read(error.to_string()))?;
    let final_size = file
        .metadata()
        .map_err(|error| ObjVisualError::Read(error.to_string()))?
        .len();
    if bytes.len() as u64 > MAX_OBJ_PREVIEW_BYTES {
        return Err(ObjVisualError::InputTooLarge {
            actual: bytes.len() as u64,
            limit: MAX_OBJ_PREVIEW_BYTES,
        });
    }
    if bytes.len() as u64 != input_size || final_size != input_size {
        return Err(ObjVisualError::InputChanged);
    }
    Ok(ObjSnapshot {
        generation: ObjPreviewSourceGeneration(Sha256::digest(&bytes).into()),
        bytes,
    })
}

fn parse_obj_snapshot(
    path: &Path,
) -> Result<(ParsedMesh, ObjPreflight, ObjPreviewSourceGeneration), ObjVisualError> {
    if !path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("obj"))
    {
        return Err(ObjVisualError::UnsupportedFormat);
    }
    let snapshot = read_obj_snapshot(path)?;
    let text = std::str::from_utf8(&snapshot.bytes).map_err(|_| ObjVisualError::InvalidText)?;
    if snapshot.bytes.contains(&0) {
        return Err(ObjVisualError::InvalidText);
    }
    let preflight = preflight_obj(text)?;
    let mut reader = BufReader::new(Cursor::new(snapshot.bytes));
    let (models, _) = tobj::load_obj_buf(
        &mut reader,
        &tobj::LoadOptions {
            triangulate: true,
            ignore_points: true,
            ignore_lines: true,
            ..Default::default()
        },
        |_| Err(tobj::LoadError::OpenFileFailed),
    )
    .map_err(|error| ObjVisualError::Parse(error.to_string()))?;
    let mut builder = MeshBuilder::new(MAX_OBJ_TRIANGLES);
    for model in models {
        if model.mesh.indices.len() % 3 != 0 {
            return Err(ObjVisualError::Parse(
                "parser returned non-triangular mesh".to_string(),
            ));
        }
        for indices in model.mesh.indices.chunks_exact(3) {
            let mut vertices = [[0.0; 3]; 3];
            for (slot, index) in indices.iter().enumerate() {
                let base = usize::try_from(*index)
                    .ok()
                    .and_then(|index| index.checked_mul(3))
                    .ok_or_else(|| {
                        ObjVisualError::Parse("mesh vertex index overflow".to_string())
                    })?;
                let position = model.mesh.positions.get(base..base + 3).ok_or_else(|| {
                    ObjVisualError::Parse("mesh vertex index out of range".to_string())
                })?;
                vertices[slot] = [position[0], position[1], position[2]];
            }
            builder.add_triangle(triangle_normal(vertices), vertices)?;
        }
    }
    let mesh = builder.finish()?;
    if mesh.triangles.len() != preflight.triangles {
        return Err(ObjVisualError::Parse(
            "parser triangle count disagreed with preflight".to_string(),
        ));
    }
    Ok((mesh, preflight, snapshot.generation))
}

fn preflight_obj(text: &str) -> Result<ObjPreflight, ObjVisualError> {
    let mut result = ObjPreflight {
        objects: 0,
        groups: 0,
        vertices: 0,
        faces: 0,
        triangles: 0,
    };
    let mut normals = 0usize;
    let mut texcoords = 0usize;
    for (line_number, raw) in text.lines().enumerate() {
        if line_number >= MAX_OBJ_LINES {
            return Err(ObjVisualError::RecordLimit(format!(
                "more than {MAX_OBJ_LINES} lines"
            )));
        }
        if raw.len() > MAX_OBJ_LINE_BYTES {
            return Err(ObjVisualError::RecordLimit(format!(
                "line {} exceeds {MAX_OBJ_LINE_BYTES} bytes",
                line_number + 1
            )));
        }
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_ascii_whitespace();
        let keyword = parts.next().expect("nonempty line");
        let rest: Vec<_> = parts.collect();
        match keyword {
            "v" => {
                exactly_numeric(&rest, 3, line_number)?;
                result.vertices += 1;
                if result.vertices > MAX_OBJ_VERTICES {
                    return Err(ObjVisualError::RecordLimit(format!(
                        "more than {MAX_OBJ_VERTICES} vertices"
                    )));
                }
            }
            "vn" => {
                exactly_numeric(&rest, 3, line_number)?;
                normals += 1;
                if normals > MAX_OBJ_NORMALS {
                    return Err(ObjVisualError::RecordLimit(format!(
                        "more than {MAX_OBJ_NORMALS} normals"
                    )));
                }
            }
            "vt" => {
                if !(1..=3).contains(&rest.len()) {
                    return Err(ObjVisualError::Parse(format!(
                        "line {} has invalid texture coordinates",
                        line_number + 1
                    )));
                }
                for value in rest {
                    bounded_float(value)?;
                }
                texcoords += 1;
                if texcoords > MAX_OBJ_TEXCOORDS {
                    return Err(ObjVisualError::RecordLimit(format!(
                        "more than {MAX_OBJ_TEXCOORDS} texture coordinates"
                    )));
                }
            }
            "f" => {
                if rest.len() < 3 || rest.len() > MAX_OBJ_FACE_ARITY {
                    return Err(ObjVisualError::RecordLimit(format!(
                        "line {} face arity is outside 3..={MAX_OBJ_FACE_ARITY}",
                        line_number + 1
                    )));
                }
                for value in &rest {
                    validate_face_index(value, result.vertices, texcoords, normals)?;
                }
                result.faces += 1;
                result.triangles =
                    result
                        .triangles
                        .checked_add(rest.len() - 2)
                        .ok_or_else(|| {
                            ObjVisualError::RecordLimit("triangle count overflow".to_string())
                        })?;
                if result.faces > MAX_OBJ_FACES || result.triangles > MAX_OBJ_TRIANGLES {
                    return Err(ObjVisualError::RecordLimit(format!(
                        "more than {MAX_OBJ_TRIANGLES} generated triangles"
                    )));
                }
            }
            "o" => {
                bounded_name(&rest, line_number)?;
                result.objects += 1;
                if result.objects > MAX_OBJ_MODELS {
                    return Err(ObjVisualError::RecordLimit(format!(
                        "more than {MAX_OBJ_MODELS} objects"
                    )));
                }
            }
            "g" => {
                bounded_name(&rest, line_number)?;
                result.groups += 1;
                if result.groups > MAX_OBJ_MODELS {
                    return Err(ObjVisualError::RecordLimit(format!(
                        "more than {MAX_OBJ_MODELS} groups"
                    )));
                }
            }
            "mtllib" | "usemtl" | "s" => bounded_name(&rest, line_number)?,
            "p" | "l" | "curv" | "curv2" | "surf" | "parm" | "trim" | "hole" | "scrv" | "sp"
            | "end" | "cstype" | "deg" | "bmat" | "step" => {
                return Err(ObjVisualError::UnsupportedGeometry(keyword.to_string()));
            }
            _ => {
                return Err(ObjVisualError::Parse(format!(
                    "line {} has unsupported OBJ record {keyword:?}",
                    line_number + 1
                )));
            }
        }
    }
    if result.faces == 0 {
        return Err(ObjVisualError::EmptyMesh);
    }
    Ok(result)
}

fn bounded_name(parts: &[&str], line: usize) -> Result<(), ObjVisualError> {
    if parts.is_empty()
        || parts.join(" ").len() > MAX_OBJ_NAME_BYTES
        || parts.iter().any(|part| part.chars().any(char::is_control))
    {
        return Err(ObjVisualError::RecordLimit(format!(
            "line {} has an invalid or oversized name",
            line + 1
        )));
    }
    Ok(())
}
fn exactly_numeric(parts: &[&str], expected: usize, line: usize) -> Result<(), ObjVisualError> {
    if parts.len() != expected {
        return Err(ObjVisualError::Parse(format!(
            "line {} expected {expected} coordinates",
            line + 1
        )));
    }
    for part in parts {
        bounded_float(part)?;
    }
    Ok(())
}
fn bounded_float(value: &str) -> Result<(), ObjVisualError> {
    let parsed: f32 = value
        .parse()
        .map_err(|_| ObjVisualError::UnsafeCoordinates)?;
    if !parsed.is_finite() || (parsed != 0.0 && !parsed.is_normal()) || parsed.abs() > 1.0e12 {
        return Err(ObjVisualError::UnsafeCoordinates);
    }
    Ok(())
}
fn validate_face_index(
    value: &str,
    vertices: usize,
    texcoords: usize,
    normals: usize,
) -> Result<(), ObjVisualError> {
    let fields: Vec<_> = value.split('/').collect();
    if fields.is_empty() || fields.len() > 3 || fields[0].is_empty() {
        return Err(ObjVisualError::InvalidIndex(value.to_string()));
    }
    validate_index(fields[0], vertices)?;
    if fields.len() > 1 && !fields[1].is_empty() {
        validate_index(fields[1], texcoords)?;
    }
    if fields.len() > 2 && !fields[2].is_empty() {
        validate_index(fields[2], normals)?;
    }
    Ok(())
}
fn validate_index(value: &str, len: usize) -> Result<(), ObjVisualError> {
    let index: i64 = value
        .parse()
        .map_err(|_| ObjVisualError::InvalidIndex(value.to_string()))?;
    if index == 0
        || (index > 0 && usize::try_from(index).ok().is_none_or(|index| index > len))
        || (index < 0 && index.unsigned_abs() as usize > len)
    {
        return Err(ObjVisualError::InvalidIndex(value.to_string()));
    }
    Ok(())
}
fn triangle_normal(vertices: [[f32; 3]; 3]) -> [f32; 3] {
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
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let length = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    if length.is_normal() {
        [cross[0] / length, cross[1] / length, cross[2] / length]
    } else {
        [0.0, 0.0, 0.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("ottrin-obj-{nonce}-{name}"))
    }

    #[test]
    fn triangle_quad_and_negative_indices_have_truthful_metadata_and_deterministic_pngs() {
        let path = temp_path("bracket.obj");
        std::fs::write(
            &path,
            "o bracket\ng bracket\nv 0 0 0\nv 1 0 0\nv 1 1 1\nv 0 1 1\nf -4 -3 -2 -1\n",
        )
        .expect("write OBJ");

        let metadata = inspect_obj_metadata(&path).expect("inspect bounded OBJ");
        assert_eq!(
            (
                metadata.object_count,
                metadata.group_count,
                metadata.vertex_count,
                metadata.face_count,
                metadata.triangle_count
            ),
            (1, 1, 4, 1, 2)
        );
        let first = render_obj_preview_png(&path, 128).expect("render OBJ");
        let expected_generation = obj_preview_source_generation(&path).expect("capture generation");
        assert_eq!(
            render_obj_preview_png_with_generation(&path, 128)
                .expect("render bound OBJ")
                .source_generation,
            expected_generation
        );
        assert_eq!(
            first,
            render_obj_preview_png(&path, 128).expect("render again")
        );
        assert_eq!(
            image::load_from_memory(&first)
                .expect("decode PNG")
                .dimensions(),
            (128, 128)
        );
        for (requested, expected) in [(0, 64), (1, 64), (1024, 1024), (1025, 1024)] {
            let png = render_obj_preview_png(&path, requested).expect("render clamped OBJ");
            assert_eq!(
                image::load_from_memory(&png)
                    .expect("decode PNG")
                    .dimensions(),
                (expected, expected)
            );
        }
        std::fs::remove_file(path).expect("remove OBJ");
    }

    #[test]
    fn parser_rejects_malformed_unsafe_and_non_mesh_obj_records_before_rendering() {
        let valid_prefix = "v 0 0 0\nv 1 0 0\nv 0 1 1\n";
        let cases = [
            ("zero-index.obj", format!("{valid_prefix}f 0 2 3\n")),
            ("out-of-range.obj", format!("{valid_prefix}f 1 2 4\n")),
            (
                "overflow-index.obj",
                format!("{valid_prefix}f 999999999999999999999 2 3\n"),
            ),
            (
                "nan.obj",
                "v NaN 0 0\nv 1 0 0\nv 0 1 1\nf 1 2 3\n".to_string(),
            ),
            (
                "subnormal.obj",
                "v 1e-44 0 0\nv 1 0 0\nv 0 1 1\nf 1 2 3\n".to_string(),
            ),
            (
                "flat.obj",
                "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n".to_string(),
            ),
            ("line-only.obj", format!("{valid_prefix}l 1 2\n")),
            ("curve.obj", format!("{valid_prefix}curv 0 1 1 2 3\n")),
            ("vertex-only.obj", valid_prefix.to_string()),
        ];
        for (name, contents) in cases {
            let path = temp_path(name);
            std::fs::write(&path, contents).expect("write invalid OBJ");
            assert!(inspect_obj_metadata(&path).is_err(), "{name} was accepted");
            assert!(
                render_obj_preview_png(&path, 128).is_err(),
                "{name} rendered"
            );
            std::fs::remove_file(path).expect("remove fixture");
        }
        let nul = temp_path("nul.obj");
        std::fs::write(&nul, b"v 0 0 0\0\nv 1 0 0\nv 0 1 1\nf 1 2 3\n").expect("write NUL");
        assert!(render_obj_preview_png(&nul, 128).is_err());
        std::fs::remove_file(nul).expect("remove NUL");
    }

    #[test]
    fn material_references_are_inert_and_do_not_require_a_sibling_file() {
        let path = temp_path("external-material.obj");
        std::fs::write(
            &path,
            "mtllib sentinel-never-read.mtl\nusemtl matte\nv 0 0 0\nv 1 0 0\nv 0 1 1\nf 1 2 3\n",
        )
        .expect("write OBJ with material reference");
        assert!(
            render_obj_preview_png(&path, 128).is_ok(),
            "missing material must stay inert"
        );
        std::fs::remove_file(path).expect("remove OBJ");
    }

    #[test]
    fn snapshot_and_preflight_limits_fail_closed_at_their_boundaries() {
        let exact = temp_path("64mib.obj");
        std::fs::write(&exact, vec![b' '; MAX_OBJ_PREVIEW_BYTES as usize])
            .expect("write exact snapshot boundary");
        assert!(obj_preview_source_generation(&exact).is_some());
        let oversized = temp_path("64mib-plus-one.obj");
        std::fs::write(&oversized, vec![b' '; MAX_OBJ_PREVIEW_BYTES as usize + 1])
            .expect("write oversized snapshot");
        assert!(matches!(
            render_obj_preview_png(&oversized, 128),
            Err(ObjVisualError::InputTooLarge { .. })
        ));
        std::fs::remove_file(exact).expect("remove exact snapshot");
        std::fs::remove_file(oversized).expect("remove oversized snapshot");

        let face = std::iter::repeat_n("1", MAX_OBJ_FACE_ARITY + 1)
            .collect::<Vec<_>>()
            .join(" ");
        let path = temp_path("large-face.obj");
        std::fs::write(&path, format!("v 0 0 0\nv 1 0 0\nv 0 1 1\nf {face}\n"))
            .expect("write oversized face");
        assert!(matches!(
            inspect_obj_metadata(&path),
            Err(ObjVisualError::RecordLimit(_))
        ));
        std::fs::remove_file(path).expect("remove face fixture");
    }
}
