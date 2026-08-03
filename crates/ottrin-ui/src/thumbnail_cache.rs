#[cfg(not(target_os = "windows"))]
use ottrin_core::default_home_dir;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};

const FOLDER_COLLAGE_ENTRY_LIMIT: usize = 64;
const FOLDER_COLLAGE_CHILD_LIMIT: usize = 4;
const FOLDER_COLLAGE_MAX_CHILD_BYTES: u64 = 16 * 1024 * 1024;
const FOLDER_COLLAGE_MAX_CHILD_PIXELS: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) enum ImagePreviewCacheEntry {
    Ready(Vec<u8>),
    Unavailable,
}

impl ImagePreviewCacheEntry {
    pub(crate) fn from_response(bytes: Option<Vec<u8>>) -> Self {
        match bytes {
            Some(bytes) => Self::Ready(bytes),
            None => Self::Unavailable,
        }
    }

    pub(crate) fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Ready(bytes) => Some(bytes),
            Self::Unavailable => None,
        }
    }
}

pub(crate) fn preview_cache_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut h = default_home_dir();
            h.push(".cache");
            h
        });
    let mut p = base;
    p.push("ottrin");
    p.push("preview_cache");
    Some(p)
}

pub(crate) fn preview_cache_key_for_page(
    path: &Path,
    target_dim: u32,
    page_index: usize,
) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    if let Ok(meta) = std::fs::metadata(path) {
        meta.len().hash(&mut hasher);
        if let Ok(mtime) = meta.modified()
            && let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH)
        {
            dur.hash(&mut hasher);
        }
    }
    if path.is_dir() {
        hash_folder_collage_inputs(path, &mut hasher);
    }
    target_dim.hash(&mut hasher);
    page_index.hash(&mut hasher);
    format!("{:016x}_{}.png", hasher.finish(), target_dim)
}

fn hash_folder_collage_inputs(path: &Path, hasher: &mut impl Hasher) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten().take(FOLDER_COLLAGE_ENTRY_LIMIT) {
        entry.file_name().hash(hasher);
        if let Ok(file_type) = entry.file_type() {
            file_type.is_file().hash(hasher);
            file_type.is_symlink().hash(hasher);
        }
        if let Ok(meta) = entry.metadata() {
            meta.len().hash(hasher);
            if let Ok(modified) = meta.modified()
                && let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH)
            {
                duration.hash(hasher);
            }
        }
    }
}

fn preview_cache_path(cache_key: &str) -> Option<PathBuf> {
    let mut dir = preview_cache_dir()?;
    dir.push(cache_key);
    Some(dir)
}

pub(crate) fn load_cached_thumbnail(path: &Path, cache_key: &str) -> Option<Vec<u8>> {
    let cache_path = preview_cache_path(cache_key)?;
    let bytes = std::fs::read(&cache_path).ok()?;
    if ottrin_preview::is_svg_path(path) && !is_png_bytes(&bytes) {
        let _ = std::fs::remove_file(cache_path);
        return None;
    }
    Some(bytes)
}

pub(crate) fn persist_cached_thumbnail(cache_key: &str, bytes: &[u8], max_bytes: u64) {
    if max_bytes == 0 {
        return;
    }
    let Some(path) = preview_cache_path(cache_key) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, bytes);
    prune_preview_cache_dir(max_bytes);
}

