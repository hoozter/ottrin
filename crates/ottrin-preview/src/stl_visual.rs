use crate::mesh_visual::{MeshBuilder, MeshVisualError, ParsedMesh, render_mesh_preview_png};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read};
use std::path::Path;

pub const MAX_STL_PREVIEW_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_STL_TRIANGLES: usize = 200_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StlPreviewSourceGeneration(pub(crate) [u8; 32]);

impl StlPreviewSourceGeneration {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StlMetadata {
    pub triangle_count: usize,
    pub dimensions: [f32; 3],
}

#[derive(Debug, thiserror::Error)]
pub enum StlVisualError {
    #[error("STL visual previews support only .stl files")]
    UnsupportedFormat,
    #[error("could not inspect STL input: {0}")]
    InputMetadata(String),
    #[error("STL input is too large for a bounded preview ({actual} bytes; limit {limit})")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("could not read STL input: {0}")]
    Read(String),
    #[error("STL input changed while its bounded snapshot was read")]
    InputChanged,
    #[error("STL is neither a complete ASCII mesh nor a length-consistent binary mesh")]
    AmbiguousFormat,
    #[error("could not parse STL mesh: {0}")]
    Parse(String),
    #[error("STL has more than {limit} triangles")]
    TriangleLimit { limit: usize },
    #[error("STL has no triangles")]
    EmptyMesh,
    #[error("STL contains non-finite or unsafe coordinates")]
    UnsafeCoordinates,
    #[error("STL contains a degenerate triangle")]
    DegenerateTriangle,
    #[error("STL has unusable or zero-size global bounds")]
    UnusableBounds,
    #[error("STL mesh would exceed bounded raster work")]
    RasterWorkLimit,
    #[error("could not encode STL preview PNG: {0}")]
    Encode(String),
}

#[derive(Debug)]
struct StlSnapshot {
    bytes: Vec<u8>,
    generation: StlPreviewSourceGeneration,
}

enum StlFormat {
    Ascii,
    Binary { triangle_count: usize },
}

impl From<MeshVisualError> for StlVisualError {
    fn from(error: MeshVisualError) -> Self {
        match error {
            MeshVisualError::TriangleLimit => Self::TriangleLimit {
                limit: MAX_STL_TRIANGLES,
            },
            MeshVisualError::UnsafeCoordinates => Self::UnsafeCoordinates,
            MeshVisualError::DegenerateTriangle => Self::DegenerateTriangle,
            MeshVisualError::EmptyMesh => Self::EmptyMesh,
            MeshVisualError::UnusableBounds => Self::UnusableBounds,
            MeshVisualError::RasterWorkLimit => Self::RasterWorkLimit,
            MeshVisualError::Encode => Self::Encode("shared mesh PNG encoder failed".to_string()),
        }
    }
}

/// Reads one immutable bounded snapshot and returns its content identity.
pub fn stl_preview_source_generation(path: &Path) -> Option<StlPreviewSourceGeneration> {
    read_stl_snapshot(path)
        .ok()
        .map(|snapshot| snapshot.generation)
}

pub fn inspect_stl_metadata(path: &Path) -> Result<StlMetadata, StlVisualError> {
    let mesh = parse_stl_snapshot(path)?;
    Ok(StlMetadata {
        triangle_count: mesh.triangles.len(),
        dimensions: mesh.dimensions,
    })
}

/// Renders a deterministic wireframe PNG only after the entire mesh is parsed
/// and validated from one bounded in-memory snapshot.
pub fn render_stl_preview_png(path: &Path, target_dim: u32) -> Result<Vec<u8>, StlVisualError> {
    render_mesh_preview_png(&parse_stl_snapshot(path)?, target_dim).map_err(Into::into)
}

fn read_stl_snapshot(path: &Path) -> Result<StlSnapshot, StlVisualError> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| StlVisualError::InputMetadata(error.to_string()))?;
    let input_size = file
        .metadata()
        .map_err(|error| StlVisualError::InputMetadata(error.to_string()))?
        .len();
    if input_size > MAX_STL_PREVIEW_BYTES {
        return Err(StlVisualError::InputTooLarge {
            actual: input_size,
            limit: MAX_STL_PREVIEW_BYTES,
        });
    }
    let capacity = usize::try_from(input_size).map_err(|_| StlVisualError::InputTooLarge {
        actual: input_size,
        limit: MAX_STL_PREVIEW_BYTES,
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    file.by_ref()
        .take(MAX_STL_PREVIEW_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| StlVisualError::Read(error.to_string()))?;
    let final_size = file
        .metadata()
        .map_err(|error| StlVisualError::Read(error.to_string()))?
        .len();
    if bytes.len() as u64 > MAX_STL_PREVIEW_BYTES {
        return Err(StlVisualError::InputTooLarge {
            actual: bytes.len() as u64,
            limit: MAX_STL_PREVIEW_BYTES,
        });
    }
    if bytes.len() as u64 != input_size || final_size != input_size {
        return Err(StlVisualError::InputChanged);
    }
    Ok(StlSnapshot {
        generation: StlPreviewSourceGeneration(Sha256::digest(&bytes).into()),
        bytes,
    })
}

fn parse_stl_snapshot(path: &Path) -> Result<ParsedMesh, StlVisualError> {
    if !path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("stl"))
    {
        return Err(StlVisualError::UnsupportedFormat);
    }
    let snapshot = read_stl_snapshot(path)?;
    match validate_format(&snapshot.bytes)? {
        StlFormat::Binary { triangle_count } => parse_binary_mesh(&snapshot.bytes, triangle_count),
        StlFormat::Ascii => parse_ascii_mesh(&snapshot.bytes),
    }
}

