#[cfg(not(target_os = "windows"))]
use ottrin_core::default_home_dir;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};

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
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    if let Ok(meta) = std::fs::metadata(path) {
        meta.len().hash(&mut hasher);
        if let Ok(mtime) = meta.modified()
            && let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH)
        {
            dur.as_secs().hash(&mut hasher);
        }
    }
    target_dim.hash(&mut hasher);
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
