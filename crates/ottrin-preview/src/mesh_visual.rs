use image::{ImageEncoder, Rgba, RgbaImage};

pub(crate) const MAX_MESH_DIMENSION: u32 = 1_024;
const MAX_MESH_RASTER_STEPS: u64 = 20 * 1024 * 1024;
const MAX_MESH_ABS_COORDINATE: f32 = 1.0e12;

#[derive(Clone, Copy)]
pub(crate) struct MeshTriangle {
    pub(crate) vertices: [[f32; 3]; 3],
    pub(crate) normal: [f32; 3],
}

pub(crate) struct ParsedMesh {
    pub(crate) triangles: Vec<MeshTriangle>,
    pub(crate) dimensions: [f32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MeshVisualError {
    TriangleLimit,
    UnsafeCoordinates,
    DegenerateTriangle,
    EmptyMesh,
    UnusableBounds,
    RasterWorkLimit,
    Encode,
}

pub(crate) struct MeshBuilder {
    triangles: Vec<MeshTriangle>,
    max_triangles: usize,
    min: [f32; 3],
    max: [f32; 3],
}

impl MeshBuilder {
    pub(crate) fn new(max_triangles: usize) -> Self {
        Self {
            triangles: Vec::new(),
            max_triangles,
            min: [f32::INFINITY; 3],
            max: [f32::NEG_INFINITY; 3],
        }
    }

    pub(crate) fn add_triangle(
        &mut self,
        normal: [f32; 3],
        vertices: [[f32; 3]; 3],
    ) -> Result<(), MeshVisualError> {
        if self.triangles.len() >= self.max_triangles {
            return Err(MeshVisualError::TriangleLimit);
        }
        if !coordinates_are_safe(normal) {
            return Err(MeshVisualError::UnsafeCoordinates);
        }
        for vertex in vertices {
            if !coordinates_are_safe(vertex) {
                return Err(MeshVisualError::UnsafeCoordinates);
            }
            for (axis, coordinate) in vertex.into_iter().enumerate() {
                self.min[axis] = self.min[axis].min(coordinate);
                self.max[axis] = self.max[axis].max(coordinate);
            }
        }
        if triangle_area_squared(vertices) <= f32::EPSILON {
            return Err(MeshVisualError::DegenerateTriangle);
        }
        self.triangles.push(MeshTriangle { vertices, normal });
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<ParsedMesh, MeshVisualError> {
        if self.triangles.is_empty() {
            return Err(MeshVisualError::EmptyMesh);
        }
        let dimensions = [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ];
        if dimensions
            .iter()
            .any(|dimension| !dimension.is_finite() || *dimension <= 0.0)
        {
            return Err(MeshVisualError::UnusableBounds);
        }
        Ok(ParsedMesh {
            triangles: self.triangles,
            dimensions,
        })
    }
}

pub(crate) fn render_mesh_preview_png(
    mesh: &ParsedMesh,
    target_dim: u32,
) -> Result<Vec<u8>, MeshVisualError> {
    let dimension = target_dim.clamp(64, MAX_MESH_DIMENSION);
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
        return Err(MeshVisualError::UnusableBounds);
    }
    let drawable = f64::from(dimension.saturating_sub(margin.saturating_mul(2)).max(1));
    let scale = (drawable / width).min(drawable / height);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(MeshVisualError::UnusableBounds);
    }
    let mut raster_steps = 0_u64;
    for (triangle, points) in mesh.triangles.iter().zip(projected) {
        let screen_point = |(u, v, _): (f64, f64, f64)| -> Result<(i32, i32), MeshVisualError> {
            let x = f64::from(margin) + (u - min_u) * scale;
            let y = f64::from(margin) + (max_v - v) * scale;
            if !x.is_finite() || !y.is_finite() {
                return Err(MeshVisualError::UnsafeCoordinates);
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
        .map_err(|_| MeshVisualError::Encode)?;
    Ok(png)
}

fn coordinates_are_safe(values: [f32; 3]) -> bool {
    values
        .into_iter()
        .all(|value| value.is_finite() && value.abs() <= MAX_MESH_ABS_COORDINATE)
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
) -> Result<(), MeshVisualError> {
    let (mut x0, mut y0) = start;
    let (x1, y1) = end;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        *raster_steps = raster_steps.saturating_add(1);
        if *raster_steps > MAX_MESH_RASTER_STEPS {
            return Err(MeshVisualError::RasterWorkLimit);
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