fn parse_binary_mesh(bytes: &[u8], triangle_count: usize) -> Result<ParsedMesh, StlVisualError> {
    if triangle_count > MAX_STL_TRIANGLES {
        return Err(StlVisualError::TriangleLimit {
            limit: MAX_STL_TRIANGLES,
        });
    }
    let mut builder = MeshBuilder::new(MAX_STL_TRIANGLES);
    let mut offset = 84_usize;
    for _ in 0..triangle_count {
        let end = offset
            .checked_add(50)
            .ok_or(StlVisualError::AmbiguousFormat)?;
        let triangle = bytes
            .get(offset..end)
            .ok_or(StlVisualError::AmbiguousFormat)?;
        builder.add_triangle(
            binary_f32x3(triangle, 0)?,
            [
                binary_f32x3(triangle, 12)?,
                binary_f32x3(triangle, 24)?,
                binary_f32x3(triangle, 36)?,
            ],
        )?;
        offset = end;
    }
    builder.finish().map_err(Into::into)
}

fn parse_ascii_mesh(bytes: &[u8]) -> Result<ParsedMesh, StlVisualError> {
    let mut cursor = Cursor::new(bytes);
    let reader = stl_io::create_stl_reader(&mut cursor)
        .map_err(|error| StlVisualError::Parse(error.to_string()))?;
    let mut builder = MeshBuilder::new(MAX_STL_TRIANGLES);
    for result in reader {
        let triangle = result.map_err(|error| StlVisualError::Parse(error.to_string()))?;
        builder.add_triangle(triangle.normal.0, triangle.vertices.map(|vertex| vertex.0))?;
    }
    builder.finish().map_err(Into::into)
}

fn binary_f32x3(bytes: &[u8], offset: usize) -> Result<[f32; 3], StlVisualError> {
    Ok([
        binary_f32(bytes, offset)?,
        binary_f32(bytes, offset + 4)?,
        binary_f32(bytes, offset + 8)?,
    ])
}

fn binary_f32(bytes: &[u8], offset: usize) -> Result<f32, StlVisualError> {
    let end = offset
        .checked_add(4)
        .ok_or(StlVisualError::AmbiguousFormat)?;
    let value = bytes
        .get(offset..end)
        .ok_or(StlVisualError::AmbiguousFormat)?;
    let value: [u8; 4] = value
        .try_into()
        .map_err(|_| StlVisualError::AmbiguousFormat)?;
    Ok(f32::from_le_bytes(value))
}