pub(crate) fn build_thumbnail_bytes(path: &Path, target_dim: u32) -> Option<Vec<u8>> {
    build_thumbnail_bytes_with_visual_renderer(path, target_dim, |path, target_dim| {
        match preview_visual_renderer(path) {
            ottrin_preview::PreviewVisualRenderer::Pdf => {
                match ottrin_preview::render_pdf_first_page_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "PDF thumbnail render failed for {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            }
            ottrin_preview::PreviewVisualRenderer::Office => {
                match ottrin_preview::render_office_content_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "Office thumbnail render failed for {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            }
            ottrin_preview::PreviewVisualRenderer::TextCard => {
                match ottrin_preview::render_text_content_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "Text thumbnail render failed for {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            }
            ottrin_preview::PreviewVisualRenderer::Image => {
                match ottrin_preview::render_svg_content_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(ottrin_preview::SvgVisualError::UnsupportedFormat) => None,
                    Err(error) => {
                        eprintln!(
                            "SVG thumbnail render failed for {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            }
            _ => None,
        }
    })
}

pub(crate) fn fallback_preview_bytes(path: &Path, generated: Option<Vec<u8>>) -> Option<Vec<u8>> {
    if generated.is_some() {
        return generated;
    }
    if ottrin_preview::is_svg_path(path)
        || !ottrin_preview::preview_profile(path).raw_fallback_allowed()
    {
        return None;
    }
    std::fs::read(path).ok()
}

pub(crate) fn thumbnail_bytes_from_cache_or_build(
    path: &Path,
    target_dim: u32,
    cached: Option<Vec<u8>>,
) -> Option<(Vec<u8>, bool)> {
    if let Some(bytes) = cached.filter(|bytes| cached_thumbnail_is_safe_for_path(path, bytes)) {
        return Some((bytes, false));
    }
    if let Some(bytes) = build_thumbnail_bytes(path, target_dim) {
        return Some((bytes, true));
    }
    fallback_preview_bytes(path, None).map(|bytes| (bytes, false))
}

fn cached_thumbnail_is_safe_for_path(path: &Path, bytes: &[u8]) -> bool {
    !ottrin_preview::is_svg_path(path) || is_png_bytes(bytes)
}

fn is_png_bytes(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

pub(crate) fn full_preview_bytes_at_page(
    path: &Path,
    target_dim: u32,
    page_index: usize,
) -> Option<Vec<u8>> {
    if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::Pdf {
        return match ottrin_preview::render_pdf_page_png(path, page_index, target_dim) {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                eprintln!("PDF preview render failed for {}: {error}", path.display());
                None
            }
        };
    }
    if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::Image {
        match ottrin_preview::render_svg_content_preview_png(path, target_dim) {
            Ok(bytes) => return Some(bytes),
            Err(ottrin_preview::SvgVisualError::UnsupportedFormat) => {}
            Err(_) => return None,
        }
    }
    full_preview_bytes_with_visual_renderer(path, target_dim, |path, target_dim| {
        if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::Office {
            match ottrin_preview::render_office_content_preview_png(path, target_dim) {
                Ok(bytes) => Some(bytes),
                Err(error) => {
                    eprintln!(
                        "Office preview render failed for {}: {error}",
                        path.display()
                    );
                    None
                }
            }
        } else {
            match ottrin_preview::render_text_content_preview_png(path, target_dim) {
                Ok(bytes) => Some(bytes),
                Err(error) => {
                    eprintln!("Text preview render failed for {}: {error}", path.display());
                    None
                }
            }
        }
    })
}

fn full_preview_bytes_with_visual_renderer(
    path: &Path,
    target_dim: u32,
    render_visual: impl FnOnce(&Path, u32) -> Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    if matches!(
        preview_visual_renderer(path),
        ottrin_preview::PreviewVisualRenderer::Pdf
            | ottrin_preview::PreviewVisualRenderer::Office
            | ottrin_preview::PreviewVisualRenderer::TextCard
    ) {
        return render_visual(path, target_dim);
    }
    std::fs::read(path).ok()
}

fn build_thumbnail_bytes_with_visual_renderer(
    path: &Path,
    target_dim: u32,
    render_visual: impl FnOnce(&Path, u32) -> Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    if path.is_dir() {
        return render_folder_collage_png(path, target_dim);
    }
    match preview_visual_renderer(path) {
        ottrin_preview::PreviewVisualRenderer::Pdf
        | ottrin_preview::PreviewVisualRenderer::Office
        | ottrin_preview::PreviewVisualRenderer::TextCard => {
            return render_visual(path, target_dim);
        }
        ottrin_preview::PreviewVisualRenderer::Image => {
            if let Some(bytes) = render_visual(path, target_dim) {
                return Some(bytes);
            }
        }
        ottrin_preview::PreviewVisualRenderer::None => return None,
    }

    let img = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let thumb = img.thumbnail(target_dim, target_dim);
    let mut bytes = Vec::new();
    thumb
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .ok()?;
    Some(bytes)
}

fn preview_visual_renderer(path: &Path) -> ottrin_preview::PreviewVisualRenderer {
    ottrin_preview::preview_profile(path).visual_renderer()
}

fn render_folder_collage_png(path: &Path, target_dim: u32) -> Option<Vec<u8>> {
    use image::{ImageEncoder, Rgba, RgbaImage};

    let mut children = std::fs::read_dir(path)
        .ok()?
        .take(FOLDER_COLLAGE_ENTRY_LIMIT)
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            (!file_type.is_symlink()
                && file_type.is_file()
                && supported_collage_child(&entry.path()))
            .then_some(entry.path())
        })
        .collect::<Vec<_>>();
    children.sort();
    children.truncate(FOLDER_COLLAGE_CHILD_LIMIT);
    if children.is_empty() {
        return None;
    }
    let dimension = target_dim.clamp(64, 2_048);
    let gap = (dimension / 32).max(2);
    let tile = (dimension.saturating_sub(gap * 3) / 2).max(1);
    let mut collage = RgbaImage::from_pixel(dimension, dimension, Rgba([226, 230, 236, 255]));
    let mut rendered = 0usize;
    for (index, child) in children.iter().enumerate() {
        let Ok(meta) = std::fs::metadata(child) else {
            continue;
        };
        if meta.len() > FOLDER_COLLAGE_MAX_CHILD_BYTES {
            continue;
        }
        let Ok(reader) = image::ImageReader::open(child) else {
            continue;
        };
        let Ok(reader) = reader.with_guessed_format() else {
            continue;
        };
        let Ok((width, height)) = reader.into_dimensions() else {
            continue;
        };
        if u64::from(width) * u64::from(height) > FOLDER_COLLAGE_MAX_CHILD_PIXELS {
            continue;
        }
        let Ok(reader) = image::ImageReader::open(child) else {
            continue;
        };
        let Ok(reader) = reader.with_guessed_format() else {
            continue;
        };
        let Ok(image) = reader.decode() else {
            continue;
        };
        let image = image.to_rgba8();
        let thumb = image::imageops::thumbnail(&image, tile, tile);
        let x = gap + (index as u32 % 2) * (tile + gap) + (tile - thumb.width()) / 2;
        let y = gap + (index as u32 / 2) * (tile + gap) + (tile - thumb.height()) / 2;
        image::imageops::overlay(&mut collage, &thumb, i64::from(x), i64::from(y));
        rendered += 1;
    }
    if rendered == 0 {
        return None;
    }
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            collage.as_raw(),
            dimension,
            dimension,
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
    Some(bytes)
}

fn supported_collage_child(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff")
    )
}

pub(crate) fn preview_cache_stats() -> Option<(usize, u64)> {
    let dir = preview_cache_dir()?;
    let mut count = 0usize;
    let mut total = 0u64;
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata()
            && meta.is_file()
        {
            count += 1;
            total += meta.len();
        }
    }
    Some((count, total))
}

pub(crate) fn prune_preview_cache_dir(max_bytes: u64) {
    if max_bytes == 0 {
        let _ = clear_preview_cache_dir();
        return;
    }
    let Some(dir) = preview_cache_dir() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut files = Vec::new();
    let mut total = 0_u64;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        total = total.saturating_add(meta.len());
        files.push((path, meta.len(), modified));
    }
    if total <= max_bytes {
        return;
    }
    files.sort_by_key(|(_, _, modified)| *modified);
    for (path, len, _) in files {
        if total <= max_bytes {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

pub(crate) fn clear_preview_cache_dir() -> u64 {
    let Some(dir) = preview_cache_dir() else {
        return 0;
    };
    let mut freed = 0_u64;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if std::fs::remove_file(path).is_ok() {
                freed = freed.saturating_add(bytes);
            }
        }
    }
    freed
}

pub fn clear_preview_cache_storage() -> u64 {
    clear_preview_cache_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    #[test]
    fn pdf_thumbnail_uses_visual_renderer_case_insensitively() {
        let path = Path::new("report.PDF");
        let bytes = build_thumbnail_bytes_with_visual_renderer(path, 384, |actual, target| {
            assert_eq!(actual, path);
            assert_eq!(target, 384);
            Some(vec![1, 2, 3])
        });

        assert_eq!(bytes, Some(vec![1, 2, 3]));
    }

    #[test]
    fn failed_preview_response_is_storable_without_exposing_image_bytes() {
        let entry = ImagePreviewCacheEntry::from_response(None);

        assert!(entry.bytes().is_none());
    }

    #[test]
    fn successful_preview_response_exposes_image_bytes() {
        let entry = ImagePreviewCacheEntry::from_response(Some(vec![1, 2, 3]));

        assert_eq!(entry.bytes(), Some(&[1, 2, 3][..]));
    }

    #[test]
    fn pdf_page_cache_keys_do_not_alias_different_pages() {
        let path = Path::new("report.pdf");

        assert_ne!(
            preview_cache_key_for_page(path, 2_048, 0),
            preview_cache_key_for_page(path, 2_048, 1),
            "each rendered PDF page needs its own cache entry"
        );
    }

    #[test]
    fn failed_pdf_render_does_not_fallback_to_raw_document_bytes() {
        let path =
            std::env::temp_dir().join(format!("ottrin-pdf-fallback-{}.PDF", std::process::id()));
        std::fs::write(&path, b"%PDF fixture").expect("write PDF fixture");

        let bytes = fallback_preview_bytes(&path, None);

        assert!(bytes.is_none());
        std::fs::remove_file(path).expect("remove PDF fixture");
    }

    #[test]
    fn full_pdf_preview_uses_bounded_renderer_not_raw_document_bytes() {
        let path = Path::new("report.pdf");
        let bytes = full_preview_bytes_with_visual_renderer(path, 2_048, |actual, target| {
            assert_eq!(actual, path);
            assert_eq!(target, 2_048);
            Some(b"rendered-png".to_vec())
        });

        assert_eq!(bytes, Some(b"rendered-png".to_vec()));
    }

    #[test]
    fn office_preview_uses_visual_renderer_not_raw_package_bytes() {
        let path = Path::new("report.XLSX");
        let bytes = full_preview_bytes_with_visual_renderer(path, 2_048, |actual, target| {
            assert_eq!(actual, path);
            assert_eq!(target, 2_048);
            Some(b"rendered-png".to_vec())
        });

        assert_eq!(bytes, Some(b"rendered-png".to_vec()));
        assert_eq!(
            ottrin_preview::preview_profile(Path::new("workbook.ODS")).visual_renderer(),
            ottrin_preview::PreviewVisualRenderer::Office
        );
    }

    #[test]
    fn text_thumbnail_uses_generated_card_not_image_decoder() {
        let path =
            std::env::temp_dir().join(format!("ottrin-text-thumbnail-{}.rs", std::process::id()));
        std::fs::write(&path, "fn main() {}\n").expect("write text fixture");

        let bytes = build_thumbnail_bytes(&path, 256);

        assert!(bytes.is_some(), "safe text should receive a generated card");
        assert!(image::load_from_memory(&bytes.expect("card")).is_ok());
        std::fs::remove_file(path).expect("remove text fixture");
    }

    #[test]
    fn svg_thumbnail_uses_safe_visual_renderer() {
        let path =
            std::env::temp_dir().join(format!("ottrin-svg-thumb-{}.svg", std::process::id()));
        std::fs::write(
            &path,
            r#"<svg width="12" height="12" xmlns="http://www.w3.org/2000/svg"><rect width='12' height='12' fill='#00ff00'/></svg>"#,
        )
        .expect("write svg fixture");

        assert_eq!(
            ottrin_preview::preview_profile(&path).visual_renderer(),
            ottrin_preview::PreviewVisualRenderer::Image
        );

        let direct = ottrin_preview::render_svg_content_preview_png(&path, 256)
            .expect("direct svg preview render");

        let bytes = build_thumbnail_bytes(&path, 256).expect("svg thumbnail");
        assert_eq!(bytes, direct);
        assert!(image::load_from_memory(&bytes).is_ok());
        let raw = std::fs::read(&path).expect("read source svg");
        assert_ne!(bytes, raw);
        std::fs::remove_file(path).expect("remove svg fixture");
    }

    #[test]
    fn svg_full_preview_uses_visual_renderer_not_raw_file_bytes() {
        let path =
            std::env::temp_dir().join(format!("ottrin-svg-preview-{}.svg", std::process::id()));
        std::fs::write(
            &path,
            r#"<svg width="24" height="24" xmlns="http://www.w3.org/2000/svg"><circle cx='12' cy='12' r='10' fill='#123456'/></svg>"#,
        )
        .expect("write svg fixture");

        let bytes = full_preview_bytes_at_page(&path, 2_048, 0).expect("svg full preview");
        assert!(image::load_from_memory(&bytes).is_ok());
        let raw = std::fs::read(&path).expect("read source svg");
        assert_ne!(bytes, raw);
        std::fs::remove_file(path).expect("remove svg fixture");
    }

    #[test]
    fn unsafe_svg_thumbnail_response_rejects_raw_fallback_and_stale_raw_cache() {
        let blocked = std::env::temp_dir().join(format!(
            "ottrin-blocked-svg-thumbnail-{}.svg",
            std::process::id()
        ));
        std::fs::write(
            &blocked,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="file:///tmp/private.png"/></svg>"#,
        )
        .expect("write blocked SVG fixture");
        let raw_blocked = std::fs::read(&blocked).expect("read blocked SVG fixture");

        assert_eq!(
            thumbnail_bytes_from_cache_or_build(&blocked, 256, Some(raw_blocked)),
            None,
            "a stale raw SVG cache entry must not bypass the safe renderer"
        );

        let oversized = std::env::temp_dir().join(format!(
            "ottrin-oversized-svg-thumbnail-{}.svg",
            std::process::id()
        ));
        std::fs::write(&oversized, vec![b' '; 256 * 1024 + 1])
            .expect("write oversized SVG fixture");

        assert_eq!(
            thumbnail_bytes_from_cache_or_build(&oversized, 256, None),
            None,
            "an oversized SVG must not fall back to raw bytes"
        );

        std::fs::remove_file(blocked).expect("remove blocked SVG fixture");
        std::fs::remove_file(oversized).expect("remove oversized SVG fixture");
    }

    #[test]
    fn text_thumbnail_cache_keys_vary_by_render_size() {
        let path = Path::new("main.rs");

        assert_ne!(
            preview_cache_key_for_page(path, 256, 0),
            preview_cache_key_for_page(path, 640, 0)
        );
    }

    #[test]
    fn folder_thumbnail_is_a_bounded_direct_child_collage() {
        let dir = std::env::temp_dir().join(format!("ottrin-folder-thumb-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("nested")).expect("make fixture");
        let image = image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255]));
        image.save(dir.join("cover.png")).expect("write image");
        image
            .save(dir.join("nested/ignored.png"))
            .expect("write nested image");

        let bytes = build_thumbnail_bytes(&dir, 128).expect("direct child collage");
        assert_eq!(
            image::load_from_memory(&bytes).expect("png").dimensions(),
            (128, 128)
        );

        std::fs::remove_dir_all(dir).expect("remove fixture");
    }

    #[test]
    fn folder_cache_key_changes_when_a_direct_child_changes() {
        let dir = std::env::temp_dir().join(format!("ottrin-folder-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("make fixture");
        let child = dir.join("cover.png");
        std::fs::write(&child, [1_u8; 8]).expect("write child");
        let before = preview_cache_key_for_page(&dir, 128, 0);
        std::thread::sleep(std::time::Duration::from_millis(2));
        std::fs::write(&child, [2_u8; 16]).expect("rewrite child");

        assert_ne!(before, preview_cache_key_for_page(&dir, 128, 0));
        std::fs::remove_dir_all(dir).expect("remove fixture");
    }

    #[test]
    fn folder_thumbnail_rejects_invalid_direct_children_without_a_blank_card() {
        let dir =
            std::env::temp_dir().join(format!("ottrin-folder-invalid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("make fixture");
        std::fs::write(dir.join("unsafe.png"), b"not an image").expect("write invalid image");

        assert!(build_thumbnail_bytes(&dir, 128).is_none());
        std::fs::remove_dir_all(dir).expect("remove fixture");
    }
}
