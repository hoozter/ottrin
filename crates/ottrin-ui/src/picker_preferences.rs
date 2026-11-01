//! Picker-only preferences. Explicit caller locations always take precedence.
use ottrin_core::native_path::NativePath;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const FILE_NAME: &str = "picker.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PickerPreferences {
    pub grid: bool,
    pub hidden: bool,
    pub preview: bool,
    pub last_directory: Option<NativePath>,
}
impl PickerPreferences {
    pub(crate) fn directory(&self) -> Result<Option<PathBuf>, String> {
        self.last_directory
            .as_ref()
            .map(|path| {
                let path = path.decode().map_err(|error| error.to_string())?;
                if !path.is_absolute() || path.to_string_lossy().contains('\0') {
                    return Err("Invalid remembered picker folder".into());
                }
                Ok(path)
            })
            .transpose()
    }
    pub(crate) fn load() -> Result<Self, String> {
        let path = preferences_path()?;
        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(format!("Could not read picker preferences: {error}")),
        };
        let mut bytes = Vec::new();
        file.take(256 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > 256 * 1024 {
            return Err("Picker preferences exceed 256 KiB".into());
        }
        let preferences: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Invalid picker preferences: {error}"))?;
        preferences.directory()?;
        Ok(preferences)
    }
    pub(crate) fn save(&self) -> Result<(), String> {
        self.directory()?;
        write_preferences(&preferences_path()?, self)
    }
}
fn preferences_path() -> Result<PathBuf, String> {
    crate::config::config_path()
        .map(|path| path.with_file_name(FILE_NAME))
        .ok_or_else(|| "The user configuration directory is unavailable".into())
}
fn write_preferences(path: &Path, preferences: &PickerPreferences) -> Result<(), String> {
    static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or("Picker preferences have no parent folder")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(
        ".picker-{}-{}.tmp",
        std::process::id(),
        NEXT_WRITE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let result = (|| {
        let bytes = serde_json::to_vec_pretty(preferences).map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(|error| error.to_string())
    })();
    if let Err(error) = &result {
        match std::fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(cleanup) => {
                return Err(format!(
                    "{}; temporary preferences remain at {}: {cleanup}",
                    error,
                    temporary.display()
                ));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picker_preferences_round_trip_without_changing_main_configuration() {
        let main = crate::config::config_path().unwrap();
        std::fs::create_dir_all(main.parent().unwrap()).unwrap();
        std::fs::write(&main, b"preserve main preferences").unwrap();
        let directory = tempfile::tempdir().unwrap();
        let prefs = PickerPreferences {
            grid: true,
            hidden: true,
            preview: true,
            last_directory: Some(NativePath::encode(directory.path())),
        };
        prefs.save().unwrap();
        let restored = PickerPreferences::load().unwrap();
        assert!(restored.grid && restored.hidden && restored.preview);
        assert_eq!(
            restored.directory().unwrap().as_deref(),
            Some(directory.path())
        );
        assert_eq!(std::fs::read(main).unwrap(), b"preserve main preferences");
        assert_eq!(
            preferences_path()
                .unwrap()
                .parent()
                .unwrap()
                .read_dir()
                .unwrap()
                .count(),
            2
        );
    }
    #[cfg(unix)]
    #[test]
    fn remembered_folder_preserves_non_utf8_native_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/folder/\xff".to_vec()));
        let prefs = PickerPreferences {
            last_directory: Some(NativePath::encode(&path)),
            ..Default::default()
        };
        prefs.save().unwrap();
        assert_eq!(
            PickerPreferences::load().unwrap().directory().unwrap(),
            Some(path)
        );
    }
}
