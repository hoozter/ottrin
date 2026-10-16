#[cfg(not(target_os = "windows"))]
use ottrin_core::default_home_dir;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

const FOLDER_COLLAGE_ENTRY_LIMIT: usize = 64;
const FOLDER_COLLAGE_CHILD_LIMIT: usize = 4;
const FOLDER_COLLAGE_MAX_CHILD_BYTES: u64 = 16 * 1024 * 1024;
const FOLDER_COLLAGE_MAX_CHILD_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_CAMERA_RAW_CACHE_PNG_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CAMERA_RAW_CACHE_PNG_DIMENSION: u32 = 2_048;
const MAX_FONT_CACHE_PNG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FONT_CACHE_PNG_DIMENSION: u32 = 2_048;
const MAX_SQLITE_CACHE_PNG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SQLITE_CACHE_PNG_DIMENSION: u32 = 2_048;
const MAX_STL_CACHE_PNG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_STL_CACHE_PNG_DIMENSION: u32 = 1_024;
const MAX_OBJ_CACHE_PNG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_OBJ_CACHE_PNG_DIMENSION: u32 = 1_024;
const MAX_PLY_CACHE_PNG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PLY_CACHE_PNG_DIMENSION: u32 = 1_024;
const MAX_3MF_CACHE_PNG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_3MF_CACHE_PNG_DIMENSION: u32 = 1_024;
const MAX_AFFINITY_CACHE_PNG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_AFFINITY_CACHE_PNG_DIMENSION: u32 = 1_024;
const MAX_SVG_CACHE_PNG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SVG_CACHE_PNG_DIMENSION: u32 = 2_048;
const MAX_PDF_CACHE_PNG_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PDF_CACHE_PNG_DIMENSION: u32 = 2_048;
#[cfg(any(target_os = "linux", target_os = "windows"))]
const MAX_EPS_CACHE_PNG_BYTES: u64 = 32 * 1024 * 1024;
#[cfg(any(target_os = "linux", target_os = "windows"))]
const MAX_EPS_CACHE_PNG_DIMENSION: u32 = 4_096;
const MAX_VIDEO_CACHE_PNG_BYTES: u64 = 16 * 1024 * 1024;
const MAX_VIDEO_CACHE_PNG_DIMENSION: u32 = 2_048;

#[derive(Debug, Clone)]
pub(crate) enum ImagePreviewCacheEntry {
    Ready(Vec<u8>),
    Unavailable { reason: Option<String> },
}

impl ImagePreviewCacheEntry {
    #[cfg(test)]
    pub(crate) fn from_response(bytes: Option<Vec<u8>>) -> Self {
        Self::from_response_with_reason(bytes, None)
    }

    pub(crate) fn from_response_with_reason(
        bytes: Option<Vec<u8>>,
        reason: Option<String>,
    ) -> Self {
        match bytes {
            Some(bytes) => Self::Ready(bytes),
            None => Self::Unavailable { reason },
        }
    }

