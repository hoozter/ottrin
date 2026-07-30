#[cfg(not(target_os = "windows"))]
use ottrin_core::default_home_dir;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};

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

pub(crate) fn preview_cache_key(path: &Path, target_dim: u32) -> String {
    preview_cache_key_for_page(path, target_dim, 0)
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
    target_dim.hash(&mut hasher);
    page_index.hash(&mut hasher);
    format!("{:016x}_{}.png", hasher.finish(), target_dim)
}

fn preview_cache_path(cache_key: &str) -> Option<PathBuf> {
    let mut dir = preview_cache_dir()?;
    dir.push(cache_key);
    Some(dir)
}

pub(crate) fn load_cached_thumbnail(cache_key: &str) -> Option<Vec<u8>> {
    let path = preview_cache_path(cache_key)?;
    std::fs::read(path).ok()
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
        if is_pdf_path(path) {
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
        } else if is_office_visual_path(path) {
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
        } else {
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
    })
}

pub(crate) fn fallback_preview_bytes(path: &Path, generated: Option<Vec<u8>>) -> Option<Vec<u8>> {
    if generated.is_some() {
        return generated;
    }
    if is_pdf_path(path) || is_office_visual_path(path) || is_text_preview_path(path) {
        return None;
    }
    std::fs::read(path).ok()
}

pub(crate) fn full_preview_bytes_at_page(
    path: &Path,
    target_dim: u32,
    page_index: usize,
) -> Option<Vec<u8>> {
    if is_pdf_path(path) {
        return match ottrin_preview::render_pdf_page_png(path, page_index, target_dim) {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                eprintln!("PDF preview render failed for {}: {error}", path.display());
                None
            }
        };
    }
    full_preview_bytes_with_visual_renderer(path, target_dim, |path, target_dim| {
        if is_office_visual_path(path) {
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
    if is_pdf_path(path) || is_office_visual_path(path) || is_text_preview_path(path) {
        return render_visual(path, target_dim);
    }
    std::fs::read(path).ok()
}

fn build_thumbnail_bytes_with_visual_renderer(
    path: &Path,
    target_dim: u32,
    render_visual: impl FnOnce(&Path, u32) -> Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    if is_pdf_path(path) || is_office_visual_path(path) || is_text_preview_path(path) {
        return render_visual(path, target_dim);
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

fn is_pdf_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

fn is_office_visual_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "docx" | "xlsx" | "xlsm"))
}

fn is_text_preview_path(path: &Path) -> bool {
    matches!(
        ottrin_preview::classify_path(path),
        ottrin_preview::PreviewKind::Text
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
    fn text_thumbnail_cache_keys_vary_by_render_size() {
        let path = Path::new("main.rs");

        assert_ne!(preview_cache_key(path, 256), preview_cache_key(path, 640));
    }
}
