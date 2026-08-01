use ottrin_core::AppConfig;
#[cfg(not(target_os = "windows"))]
use ottrin_core::default_home_dir;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

fn config_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let fallback_home = default_home_dir();
    #[cfg(target_os = "windows")]
    let fallback_home = PathBuf::new();
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let base = config_root_path(base.as_deref(), &fallback_home);
    let p = base;

    #[cfg(windows)]
    let p = match base {
        Some(base) => {
            let mut path = base;
            path.push("ottrin");
            path.push("config.json");
            Some(path)
        }
        None => None,
    };

    #[cfg(not(target_os = "windows"))]
    let p = {
        let mut path = p;
        path.push("ottrin");
        path.push("config.json");
        Some(path)
    };

    p
}

#[cfg(not(target_os = "windows"))]
fn config_root_path(xdg_config_home: Option<&Path>, home: &Path) -> PathBuf {
    match xdg_config_home {
        Some(base) => base.to_path_buf(),
        None => {
            let mut base = home.to_path_buf();
            base.push(".config");
            base
        }
    }
}

#[cfg(target_os = "windows")]
fn config_root_path(appdata: Option<&Path>, _home: &Path) -> PathBuf {
    appdata.map_or_else(PathBuf::new, Path::to_path_buf)
}

pub fn load_config() -> AppConfig {
    load_config_checked().unwrap_or_default()
}

pub fn load_config_checked() -> Result<AppConfig, String> {
    match config_path() {
        Some(path) => load_config_checked_with_path(&path),
        None => Ok(AppConfig::default()),
    }
}

pub(crate) fn load_config_checked_with_path(path: &Path) -> Result<AppConfig, String> {
    let bytes = match std::fs::read(path) {
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

    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|err| format!("invalid config file {}: {}", path.display(), err))?;

    serde_json::from_value::<AppConfig>(value)
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
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn with_temp_config_file<T>(test: T)
    where
        T: FnOnce(&Path),
    {
        let workspace = std::env::temp_dir();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = workspace.join(format!("ottrin-ui-config-test-{unique}"));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.json");
        test(&path);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn checked_config_loads_partial_valid_payload_with_defaults() {
        with_temp_config_file(|config_path| {
            fs::write(config_path, r#"{"search":{"include_roots":["tmp/fx"]}}"#).unwrap();

            let config = load_config_checked_with_path(config_path).unwrap();
            assert_eq!(config.search.include_roots, vec![PathBuf::from("tmp/fx")]);
            assert_eq!(config.theme, ThemeMode::System);
            assert_eq!(config.search.default_scope, SearchScope::Global);
            assert!(!config.search.exclude_roots.is_empty());
        });
    }

    #[test]
    fn checked_config_rejects_unknown_nested_search_field() {
        with_temp_config_file(|config_path| {
            fs::write(config_path, r#"{"search":{"include_root":["tmp/fx"]}}"#).unwrap();

            let err = load_config_checked_with_path(config_path).unwrap_err();
            assert!(err.contains("include_root"));
        });
    }

    #[test]
    fn checked_config_rejects_unknown_top_level_field() {
        with_temp_config_file(|config_path| {
            fs::write(
                config_path,
                r#"{"search":{"include_roots":["tmp/fx"]},"unknown":true}"#,
            )
            .unwrap();

            let err = load_config_checked_with_path(config_path).unwrap_err();
            assert!(err.contains("unknown field `unknown`"));
        });
    }

    #[test]
    fn checked_config_rejects_unknown_nested_indexed_volume_field() {
        with_temp_config_file(|config_path| {
            fs::write(
                config_path,
                r#"{"search":{"indexed_volumes":[{"identity":"id","last_known_path":"/tmp","surprise":"x"}]}}"#,
            )
            .unwrap();

            let err = load_config_checked_with_path(config_path).unwrap_err();
            assert!(err.contains("unknown field `surprise`"));
        });
    }

    #[test]
    fn checked_config_rejects_malformed_json() {
        with_temp_config_file(|config_path| {
            fs::write(config_path, r#"{"search":{"include_roots":1}}"#).unwrap();

            let err = load_config_checked_with_path(config_path).unwrap_err();
            assert!(err.contains("invalid config file"));
        });
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn config_path_uses_injected_xdg_config_home_without_env_mutation() {
        let xdg = PathBuf::from("/tmp/injected-xdg-config");
        let path = config_root_path(Some(&xdg), &PathBuf::from("/tmp/unused"));
        assert_eq!(
            path.join("ottrin").join("config.json"),
            xdg.join("ottrin").join("config.json")
        );
    }

    #[cfg(windows)]
    #[test]
    fn config_path_uses_injected_appdata_without_env_mutation() {
        use std::path::Path;
        let appdata = PathBuf::from("C:/injected-appdata");
        let path = config_root_path(Some(appdata.as_path()), Path::new(""));
        assert_eq!(
            path.join("ottrin").join("config.json"),
            appdata.join("ottrin").join("config.json")
        );
    }
}
