use ottrin_core::AppConfig;
#[cfg(not(target_os = "windows"))]
use ottrin_core::default_home_dir;
use std::path::{Path, PathBuf};

fn config_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut h = default_home_dir();
            h.push(".config");
            h
        });
    let mut p = base;
    p.push("ottrin");
    p.push("config.json");
    Some(p)
}

pub fn load_config() -> AppConfig {
    let path = match config_path() {
        Some(p) => p,
        None => return AppConfig::default(),
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return AppConfig::default(),
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn remove_config_file() -> Result<u64, String> {
    let Some(path) = config_path() else {
        return Ok(0);
    };
    remove_file_counting_bytes(&path)
}

pub(crate) fn save_config(config: &AppConfig) {
    let path = match config_path() {
        Some(p) => p,
        None => return,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(path, json);
    }
}

fn remove_file_counting_bytes(path: &Path) -> Result<u64, String> {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let len = meta.len();
            std::fs::remove_file(path)
                .map_err(|e| format!("Failed to remove {}: {}", path.display(), e))?;
            Ok(len)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(err) => Err(format!("Failed to inspect {}: {}", path.display(), err)),
    }
}
