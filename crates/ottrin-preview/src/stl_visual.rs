use image::{ImageEncoder, Rgba, RgbaImage};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read};
use std::path::Path;

pub const MAX_STL_PREVIEW_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_STL_TRIANGLES: usize = 200_000;
const MAX_STL_DIMENSION: u32 = 1_024;
const MAX_STL_RASTER_STEPS: u64 = 20 * 1024 * 1024;
const MAX_STL_ABS_COORDINATE: f32 = 1.0e12;

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

#[derive(Clone, Copy)]
struct MeshTriangle {
    vertices: [[f32; 3]; 3],
    normal: [f32; 3],
}

struct ParsedMesh {
    triangles: Vec<MeshTriangle>,
    metadata: StlMetadata,
}

/// Reads one immutable bounded snapshot and returns its content identity.
pub fn stl_preview_source_generation(path: &Path) -> Option<StlPreviewSourceGeneration> {
    read_stl_snapshot(path)
        .ok()
        .map(|snapshot| snapshot.generation)
}

pub fn inspect_stl_metadata(path: &Path) -> Result<StlMetadata, StlVisualError> {
    Ok(parse_stl_snapshot(path)?.metadata)
}

/// Renders a deterministic wireframe PNG only after the entire mesh is parsed
/// and validated from one bounded in-memory snapshot.
pub fn render_stl_preview_png(path: &Path, target_dim: u32) -> Result<Vec<u8>, StlVisualError> {
    let mesh = parse_stl_snapshot(path)?;
    let dimension = target_dim.clamp(64, MAX_STL_DIMENSION);
    let mut image = RgbaImage::from_pixel(dimension, dimension, Rgba([247, 249, 252, 255]));
    let margin = (dimension / 12).clamp(4, 96);
    let projected = mesh
        .triangles
        .iter()
        .map(|triangle| triangle.vertices.map(project))
        .collect::<Vec<_>>();
    let (min_u, max_u, min_v, max_v) = projected.iter().flatten().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(min_u, max_u, min_v, max_v), point| {
            (
                min_u.min(point.0),
                max_u.max(point.0),
                min_v.min(point.1),
                max_v.max(point.1),
            )
        },
    );
    let width = max_u - min_u;
    let height = max_v - min_v;
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(StlVisualError::UnusableBounds);
    }
    let drawable = f64::from(dimension.saturating_sub(margin.saturating_mul(2)).max(1));
    let scale = (drawable / width).min(drawable / height);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(StlVisualError::UnusableBounds);
    }

    let mut raster_steps = 0_u64;
    for (triangle, points) in mesh.triangles.iter().zip(projected) {
        let screen_point = |(u, v, _depth): (f64, f64, f64)| {
            let x = f64::from(margin) + (u - min_u) * scale;
            let y = f64::from(margin) + (max_v - v) * scale;
            if !x.is_finite() || !y.is_finite() {
                return Err(StlVisualError::UnsafeCoordinates);
            }
            Ok((x.round() as i32, y.round() as i32))
        };
        let [first, second, third] = points;
        let screen = [
            screen_point(first)?,
            screen_point(second)?,
            screen_point(third)?,
        ];
        let brightness = ((triangle.normal[2].abs() * 90.0) as u8).saturating_add(85);
        let color = [40, brightness, 170, 255];
        draw_line(&mut image, screen[0], screen[1], color, &mut raster_steps)?;
        draw_line(&mut image, screen[1], screen[2], color, &mut raster_steps)?;
        draw_line(&mut image, screen[2], screen[0], color, &mut raster_steps)?;
    }

    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            dimension,
            dimension,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| StlVisualError::Encode(error.to_string()))?;
    Ok(png)
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
    validate_format(&snapshot.bytes)?;
    let mut cursor = Cursor::new(snapshot.bytes.as_slice());
    let reader = stl_io::create_stl_reader(&mut cursor)
        .map_err(|error| StlVisualError::Parse(error.to_string()))?;
    let mut triangles = Vec::new();
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for result in reader {
        if triangles.len() >= MAX_STL_TRIANGLES {
            return Err(StlVisualError::TriangleLimit {
                limit: MAX_STL_TRIANGLES,
            });
        }
        let triangle = result.map_err(|error| StlVisualError::Parse(error.to_string()))?;
        let normal = triangle.normal.0;
        if !coordinates_are_safe(normal) {
            return Err(StlVisualError::UnsafeCoordinates);
        }
        let vertices = triangle.vertices.map(|vertex| vertex.0);
        for vertex in vertices {
            if !coordinates_are_safe(vertex) {
                return Err(StlVisualError::UnsafeCoordinates);
            }
            for axis in 0..3 {
                min[axis] = min[axis].min(vertex[axis]);
                max[axis] = max[axis].max(vertex[axis]);
            }
        }
        if triangle_area_squared(vertices) <= f32::EPSILON {
            return Err(StlVisualError::DegenerateTriangle);
        }
        triangles.push(MeshTriangle { vertices, normal });
    }
    if triangles.is_empty() {
        return Err(StlVisualError::EmptyMesh);
    }
    let dimensions = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    if dimensions
        .iter()
        .any(|dimension| !dimension.is_finite() || *dimension <= 0.0)
    {
        return Err(StlVisualError::UnusableBounds);
    }
    Ok(ParsedMesh {
        metadata: StlMetadata {
            triangle_count: triangles.len(),
            dimensions,
        },
        triangles,
    })
}

