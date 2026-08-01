use ottrin_core::AppConfig;
#[cfg(not(target_os = "windows"))]
use ottrin_core::default_home_dir;
use std::io::ErrorKind;
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
    load_config_checked().unwrap_or_default()
}

pub fn load_config_checked() -> Result<AppConfig, String> {
    let path = match config_path() {
        Some(p) => p,
        None => return Ok(AppConfig::default()),
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(AppConfig::default()),
        Err(err) => {
            return Err(format!(
                "failed to read config file {}: {}",
                path.display(),
                err
            ));
        }
    };
    serde_json::from_slice(&bytes)
        .map_err(|err| format!("invalid config file {}: {}", path.display(), err))
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
    use ottrin_core::{SearchScope, ThemeMode};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn with_temp_xdg_config<T>(test: T)
    where
        T: FnOnce(&PathBuf),
    {
        let workspace = std::env::temp_dir();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = workspace.join(format!("ottrin-ui-config-test-{unique}"));
        let config_dir = home.join("ottrin");
        fs::create_dir_all(&config_dir).unwrap();
        let old = std::env::var_os("XDG_CONFIG_HOME");
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &home);
        }
        test(&config_dir);
        match old {
            Some(value) => unsafe {
                std::env::set_var("XDG_CONFIG_HOME", value);
            },
            None => unsafe {
                std::env::remove_var("XDG_CONFIG_HOME");
            },
        }
        let _ = fs::remove_dir_all(home);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn checked_config_loads_partial_valid_payload_with_defaults() {
        with_temp_xdg_config(|config_dir| {
            fs::write(
                config_dir.join("config.json"),
                r#"{"search":{"include_roots":["/tmp/fx"]}}"#,
            )
            .unwrap();

            let config = load_config_checked().unwrap();
            assert_eq!(config.search.include_roots, vec![PathBuf::from("/tmp/fx")]);
            assert_eq!(config.theme, ThemeMode::System);
            assert_eq!(config.search.default_scope, SearchScope::Global);
            assert!(!config.search.exclude_roots.is_empty());
        });
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn checked_config_rejects_malformed_json() {
        with_temp_xdg_config(|config_dir| {
            fs::write(
                config_dir.join("config.json"),
                r#"{"search":{"include_roots":1}}"#,
            )
            .unwrap();

            let err = load_config_checked().unwrap_err();
            assert!(err.contains("invalid config file"));
        });
    }
}