    pub(crate) fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Ready(bytes) => Some(bytes),
            Self::Unavailable { .. } => None,
        }
    }

    pub(crate) fn unavailable_reason(&self) -> Option<&str> {
        match self {
            Self::Unavailable {
                reason: Some(reason),
            } => Some(reason),
            _ => None,
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
    preview_cache_key_for_page_with_generations(
        path, target_dim, page_index, None, None, None, None,
    )
}

pub(crate) fn preview_cache_key_for_page_with_sqlite_generation(
    path: &Path,
    target_dim: u32,
    page_index: usize,
    sqlite_source_generation: Option<&ottrin_preview::SqlitePreviewSourceGeneration>,
) -> String {
    preview_cache_key_for_page_with_generations(
        path,
        target_dim,
        page_index,
        sqlite_source_generation,
        None,
        None,
        None,
    )
}

pub(crate) fn preview_cache_key_for_page_with_generations(
    path: &Path,
    target_dim: u32,
    page_index: usize,
    sqlite_source_generation: Option<&ottrin_preview::SqlitePreviewSourceGeneration>,
    obj_source_generation: Option<&ottrin_preview::ObjPreviewSourceGeneration>,
    ply_source_generation: Option<&ottrin_preview::PlyPreviewSourceGeneration>,
    office_source_generation: Option<&ottrin_preview::OfficePreviewSourceGeneration>,
) -> String {
    if ottrin_preview::is_stl_path(path) {
        let generation = ottrin_preview::stl_preview_source_generation(path);
        return stl_preview_cache_key(target_dim, page_index, generation.as_ref());
    }
    if ottrin_preview::is_obj_path(path) {
        let generation = obj_source_generation
            .cloned()
            .or_else(|| ottrin_preview::obj_preview_source_generation(path));
        return obj_preview_cache_key(target_dim, page_index, generation.as_ref());
    }
    if ottrin_preview::is_ply_path(path) {
        let generation = ply_source_generation
            .cloned()
            .or_else(|| ottrin_preview::ply_preview_source_generation(path));
        return ply_preview_cache_key(target_dim, page_index, generation.as_ref());
    }
    if ottrin_preview::is_3mf_path(path) {
        let generation = ottrin_preview::threemf_preview_source_generation(path);
        return threemf_preview_cache_key(target_dim, page_index, generation.as_ref());
    }
    if ottrin_preview::is_affinity_path(path) {
        let generation = ottrin_preview::affinity_preview_source_generation(path);
        return affinity_preview_cache_key(target_dim, page_index, generation.as_ref());
    }
    if ottrin_preview::is_office_visual_path(path) {
        let generation = office_source_generation
            .cloned()
            .or_else(|| ottrin_preview::office_preview_source_generation(path));
        return office_preview_cache_key(target_dim, page_index, generation.as_ref());
    }
    if ottrin_preview::is_sqlite_preview_candidate(path) {
        let generation = sqlite_source_generation
            .cloned()
            .or_else(|| ottrin_preview::sqlite_preview_source_generation(path));
        return sqlite_preview_cache_key(target_dim, page_index, generation.as_ref());
    }
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    if let Ok(meta) = std::fs::metadata(path) {
        // The path and metadata are not enough to distinguish a same-size,
        // same-timestamp replacement. `same_file::Handle` hashes the stable
        // platform identity (inode on Unix; volume/file identity on Windows)
        // so a replacement cannot inherit the old preview generation.
        if let Ok(identity) = same_file::Handle::from_path(path) {
            identity.hash(&mut hasher);
        }
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

fn stl_preview_cache_key(
    target_dim: u32,
    page_index: usize,
    source_generation: Option<&ottrin_preview::StlPreviewSourceGeneration>,
) -> String {
    let generation = source_generation
        .map(|generation| hex_digest(generation.as_bytes()))
        .unwrap_or_else(|| "unavailable".to_string());
    format!("stl-v1-{generation}-{target_dim}-{page_index}.png")
}

fn obj_preview_cache_key(
    target_dim: u32,
    page_index: usize,
    source_generation: Option<&ottrin_preview::ObjPreviewSourceGeneration>,
) -> String {
    let generation = source_generation
        .map(|generation| hex_digest(generation.as_bytes()))
        .unwrap_or_else(|| "unavailable".to_string());
    format!("obj-v1-{generation}-{target_dim}-{page_index}.png")
}

fn ply_preview_cache_key(
    target_dim: u32,
    page_index: usize,
    source_generation: Option<&ottrin_preview::PlyPreviewSourceGeneration>,
) -> String {
    let generation = source_generation
        .map(|generation| hex_digest(generation.as_bytes()))
        .unwrap_or_else(|| "unavailable".to_string());
    format!("ply-v1-{generation}-{target_dim}-{page_index}.png")
}

fn office_preview_cache_key(
    target_dim: u32,
    page_index: usize,
    source_generation: Option<&ottrin_preview::OfficePreviewSourceGeneration>,
) -> String {
    let generation = source_generation
        .map(|generation| hex_digest(generation.as_bytes()))
        .unwrap_or_else(|| "unavailable".to_string());
    format!("office-v1-{generation}-{target_dim}-{page_index}.png")
}

pub(crate) fn office_provider_preview_cache_key(
    target_dim: u32,
    page_index: usize,
    source_generation: Option<&ottrin_preview::OfficePreviewSourceGeneration>,
    identity: &ottrin_preview::OfficeRenderCacheIdentity,
) -> String {
    let generation = source_generation
        .map(|generation| hex_digest(generation.as_bytes()))
        .unwrap_or_else(|| "unavailable".to_string());
    let provider = match identity.provider {
        ottrin_preview::OfficeProviderId::InstalledLibreOffice => "installed-libreoffice",
        ottrin_preview::OfficeProviderId::LightweightPack => "lightweight-pack",
        ottrin_preview::OfficeProviderId::FidelityPack => "fidelity-pack",
    };
    format!(
        "office-provider-v1-{generation}-{provider}-{}-policy{}-{target_dim}-{page_index}.png",
        identity.provider_version, identity.policy_version
    )
}

fn threemf_preview_cache_key(
    target_dim: u32,
    page_index: usize,
    source_generation: Option<&ottrin_preview::ThreeMfPreviewSourceGeneration>,
) -> String {
    let generation = source_generation
        .map(|generation| hex_digest(generation.as_bytes()))
        .unwrap_or_else(|| "unavailable".to_string());
    format!("3mf-v1-{generation}-{target_dim}-{page_index}.png")
}

fn affinity_preview_cache_key(
    target_dim: u32,
    page_index: usize,
    source_generation: Option<&ottrin_preview::AffinityPreviewSourceGeneration>,
) -> String {
    let generation = source_generation
        .map(|generation| hex_digest(generation.as_bytes()))
        .unwrap_or_else(|| "unavailable".to_string());
    format!("affinity-v1-{generation}-{target_dim}-{page_index}.png")
}

fn sqlite_preview_cache_key(
    target_dim: u32,
    page_index: usize,
    source_generation: Option<&ottrin_preview::SqlitePreviewSourceGeneration>,
) -> String {
    let generation = source_generation
        .map(|generation| hex_digest(generation.as_bytes()))
        .unwrap_or_else(|| "unavailable".to_string());
    format!("sqlite-v1-{generation}-{target_dim}-{page_index}.png")
}

fn hex_digest(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
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

pub(crate) fn load_cached_video_thumbnail(cache_key: &str) -> Option<Vec<u8>> {
    let cache_path = preview_cache_path(cache_key)?;
    let bytes = read_bounded_cache(
        std::fs::File::open(&cache_path).ok()?,
        MAX_VIDEO_CACHE_PNG_BYTES,
    )
    .ok()??;
    if bounded_cached_png_is_safe(
        &bytes,
        MAX_VIDEO_CACHE_PNG_BYTES,
        MAX_VIDEO_CACHE_PNG_DIMENSION,
    ) {
        Some(bytes)
    } else {
        let _ = std::fs::remove_file(cache_path);
        None
    }
}

/// A video cache key is derived from an already-admitted native file handle.
/// Persisting it must not reopen the user path after that admission.
pub(crate) fn persist_cached_video_thumbnail(cache_key: &str, bytes: &[u8], max_bytes: u64) {
    if max_bytes == 0
        || !bounded_cached_png_is_safe(
            bytes,
            MAX_VIDEO_CACHE_PNG_BYTES,
            MAX_VIDEO_CACHE_PNG_DIMENSION,
        )
    {
        return;
    }
    let Some(cache_path) = preview_cache_path(cache_key) else {
        return;
    };
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(cache_path, bytes).is_ok() {
        prune_preview_cache_dir(max_bytes);
    }
}

pub(crate) fn load_cached_thumbnail(
    path: &Path,
    target_dim: u32,
    page_index: usize,
    cache_key: &str,
) -> Option<Vec<u8>> {
    if !preview_cache_key_matches(path, target_dim, page_index, cache_key) {
        return None;
    }
    let cache_path = preview_cache_path(cache_key)?;
    let bytes = load_cached_thumbnail_at_path(path, &cache_path)?;
    preview_cache_key_matches(path, target_dim, page_index, cache_key).then_some(bytes)
}

fn preview_cache_key_matches(
    path: &Path,
    target_dim: u32,
    page_index: usize,
    cache_key: &str,
) -> bool {
    preview_cache_key_for_page(path, target_dim, page_index) == cache_key
}

/// Opens an already-resolved cache entry only after the preview profile permits
/// that file type to use thumbnail bytes. Formats with a dedicated renderer
/// accept cached PNG only, so stale source bytes cannot bypass it.
fn load_cached_thumbnail_at_path(path: &Path, cache_path: &Path) -> Option<Vec<u8>> {
    if ottrin_preview::is_epub_path(path) {
        let _ = std::fs::remove_file(cache_path);
        return None;
    }
    let bytes = if ottrin_preview::is_camera_raw_path(path) {
        match read_camera_raw_cache_bytes(cache_path) {
            Some(bytes) => bytes,
            None => {
                let _ = std::fs::remove_file(cache_path);
                return None;
            }
        }
    } else if ottrin_preview::is_font_path(path) {
        match read_font_cache_bytes(cache_path) {
            Some(bytes) => bytes,
            None => {
                let _ = std::fs::remove_file(cache_path);
                return None;
            }
        }
    } else if ottrin_preview::is_sqlite_preview_candidate(path) {
        match read_sqlite_cache_bytes(cache_path) {
            Some(bytes) => bytes,
            None => {
                let _ = std::fs::remove_file(cache_path);
                return None;
            }
        }
    } else if ottrin_preview::is_stl_path(path) {
        match read_stl_cache_bytes(cache_path) {
            Some(bytes) => bytes,
            None => {
                let _ = std::fs::remove_file(cache_path);
                return None;
            }
        }
    } else if ottrin_preview::is_obj_path(path) {
        match read_obj_cache_bytes(cache_path) {
            Some(bytes) => bytes,
            None => {
                let _ = std::fs::remove_file(cache_path);
                return None;
            }
        }
    } else if ottrin_preview::is_ply_path(path) {
        match read_ply_cache_bytes(cache_path) {
            Some(bytes) => bytes,
            None => {
                let _ = std::fs::remove_file(cache_path);
                return None;
            }
        }
    } else if ottrin_preview::is_3mf_path(path) {
        match read_threemf_cache_bytes(cache_path) {
            Some(bytes) => bytes,
            None => {
                let _ = std::fs::remove_file(cache_path);
                return None;
            }
        }
    } else if ottrin_preview::is_affinity_path(path) {
        match read_affinity_cache_bytes(cache_path) {
            Some(bytes) => bytes,
            None => {
                let _ = std::fs::remove_file(cache_path);
                return None;
            }
        }
    } else if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::Pdf {
        match read_pdf_cache_bytes(cache_path) {
            Some(bytes) => bytes,
            None => {
                let _ = std::fs::remove_file(cache_path);
                return None;
            }
        }
    } else {
        std::fs::read(cache_path).ok()?
    };
    if !cached_thumbnail_is_safe_for_path(path, &bytes) {
        let _ = std::fs::remove_file(cache_path);
        return None;
    }
    Some(bytes)
}

fn read_camera_raw_cache_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    read_bounded_camera_raw_cache(file).ok()?
}

fn read_bounded_camera_raw_cache(reader: impl Read) -> std::io::Result<Option<Vec<u8>>> {
    read_bounded_cache(reader, MAX_CAMERA_RAW_CACHE_PNG_BYTES)
}

fn read_font_cache_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    read_bounded_cache(file, MAX_FONT_CACHE_PNG_BYTES).ok()?
}

fn read_sqlite_cache_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    read_bounded_cache(file, MAX_SQLITE_CACHE_PNG_BYTES).ok()?
}

fn read_stl_cache_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    read_bounded_cache(file, MAX_STL_CACHE_PNG_BYTES).ok()?
}

fn read_obj_cache_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    read_bounded_cache(file, MAX_OBJ_CACHE_PNG_BYTES).ok()?
}

fn read_ply_cache_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    read_bounded_cache(file, MAX_PLY_CACHE_PNG_BYTES).ok()?
}

fn read_threemf_cache_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    read_bounded_cache(file, MAX_3MF_CACHE_PNG_BYTES).ok()?
}

fn read_affinity_cache_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    read_bounded_cache(file, MAX_AFFINITY_CACHE_PNG_BYTES).ok()?
}

fn read_pdf_cache_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    read_bounded_pdf_cache(file).ok()?
}

fn read_bounded_pdf_cache(reader: impl Read) -> std::io::Result<Option<Vec<u8>>> {
    read_bounded_cache(reader, MAX_PDF_CACHE_PNG_BYTES)
}

fn read_bounded_cache(reader: impl Read, max_bytes: u64) -> std::io::Result<Option<Vec<u8>>> {
    let limit = usize::try_from(max_bytes)
        .expect("preview cache byte limit fits in usize")
        .saturating_add(1);
    let mut bytes = Vec::with_capacity(limit);
    reader.take(limit as u64).read_to_end(&mut bytes)?;
    Ok((bytes.len() < limit).then_some(bytes))
}

pub(crate) fn persist_cached_thumbnail(
    source_path: &Path,
    target_dim: u32,
    page_index: usize,
    cache_key: &str,
    bytes: &[u8],
    max_bytes: u64,
) {
    let Some(cache_path) = preview_cache_path(cache_key) else {
        return;
    };
    let _ = persist_cached_thumbnail_at_path_if_current(
        source_path,
        target_dim,
        page_index,
        cache_key,
        bytes,
        max_bytes,
        cache_path,
    );
}

