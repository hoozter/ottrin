use ottrin_core::AppConfig;
#[cfg(not(target_os = "windows"))]
use ottrin_core::default_home_dir;
use std::path::{Path, PathBuf};

fn config_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
    #[cfg(not(target_os = "windows"))]
    return Some(config_path_for_unix_user(
        // SAFETY: geteuid has no preconditions and no side effects.
        unsafe { libc::geteuid() },
        std::env::var_os("XDG_CONFIG_HOME")
            .as_deref()
            .map(Path::new),
        &default_home_dir(),
    ));
    #[cfg(target_os = "windows")]
    let mut p = base;
    #[cfg(target_os = "windows")]
    p.push("ottrin");
    #[cfg(target_os = "windows")]
    p.push("config.json");
    #[cfg(target_os = "windows")]
    Some(p)
}

#[cfg(not(target_os = "windows"))]
fn config_path_for_unix_user(
    effective_uid: u32,
    xdg_config_home: Option<&Path>,
    home: &Path,
) -> PathBuf {
    if effective_uid == 0 {
        return PathBuf::from("/root/.config/ottrin-administrator/config.json");
    }
    xdg_config_home
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".config"))
        .join("ottrin/config.json")
}

#[cfg(not(target_os = "windows"))]
fn default_config_for_unix_user(effective_uid: u32) -> AppConfig {
    let mut config = AppConfig::default();
    if effective_uid == 0 {
        config.search.include_roots.clear();
        config.search.active_indexed_volume_roots.clear();
        config.search.content_indexing_enabled = false;
        config.search.content_include_roots.clear();
        config.search.content_exclude_roots.clear();
    }
    config
}

fn default_config() -> AppConfig {
    #[cfg(not(target_os = "windows"))]
    {
        // SAFETY: geteuid has no preconditions and no side effects.
        default_config_for_unix_user(unsafe { libc::geteuid() })
    }
    #[cfg(target_os = "windows")]
    AppConfig::default()
}

pub fn load_config() -> AppConfig {
    let path = match config_path() {
        Some(p) => p,
        None => return default_config(),
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return default_config(),
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|_| default_config())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn new_administrator_profile_has_no_implicitly_selected_index_roots() {
        let config = default_config_for_unix_user(0);

        assert!(config.search.include_roots.is_empty());
        assert!(!config.search.content_indexing_enabled);
        assert!(config.search.content_include_roots.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn administrator_config_path_ignores_preserved_user_environment() {
        assert_eq!(
            config_path_for_unix_user(
                0,
                Some(Path::new("/home/alice/.config")),
                Path::new("/home/alice"),
            ),
            PathBuf::from("/root/.config/ottrin-administrator/config.json")
        );
    }
}
