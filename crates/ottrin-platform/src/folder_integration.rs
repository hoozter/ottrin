//! Explicit local-directory association; other MIME types and applications are retained.
use super::{
    integration_preferences as preferences,
    integration_storage::{self as storage, Directories},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
const FILE_MANAGER_BUS: &str = "org.freedesktop.FileManager1";
const ID: &str = "org.ottrin.Ottrin.Folders.desktop";
const GROUP: &str = "Default Applications";
const MIME: &str = "inode/directory";
const ICON: &[u8] = include_bytes!("../../../assets/icon.png");
#[derive(Clone, Debug, Serialize)]
pub struct FolderStatus {
    pub enabled: bool,
    pub ready: bool,
    pub detail: String,
}
#[derive(Serialize, Deserialize)]
struct OwnedFile {
    path: PathBuf,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
struct Registration {
    config: PathBuf,
    original: String,
    installed: String,
    previous: Option<String>,
    created: bool,
    files: Vec<OwnedFile>,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn receipt(d: &Directories) -> PathBuf {
    d.state.join("ottrin/folder-registration.json")
}
fn registration(d: &Directories) -> Result<Option<Registration>, String> {
    storage::read_text(&receipt(d))?
        .map(|text| {
            serde_json::from_str(&text)
                .map_err(|e| format!("Folder integration receipt is invalid: {e}"))
        })
        .transpose()
}
fn desktop_names() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .map(|name| format!("{}-mimeapps.list", name.to_ascii_lowercase()))
        .collect()
}
fn configuration(d: &Directories) -> Result<(PathBuf, String), String> {
    // Only defaults are changed. Desktop-specific defaults precede generic user defaults.
    for name in desktop_names()
        .into_iter()
        .chain(std::iter::once("mimeapps.list".into()))
    {
        let path = d.config.join(name);
        if let Some(text) = storage::read_text(&path)? {
            return Ok((path, text));
        }
    }
    Ok((d.config.join("mimeapps.list"), String::new()))
}
fn effective_default(d: &Directories) -> Result<Option<(PathBuf, String)>, String> {
    for name in desktop_names()
        .into_iter()
        .chain(std::iter::once("mimeapps.list".into()))
    {
        let path = d.config.join(name);
        if let Some(text) = storage::read_text(&path)?
            && let Some(value) = preferences::value(&text, GROUP, MIME)?
        {
            return Ok(Some((path, value)));
        }
    }
    Ok(None)
}
fn check_files(record: &Registration) -> Result<(), String> {
    for file in &record.files {
        if let Some(data) = storage::read_bytes(&file.path)?
            && digest(&data) != file.sha256
        {
            return Err(format!(
                "Modified integration file retained: {}",
                file.path.display()
            ));
        }
    }
    Ok(())
}
fn normal_user() -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 {
        Err("Configure folder opening from a normal user session".into())
    } else {
        Ok(())
    }
}
pub fn status() -> FolderStatus {
    let result = (|| {
        let d = Directories::current()?;
        let Some(record) = registration(&d)? else {
            return Ok((
                false,
                false,
                "Off — your desktop keeps its current folder handler".into(),
            ));
        };
        let effective = effective_default(&d)?;
        let intact = record.files.iter().all(|file| {
            storage::read_bytes(&file.path)
                .ok()
                .flatten()
                .is_some_and(|data| digest(&data) == file.sha256)
        });
        let selected = effective.is_some_and(|(path, value)| {
            path == record.config && value.split(';').next() == Some(ID)
        });
        let activation = d
            .data
            .join(format!("dbus-1/services/{FILE_MANAGER_BUS}.service"));
        let complete = record.files.iter().any(|file| file.path == activation);
        let ready = intact && selected && complete;
        Ok::<_, String>((
            true,
            ready,
            if ready {
                "Enabled — directory links open in Ottrin".into()
            } else if intact && complete {
                "Installed — another directory handler is selected".into()
            } else {
                "Folder setup needs repair; existing files have been retained".into()
            },
        ))
    })();
    match result {
        Ok((enabled, ready, detail)) => FolderStatus {
            enabled,
            ready,
            detail,
        },
        Err(detail) => FolderStatus {
            enabled: false,
            ready: false,
            detail,
        },
    }
}
/// Desktop Exec syntax has an INI escaping layer as well as command-line quoting.
fn desktop_argument(path: &Path) -> Result<String, String> {
    let text = path
        .to_str()
        .ok_or("Desktop integration needs a UTF-8 installation path")?;
    if text.contains(['\n', '\r', '=']) {
        return Err("Unsupported newline or equal sign in desktop installation path".into());
    }
    let command = format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('`', "\\`")
            .replace('$', "\\$")
            .replace('%', "%%")
    );
    Ok(command.replace('\\', "\\\\"))
}
pub fn enable() -> Result<String, String> {
    normal_user()?;
    let message = enable_at(&Directories::current()?, &super::application_launch_path()?)?;
    stop_owned_file_manager()?;
    Ok(message)
}
fn enable_at(d: &Directories, executable: &Path) -> Result<String, String> {
    let prior = registration(d)?;
    if let Some(record) = &prior {
        check_files(record)?;
    }
    let (config, current) = if let Some(record) = &prior {
        (
            record.config.clone(),
            storage::read_text(&record.config)?.unwrap_or_default(),
        )
    } else {
        configuration(d)?
    };
    if let Some((effective, _)) = effective_default(d)?
        && effective != config
    {
        return Err("Another desktop-specific setting controls folder opening. Restore this setup first, then enable Ottrin again.".into());
    }
    let current_value = preferences::value(&current, GROUP, MIME)?;
    let still_owned = preferences::restore_provider(current_value.as_deref(), ID, None).0;
    let previous = if still_owned {
        prior.as_ref().and_then(|record| record.previous.clone())
    } else {
        current_value
    };
    let original = if let Some(record) = &prior
        && still_owned
    {
        if current == record.installed {
            record.original.clone()
        } else {
            preferences::replace(&current, GROUP, MIME, record.previous.as_deref())?
        }
    } else {
        current.clone()
    };
    let created = if let Some(record) = &prior {
        record.created && current == record.installed
    } else {
        storage::read_text(&config)?.is_none()
    };
    let icon = d.data.join("ottrin/integration/icon.png");
    let icon_value = icon
        .to_str()
        .ok_or("Desktop integration needs a UTF-8 data directory")?;
    if icon_value.contains(['\n', '\r']) {
        return Err("Unsupported newline in desktop data directory".into());
    }
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=Ottrin\nComment=Browse folders with Ottrin\nExec={} -- %f\nIcon={}\nTerminal=false\nNoDisplay=true\nMimeType=inode/directory;\nCategories=System;FileManager;\nStartupWMClass=org.ottrin.Ottrin\n",
        desktop_argument(executable)?,
        icon_value.replace('\\', "\\\\")
    );
    let key_file = gio::glib::KeyFile::new();
    key_file
        .load_from_data(&desktop, gio::glib::KeyFileFlags::NONE)
        .map_err(|e| e.to_string())?;
    if gio::DesktopAppInfo::from_keyfile(&key_file).is_none() {
        return Err("Your desktop cannot launch Ottrin from this installation path. Choose an executable path your desktop supports (without '%' or '=') and try again.".into());
    }
    let assets = vec![
        (d.data.join("applications").join(ID), desktop.into_bytes()),
        (icon, ICON.to_vec()),
        (
            d.data
                .join(format!("dbus-1/services/{FILE_MANAGER_BUS}.service")),
            format!(
                "[D-BUS Service]\nName={FILE_MANAGER_BUS}\nExec={} file-manager\n",
                storage::dbus_executable(executable)?
            )
            .into_bytes(),
        ),
    ];
    for (path, _) in &assets {
        if !prior
            .as_ref()
            .is_some_and(|record| record.files.iter().any(|file| &file.path == path))
            && fs::symlink_metadata(path).is_ok()
        {
            return Err(format!(
                "Existing integration file retained: {}",
                path.display()
            ));
        }
    }
    let installed = preferences::replace(&current, GROUP, MIME, Some(&format!("{ID};")))?;
    let record = Registration {
        config,
        original,
        installed,
        previous,
        created,
        files: assets
            .iter()
            .map(|(path, data)| OwnedFile {
                path: path.clone(),
                sha256: digest(data),
            })
            .collect(),
    };
    storage::atomic_write(
        &receipt(d),
        &serde_json::to_vec(&record).map_err(|e| e.to_string())?,
    )?;
    for (path, data) in assets {
        storage::atomic_write(&path, &data)?;
    }
    storage::atomic_write(&record.config, record.installed.as_bytes())?;
    Ok("Directory links now open in Ottrin. Standard show-in-folder and Properties requests use Ottrin on demand. An already-running desktop file manager keeps its current reveal service until it exits. Applications with their own file manager keep that behavior.".into())
}
pub fn disable() -> Result<String, String> {
    let directories = Directories::current()?;
    if registration(&directories)?.is_none() {
        return Ok("Ottrin folder integration is already off".into());
    }
    normal_user()?;
    let message = disable_at(&directories)?;
    stop_owned_file_manager()?;
    Ok(message)
}
fn disable_at(d: &Directories) -> Result<String, String> {
    let Some(record) = registration(d)? else {
        return Ok("Ottrin folder integration is already off".into());
    };
    check_files(&record)?;
    let current = storage::read_text(&record.config)?.unwrap_or_default();
    let current_value = preferences::value(&current, GROUP, MIME)?;
    let (owned, new_value) =
        preferences::restore_provider(current_value.as_deref(), ID, record.previous.as_deref());
    if owned {
        if record.created && current == record.installed {
            fs::remove_file(&record.config).map_err(|e| e.to_string())?;
        } else {
            let restored = if current == record.installed {
                record.original
            } else {
                preferences::replace(&current, GROUP, MIME, new_value.as_deref())?
            };
            storage::atomic_write(&record.config, restored.as_bytes())?;
        }
    }
    for file in record.files {
        match fs::remove_file(file.path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    fs::remove_file(receipt(d)).map_err(|e| e.to_string())?;
    Ok("Previous directory handler restored; Ottrin's integration files removed".into())
}

/// Never activate or stop another provider. Our private control interface is absent there.
fn stop_owned_file_manager() -> Result<(), String> {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        return Ok(());
    }
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .map_err(|e| e.to_string())?;
    match bus.call_sync(
        Some(FILE_MANAGER_BUS),
        "/org/freedesktop/FileManager1",
        "org.ottrin.FileManager",
        "Stop",
        None,
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        2000,
        gio::Cancellable::NONE,
    ) {
        Ok(_) => Ok(()),
        Err(error) => {
            let name = gio::DBusError::remote_error(&error);
            if name.as_deref().is_some_and(|name| {
                matches!(
                    name,
                    "org.freedesktop.DBus.Error.NameHasNoOwner"
                        | "org.freedesktop.DBus.Error.ServiceUnknown"
                        | "org.freedesktop.DBus.Error.UnknownMethod"
                        | "org.freedesktop.DBus.Error.UnknownInterface"
                        | "org.freedesktop.DBus.Error.UnknownObject"
                )
            }) {
                Ok(())
            } else {
                Err(format!(
                    "Configuration saved; cannot stop Ottrin's desktop interface: {error}"
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn directories(root: &Path) -> Directories {
        Directories {
            config: root.join("config"),
            data: root.join("data"),
            state: root.join("state"),
        }
    }
    fn fixture_executable(d: &Directories, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = d.state.parent().unwrap().join(name);
        fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    #[test]
    fn reveal_activation_never_replaces_an_unowned_provider_even_during_repair() {
        let root = tempfile::tempdir().unwrap();
        let d = directories(root.path());
        let exe = fixture_executable(&d, "ottrin");
        enable_at(&d, &exe).unwrap();
        let mut record = registration(&d).unwrap().unwrap();
        let service = d
            .data
            .join(format!("dbus-1/services/{FILE_MANAGER_BUS}.service"));
        assert!(
            storage::read_text(&service)
                .unwrap()
                .unwrap()
                .contains(" file-manager\n")
        );
        record.files.retain(|file| file.path != service);
        storage::atomic_write(&receipt(&d), &serde_json::to_vec(&record).unwrap()).unwrap();
        let foreign = b"[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec=/usr/bin/false\n";
        storage::atomic_write(&service, foreign).unwrap();
        let receipt_before = fs::read(receipt(&d)).unwrap();
        assert!(enable_at(&d, &exe).unwrap_err().contains("retained"));
        assert_eq!(fs::read(&service).unwrap(), foreign);
        assert_eq!(fs::read(receipt(&d)).unwrap(), receipt_before);
        disable_at(&d).unwrap();
        assert_eq!(fs::read(service).unwrap(), foreign);
    }
    #[test]
    fn directory_override_restores_previous_handler_and_keeps_unrelated_edits() {
        let root = tempfile::tempdir().unwrap();
        let d = directories(root.path());
        let config = d.config.join("mimeapps.list");
        let original = "[Default Applications]\ninode/directory=org.example.Files.desktop;\napplication/pdf=reader.desktop;\n[Added Associations]\nimage/png=paint.desktop;\n";
        storage::atomic_write(&config, original.as_bytes()).unwrap();
        enable_at(&d, &fixture_executable(&d, "Ottrin")).unwrap();
        let record = registration(&d).unwrap().unwrap();
        assert_eq!(
            preferences::value(&storage::read_text(&config).unwrap().unwrap(), GROUP, MIME)
                .unwrap()
                .as_deref(),
            Some(&format!("{ID};")[..])
        );
        let edited = storage::read_text(&config)
            .unwrap()
            .unwrap()
            .replace("reader.desktop", "new-reader.desktop");
        storage::atomic_write(&config, edited.as_bytes()).unwrap();
        enable_at(&d, &fixture_executable(&d, "New Ottrin")).unwrap();
        disable_at(&d).unwrap();
        let restored = storage::read_text(&config).unwrap().unwrap();
        assert!(restored.contains("inode/directory=org.example.Files.desktop;"));
        assert!(restored.contains("application/pdf=new-reader.desktop;"));
        assert!(restored.contains("image/png=paint.desktop;"));
        assert!(!receipt(&d).exists());
        assert!(record.files.iter().all(|file| !file.path.exists()));
    }
    #[test]
    fn new_user_override_is_removed_and_foreign_assets_are_retained() {
        let root = tempfile::tempdir().unwrap();
        let d = directories(root.path());
        enable_at(&d, &fixture_executable(&d, "ottrin")).unwrap();
        let record = registration(&d).unwrap().unwrap();
        assert!(record.created);
        disable_at(&d).unwrap();
        assert!(!record.config.exists());
        let foreign = d.data.join("applications").join(ID);
        storage::atomic_write(&foreign, b"foreign").unwrap();
        assert!(enable_at(&d, &fixture_executable(&d, "ottrin")).is_err());
        assert_eq!(fs::read(foreign).unwrap(), b"foreign");
        assert!(!receipt(&d).exists());
    }
    #[test]
    fn modified_asset_blocks_removal_before_changing_preferences() {
        let root = tempfile::tempdir().unwrap();
        let d = directories(root.path());
        enable_at(&d, &fixture_executable(&d, "ottrin")).unwrap();
        let record = registration(&d).unwrap().unwrap();
        let prior = fs::read(&record.config).unwrap();
        storage::atomic_write(&record.files[0].path, b"user edit").unwrap();
        assert!(disable_at(&d).is_err());
        assert_eq!(fs::read(record.config).unwrap(), prior);
        assert_eq!(fs::read(&record.files[0].path).unwrap(), b"user edit");
    }
    #[test]
    fn edited_provider_lists_keep_new_choices_and_disable_removes_only_ottrin() {
        let root = tempfile::tempdir().unwrap();
        let d = directories(root.path());
        enable_at(&d, &fixture_executable(&d, "ottrin")).unwrap();
        let record = registration(&d).unwrap().unwrap();
        let data = preferences::replace(
            &record.installed,
            GROUP,
            MIME,
            Some(&format!("{ID};new-files.desktop;")),
        )
        .unwrap();
        storage::atomic_write(&record.config, data.as_bytes()).unwrap();
        disable_at(&d).unwrap();
        assert_eq!(
            preferences::value(&fs::read_to_string(&record.config).unwrap(), GROUP, MIME)
                .unwrap()
                .as_deref(),
            Some("new-files.desktop;")
        );
        enable_at(&d, &fixture_executable(&d, "ottrin")).unwrap();
        let changed = preferences::replace(
            &fs::read_to_string(&record.config).unwrap(),
            GROUP,
            MIME,
            Some("another.desktop;"),
        )
        .unwrap();
        storage::atomic_write(&record.config, changed.as_bytes()).unwrap();
        enable_at(&d, &fixture_executable(&d, "new-ottrin")).unwrap();
        disable_at(&d).unwrap();
        assert_eq!(
            preferences::value(&fs::read_to_string(&record.config).unwrap(), GROUP, MIME)
                .unwrap()
                .as_deref(),
            Some("another.desktop;")
        );
    }

    #[test]
    fn desktop_entry_is_valid_for_glib_and_uses_one_native_file_argument() {
        use gio::prelude::*;
        let root = tempfile::tempdir().unwrap();
        let d = directories(root.path());
        let executable = root.path().join("Ottrin $USER `local`");
        fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        enable_at(&d, &executable).unwrap();
        let record = registration(&d).unwrap().unwrap();
        let app = gio::DesktopAppInfo::from_filename(&record.files[0].path).unwrap_or_else(|| {
            panic!(
                "Valid desktop command quoting for {:?}: {}",
                executable,
                fs::read_to_string(&record.files[0].path).unwrap()
            )
        });
        assert!(app.supports_files());
        assert!(!app.supports_uris());
        assert!(!app.should_show());
        assert!(desktop_argument(Path::new("/opt/invalid=path/app")).is_err());
        let before = fs::read(&record.files[0].path).unwrap();
        let unsupported = fixture_executable(&d, "invalid=path");
        assert!(enable_at(&d, &unsupported).is_err());
        assert_eq!(
            fs::read(&record.files[0].path).unwrap(),
            before,
            "An unsupported path must not replace a working entry"
        );
    }
}
