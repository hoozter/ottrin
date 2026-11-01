//! Atomic, bounded per-user integration files. Modified ownership is checked by callers.
use std::{fs, io::Write, path::Path};
pub(super) fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
        Ok(metadata) if !metadata.is_file() || metadata.len() > 1024 * 1024 => Err(format!(
            "Unsafe or oversized configuration retained: {}",
            path.display()
        )),
        Ok(_) => fs::read(path).map(Some).map_err(|e| e.to_string()),
    }
}
pub(super) fn read_text(path: &Path) -> Result<Option<String>, String> {
    read_bytes(path)?
        .map(|data| String::from_utf8(data).map_err(|e| e.to_string()))
        .transpose()
}
pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Missing integration directory")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    read_bytes(path)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    fs::File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())
}

pub(super) struct Directories {
    pub config: std::path::PathBuf,
    pub data: std::path::PathBuf,
    pub state: std::path::PathBuf,
}
impl Directories {
    pub fn current() -> Result<Self, String> {
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .ok_or("HOME is unavailable")?;
        let root = |key, default: &str| {
            std::env::var_os(key)
                .filter(|value| !value.is_empty())
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| home.join(default))
        };
        Ok(Self {
            config: root("XDG_CONFIG_HOME", ".config"),
            data: root("XDG_DATA_HOME", ".local/share"),
            state: root("XDG_STATE_HOME", ".local/state"),
        })
    }
}