fn validate_format(bytes: &[u8]) -> Result<(), StlVisualError> {
    let binary_length_matches = bytes.len() >= 84
        && bytes
            .get(80..84)
            .and_then(|count| <[u8; 4]>::try_from(count).ok())
            .and_then(|count| {
                84_usize.checked_add(
                    usize::try_from(u32::from_le_bytes(count))
                        .ok()?
                        .checked_mul(50)?,
                )
            })
            == Some(bytes.len());
    if binary_length_matches {
        return Ok(());
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Err(StlVisualError::AmbiguousFormat);
    };
    if !text.starts_with("solid ")
        || text.as_bytes().iter().any(|byte| {
            !byte.is_ascii() || (*byte < b' ' && !matches!(*byte, b'\n' | b'\r' | b'\t'))
        })
        || text
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .is_none_or(|line| line.split_ascii_whitespace().next() != Some("endsolid"))
    {
        return Err(StlVisualError::AmbiguousFormat);
    }
    Ok(())
}

fn coordinates_are_safe(values: [f32; 3]) -> bool {
    values
        .into_iter()
        .all(|value| value.is_finite() && value.abs() <= MAX_STL_ABS_COORDINATE)
}

fn triangle_area_squared(vertices: [[f32; 3]; 3]) -> f32 {
    let a = subtract(vertices[1], vertices[0]);
    let b = subtract(vertices[2], vertices[0]);
    let cross = cross(a, b);
    dot(cross, cross)
}

fn project(point: [f32; 3]) -> (f64, f64, f64) {
    let [x, y, z] = point.map(f64::from);
    (
        (x - y) * std::f64::consts::FRAC_1_SQRT_2,
        (x + y) / 6.0_f64.sqrt() - z * (2.0_f64 / 3.0).sqrt(),
        x + y + z,
    )
}

fn draw_line(
    image: &mut RgbaImage,
    start: (i32, i32),
    end: (i32, i32),
    color: [u8; 4],
    raster_steps: &mut u64,
) -> Result<(), StlVisualError> {
    let (mut x0, mut y0) = start;
    let (x1, y1) = end;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        *raster_steps = raster_steps.saturating_add(1);
        if *raster_steps > MAX_STL_RASTER_STEPS {
            return Err(StlVisualError::RasterWorkLimit);
        }
        if x0 >= 0 && y0 >= 0 && (x0 as u32) < image.width() && (y0 as u32) < image.height() {
            image.put_pixel(x0 as u32, y0 as u32, Rgba(color));
        }
        if x0 == x1 && y0 == y1 {
            return Ok(());
        }
        let double_error = error.saturating_mul(2);
        if double_error >= dy {
            error += dy;
            x0 += sx;
        }
        if double_error <= dx {
            error += dx;
            y0 += sy;
        }
    }
}

fn subtract(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn cross(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
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