fn validate_format(bytes: &[u8]) -> Result<StlFormat, StlVisualError> {
    let binary_triangle_count = bytes
        .get(80..84)
        .and_then(|count| <[u8; 4]>::try_from(count).ok())
        .map(u32::from_le_bytes)
        .and_then(|count| usize::try_from(count).ok());
    if let Some(triangle_count) = binary_triangle_count
        && bytes.len() >= 84
        && triangle_count
            .checked_mul(50)
            .and_then(|triangle_bytes| 84_usize.checked_add(triangle_bytes))
            == Some(bytes.len())
    {
        return Ok(StlFormat::Binary { triangle_count });
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Err(StlVisualError::AmbiguousFormat);
    };
    if text
        .as_bytes()
        .iter()
        .any(|byte| !byte.is_ascii() || (*byte < b' ' && !matches!(*byte, b'\n' | b'\r' | b'\t')))
        || !has_complete_ascii_envelope(text)
    {
        return Err(StlVisualError::AmbiguousFormat);
    }
    Ok(StlFormat::Ascii)
}

fn has_complete_ascii_envelope(text: &str) -> bool {
    let mut records = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let Some(first) = records.next() else {
        return false;
    };
    if !is_ascii_stl_keyword(first, "solid") {
        return false;
    }
    let mut saw_terminator = false;
    for record in records {
        if saw_terminator || is_ascii_stl_keyword(record, "solid") {
            return false;
        }
        if is_ascii_stl_keyword(record, "endsolid") {
            saw_terminator = true;
        }
    }
    saw_terminator
}