fn persist_cached_thumbnail_at_path_if_current(
    source_path: &Path,
    target_dim: u32,
    page_index: usize,
    cache_key: &str,
    bytes: &[u8],
    max_bytes: u64,
    cache_path: PathBuf,
) -> bool {
    if max_bytes == 0 {
        return false;
    }
    if preview_cache_key_for_page(source_path, target_dim, page_index) != cache_key {
        return false;
    }
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(&cache_path, bytes).is_err()
        || preview_cache_key_for_page(source_path, target_dim, page_index) != cache_key
    {
        let _ = std::fs::remove_file(cache_path);
        return false;
    }
    prune_preview_cache_dir(max_bytes);
    true
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
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            ottrin_preview::PreviewVisualRenderer::Eps => {
                match ottrin_preview::render_eps_content_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "EPS thumbnail render failed for {}: {error:?}",
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
            #[cfg(target_os = "linux")]
            ottrin_preview::PreviewVisualRenderer::Html => {
                match ottrin_preview::render_html_content_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "Offline HTML thumbnail render failed for {}: {error}",
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
            ottrin_preview::PreviewVisualRenderer::CameraRaw => {
                match ottrin_preview::render_camera_raw_embedded_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "Camera RAW thumbnail render failed for {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            }
            ottrin_preview::PreviewVisualRenderer::Font => {
                match ottrin_preview::render_font_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "Font thumbnail render failed for {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            }
            ottrin_preview::PreviewVisualRenderer::Sqlite => {
                match ottrin_preview::render_sqlite_metadata_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "SQLite thumbnail render failed for {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            }
            ottrin_preview::PreviewVisualRenderer::Stl => {
                match ottrin_preview::render_stl_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "STL thumbnail render failed for {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            }
            ottrin_preview::PreviewVisualRenderer::Obj => {
                match ottrin_preview::render_obj_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "OBJ thumbnail render failed for {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            }
            ottrin_preview::PreviewVisualRenderer::Ply => {
                match ottrin_preview::render_ply_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "PLY thumbnail render failed for {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            }
            ottrin_preview::PreviewVisualRenderer::ThreeMf => {
                match ottrin_preview::render_3mf_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "3MF thumbnail render failed for {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            }
            ottrin_preview::PreviewVisualRenderer::Affinity => {
                match ottrin_preview::render_affinity_embedded_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "Affinity thumbnail render failed for {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            }
            ottrin_preview::PreviewVisualRenderer::Video => None,
            _ => None,
        }
    })
}

#[cfg(test)]
pub(crate) fn build_thumbnail_bytes_with_error(
    path: &Path,
    target_dim: u32,
) -> Result<Vec<u8>, String> {
    if ottrin_preview::is_svg_path(path) {
        return ottrin_preview::render_svg_content_preview_png(path, target_dim)
            .map_err(|error| error.to_string());
    }
    build_thumbnail_bytes(path, target_dim).ok_or_else(|| "Image preview unavailable".to_string())
}

pub(crate) fn fallback_preview_bytes(path: &Path, generated: Option<Vec<u8>>) -> Option<Vec<u8>> {
    if generated.is_some() {
        return generated;
    }
    if ottrin_preview::is_sqlite_preview_candidate(path)
        || ottrin_preview::is_svg_path(path)
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
    if ottrin_preview::is_obj_path(path) {
        let rendered =
            ottrin_preview::render_obj_preview_png_with_generation(path, target_dim).ok()?;
        return (ottrin_preview::obj_preview_source_generation(path).as_ref()
            == Some(&rendered.source_generation))
        .then_some((rendered.png, true));
    }
    if ottrin_preview::is_ply_path(path) {
        let rendered =
            ottrin_preview::render_ply_preview_png_with_generation(path, target_dim).ok()?;
        return (ottrin_preview::ply_preview_source_generation(path).as_ref()
            == Some(&rendered.source_generation))
        .then_some((rendered.png, true));
    }
    if ottrin_preview::is_3mf_path(path) {
        let rendered =
            ottrin_preview::render_3mf_preview_png_with_generation(path, target_dim).ok()?;
        return (ottrin_preview::threemf_preview_source_generation(path).as_ref()
            == Some(&rendered.source_generation))
        .then_some((rendered.png, true));
    }
    if ottrin_preview::is_affinity_path(path) {
        let rendered =
            ottrin_preview::render_affinity_embedded_preview_png_with_generation(path, target_dim)
                .ok()?;
        return (ottrin_preview::affinity_preview_source_generation(path).as_ref()
            == Some(&rendered.source_generation))
        .then_some((rendered.png, true));
    }
    if let Some(bytes) = build_thumbnail_bytes(path, target_dim) {
        return Some((bytes, true));
    }
    fallback_preview_bytes(path, None).map(|bytes| (bytes, false))
}

fn cached_thumbnail_is_safe_for_path(path: &Path, bytes: &[u8]) -> bool {
    cached_thumbnail_is_safe_for_path_with_pdfium_runtime(path, bytes, || {
        ottrin_preview::pdfium_runtime_is_available()
    })
}

fn cached_thumbnail_is_safe_for_path_with_pdfium_runtime(
    path: &Path,
    bytes: &[u8],
    pdfium_runtime_is_available: impl FnOnce() -> bool,
) -> bool {
    if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::None {
        // Metadata-only types may not resurrect a stale raster cache as a visual claim.
        return false;
    }
    if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::Pdf {
        // A cache hit is admissible only while the same runtime discovery path
        // can bind PDFium. Otherwise a stale sidebar PNG could disagree with a
        // fresh Quick Inspector render that truthfully reports RuntimeLoad.
        return pdfium_runtime_is_available()
            && bounded_cached_png_is_safe(
                bytes,
                MAX_PDF_CACHE_PNG_BYTES,
                MAX_PDF_CACHE_PNG_DIMENSION,
            );
    }
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::Eps {
        return ottrin_preview::eps_preview_runtime_is_available()
            && bounded_cached_png_is_safe(
                bytes,
                MAX_EPS_CACHE_PNG_BYTES,
                MAX_EPS_CACHE_PNG_DIMENSION,
            );
    }
    if ottrin_preview::is_svg_path(path) {
        // Cached pixels are valid only while the current SVG passes the same
        // safe renderer admission used by both selected-result and inspector
        // previews. A stale valid raster must not mask a blocked replacement.
        return ottrin_preview::render_svg_content_preview_png(path, 1).is_ok()
            && bounded_cached_png_is_safe(
                bytes,
                MAX_SVG_CACHE_PNG_BYTES,
                MAX_SVG_CACHE_PNG_DIMENSION,
            );
    }
    if ottrin_preview::is_camera_raw_path(path) {
        return camera_raw_cached_png_is_safe(bytes);
    }
    if ottrin_preview::is_font_path(path) {
        return font_cached_png_is_safe(bytes);
    }
    if ottrin_preview::is_sqlite_preview_candidate(path) {
        return ottrin_preview::is_sqlite_preview_path(path) && sqlite_cached_png_is_safe(bytes);
    }
    if ottrin_preview::is_stl_path(path) {
        // A PNG-shaped cache file is not enough: a malformed replacement must
        // not inherit a content-addressed cache entry or bypass the mesh
        // parser. Validate the current bounded source before accepting it.
        return ottrin_preview::inspect_stl_metadata(path).is_ok() && stl_cached_png_is_safe(bytes);
    }
    if ottrin_preview::is_obj_path(path) {
        return ottrin_preview::inspect_obj_metadata(path).is_ok() && obj_cached_png_is_safe(bytes);
    }
    if ottrin_preview::is_ply_path(path) {
        return ottrin_preview::inspect_ply_metadata(path).is_ok() && ply_cached_png_is_safe(bytes);
    }
    if ottrin_preview::is_3mf_path(path) {
        return ottrin_preview::inspect_3mf_metadata(path).is_ok()
            && threemf_cached_png_is_safe(bytes);
    }
    if ottrin_preview::is_affinity_path(path) {
        return ottrin_preview::affinity_preview_is_available(path)
            && bounded_cached_png_is_safe(
                bytes,
                MAX_AFFINITY_CACHE_PNG_BYTES,
                MAX_AFFINITY_CACHE_PNG_DIMENSION,
            );
    }
    if !ottrin_preview::preview_profile(path).raw_fallback_allowed() {
        return is_png_bytes(bytes);
    }
    !ottrin_preview::is_svg_path(path) && !ottrin_preview::is_epub_path(path)
}

fn is_png_bytes(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

fn camera_raw_cached_png_is_safe(bytes: &[u8]) -> bool {
    bounded_cached_png_is_safe(
        bytes,
        MAX_CAMERA_RAW_CACHE_PNG_BYTES,
        MAX_CAMERA_RAW_CACHE_PNG_DIMENSION,
    )
}

fn font_cached_png_is_safe(bytes: &[u8]) -> bool {
    bounded_cached_png_is_safe(
        bytes,
        MAX_FONT_CACHE_PNG_BYTES,
        MAX_FONT_CACHE_PNG_DIMENSION,
    )
}

fn sqlite_cached_png_is_safe(bytes: &[u8]) -> bool {
    bounded_cached_png_is_safe(
        bytes,
        MAX_SQLITE_CACHE_PNG_BYTES,
        MAX_SQLITE_CACHE_PNG_DIMENSION,
    )
}

fn stl_cached_png_is_safe(bytes: &[u8]) -> bool {
    bounded_cached_png_is_safe(bytes, MAX_STL_CACHE_PNG_BYTES, MAX_STL_CACHE_PNG_DIMENSION)
}

fn obj_cached_png_is_safe(bytes: &[u8]) -> bool {
    bounded_cached_png_is_safe(bytes, MAX_OBJ_CACHE_PNG_BYTES, MAX_OBJ_CACHE_PNG_DIMENSION)
}

fn ply_cached_png_is_safe(bytes: &[u8]) -> bool {
    bounded_cached_png_is_safe(bytes, MAX_PLY_CACHE_PNG_BYTES, MAX_PLY_CACHE_PNG_DIMENSION)
}

fn threemf_cached_png_is_safe(bytes: &[u8]) -> bool {
    bounded_cached_png_is_safe(bytes, MAX_3MF_CACHE_PNG_BYTES, MAX_3MF_CACHE_PNG_DIMENSION)
}

fn bounded_cached_png_is_safe(bytes: &[u8], max_bytes: u64, max_dimension: u32) -> bool {
    if bytes.len() as u64 > max_bytes || !is_png_bytes(bytes) {
        return false;
    }
    let Some((width, height)) = structurally_valid_png_dimensions(bytes) else {
        return false;
    };
    if width == 0 || height == 0 || width > max_dimension || height > max_dimension {
        return false;
    }
    image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png)
        .decode()
        .is_ok()
}

fn structurally_valid_png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let mut offset = 8usize;
    let mut dimensions = None;
    while offset < bytes.len() {
        let length = u32::from_be_bytes(bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?)
            as usize;
        let chunk_type = bytes.get(offset + 4..offset.checked_add(8)?)?;
        let data_start = offset.checked_add(8)?;
        let data_end = data_start.checked_add(length)?;
        let chunk_end = data_end.checked_add(4)?;
        let data = bytes.get(data_start..data_end)?;
        bytes.get(data_end..chunk_end)?;
        if dimensions.is_none() {
            if chunk_type != b"IHDR" || data.len() != 13 {
                return None;
            }
            dimensions = Some((
                u32::from_be_bytes(data.get(..4)?.try_into().ok()?),
                u32::from_be_bytes(data.get(4..8)?.try_into().ok()?),
            ));
        }
        offset = chunk_end;
        if chunk_type == b"IEND" {
            return (data.is_empty() && offset == bytes.len())
                .then_some(dimensions)
                .flatten();
        }
    }
    None
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
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::Eps {
        return match ottrin_preview::render_eps_content_preview_png(path, target_dim) {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                eprintln!(
                    "EPS preview render failed for {}: {error:?}",
                    path.display()
                );
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
    if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::CameraRaw {
        return match ottrin_preview::render_camera_raw_embedded_preview_png(path, target_dim) {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                eprintln!(
                    "Camera RAW preview render failed for {}: {error}",
                    path.display()
                );
                None
            }
        };
    }
    if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::Stl {
        return match ottrin_preview::render_stl_preview_png(path, target_dim) {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                eprintln!("STL preview render failed for {}: {error}", path.display());
                None
            }
        };
    }
    if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::Obj {
        return match ottrin_preview::render_obj_preview_png_with_generation(path, target_dim) {
            Ok(rendered)
                if ottrin_preview::obj_preview_source_generation(path).as_ref()
                    == Some(&rendered.source_generation) =>
            {
                Some(rendered.png)
            }
            Ok(_) => None,
            Err(error) => {
                eprintln!("OBJ preview render failed for {}: {error}", path.display());
                None
            }
        };
    }
    if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::Ply {
        return match ottrin_preview::render_ply_preview_png_with_generation(path, target_dim) {
            Ok(rendered)
                if ottrin_preview::ply_preview_source_generation(path).as_ref()
                    == Some(&rendered.source_generation) =>
            {
                Some(rendered.png)
            }
            Ok(_) => None,
            Err(error) => {
                eprintln!("PLY preview render failed for {}: {error}", path.display());
                None
            }
        };
    }
    if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::ThreeMf {
        return match ottrin_preview::render_3mf_preview_png_with_generation(path, target_dim) {
            Ok(rendered)
                if ottrin_preview::threemf_preview_source_generation(path).as_ref()
                    == Some(&rendered.source_generation) =>
            {
                Some(rendered.png)
            }
            Ok(_) => None,
            Err(error) => {
                eprintln!("3MF preview render failed for {}: {error}", path.display());
                None
            }
        };
    }
    if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::Affinity {
        return match ottrin_preview::render_affinity_embedded_preview_png(path, target_dim) {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                eprintln!(
                    "Affinity preview render failed for {}: {error}",
                    path.display()
                );
                None
            }
        };
    }
    #[cfg(target_os = "linux")]
    if preview_visual_renderer(path) == ottrin_preview::PreviewVisualRenderer::Html {
        return match ottrin_preview::render_html_content_preview_png(path, target_dim) {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                eprintln!(
                    "Offline HTML preview render failed for {}: {error}",
                    path.display()
                );
                None
            }
        };
    }
    full_preview_bytes_with_visual_renderer(path, target_dim, |path, target_dim| {
        match preview_visual_renderer(path) {
            ottrin_preview::PreviewVisualRenderer::Office => {
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
            }
            ottrin_preview::PreviewVisualRenderer::TextCard => {
                match ottrin_preview::render_text_content_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!("Text preview render failed for {}: {error}", path.display());
                        None
                    }
                }
            }
            ottrin_preview::PreviewVisualRenderer::Font => {
                match ottrin_preview::render_font_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!("Font preview render failed for {}: {error}", path.display());
                        None
                    }
                }
            }
            ottrin_preview::PreviewVisualRenderer::Sqlite => {
                match ottrin_preview::render_sqlite_metadata_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "SQLite preview render failed for {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            }
            ottrin_preview::PreviewVisualRenderer::Affinity => {
                match ottrin_preview::render_affinity_embedded_preview_png(path, target_dim) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        eprintln!(
                            "Affinity preview render failed for {}: {error}",
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

fn full_preview_bytes_with_visual_renderer(
    path: &Path,
    target_dim: u32,
    render_visual: impl FnOnce(&Path, u32) -> Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    if matches!(
        preview_visual_renderer(path),
        ottrin_preview::PreviewVisualRenderer::Pdf
            | ottrin_preview::PreviewVisualRenderer::Eps
            | ottrin_preview::PreviewVisualRenderer::Office
            | ottrin_preview::PreviewVisualRenderer::CameraRaw
            | ottrin_preview::PreviewVisualRenderer::TextCard
            | ottrin_preview::PreviewVisualRenderer::Font
            | ottrin_preview::PreviewVisualRenderer::Sqlite
            | ottrin_preview::PreviewVisualRenderer::Stl
            | ottrin_preview::PreviewVisualRenderer::Obj
            | ottrin_preview::PreviewVisualRenderer::Ply
            | ottrin_preview::PreviewVisualRenderer::ThreeMf
            | ottrin_preview::PreviewVisualRenderer::Affinity
    ) {
        return render_visual(path, target_dim);
    }
    fallback_preview_bytes(path, None)
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
        | ottrin_preview::PreviewVisualRenderer::Eps
        | ottrin_preview::PreviewVisualRenderer::Office
        | ottrin_preview::PreviewVisualRenderer::CameraRaw
        | ottrin_preview::PreviewVisualRenderer::TextCard
        | ottrin_preview::PreviewVisualRenderer::Html
        | ottrin_preview::PreviewVisualRenderer::Font
        | ottrin_preview::PreviewVisualRenderer::Sqlite
        | ottrin_preview::PreviewVisualRenderer::Stl
        | ottrin_preview::PreviewVisualRenderer::Obj
        | ottrin_preview::PreviewVisualRenderer::Ply
        | ottrin_preview::PreviewVisualRenderer::ThreeMf => {
            return render_visual(path, target_dim);
        }
        ottrin_preview::PreviewVisualRenderer::Affinity => return render_visual(path, target_dim),
        // Video requires a retained admitted handle and is rendered only by the
        // live preview worker, never by this path-only fallback.
        ottrin_preview::PreviewVisualRenderer::Video => return None,
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
        if let Err(error) = clear_preview_cache_dir() {
            eprintln!("Failed to clear thumbnail cache: {error}");
        }
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

fn clear_preview_cache_at(dir: &Path) -> Result<u64, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(format!("Failed to read {}: {error}", dir.display())),
    };
    let mut freed = 0_u64;
    for entry in entries {
        let entry = entry.map_err(|error| format!("Failed to read {}: {error}", dir.display()))?;
        let path = entry.path();
        let bytes = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?
            .len();
        std::fs::remove_file(&path)
            .map_err(|error| format!("Failed to remove {}: {error}", path.display()))?;
        freed = freed.saturating_add(bytes);
    }
    Ok(freed)
}

pub(crate) fn clear_preview_cache_dir() -> Result<u64, String> {
    let dir = preview_cache_dir()
        .ok_or_else(|| "Cannot determine thumbnail cache directory".to_owned())?;
    clear_preview_cache_at(&dir)
}

pub fn clear_preview_cache_storage() -> Result<u64, String> {
    clear_preview_cache_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;
    use std::io::Write;

    #[test]
    fn cache_cleanup_reports_blocked_entries_and_allows_retry() {
        let fixture = tempfile::tempdir().unwrap();
        let cache = fixture.path().join("cache");
        assert_eq!(clear_preview_cache_at(&cache).unwrap(), 0);
        let blocked = cache.join("blocked");
        std::fs::create_dir_all(&blocked).unwrap();
        let preserved = blocked.join("preserved.txt");
        std::fs::write(&preserved, b"preserve nested data").unwrap();
        let error = clear_preview_cache_at(&cache).unwrap_err();
        assert!(error.contains(&blocked.display().to_string()), "{error}");
        assert_eq!(std::fs::read(&preserved).unwrap(), b"preserve nested data");
        std::fs::remove_dir_all(&blocked).unwrap();
        std::fs::write(cache.join("thumbnail"), b"cached bytes").unwrap();
        assert_eq!(clear_preview_cache_at(&cache).unwrap(), 12);
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
    }

    #[test]
    fn cache_cleanup_reports_an_unreadable_cache_location() {
        let fixture = tempfile::tempdir().unwrap();
        let cache = fixture.path().join("cache");
        std::fs::write(&cache, b"not a directory").unwrap();
        let error = clear_preview_cache_at(&cache).unwrap_err();
        assert!(error.contains(&cache.display().to_string()), "{error}");
        assert_eq!(std::fs::read(&cache).unwrap(), b"not a directory");
    }

    #[cfg(unix)]
    #[test]
    fn cache_cleanup_removes_links_without_touching_their_targets() {
        let fixture = tempfile::tempdir().unwrap();
        let cache = fixture.path().join("cache");
        std::fs::create_dir(&cache).unwrap();
        let source = fixture.path().join("source");
        std::fs::write(&source, b"preserve source").unwrap();
        std::os::unix::fs::symlink(&source, cache.join("link")).unwrap();
        clear_preview_cache_at(&cache).unwrap();
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
        assert_eq!(std::fs::read(&source).unwrap(), b"preserve source");
    }

    fn write_3mf_fixture(path: &Path) {
        let file = std::fs::File::create(path).expect("create 3MF fixture");
        let mut package = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (name, value) in [
            (
                "[Content_Types].xml",
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel" Target="/3D/model.model"/></Relationships>"#,
            ),
            (
                "3D/model.model",
                r#"<model xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources><object id="1" type="model"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="1"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources><build><item objectid="1"/></build></model>"#,
            ),
        ] {
            package.start_file(name, options).expect("start 3MF entry");
            package
                .write_all(value.as_bytes())
                .expect("write 3MF entry");
        }
        package.finish().expect("finish 3MF fixture");
    }

    fn raw_tiff_with_embedded_jpeg(jpeg: &[u8]) -> Vec<u8> {
        const IFD0_OFFSET: u32 = 8;
        const IFD1_OFFSET: u32 = IFD0_OFFSET + 2 + 4;
        const JPEG_OFFSET: u32 = IFD1_OFFSET + 2 + 2 * 12 + 4;

        let mut bytes = b"II*\0".to_vec();
        bytes.extend_from_slice(&IFD0_OFFSET.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&IFD1_OFFSET.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        for (tag, value) in [(0x0201u16, JPEG_OFFSET), (0x0202u16, jpeg.len() as u32)] {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&4u16.to_le_bytes());
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(jpeg);
        bytes
    }

    fn small_jpeg() -> Vec<u8> {
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            12,
            8,
            image::Rgb([32, 160, 224]),
        ))
        .write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
        .expect("encode embedded JPEG");
        jpeg
    }

    fn affinity_fixture_png(color: [u8; 4]) -> Vec<u8> {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(256, 256, image::Rgba(color))
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode Affinity embedded PNG");
        png
    }

    fn write_material_font(path: &Path) {
        std::fs::write(path, include_bytes!("../assets/MaterialSymbolsFilled.ttf"))
            .expect("write embedded font fixture");
    }

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
    fn cached_pdf_preview_is_never_accepted_without_a_fresh_pdfium_render() {
        let pdf = Path::new("report.pdf");
        let cached_png = affinity_fixture_png([32, 64, 96, 255]);

        assert!(
            !cached_thumbnail_is_safe_for_path_with_pdfium_runtime(pdf, &cached_png, || false),
            "an otherwise valid cached PDF PNG must be rejected when a fresh inspector render cannot bind PDFium"
        );
    }

    #[test]
    fn pdf_cache_reader_rejects_oversized_bytes_before_png_validation() {
        let oversized = vec![0_u8; MAX_PDF_CACHE_PNG_BYTES as usize + 1];

        assert_eq!(
            read_bounded_pdf_cache(Cursor::new(oversized)).unwrap(),
            None
        );
    }

    #[test]
    fn failed_preview_response_preserves_reason_without_exposing_image_bytes() {
        let entry = ImagePreviewCacheEntry::from_response_with_reason(
            None,
            Some("SVG exceeds bounded render work".to_string()),
        );

        assert!(entry.bytes().is_none());
        assert_eq!(
            entry.unavailable_reason(),
            Some("SVG exceeds bounded render work")
        );
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
    fn video_candidate_never_falls_back_to_raw_container_bytes() {
        let mut fixture = tempfile::Builder::new()
            .prefix("ottrin-video-fallback-")
            .suffix(".MP4")
            .tempfile()
            .expect("create video fixture");
        fixture
            .write_all(b"synthetic video container")
            .expect("write video fixture");

        assert_eq!(fallback_preview_bytes(fixture.path(), None), None);
    }

    #[test]
    fn affinity_preview_never_returns_raw_container_or_stale_cache_after_replacement() {
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos()
        );
        let source = std::env::temp_dir().join(format!("ottrin-affinity-{nonce}.AfDeSiGn"));
        let cache = std::env::temp_dir().join(format!("ottrin-affinity-{nonce}.png"));
        let embedded_png = affinity_fixture_png([20, 40, 60, 255]);
        let container = [b"synthetic-affinity".as_slice(), embedded_png.as_slice()].concat();
        std::fs::write(&source, &container).expect("write Affinity fixture");

        let thumbnail =
            build_thumbnail_bytes(&source, 128).expect("render bounded Affinity thumbnail");
        assert_ne!(thumbnail, container);
        assert!(fallback_preview_bytes(&source, None).is_none());
        std::fs::write(&cache, &thumbnail).expect("write bounded cache PNG");
        assert_eq!(
            load_cached_thumbnail_at_path(&source, &cache),
            Some(thumbnail)
        );

        std::fs::write(&source, b"malformed replacement").expect("replace Affinity source");
        assert!(load_cached_thumbnail_at_path(&source, &cache).is_none());
        assert!(
            !cache.exists(),
            "invalid replacement must discard stale cache"
        );
        assert_eq!(
            thumbnail_bytes_from_cache_or_build(&source, 128, Some(vec![1, 2, 3])),
            None
        );
        assert_eq!(full_preview_bytes_at_page(&source, 128, 0), None);
        assert!(fallback_preview_bytes(&source, None).is_none());
        let _ = std::fs::remove_file(source);
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
    fn sqlite_thumbnail_and_full_preview_use_the_metadata_card_renderer_case_insensitively() {
        let path = Path::new("records.SQLITE3");
        let thumbnail = build_thumbnail_bytes_with_visual_renderer(path, 256, |actual, target| {
            assert_eq!(actual, path);
            assert_eq!(target, 256);
            Some(b"sqlite-card".to_vec())
        });
        let full = full_preview_bytes_with_visual_renderer(path, 512, |actual, target| {
            assert_eq!(actual, path);
            assert_eq!(target, 512);
            Some(b"sqlite-card".to_vec())
        });

        assert_eq!(thumbnail, Some(b"sqlite-card".to_vec()));
        assert_eq!(full, Some(b"sqlite-card".to_vec()));
        assert!(fallback_preview_bytes(path, None).is_none());
    }

    #[test]
    fn sqlite_cache_refuses_stale_raw_bytes_when_the_source_is_not_content_verified() {
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos()
        );
        let source = std::env::temp_dir().join(format!("ottrin-stale-{nonce}.sqlite"));
        let cache = std::env::temp_dir().join(format!("ottrin-stale-{nonce}.png"));
        std::fs::write(&source, b"not a SQLite database").expect("write invalid SQLite source");
        std::fs::write(&cache, b"stale source bytes").expect("write stale cache entry");

        assert!(load_cached_thumbnail_at_path(&source, &cache).is_none());
        assert!(!cache.exists(), "unsafe stale SQLite cache must be removed");
        assert!(fallback_preview_bytes(&source, None).is_none());

        std::fs::remove_file(source).expect("remove SQLite source fixture");
    }

    #[test]
    fn sqlite_candidate_extensions_never_return_raw_cache_or_source_bytes_after_replacement() {
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos()
        );
        let oversized_raw_cache = vec![b'x'; MAX_SQLITE_CACHE_PNG_BYTES as usize + 1];

        for extension in ["DB", "Sqlite", "sQLiTe3"] {
            let source = std::env::temp_dir().join(format!("ottrin-replaced-{nonce}.{extension}"));
            let cache =
                std::env::temp_dir().join(format!("ottrin-replaced-{nonce}.{extension}.png"));
            std::fs::write(&source, b"replacement is not a SQLite database")
                .expect("write replacement source");
            std::fs::write(&cache, &oversized_raw_cache).expect("write oversized stale cache");

            assert!(
                load_cached_thumbnail_at_path(&source, &cache).is_none(),
                "SQLite candidates must reject stale cache before generic reads"
            );
            assert!(
                !cache.exists(),
                "unsafe SQLite candidate cache must be removed for {extension}"
            );
            assert_eq!(
                thumbnail_bytes_from_cache_or_build(
                    &source,
                    128,
                    Some(oversized_raw_cache.clone())
                ),
                None,
                "cache miss/rebuild must not return replacement source bytes for {extension}"
            );
            assert_eq!(
                full_preview_bytes_at_page(&source, 128, 0),
                None,
                "full preview must not fall back to replacement source bytes for {extension}"
            );
            assert_eq!(
                fallback_preview_bytes(&source, None),
                None,
                "generic fallback must be permanently denied for {extension}"
            );

            std::fs::remove_file(source).expect("remove replacement source");
        }
    }

    #[test]
    fn sqlite_candidate_cache_hits_require_a_decoded_png_for_case_variants() {
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos()
        );
        let mut cached_png = Vec::new();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([20, 40, 60, 255]))
            .write_to(&mut Cursor::new(&mut cached_png), image::ImageFormat::Png)
            .expect("encode bounded cache PNG");

        for extension in ["DB", "Sqlite", "sQLiTe3"] {
            let source = std::env::temp_dir().join(format!("ottrin-cache-hit-{nonce}.{extension}"));
            let cache =
                std::env::temp_dir().join(format!("ottrin-cache-hit-{nonce}.{extension}.png"));
            std::fs::write(&source, b"SQLite format 3\0content verification")
                .expect("write SQLite signature source");
            std::fs::write(&cache, &cached_png).expect("write decoded SQLite cache PNG");

            assert_eq!(
                load_cached_thumbnail_at_path(&source, &cache),
                Some(cached_png.clone()),
                "valid bounded PNG cache may be used for content-verified {extension}"
            );

            std::fs::remove_file(source).expect("remove SQLite source");
            std::fs::remove_file(cache).expect("remove SQLite cache");
        }
    }

    #[test]
    fn sqlite_cache_generation_rejects_a_replacement_between_key_and_persist() {
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos()
        );
        let source = std::env::temp_dir().join(format!("ottrin-generation-{nonce}.sqlite"));
        let replacement =
            std::env::temp_dir().join(format!("ottrin-generation-replacement-{nonce}.sqlite"));
        let cache = std::env::temp_dir().join(format!("ottrin-generation-{nonce}.png"));
        std::fs::write(&source, b"SQLite format 3\0first-generation")
            .expect("write first source generation");
        let first_key = preview_cache_key_for_page(&source, 128, 0);
        std::fs::write(&replacement, b"SQLite format 3\0next--generation")
            .expect("write replacement generation");
        std::fs::rename(&replacement, &source).expect("atomically replace source generation");

        assert!(
            !persist_cached_thumbnail_at_path_if_current(
                &source,
                128,
                0,
                &first_key,
                b"rendered card for the superseded generation",
                MAX_SQLITE_CACHE_PNG_BYTES,
                cache.clone(),
            ),
            "a card rendered after replacement must not be persisted under the prior generation"
        );
        assert!(
            !cache.exists(),
            "superseded-generation cache output must be removed rather than served later"
        );

        std::fs::remove_file(source).expect("remove replacement source");
    }

    #[test]
    fn sqlite_cache_key_changes_after_equal_size_hard_link_write_with_restored_mtime() {
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos()
        );
        let source = std::env::temp_dir().join(format!("ottrin-key-{nonce}.sqlite"));
        let alias = std::env::temp_dir().join(format!("ottrin-key-alias-{nonce}.sqlite"));
        std::fs::write(&source, b"SQLite format 3\0first-generation")
            .expect("write first SQLite generation");
        let original_mtime = std::fs::metadata(&source)
            .expect("read source metadata")
            .modified()
            .expect("read source mtime");
        let first = preview_cache_key_for_page(&source, 128, 0);

        std::fs::hard_link(&source, &alias).expect("create hard-link alias");
        std::fs::write(&alias, b"SQLite format 3\0next--generation")
            .expect("replace bytes through hard-link alias");
        std::fs::File::options()
            .write(true)
            .open(&source)
            .expect("open source to restore mtime")
            .set_times(std::fs::FileTimes::new().set_modified(original_mtime))
            .expect("restore source mtime");
        let second = preview_cache_key_for_page(&source, 128, 0);

        assert_ne!(
            first, second,
            "cache keys must include the full snapshot fingerprint, not object identity"
        );

        std::fs::remove_file(alias).expect("remove hard-link alias");
        std::fs::remove_file(source).expect("remove SQLite fixture");
    }

    #[test]
    fn camera_raw_thumbnail_and_full_preview_use_the_bounded_renderer() {
        let path = std::env::temp_dir().join(format!(
            "ottrin-camera-raw-thumbnail-{}.ARW",
            std::process::id()
        ));
        let raw = raw_tiff_with_embedded_jpeg(&small_jpeg());
        std::fs::write(&path, &raw).expect("write camera RAW fixture");

        let thumbnail = build_thumbnail_bytes(&path, 128).expect("RAW thumbnail");
        let full = full_preview_bytes_at_page(&path, 128, 0).expect("RAW full preview");
        let (cached, rebuilt) =
            thumbnail_bytes_from_cache_or_build(&path, 128, Some(raw)).expect("rebuilt RAW cache");

        assert!(is_png_bytes(&thumbnail));
        assert!(is_png_bytes(&full));
        assert!(is_png_bytes(&cached));
        assert!(rebuilt, "raw cache bytes must not bypass the RAW renderer");
        assert!(image::load_from_memory(&thumbnail).is_ok());
        std::fs::remove_file(path).expect("remove camera RAW fixture");
    }

    #[test]
    fn camera_raw_rebuilds_magic_only_png_cache_entries() {
        let path = std::env::temp_dir().join(format!(
            "ottrin-camera-raw-magic-cache-{}.dng",
            std::process::id()
        ));
        std::fs::write(&path, raw_tiff_with_embedded_jpeg(&small_jpeg()))
            .expect("write camera RAW fixture");
        let fake_png = b"\x89PNG\r\n\x1a\nnot actually a PNG".to_vec();

        let (bytes, rebuilt) = thumbnail_bytes_from_cache_or_build(&path, 128, Some(fake_png))
            .expect("RAW preview should rebuild a structurally invalid cache entry");

        assert!(rebuilt, "magic-only cache bytes must be rebuilt");
        assert!(image::load_from_memory(&bytes).is_ok());
        std::fs::remove_file(path).expect("remove camera RAW fixture");
    }

    #[test]
    fn camera_raw_cache_key_changes_when_the_container_changes() {
        let path = std::env::temp_dir().join(format!(
            "ottrin-camera-raw-cache-key-{}.dng",
            std::process::id()
        ));
        std::fs::write(&path, raw_tiff_with_embedded_jpeg(&small_jpeg()))
            .expect("write camera RAW fixture");
        let before = preview_cache_key_for_page(&path, 128, 0);
        std::fs::write(
            &path,
            [raw_tiff_with_embedded_jpeg(&small_jpeg()), vec![0]].concat(),
        )
        .expect("replace camera RAW fixture");
        let after = preview_cache_key_for_page(&path, 128, 0);

        assert_ne!(before, after);
        std::fs::remove_file(path).expect("remove camera RAW fixture");
    }

    #[test]
    fn camera_raw_cache_entry_is_discarded_before_thumbnail_bytes_are_read() {
        let path = std::env::temp_dir().join(format!(
            "ottrin-camera-raw-cache-policy-{}.dng",
            std::process::id()
        ));
        let cache_path = std::env::temp_dir().join(format!(
            "ottrin-camera-raw-stale-cache-{}.bin",
            std::process::id()
        ));
        std::fs::write(&path, raw_tiff_with_embedded_jpeg(&small_jpeg()))
            .expect("write camera RAW fixture");
        std::fs::write(&cache_path, b"stale camera RAW bytes").expect("write cache entry");

        assert_eq!(load_cached_thumbnail_at_path(&path, &cache_path), None);
        assert!(!cache_path.exists(), "unsafe RAW cache must be discarded");
        std::fs::remove_file(path).expect("remove camera RAW fixture");
    }

    #[test]
    fn camera_raw_cache_reader_bounds_replacement_growth() {
        struct ObservedReader {
            bytes: Vec<u8>,
            requested: Vec<usize>,
        }

        impl Read for ObservedReader {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.requested.push(buffer.len());
                let count = self.bytes.len().min(buffer.len());
                buffer[..count].copy_from_slice(&self.bytes[..count]);
                self.bytes.drain(..count);
                Ok(count)
            }
        }

        let limit = usize::try_from(MAX_CAMERA_RAW_CACHE_PNG_BYTES).unwrap();
        let mut reader = ObservedReader {
            bytes: vec![0; limit + 1],
            requested: Vec::new(),
        };

        assert_eq!(read_bounded_camera_raw_cache(&mut reader).unwrap(), None);
        assert!(reader.requested.iter().all(|&size| size <= limit + 1));
        assert_eq!(reader.requested.iter().sum::<usize>(), limit + 1);
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
        let pptx = Path::new("deck.PPTX");
        assert_eq!(
            ottrin_preview::preview_profile(pptx).visual_renderer(),
            ottrin_preview::PreviewVisualRenderer::None
        );
        assert!(!ottrin_preview::preview_profile(pptx).raw_fallback_allowed());
        assert_eq!(fallback_preview_bytes(pptx, None), None);
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
    fn font_thumbnail_and_full_preview_use_dedicated_visual_renderer() {
        let path =
            std::env::temp_dir().join(format!("ottrin-font-preview-{}.ttf", std::process::id()));
        write_material_font(&path);

        let direct =
            ottrin_preview::render_font_preview_png(&path, 256).expect("font visual render");
        let thumbnail = build_thumbnail_bytes(&path, 256).expect("font thumbnail");
        let full = full_preview_bytes_at_page(&path, 256, 0).expect("font full preview");

        assert_eq!(
            thumbnail, direct,
            "thumbnail must be routed through font renderer"
        );
        assert_eq!(
            full, direct,
            "full preview must be routed through font renderer"
        );
        let raw = std::fs::read(&path).expect("read source font");
        assert_ne!(thumbnail, raw, "font visual output must not be raw bytes");
        assert!(image::load_from_memory(&thumbnail).is_ok());
        assert!(image::load_from_memory(&full).is_ok());

        std::fs::remove_file(path).expect("remove font fixture");
    }

    #[test]
    fn font_thumbnail_rebuilds_from_stale_non_png_cache_entry() {
        let path = std::env::temp_dir().join(format!(
            "ottrin-font-cache-policy-{}.ttf",
            std::process::id()
        ));
        write_material_font(&path);
        let (cached, rebuilt) =
            thumbnail_bytes_from_cache_or_build(&path, 256, Some(b"not a png".to_vec()))
                .expect("font thumbnail with stale cache");

        assert!(
            rebuilt,
            "font preview must not consume stale raw cache bytes"
        );
        assert!(is_png_bytes(&cached));
        assert!(image::load_from_memory(&cached).is_ok());
        std::fs::remove_file(path).expect("remove font fixture");
    }

    #[test]
    fn font_cache_rejects_oversized_png_named_entries_before_loading_them() {
        let font_path = std::env::temp_dir().join(format!(
            "ottrin-font-cache-bounds-{}.ttf",
            std::process::id()
        ));
        let cache_path = std::env::temp_dir().join(format!(
            "ottrin-font-cache-bounds-{}.png",
            std::process::id()
        ));
        write_material_font(&font_path);
        std::fs::write(&cache_path, b"\x89PNG\r\n\x1a\n").expect("write cache header");
        std::fs::File::options()
            .write(true)
            .open(&cache_path)
            .expect("open oversized cache")
            .set_len(MAX_FONT_CACHE_PNG_BYTES + 1)
            .expect("extend cache past limit");

        assert!(load_cached_thumbnail_at_path(&font_path, &cache_path).is_none());
        assert!(
            !cache_path.exists(),
            "unsafe font cache data must be discarded rather than passed to the image loader"
        );

        std::fs::remove_file(font_path).expect("remove font fixture");
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
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn eps_rejects_stale_raster_cache_and_uses_only_the_isolated_renderer() {
        if !ottrin_preview::eps_preview_runtime_is_available() {
            return;
        }
        let fixture = tempfile::tempdir().expect("private EPS fixture");
        let eps = fixture.path().join("document.eps");
        std::fs::write(
            &eps,
            b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 160 100\n1 0 0 setrgbcolor\n10 10 140 80 rectfill\nshowpage\n",
        )
        .expect("write EPS fixture");
        let stale_cache = b"not renderer-produced PNG data".to_vec();

        let (thumbnail, should_persist) =
            thumbnail_bytes_from_cache_or_build(&eps, 128, Some(stale_cache.clone()))
                .expect("isolated EPS renderer must replace the stale cache");
        assert_ne!(
            thumbnail, stale_cache,
            "EPS must not reuse a stale raster cache"
        );
        assert!(
            should_persist,
            "fresh isolated render must replace the cache"
        );
        assert!(image::load_from_memory(&thumbnail).is_ok());
        assert!(full_preview_bytes_at_page(&eps, 128, 0).is_some());
    }

    #[test]
    fn blocked_svg_thumbnail_preserves_its_actionable_admission_reason() {
        let path = std::env::temp_dir().join(format!(
            "ottrin-svg-visible-error-{}.svg",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r##"<svg xmlns="http://www.w3.org/2000/svg"><use href="#shape"/></svg>"##,
        )
        .expect("write blocked SVG fixture");

        let error = build_thumbnail_bytes_with_error(&path, 128)
            .expect_err("blocked SVG must retain its admission error");
        assert!(
            error.contains("reuse"),
            "unexpected visible reason: {error}"
        );

        std::fs::remove_file(path).expect("remove blocked SVG fixture");
    }

    #[test]
    fn blocked_svg_rejects_a_stale_raster_cache_before_both_preview_routes() {
        let valid = std::env::temp_dir().join(format!(
            "ottrin-valid-svg-cache-source-{}.svg",
            std::process::id()
        ));
        let blocked = std::env::temp_dir().join(format!(
            "ottrin-blocked-svg-raster-cache-{}.svg",
            std::process::id()
        ));
        std::fs::write(
            &valid,
            r##"<svg width="12" height="12" xmlns="http://www.w3.org/2000/svg"><rect width="12" height="12" fill="#00ff00"/></svg>"##,
        )
        .expect("write valid SVG fixture");
        std::fs::write(
            &blocked,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><image href = "file:///tmp/private.png"/></svg>"#,
        )
        .expect("write blocked SVG fixture");
        let stale_png = build_thumbnail_bytes(&valid, 256).expect("render valid SVG fixture");

        assert_eq!(
            thumbnail_bytes_from_cache_or_build(&blocked, 256, Some(stale_png)),
            None,
            "a blocked SVG must not reuse a stale raster cache in the selected-result route"
        );
        assert_eq!(
            full_preview_bytes_at_page(&blocked, 256, 0),
            None,
            "a blocked SVG must not produce a full inspector preview"
        );

        std::fs::remove_file(valid).expect("remove valid SVG fixture");
        std::fs::remove_file(blocked).expect("remove blocked SVG fixture");
    }

    #[test]
    fn epub_never_uses_raw_bytes_for_thumbnail_or_full_preview() {
        let path = std::env::temp_dir().join(format!(
            "ottrin-epub-raw-preview-{}.epub",
            std::process::id()
        ));
        std::fs::write(&path, b"PK\x03\x04untrusted EPUB package bytes")
            .expect("write EPUB fixture");
        let raw = std::fs::read(&path).expect("read EPUB fixture");

        assert_eq!(
            thumbnail_bytes_from_cache_or_build(&path, 256, Some(raw.clone())),
            None,
            "a stale raw EPUB cache entry must not be returned as a thumbnail"
        );
        assert_eq!(
            full_preview_bytes_at_page(&path, 2_048, 0),
            None,
            "the full-preview path must not read raw EPUB bytes"
        );

        std::fs::remove_file(path).expect("remove EPUB fixture");
    }

    #[test]
    fn epub_cache_entry_is_discarded_before_thumbnail_bytes_are_read() {
        let path = std::env::temp_dir().join(format!(
            "ottrin-epub-cache-policy-{}.epub",
            std::process::id()
        ));
        let cache_path = std::env::temp_dir().join(format!(
            "ottrin-epub-cache-policy-{}.png",
            std::process::id()
        ));
        std::fs::write(&path, b"EPUB package path is sufficient for policy").expect("write EPUB");
        std::fs::write(&cache_path, b"stale EPUB cache bytes").expect("write cache entry");

        assert_eq!(
            load_cached_thumbnail_at_path(&path, &cache_path),
            None,
            "disabled EPUB must reject a cache entry before it can reach a decoder"
        );
        assert!(
            !cache_path.exists(),
            "a stale EPUB cache entry must be removed without returning its bytes"
        );

        std::fs::remove_file(path).expect("remove EPUB fixture");
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

    #[test]
    fn stl_thumbnail_and_full_preview_are_content_keyed_pngs_without_raw_fallback() {
        let path =
            std::env::temp_dir().join(format!("ottrin-stl-preview-{}.stl", std::process::id()));
        let first = b"solid tetra\nfacet normal 0 0 1\n outer loop\n  vertex 0 0 0\n  vertex 1 0 0\n  vertex 0 1 1\n endloop\nendfacet\nendsolid tetra\n";
        let replacement = b"solid tetra\nfacet normal 0 1 0\n outer loop\n  vertex 0 0 0\n  vertex 2 0 0\n  vertex 0 1 1\n endloop\nendfacet\nendsolid tetra\n";
        std::fs::write(&path, first).expect("write STL");
        let first_key = preview_cache_key_for_page(&path, 128, 0);
        let thumbnail = build_thumbnail_bytes(&path, 128).expect("STL thumbnail");
        let full = full_preview_bytes_at_page(&path, 128, 0).expect("STL full preview");
        assert!(stl_cached_png_is_safe(&thumbnail));
        assert_eq!(thumbnail, full, "shared STL renderer must serve both paths");
        assert_ne!(thumbnail, first);
        assert_eq!(fallback_preview_bytes(&path, None), None);

        std::fs::write(&path, replacement).expect("replace STL");
        assert_ne!(first_key, preview_cache_key_for_page(&path, 128, 0));
        let (rebuilt, should_persist) =
            thumbnail_bytes_from_cache_or_build(&path, 128, Some(first.to_vec()))
                .expect("valid replacement must rebuild a safe STL preview");
        assert!(should_persist, "raw stale STL cache bytes must be rebuilt");
        assert!(stl_cached_png_is_safe(&rebuilt));
        std::fs::write(&path, b"not a mesh").expect("replace with malformed STL");
        assert_eq!(thumbnail_bytes_from_cache_or_build(&path, 128, None), None);
        assert_eq!(full_preview_bytes_at_page(&path, 128, 0), None);
        let cache_path = path.with_extension("cached.png");
        std::fs::write(&cache_path, &thumbnail).expect("write stale STL cache PNG");
        assert_eq!(load_cached_thumbnail_at_path(&path, &cache_path), None);
        assert!(
            !cache_path.exists(),
            "malformed STL must discard cached PNG"
        );
        std::fs::remove_file(path).expect("remove STL fixture");
    }

    #[test]
    fn obj_thumbnail_and_full_preview_are_content_keyed_pngs_without_raw_fallback() {
        let path =
            std::env::temp_dir().join(format!("ottrin-obj-preview-{}.obj", std::process::id()));
        let source = b"o bracket\nv 0 0 0\nv 1 0 0\nv 0 1 1\nf 1 2 3\n";
        std::fs::write(&path, source).expect("write OBJ");

        let thumbnail = build_thumbnail_bytes(&path, 128).expect("OBJ thumbnail");
        let full = full_preview_bytes_at_page(&path, 128, 0).expect("OBJ full preview");
        assert!(obj_cached_png_is_safe(&thumbnail));
        assert_eq!(thumbnail, full);
        assert_ne!(thumbnail, source);
        assert_eq!(fallback_preview_bytes(&path, None), None);

        let original_mtime = std::fs::metadata(&path)
            .expect("read source metadata")
            .modified()
            .expect("read source mtime");
        let first_key = preview_cache_key_for_page(&path, 128, 0);
        let alias = path.with_extension("alias.obj");
        std::fs::hard_link(&path, &alias).expect("create hard-link alias");
        let replacement = b"o bracket\nv 0 0 0\nv 2 0 0\nv 0 1 1\nf 1 2 3\n";
        assert_eq!(
            source.len(),
            replacement.len(),
            "replacement stays same-sized"
        );
        std::fs::write(&alias, replacement).expect("replace through hard-link alias");
        std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("open source")
            .set_times(std::fs::FileTimes::new().set_modified(original_mtime))
            .expect("restore source mtime");
        assert_ne!(first_key, preview_cache_key_for_page(&path, 128, 0));
        std::fs::write(&path, b"not an OBJ mesh").expect("replace with invalid OBJ");
        assert_eq!(
            thumbnail_bytes_from_cache_or_build(&path, 128, Some(thumbnail)),
            None
        );

        std::fs::remove_file(path).expect("remove OBJ fixture");
        std::fs::remove_file(alias).expect("remove OBJ alias");
    }

    #[test]
    fn obj_cache_is_bounded_before_decoding_or_allocation() {
        let path =
            std::env::temp_dir().join(format!("ottrin-obj-cache-{}.obj", std::process::id()));
        let cache = path.with_extension("png");
        std::fs::write(&path, b"v 0 0 0\nv 1 0 0\nv 0 1 1\nf 1 2 3\n").expect("write OBJ");
        std::fs::write(&cache, b"\x89PNG\r\n\x1a\n").expect("write cache header");
        std::fs::File::options()
            .write(true)
            .open(&cache)
            .expect("open cache")
            .set_len(MAX_OBJ_CACHE_PNG_BYTES + 1)
            .expect("extend cache");

        assert_eq!(load_cached_thumbnail_at_path(&path, &cache), None);
        assert!(
            !cache.exists(),
            "oversized OBJ cache must be removed before image decode"
        );
        std::fs::remove_file(path).expect("remove OBJ");
    }

    #[test]
    fn ply_thumbnail_compact_and_full_routes_use_only_content_keyed_pngs() {
        let path =
            std::env::temp_dir().join(format!("ottrin-ply-preview-{}.ply", std::process::id()));
        let source = b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 1\n3 0 1 2\n";
        std::fs::write(&path, source).expect("write PLY fixture");
        let thumbnail = build_thumbnail_bytes(&path, 128).expect("PLY thumbnail");
        let full = full_preview_bytes_at_page(&path, 128, 0).expect("PLY full preview");
        assert!(ply_cached_png_is_safe(&thumbnail));
        assert_eq!(
            thumbnail, full,
            "shared PLY renderer must serve thumbnail and Inspector"
        );
        assert_ne!(thumbnail, source);
        assert_eq!(fallback_preview_bytes(&path, None), None);

        let first_key = preview_cache_key_for_page(&path, 128, 0);
        let original_mtime = std::fs::metadata(&path)
            .expect("read PLY metadata")
            .modified()
            .expect("read PLY mtime");
        let replacement = b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n2 0 0\n0 1 1\n3 0 1 2\n";
        assert_eq!(
            source.len(),
            replacement.len(),
            "replacement stays same-sized"
        );
        let alias = path.with_extension("alias.ply");
        std::fs::hard_link(&path, &alias).expect("create PLY hard-link alias");
        std::fs::write(&alias, replacement).expect("replace PLY source through hard-link alias");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open PLY source with attribute-write access")
            .set_times(std::fs::FileTimes::new().set_modified(original_mtime))
            .expect("restore PLY mtime");
        assert_ne!(first_key, preview_cache_key_for_page(&path, 128, 0));

        std::fs::write(&path, b"not a PLY mesh").expect("replace with invalid PLY");
        assert_eq!(
            thumbnail_bytes_from_cache_or_build(&path, 128, Some(thumbnail.clone())),
            None
        );
        assert_eq!(full_preview_bytes_at_page(&path, 128, 0), None);
        let cache = path.with_extension("cached.png");
        std::fs::write(&cache, thumbnail).expect("write stale PLY cache");
        assert_eq!(load_cached_thumbnail_at_path(&path, &cache), None);
        assert!(
            !cache.exists(),
            "invalid PLY source must discard cached preview"
        );
        std::fs::remove_file(path).expect("remove PLY fixture");
        std::fs::remove_file(alias).expect("remove PLY hard-link alias");
    }

    #[test]
    fn threemf_thumbnail_compact_and_full_routes_use_only_content_keyed_pngs() {
        let path =
            std::env::temp_dir().join(format!("ottrin-3mf-preview-{}.3mf", std::process::id()));
        write_3mf_fixture(&path);
        let thumbnail = build_thumbnail_bytes(&path, 128).expect("3MF thumbnail");
        let full = full_preview_bytes_at_page(&path, 128, 0).expect("3MF full preview");
        assert!(threemf_cached_png_is_safe(&thumbnail));
        assert_eq!(
            thumbnail, full,
            "shared 3MF renderer must serve thumbnail and Inspector"
        );
        assert_ne!(thumbnail, std::fs::read(&path).expect("read package"));
        assert_eq!(fallback_preview_bytes(&path, None), None);
        let key = preview_cache_key_for_page(&path, 128, 0);
        std::fs::write(&path, b"not a 3MF package").expect("replace 3MF source");
        assert_ne!(key, preview_cache_key_for_page(&path, 128, 0));
        assert_eq!(
            thumbnail_bytes_from_cache_or_build(&path, 128, Some(thumbnail.clone())),
            None
        );
        let cache = path.with_extension("cached.png");
        std::fs::write(&cache, thumbnail).expect("write stale 3MF cache");
        assert_eq!(load_cached_thumbnail_at_path(&path, &cache), None);
        assert!(
            !cache.exists(),
            "invalid 3MF source must discard cached preview"
        );
        std::fs::remove_file(path).expect("remove 3MF fixture");
    }
}
