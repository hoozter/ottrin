use ottrin_core::AppConfig;
#[cfg(all(not(target_os = "windows"), not(test)))]
use ottrin_core::default_home_dir;
use std::path::{Path, PathBuf};

#[cfg(test)]
pub(crate) fn config_path() -> Option<PathBuf> {
    // Exercise real persistence without reading or overwriting the developer's
    // profile. Each test thread owns its directory and removes it on exit.
    thread_local! {
        static PROFILE: tempfile::TempDir = tempfile::tempdir().expect("private test profile");
    }
    Some(PROFILE.with(|profile| profile.path().join("ottrin/config.json")))
}

#[cfg(not(test))]
pub(crate) fn config_path() -> Option<PathBuf> {
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

pub fn load_config() -> Result<AppConfig, String> {
    let path = match config_path() {
        Some(p) => p,
        None => return Err("Ottrin could not locate your settings directory.".into()),
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && std::fs::symlink_metadata(&path).is_err() =>
        {
            return Ok(default_config());
        }
        Err(error) => {
            return Err(format!(
                "Could not read Ottrin settings {}: {error}. The file has been preserved.",
                path.display()
            ));
        }
    };
    serde_json::from_slice(&bytes).map_err(|error| format!("Could not load Ottrin settings {}: {error}. Correct the file or move it aside to start with defaults; Ottrin has preserved it.", path.display()))
}

pub fn remove_config_file() -> Result<u64, String> {
    let Some(path) = config_path() else {
        return Ok(0);
    };
    let removed = remove_file_counting_bytes(&path)?;
    let picker = path.with_file_name(crate::picker_preferences::FILE_NAME);
    Ok(removed + remove_file_counting_bytes(&picker)?)
}

pub(crate) fn save_config(config: &AppConfig) -> Result<(), String> {
    use std::io::Write;
    let path = config_path().ok_or("Ottrin could not locate your settings directory.")?;
    // A file changed or made unreadable since startup must not be replaced by
    // this process's in-memory settings. Only an absent file uses defaults.
    load_config()?;
    // Preserve a user's linked settings file rather than replacing the link.
    let path = match std::fs::canonicalize(&path) {
        Ok(target) => target,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => path,
        Err(error) => {
            return Err(format!(
                "Could not resolve Ottrin settings {}: {error}",
                path.display()
            ));
        }
    };
    let parent = path
        .parent()
        .ok_or("Ottrin settings path has no parent directory.")?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(parent)?;
        let json = serde_json::to_vec_pretty(config)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&json)?;
        match std::fs::metadata(&path) {
            Ok(metadata) => {
                if metadata.permissions().readonly() {
                    return Err("The settings file is read-only; it has been preserved.".into());
                }
                temporary
                    .as_file()
                    .set_permissions(metadata.permissions())?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        temporary.as_file().sync_all()?;
        temporary.persist(&path)?;
        Ok(())
    })();
    result.map_err(|error| format!("Could not save Ottrin settings {}: {error}", path.display()))
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

    #[test]
    fn invalid_existing_settings_are_reported_and_never_replaced_by_defaults() {
        let path = config_path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for original in [
            b"{ broken settings".as_slice(),
            br#"{"custom_themes":[{"name":"missing base preset"}]}"#,
        ] {
            std::fs::write(&path, original).unwrap();
            let error = load_config().unwrap_err();
            assert!(error.contains(&path.display().to_string()));
            assert!(error.contains("preserved"));
            assert!(save_config(&AppConfig::default()).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), original);
            assert_eq!(
                std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
                1
            );
        }
    }

    #[test]
    fn unreadable_settings_location_is_an_error_instead_of_new_defaults() {
        let path = config_path().unwrap();
        std::fs::create_dir_all(&path).unwrap();
        assert!(load_config().is_err());
        assert!(save_config(&AppConfig::default()).is_err());
        assert!(path.is_dir());
    }

    #[test]
    fn missing_settings_use_defaults_and_atomic_saves_leave_no_staging() {
        let path = config_path().unwrap();
        assert!(!path.exists());
        let mut config = load_config().unwrap();
        config.default_view_mode = ottrin_core::ViewMode::Grid;
        save_config(&config).unwrap();
        config.default_view_mode = ottrin_core::ViewMode::Miller;
        save_config(&config).unwrap();
        assert_eq!(
            load_config().unwrap().default_view_mode,
            ottrin_core::ViewMode::Miller
        );
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_saves_preserve_linked_config_target_and_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let path = config_path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let target = path.with_file_name("linked-settings.json");
        std::fs::write(&target, serde_json::to_vec(&AppConfig::default()).unwrap()).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&target, &path).unwrap();
        let config = AppConfig {
            default_view_mode: ottrin_core::ViewMode::Grid,
            ..AppConfig::default()
        };
        save_config(&config).unwrap();
        assert!(std::fs::symlink_metadata(&path).unwrap().is_symlink());
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(
            load_config().unwrap().default_view_mode,
            ottrin_core::ViewMode::Grid
        );
        std::fs::remove_file(&target).unwrap();
        assert!(load_config().is_err());
        assert!(save_config(&config).is_err());
        assert!(std::fs::symlink_metadata(&path).unwrap().is_symlink());
    }

    #[test]
    fn refused_atomic_save_preserves_original_and_retires_temporary_file() {
        let path = config_path().unwrap();
        let config = AppConfig::default();
        save_config(&config).unwrap();
        let original = std::fs::read(&path).unwrap();
        let permissions = std::fs::metadata(&path).unwrap().permissions();
        let mut protected = permissions.clone();
        protected.set_readonly(true);
        std::fs::set_permissions(&path, protected).unwrap();
        assert!(save_config(&config).unwrap_err().contains("read-only"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        std::fs::set_permissions(&path, permissions).unwrap();
    }

    #[test]
    fn test_profiles_are_private_to_each_test_thread() {
        let here = config_path().expect("test config path");
        let other = std::thread::spawn(|| config_path().expect("other test config path"))
            .join()
            .expect("test thread");
        assert_ne!(here, other, "tests must not share the user's settings path");
    }

    #[test]
    fn saved_view_round_trips_without_crossing_test_profiles() {
        let mut config = AppConfig {
            default_view_mode: ottrin_core::ViewMode::Miller,
            ..AppConfig::default()
        };
        save_config(&config).expect("save private test config");
        assert_eq!(
            load_config()
                .expect("load private test config")
                .default_view_mode,
            ottrin_core::ViewMode::Miller
        );
        let other_path = std::thread::spawn(|| {
            let config = AppConfig {
                default_view_mode: ottrin_core::ViewMode::List,
                ..AppConfig::default()
            };
            save_config(&config).expect("save private test config");
            assert_eq!(
                load_config()
                    .expect("load private test config")
                    .default_view_mode,
                ottrin_core::ViewMode::List
            );
            config_path().expect("other profile")
        })
        .join()
        .expect("other test thread");
        assert!(!other_path.exists(), "test profiles are cleaned up on exit");
        assert_eq!(
            load_config()
                .expect("load private test config")
                .default_view_mode,
            ottrin_core::ViewMode::Miller
        );
        config.default_view_mode = ottrin_core::ViewMode::Grid;
        save_config(&config).expect("save private test config");
        assert_eq!(
            load_config()
                .expect("load private test config")
                .default_view_mode,
            ottrin_core::ViewMode::Grid
        );
    }

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
    #[test]
    fn removing_preferences_clears_main_and_picker_files_and_preserves_other_data() {
        let main = config_path().unwrap();
        std::fs::create_dir_all(main.parent().unwrap()).unwrap();
        let picker = main.with_file_name(crate::picker_preferences::FILE_NAME);
        let keep = main.with_file_name("keep.txt");
        std::fs::write(&main, b"main").unwrap();
        std::fs::write(&picker, b"{}").unwrap();
        std::fs::write(&keep, b"user data").unwrap();
        assert_eq!(remove_config_file().unwrap(), 6);
        assert!(!main.exists() && !picker.exists());
        assert_eq!(std::fs::read(keep).unwrap(), b"user data");
    }
}