fn is_ascii_stl_keyword(line: &str, keyword: &str) -> bool {
    line.strip_prefix(keyword).is_some_and(|suffix| {
        suffix.is_empty()
            || suffix
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_whitespace())
    })
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
        std::env::temp_dir().join(format!("ottrin-stl-{nonce}-{name}"))
    }

    fn triangle_bytes(normal: [f32; 3], vertices: [[f32; 3]; 3]) -> Vec<u8> {
        normal
            .into_iter()
            .chain(vertices.into_iter().flatten())
            .flat_map(f32::to_le_bytes)
            .chain([0, 0])
            .collect()
    }

    fn binary_mesh(triangle_count: u32, triangle: Vec<u8>) -> Vec<u8> {
        let mut bytes = vec![0_u8; 80];
        bytes.extend_from_slice(&triangle_count.to_le_bytes());
        for _ in 0..triangle_count {
            bytes.extend_from_slice(&triangle);
        }
        bytes
    }

    fn tetra_triangle() -> Vec<u8> {
        triangle_bytes(
            [0.0, 0.0, 1.0],
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 1.0]],
        )
    }

    #[test]
    fn exact_length_binary_stl_with_a_solid_header_is_rendered_as_binary() {
        let path = temp_path("solid-binary-header.stl");
        let mut bytes = binary_mesh(1, tetra_triangle());
        bytes[..80].copy_from_slice(&{
            let mut header = [0_u8; 80];
            header[..13].copy_from_slice(b"solid binary\n");
            header
        });
        std::fs::write(&path, bytes).expect("write binary STL with solid header");

        assert_eq!(
            inspect_stl_metadata(&path)
                .expect("inspect exact-length binary STL")
                .triangle_count,
            1
        );
        assert!(
            !render_stl_preview_png(&path, 128)
                .expect("render exact-length binary STL")
                .is_empty()
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ascii_stl_rejects_nonblank_data_after_its_first_terminator() {
        let path = temp_path("ascii-trailing-data.stl");
        std::fs::write(
            &path,
            b"solid tetra\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 1\nendloop\nendfacet\nendsolid tetra\nTHIS MUST NOT BE IGNORED\nendsolid tetra\n",
        )
        .expect("write ASCII STL with trailing data");

        assert!(inspect_stl_metadata(&path).is_err());
        assert!(render_stl_preview_png(&path, 128).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn parser_enforces_binary_structure_and_ascii_completeness() {
        let cases: &[(&str, Vec<u8>)] = &[
            (
                "truncated.stl",
                binary_mesh(1, tetra_triangle())[..90].to_vec(),
            ),
            ("mismatch.stl", {
                let mut bytes = binary_mesh(1, tetra_triangle());
                bytes[80..84].copy_from_slice(&2_u32.to_le_bytes());
                bytes
            }),
            ("trailing.stl", {
                let mut bytes = binary_mesh(1, tetra_triangle());
                bytes.push(0);
                bytes
            }),
            (
                "malformed.stl",
                b"solid bad\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nendsolid bad\n".to_vec(),
            ),
        ];
        for (name, bytes) in cases {
            let path = temp_path(name);
            std::fs::write(&path, bytes).expect("write malformed STL");
            assert!(
                render_stl_preview_png(&path, 128).is_err(),
                "{name} was accepted"
            );
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn parser_rejects_nonfinite_extreme_degenerate_and_flat_meshes() {
        let cases = [
            (
                "nan.stl",
                triangle_bytes(
                    [f32::NAN, 0.0, 1.0],
                    [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 1.0]],
                ),
            ),
            (
                "extreme.stl",
                triangle_bytes(
                    [0.0, 0.0, 1.0],
                    [[0.0, 0.0, 0.0], [1.0e20, 0.0, 0.0], [0.0, 1.0, 1.0]],
                ),
            ),
            (
                "degenerate.stl",
                triangle_bytes(
                    [0.0, 0.0, 1.0],
                    [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [1.0, 1.0, 1.0]],
                ),
            ),
            (
                "flat.stl",
                triangle_bytes(
                    [0.0, 0.0, 1.0],
                    [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                ),
            ),
        ];
        for (name, triangle) in cases {
            let path = temp_path(name);
            std::fs::write(&path, binary_mesh(1, triangle)).expect("write unsafe STL");
            assert!(inspect_stl_metadata(&path).is_err(), "{name} was accepted");
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn target_dimensions_are_clamped_to_the_stl_renderer_policy() {
        let path = temp_path("dimensions.stl");
        std::fs::write(&path, binary_mesh(1, tetra_triangle())).expect("write STL");
        for (requested, expected) in [(0, 64), (1, 64), (1024, 1024), (1025, 1024)] {
            let png = render_stl_preview_png(&path, requested).expect("render STL");
            assert_eq!(
                image::load_from_memory(&png)
                    .expect("decode PNG")
                    .dimensions(),
                (expected, expected)
            );
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn triangle_and_snapshot_limits_fail_closed_at_the_exact_boundaries() {
        let triangle = tetra_triangle();
        let within = temp_path("200000.stl");
        std::fs::write(
            &within,
            binary_mesh(MAX_STL_TRIANGLES as u32, triangle.clone()),
        )
        .expect("write bounded mesh");
        assert_eq!(
            inspect_stl_metadata(&within)
                .expect("within limit")
                .triangle_count,
            MAX_STL_TRIANGLES
        );

        let over = temp_path("200001.stl");
        std::fs::write(&over, binary_mesh(MAX_STL_TRIANGLES as u32 + 1, triangle))
            .expect("write oversized mesh");
        assert!(matches!(
            inspect_stl_metadata(&over),
            Err(StlVisualError::TriangleLimit { .. })
        ));

        let exact = temp_path("64mib.stl");
        std::fs::write(&exact, vec![0_u8; MAX_STL_PREVIEW_BYTES as usize])
            .expect("write exact snapshot bound");
        assert!(stl_preview_source_generation(&exact).is_some());
        let over_snapshot = temp_path("64mib-plus-one.stl");
        std::fs::write(
            &over_snapshot,
            vec![0_u8; MAX_STL_PREVIEW_BYTES as usize + 1],
        )
        .expect("write oversized snapshot");
        assert!(matches!(
            render_stl_preview_png(&over_snapshot, 128),
            Err(StlVisualError::InputTooLarge { .. })
        ));
        for path in [within, over, exact, over_snapshot] {
            let _ = std::fs::remove_file(path);
        }
    }
}
