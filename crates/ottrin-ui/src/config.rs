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
    let top = value.as_object().ok_or_else(|| {
        format!(
            "invalid config file {}: expected JSON object",
            path.display()
        )
    })?;
    validate_known_keys(top, "config file", path, STRICT_CONFIG_KEYS)?;
    validate_strict_search_fragment(&value, path)?;

    serde_json::from_value(value)
        .map_err(|err| format!("invalid config file {}: {}", path.display(), err))
}

fn validate_strict_search_fragment(value: &serde_json::Value, path: &Path) -> Result<(), String> {
    let Some(search) = value.get("search") else {
        return Ok(());
    };
    if search.is_null() {
        return Ok(());
    }
    let search_obj = search.as_object().ok_or_else(|| {
        format!(
            "invalid config file {}: search must be an object",
            path.display()
        )
    })?;
    validate_known_keys(search_obj, "search", path, SEARCH_CONFIG_KEYS)?;
    if let Some(indexed_volumes) = search_obj.get("indexed_volumes") {
        for indexed_volume in indexed_volumes.as_array().ok_or_else(|| {
            format!(
                "invalid config file {}: search.indexed_volumes must be an array",
                path.display()
            )
        })? {
            let volume_obj = indexed_volume.as_object().ok_or_else(|| {
                format!(
                    "invalid config file {}: search.indexed_volumes entries must be objects",
                    path.display()
                )
            })?;
            validate_known_keys(
                volume_obj,
                "search.indexed_volumes",
                path,
                SEARCH_INDEXED_VOLUME_KEYS,
            )?;
        }
    }
    Ok(())
}

const STRICT_CONFIG_KEYS: &[&str] = &[
    "theme",
    "theme_preset",
    "theme_custom",
    "theme_custom_by_preset",
    "custom_themes",
    "active_custom_theme",
    "default_view_mode",
    "colorize_file_types",
    "colorize_folder_labels",
    "list_columns",
    "miller_view_scale",
    "list_view_scale",
    "grid_view_scale",
    "command_frame_always_visible",
    "restore_session",
    "show_hidden_files",
    "list_show_thumbnails",
    "grid_show_thumbnails",
    "folder_list_thumbnails",
    "folder_grid_thumbnails",
    "target",
    "last_session_dirs",
    "bookmarks",
    "search",
    "folder_column_widths",
    "miller_column_width_mode",
    "smart_panel_width",
    "window_size",
    "thumbnail_cache_max_mb",
    "graphics_renderer",
    "graphics_compatibility_notice_seen",
];

fn validate_known_keys(
    value: &serde_json::Map<String, serde_json::Value>,
    kind: &str,
    path: &Path,
    allowed: &[&str],
) -> Result<(), String> {
    for key in value.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!(
                "invalid {kind} {}: unknown field `{}`",
                path.display(),
                key
            ));
        }
    }
    Ok(())
}

const SEARCH_CONFIG_KEYS: &[&str] = &[
    "default_scope",
    "include_hidden_system",
    "include_roots",
    "indexed_volumes",
    "exclude_roots",
    "content_indexing_enabled",
    "content_include_roots",
    "content_exclude_roots",
    "include_globs",
    "exclude_globs",
    "default_sort",
    "result_limit",
    "refresh_interval_hours",
    "use_system_locate",
    "manage_locate_db",
    "locate_update_hours",
];

const SEARCH_INDEXED_VOLUME_KEYS: &[&str] = &["identity", "last_known_path"];

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
    fn checked_config_rejects_malformed_json() {
        with_temp_config_file(|config_path| {
            fs::write(config_path, r#"{"search":{"include_roots":1}}"#).unwrap();

            let err = load_config_checked_with_path(config_path).unwrap_err();
            assert!(err.contains("invalid config file"));
        });
    }
}
