//! Explicit, reversible per-user FileChooser registration. Called on a worker.
use super::integration_preferences;
use super::integration_storage::{atomic_write as write, read_text as read_regular};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
const BUS: &str = "org.freedesktop.impl.portal.desktop.ottrin";
const CHOOSER: &str = "org.freedesktop.impl.portal.FileChooser";
const UNIT: &str = "ottrin-portal.service";
const DEFINITION: &[u8] = include_bytes!("../../../packaging/linux/ottrin.portal");

#[derive(Clone, Debug, Serialize)]
pub struct PortalStatus {
    pub enabled: bool,
    pub ready: bool,
    pub pending: bool,
    pub detail: String,
}
#[derive(Serialize, Deserialize)]
struct Registration {
    enabled: bool,
    applied: bool,
    created: bool,
    config: PathBuf,
    previous: Option<String>,
    original: String,
    installed: String,
    files: Vec<(PathBuf, String)>,
}
struct Paths {
    config: PathBuf,
    data: PathBuf,
    state: PathBuf,
}
impl Paths {
    fn current() -> Result<Self, String> {
        let directories = super::integration_storage::Directories::current()?;
        Ok(Self {
            config: directories.config,
            data: directories.data,
            state: directories.state.join("ottrin/portal-registration.json"),
        })
    }
    fn names() -> Vec<String> {
        std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .filter(|s| {
                !s.is_empty()
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
            .map(|s| format!("{}-portals.conf", s.to_ascii_lowercase()))
            .chain(std::iter::once("portals.conf".into()))
            .collect()
    }
    fn configuration(&self) -> Result<(PathBuf, String), String> {
        let names = Self::names();
        let local = self.config.join("xdg-desktop-portal");
        let mut directories = vec![local.clone()];
        directories.extend(
            std::env::var("XDG_CONFIG_DIRS")
                .unwrap_or_else(|_| "/etc/xdg".into())
                .split(':')
                .filter(|p| Path::new(p).is_absolute())
                .map(|p| Path::new(p).join("xdg-desktop-portal")),
        );
        directories.push(PathBuf::from("/etc/xdg-desktop-portal"));
        directories.push(self.data.join("xdg-desktop-portal"));
        directories.extend(
            std::env::var("XDG_DATA_DIRS")
                .unwrap_or_else(|_| "/usr/local/share:/usr/share".into())
                .split(':')
                .filter(|p| Path::new(p).is_absolute())
                .map(|p| Path::new(p).join("xdg-desktop-portal")),
        );
        directories.push(PathBuf::from("/usr/share/xdg-desktop-portal"));
        for directory in directories {
            for name in &names {
                let path = directory.join(name);
                if let Some(data) = read_regular(&path)? {
                    return Ok((
                        if directory == local {
                            path
                        } else {
                            local.join(&names[0])
                        },
                        data,
                    ));
                }
            }
        }
        Ok((local.join(&names[0]), String::new()))
    }
}
fn systemctl(arguments: &[&str]) -> Result<(), String> {
    let result = Command::new("systemctl")
        .args(["--user", "--no-ask-password", "--no-pager"])
        .args(arguments)
        .output()
        .map_err(|e| e.to_string())?;
    if result.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&result.stderr).trim().to_owned())
    }
}
fn registration(paths: &Paths) -> Result<Option<Registration>, String> {
    read_regular(&paths.state)?
        .map(|data| {
            serde_json::from_str(&data).map_err(|e| format!("Portal setup receipt is invalid: {e}"))
        })
        .transpose()
}
pub fn status() -> PortalStatus {
    let result = (|| {
        let paths = Paths::current()?;
        let Some(record) = registration(&paths)? else {
            return Ok((
                false,
                false,
                false,
                "Off — applications use your desktop's file picker".into(),
            ));
        };
        let valid = fs::read("/usr/share/xdg-desktop-portal/portals/ottrin.portal")
            .ok()
            .as_deref()
            == Some(DEFINITION)
            && record
                .files
                .iter()
                .all(|(path, data)| read_regular(path).ok().flatten().as_ref() == Some(data));
        let (effective_path, effective) = paths.configuration()?;
        let selected = effective_path == record.config
            && integration_preferences::value(&effective, "preferred", CHOOSER)?
                .is_some_and(|value| value.split(';').next() == Some("ottrin"));
        // A configured on-demand backend may be idle with no bus owner. Status
        // must not start it or turn a deliberately idle service into a setup error.
        let reachable = !record.enabled
            || !record.applied
            || if backend_running()? {
                backend_ready(gio::DBusCallFlags::NO_AUTO_START).is_ok()
            } else {
                backend_idle()?
            };
        Ok::<_, String>((
            record.enabled,
            record.enabled && record.applied && valid && selected && reachable,
            !record.applied || !reachable,
            if !record.enabled {
                "Off — apply the change to restore the desktop picker in running applications"
                    .into()
            } else if valid && selected && record.applied && reachable {
                "Enabled — Ottrin handles supported portal file dialogs".into()
            } else if valid && selected {
                "Configured — apply the change when no portal dialogs or sessions are active".into()
            } else {
                "Setup needs repair; existing configuration has been retained".into()
            },
        ))
    })();
    match result {
        Ok((enabled, ready, pending, detail)) => PortalStatus {
            enabled,
            ready,
            pending,
            detail,
        },
        Err(detail) => PortalStatus {
            enabled: false,
            ready: false,
            pending: false,
            detail,
        },
    }
}

fn normal_user() -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 {
        Err("Configure desktop file dialogs from a normal user session".into())
    } else {
        Ok(())
    }
}

pub fn enable() -> Result<String, String> {
    normal_user()?;
    if std::env::var_os("XDG_DESKTOP_PORTAL_DIR").is_some() {
        return Err("A custom desktop portal directory overrides normal desktop configuration. Remove that override before enabling the Ottrin picker.".into());
    }
    let paths = Paths::current()?;
    if fs::read("/usr/share/xdg-desktop-portal/portals/ottrin.portal")
        .ok()
        .as_deref()
        != Some(DEFINITION)
    {
        super::install_privileged_helper()?;
        if fs::read("/usr/share/xdg-desktop-portal/portals/ottrin.portal")
            .ok()
            .as_deref()
            != Some(DEFINITION)
        {
            return Err("Administrator setup did not install the desktop portal definition".into());
        }
    }
    enable_at(&paths, &super::application_launch_path()?, systemctl)
}
fn enable_at(
    paths: &Paths,
    executable: &Path,
    mut control: impl FnMut(&[&str]) -> Result<(), String>,
) -> Result<String, String> {
    let executable = executable
        .to_str()
        .ok_or("The desktop activation path must be UTF-8")?;
    let quoted = super::integration_storage::dbus_executable(Path::new(executable))?;
    // systemd performs its own specifier/environment expansion; it does not
    // use the D-Bus command line grammar above.
    let unit_quoted = format!(
        "\"{}\"",
        executable
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('$', "$$")
    );
    if executable.contains(['\n', '\r']) {
        return Err("Unsupported newline in desktop activation path".into());
    }
    let prior = registration(paths)?;
    if let Some(prior) = &prior {
        if !prior.enabled {
            return Err("Apply the pending disable before enabling the picker again.".into());
        }
        for (path, expected) in &prior.files {
            if let Some(data) = read_regular(path)?
                && data != *expected
            {
                return Err(format!(
                    "Modified activation file retained: {}",
                    path.display()
                ));
            }
        }
    }
    let (config, current) = if let Some(prior) = &prior {
        (
            prior.config.clone(),
            read_regular(&prior.config)?.unwrap_or_default(),
        )
    } else {
        paths.configuration()?
    };
    let created = if let Some(prior) = &prior {
        prior.created && current == prior.installed
    } else {
        read_regular(&config)?.is_none()
    };
    let current_value = integration_preferences::value(&current, "preferred", CHOOSER)?;
    let still_owned =
        integration_preferences::restore_provider(current_value.as_deref(), "ottrin", None).0;
    let previous = if still_owned {
        prior.as_ref().and_then(|record| record.previous.clone())
    } else {
        current_value
    };
    let original = if let Some(prior) = &prior
        && still_owned
    {
        if current == prior.installed {
            prior.original.clone()
        } else {
            integration_preferences::replace(
                &current,
                "preferred",
                CHOOSER,
                prior.previous.as_deref(),
            )?
        }
    } else {
        current.clone()
    };
    let installed =
        integration_preferences::replace(&current, "preferred", CHOOSER, Some("ottrin"))?;
    let files = vec![
        (
            paths.data.join(format!("dbus-1/services/{BUS}.service")),
            format!("[D-BUS Service]\nName={BUS}\nExec={quoted} portal\nSystemdService={UNIT}\n"),
        ),
        (
            paths.config.join(format!("systemd/user/{UNIT}")),
            format!(
                "[Unit]\nDescription=Ottrin on-demand file picker\n[Service]\nType=dbus\nBusName={BUS}\nExecStart={} portal\nKillMode=control-group\nTimeoutStopSec=5\n",
                unit_quoted
            ),
        ),
    ];
    for (path, _) in &files {
        if prior.is_none() && fs::symlink_metadata(path).is_ok() {
            return Err(format!(
                "Existing activation file retained: {}",
                path.display()
            ));
        }
    }
    // Save ownership and restoration information before changing any setting.
    let record = Registration {
        enabled: true,
        applied: false,
        created,
        config,
        previous,
        original,
        installed,
        files,
    };
    write(
        &paths.state,
        &serde_json::to_vec(&record).map_err(|e| e.to_string())?,
    )?;
    for (path, data) in &record.files {
        write(path, data.as_bytes())?;
    }
    control(&["daemon-reload"])?;
    write(&record.config, record.installed.as_bytes())?;
    Ok("Ottrin file picker configured. Apply the change to reload the desktop portal.".into())
}

/// Uninstall also applies the restored preference, removing the pending receipt.
/// Absence is a no-op, including in package-maintenance contexts without a user bus.
pub fn remove_for_uninstall() -> Result<Vec<String>, String> {
    let Some(record) = registration(&Paths::current()?)? else {
        return Ok(Vec::new());
    };
    let mut messages = Vec::new();
    if record.enabled {
        messages.push(disable()?);
    }
    messages.push(apply()?);
    Ok(messages)
}

fn native_registration(paths: &Paths, record: &Registration) -> bool {
    let activation = paths.data.join(format!("dbus-1/services/{BUS}.service"));
    let expected = format!(
        "[D-BUS Service]\nName={BUS}\nExec=\"/usr/bin/ottrin\" portal\nSystemdService={UNIT}\n"
    );
    record
        .files
        .iter()
        .any(|(path, data)| *path == activation && *data == expected)
}

pub(crate) fn portable_definition_needed() -> Result<bool, String> {
    let paths = Paths::current()?;
    Ok(registration(&paths)?
        .is_some_and(|record| record.enabled && !native_registration(&paths, &record)))
}

pub(crate) fn native_package_owned(check_only: bool, online: bool) -> Result<(), String> {
    native_package_at(&Paths::current()?, check_only, online, systemctl)
}
fn native_package_at(
    paths: &Paths,
    check_only: bool,
    online: bool,
    mut control: impl FnMut(&[&str]) -> Result<(), String>,
) -> Result<(), String> {
    let Some(record) = registration(paths)? else {
        return Ok(());
    };
    if !native_registration(paths, &record) {
        return Ok(());
    }
    for (path, expected) in &record.files {
        if let Some(data) = read_regular(path)?
            && data != *expected
        {
            return Err(format!(
                "Modified activation file retained: {}",
                path.display()
            ));
        }
    }
    // Overrides can redirect the generated unit to a portable process. Refuse
    // ambiguous ownership instead of stopping it during native package removal.
    for base in [
        paths.config.join("systemd/user"),
        paths.data.join("systemd/user"),
        PathBuf::from("/etc/systemd/user"),
        PathBuf::from("/usr/lib/systemd/user"),
    ] {
        let overrides = base.join(format!("{UNIT}.d"));
        match fs::read_dir(&overrides) {
            Ok(entries) => {
                for entry in entries {
                    let path = entry.map_err(|e| e.to_string())?.path();
                    if path
                        .extension()
                        .is_some_and(|extension| extension == "conf")
                    {
                        return Err(format!(
                            "Cannot resolve native picker override: {}",
                            path.display()
                        ));
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    let current = read_regular(&record.config)?.unwrap_or_default();
    integration_preferences::value(&current, "preferred", CHOOSER)?;
    if online {
        let effective = Command::new("systemctl")
            .args([
                "--user",
                "--no-ask-password",
                "--no-pager",
                "show",
                UNIT,
                "--property=ExecStart",
                "--value",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if !effective.status.success() {
            return Err(String::from_utf8_lossy(&effective.stderr).trim().into());
        }
        let effective = String::from_utf8(effective.stdout).map_err(|e| e.to_string())?;
        let effective = effective.trim();
        if !effective.is_empty()
            && (!effective.starts_with("{ path=/usr/bin/ottrin ; argv[]=/usr/bin/ottrin portal ; ")
                || effective.lines().count() != 1)
        {
            return Err(
                "The effective picker unit is not owned by the native package; retained unchanged"
                    .into(),
            );
        }
        super::native_desktop::check_native_portal()?;
        ensure_idle()?;
    }
    if !check_only {
        if record.enabled {
            disable_at(paths, |args| if online { control(args) } else { Ok(()) })?;
        }
        if online {
            super::integration_storage::reload_activation()?;
            control(&["try-restart", "xdg-desktop-portal.service"])?;
        }
        // With no user manager there is no frontend to reload. The restored
        // preference will be read at login; no pending activation remains.
        fs::remove_file(&paths.state).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn disable() -> Result<String, String> {
    normal_user()?;
    let paths = Paths::current()?;
    ensure_idle()?;
    disable_at(&paths, systemctl)
}
fn disable_at(
    paths: &Paths,
    mut control: impl FnMut(&[&str]) -> Result<(), String>,
) -> Result<String, String> {
    let Some(mut record) = registration(paths)? else {
        return Ok("Ottrin file picker is already off".into());
    };
    for (path, expected) in &record.files {
        if let Some(data) = read_regular(path)?
            && data != *expected
        {
            return Err(format!(
                "Modified activation file retained: {}",
                path.display()
            ));
        }
    }
    let data = read_regular(&record.config)?.unwrap_or_default();
    let current_value = integration_preferences::value(&data, "preferred", CHOOSER)?;
    let (owned, new_value) = integration_preferences::restore_provider(
        current_value.as_deref(),
        "ottrin",
        record.previous.as_deref(),
    );
    if owned {
        let restored = if data == record.installed {
            record.original.clone()
        } else {
            integration_preferences::replace(&data, "preferred", CHOOSER, new_value.as_deref())?
        };
        if record.created && data == record.installed {
            fs::remove_file(&record.config).map_err(|e| e.to_string())?;
        } else {
            write(&record.config, restored.as_bytes())?;
        }
    }
    control(&["stop", UNIT])?;
    for (path, _) in &record.files {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    control(&["daemon-reload"])?;
    record.enabled = false;
    record.applied = false;
    write(
        &paths.state,
        &serde_json::to_vec(&record).map_err(|e| e.to_string())?,
    )?;
    Ok("Previous file picker restored. Apply the change to reload the desktop portal.".into())
}

/// Reload only after confirming that the frontend has no live requests/sessions.
/// This runs in a worker and never interrupts an active dialog or screen share.
fn ensure_idle() -> Result<(), String> {
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .map_err(|e| e.to_string())?;
    ensure_idle_on(&bus)
}
fn ensure_idle_on(bus: &gio::DBusConnection) -> Result<(), String> {
    use gio::glib::variant::ToVariant;
    let owner = bus
        .call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            Some(&("org.freedesktop.portal.Desktop",).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            2000,
            gio::Cancellable::NONE,
        )
        .map_err(|e| e.to_string())?;
    if !owner
        .get::<(bool,)>()
        .ok_or("Invalid portal ownership reply")?
        .0
    {
        return Ok(());
    }
    for path in [
        "/org/freedesktop/portal/desktop/request",
        "/org/freedesktop/portal/desktop/session",
    ] {
        let response = bus
            .call_sync(
                Some("org.freedesktop.portal.Desktop"),
                path,
                "org.freedesktop.DBus.Introspectable",
                "Introspect",
                None,
                None,
                gio::DBusCallFlags::NO_AUTO_START,
                2000,
                gio::Cancellable::NONE,
            )
            .map_err(|e| format!("Cannot confirm the desktop portal is idle: {e}"))?;
        let xml = response
            .get::<(String,)>()
            .ok_or("Invalid portal introspection reply")?
            .0;
        if xml.contains("<node name=") {
            return Err("A desktop portal dialog or session is active. Close it before applying this change.".into());
        }
    }
    Ok(())
}
fn backend_idle() -> Result<bool, String> {
    let state = Command::new("systemctl")
        .args([
            "--user",
            "--no-ask-password",
            "--no-pager",
            "show",
            UNIT,
            "--property=ActiveState",
            "--value",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !state.status.success() {
        return Err(String::from_utf8_lossy(&state.stderr).trim().into());
    }
    match String::from_utf8_lossy(&state.stdout).trim() {
        "inactive" => Ok(true),
        "failed" | "active" | "activating" | "deactivating" | "reloading" => Ok(false),
        _ => Err("Cannot determine the on-demand picker service state".into()),
    }
}

fn backend_running() -> Result<bool, String> {
    use gio::glib::variant::ToVariant;
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .map_err(|e| e.to_string())?;
    bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&(BUS,).to_variant()),
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        2000,
        gio::Cancellable::NONE,
    )
    .map_err(|e| e.to_string())?
    .get::<(bool,)>()
    .map(|(running,)| running)
    .ok_or_else(|| "Invalid picker ownership reply".into())
}

fn backend_ready(flags: gio::DBusCallFlags) -> Result<(), String> {
    use gio::glib::variant::ToVariant;
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .map_err(|e| e.to_string())?;
    let reply = bus
        .call_sync(
            Some(BUS),
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&("org.freedesktop.impl.portal.FileChooser", "version").to_variant()),
            None,
            flags,
            3000,
            gio::Cancellable::NONE,
        )
        .map_err(|e| {
            format!("The desktop portal restarted, but Ottrin's picker is not ready: {e}")
        })?;
    let (value,) = reply
        .get::<(gio::glib::Variant,)>()
        .ok_or("Invalid Ottrin portal version reply")?;
    if value.get::<u32>() == Some(4) {
        Ok(())
    } else {
        Err("Unexpected Ottrin file picker version".into())
    }
}

pub fn apply() -> Result<String, String> {
    normal_user()?;
    let paths = Paths::current()?;
    let Some(mut record) = registration(&paths)? else {
        return Ok("No pending file picker changes".into());
    };
    ensure_idle()?;
    super::integration_storage::reload_activation()?;
    systemctl(&["restart", "xdg-desktop-portal.service"])?;
    if record.enabled {
        // Explicit setup validates the installed activation path, including on
        // frontends which defer backend startup until the first chooser request.
        backend_ready(gio::DBusCallFlags::NONE)?;
        record.applied = true;
        write(
            &paths.state,
            &serde_json::to_vec(&record).map_err(|e| e.to_string())?,
        )?;
    } else {
        fs::remove_file(&paths.state).map_err(|e| e.to_string())?;
    }
    Ok("Desktop file picker preference applied".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root: &Path) -> Paths {
        Paths {
            config: root.join("config"),
            data: root.join("data"),
            state: root.join("state/registration.json"),
        }
    }
    #[test]
    fn native_removal_restores_offline_picker_and_preserves_portable_choice() {
        let root = tempfile::tempdir().unwrap();
        let paths = fixture(root.path());
        enable_at(&paths, Path::new("/opt/ottrin.AppImage"), |_| Ok(())).unwrap();
        let before = fs::read(&paths.state).unwrap();
        native_package_at(&paths, false, false, |_| panic!("offline control")).unwrap();
        assert_eq!(fs::read(&paths.state).unwrap(), before);
        enable_at(&paths, Path::new("/usr/bin/ottrin"), |_| Ok(())).unwrap();
        let record = registration(&paths).unwrap().unwrap();
        let installed = fs::read(&record.config).unwrap();
        native_package_at(&paths, true, false, |_| panic!("offline control")).unwrap();
        assert_eq!(fs::read(&record.config).unwrap(), installed);
        let override_path = paths
            .config
            .join(format!("systemd/user/{UNIT}.d/override.conf"));
        write(&override_path, b"[Service]\nExecStart=/opt/other\n").unwrap();
        assert!(native_package_at(&paths, false, false, |_| panic!("offline control")).is_err());
        assert_eq!(fs::read(&record.config).unwrap(), installed);
        fs::remove_file(override_path).unwrap();
        native_package_at(&paths, false, false, |_| panic!("offline control")).unwrap();
        assert!(!paths.state.exists());
        assert!(!record.config.exists());
        assert!(record.files.iter().all(|(path, _)| !path.exists()));
        // Disabled-but-not-applied native registrations also finish offline.
        enable_at(&paths, Path::new("/usr/bin/ottrin"), |_| Ok(())).unwrap();
        disable_at(&paths, |_| Ok(())).unwrap();
        native_package_at(&paths, false, false, |_| panic!("offline control")).unwrap();
        assert!(!paths.state.exists());
    }
    #[test]
    fn enable_disable_preserves_other_providers_and_unrelated_user_changes() {
        let directory = tempfile::tempdir().unwrap();
        let paths = fixture(directory.path());
        let config = paths
            .config
            .join("xdg-desktop-portal")
            .join(&Paths::names()[0]);
        write(
            &config,
            b"# keep\n[preferred]\ndefault=kde;gtk\norg.freedesktop.impl.portal.FileChooser=kde\n",
        )
        .unwrap();
        let mut commands = Vec::new();
        enable_at(&paths, Path::new("/opt/My Ottrin/ottrin"), |args| {
            commands.push(args.join(" "));
            Ok(())
        })
        .unwrap();
        let record = registration(&paths).unwrap().unwrap();
        assert!(record.enabled && !record.applied);
        assert_eq!(commands, vec!["daemon-reload"]);
        assert!(
            record
                .files
                .iter()
                .all(|(p, d)| fs::read_to_string(p).unwrap() == *d)
        );
        assert!(
            record.files[0]
                .1
                .contains("Exec=\"/opt/My Ottrin/ottrin\" portal")
        );
        let edited = fs::read_to_string(&config)
            .unwrap()
            .replace("default=kde;gtk", "default=gtk");
        write(&config, edited.as_bytes()).unwrap();
        enable_at(&paths, Path::new("/opt/New Ottrin/ottrin"), |_| Ok(())).unwrap();
        let updated = registration(&paths).unwrap().unwrap();
        assert!(updated.files[0].1.contains("/opt/New Ottrin/ottrin"));
        disable_at(&paths, |_| Ok(())).unwrap();
        let restored = fs::read_to_string(&config).unwrap();
        assert_eq!(
            integration_preferences::value(&restored, "preferred", CHOOSER)
                .unwrap()
                .as_deref(),
            Some("kde")
        );
        assert!(restored.contains("default=gtk") && restored.starts_with("# keep"));
        assert!(record.files.iter().all(|(p, _)| !p.exists()));
        assert!(!registration(&paths).unwrap().unwrap().enabled);
    }
    #[test]
    fn activation_uses_each_consumers_literal_path_grammar() {
        let directory = tempfile::tempdir().unwrap();
        let paths = fixture(directory.path());
        enable_at(
            &paths,
            Path::new("/opt/Ottrin $USER 100% `local`/app"),
            |_| Ok(()),
        )
        .unwrap();
        let record = registration(&paths).unwrap().unwrap();
        let unit = &record.files[1].1;
        assert!(
            unit.contains("ExecStart=\"/opt/Ottrin $$USER 100%% `local`/app\" portal"),
            "{unit}"
        );
        let dbus = &record.files[0].1;
        assert!(!dbus.contains("100%%"));
        assert!(dbus.contains("\\$USER"));
    }

    #[test]
    fn generated_override_is_removed_but_user_edits_after_install_are_retained() {
        let directory = tempfile::tempdir().unwrap();
        let paths = fixture(directory.path());
        enable_at(&paths, Path::new("/opt/ottrin"), |_| Ok(())).unwrap();
        let record = registration(&paths).unwrap().unwrap();
        assert!(record.created);
        disable_at(&paths, |_| Ok(())).unwrap();
        assert!(!record.config.exists());
        fs::remove_file(&paths.state).unwrap();
        enable_at(&paths, Path::new("/opt/ottrin"), |_| Ok(())).unwrap();
        let record = registration(&paths).unwrap().unwrap();
        let edited = format!("{}\n# new user preference\n", record.installed);
        write(&record.config, edited.as_bytes()).unwrap();
        enable_at(&paths, Path::new("/opt/new-ottrin"), |_| Ok(())).unwrap();
        disable_at(&paths, |_| Ok(())).unwrap();
        assert!(
            fs::read_to_string(record.config)
                .unwrap()
                .contains("# new user preference")
        );
    }

    #[test]
    fn modified_provider_lists_and_reselected_foreign_defaults_are_retained() {
        let directory = tempfile::tempdir().unwrap();
        let paths = fixture(directory.path());
        enable_at(&paths, Path::new("/opt/ottrin"), |_| Ok(())).unwrap();
        let record = registration(&paths).unwrap().unwrap();
        let modified = integration_preferences::replace(
            &record.installed,
            "preferred",
            CHOOSER,
            Some("ottrin;gtk;"),
        )
        .unwrap();
        write(&record.config, modified.as_bytes()).unwrap();
        disable_at(&paths, |_| Ok(())).unwrap();
        assert_eq!(
            integration_preferences::value(
                &read_regular(&record.config).unwrap().unwrap(),
                "preferred",
                CHOOSER
            )
            .unwrap()
            .as_deref(),
            Some("gtk;")
        );
        fs::remove_file(&paths.state).unwrap();
        enable_at(&paths, Path::new("/opt/ottrin"), |_| Ok(())).unwrap();
        let modified = integration_preferences::replace(
            &read_regular(&record.config).unwrap().unwrap(),
            "preferred",
            CHOOSER,
            Some("kde"),
        )
        .unwrap();
        write(&record.config, modified.as_bytes()).unwrap();
        enable_at(&paths, Path::new("/opt/new-ottrin"), |_| Ok(())).unwrap();
        disable_at(&paths, |_| Ok(())).unwrap();
        assert_eq!(
            integration_preferences::value(
                &read_regular(&record.config).unwrap().unwrap(),
                "preferred",
                CHOOSER
            )
            .unwrap()
            .as_deref(),
            Some("kde")
        );
    }

    #[test]
    fn modified_activation_file_blocks_uninstall_before_preferences_change() {
        let directory = tempfile::tempdir().unwrap();
        let paths = fixture(directory.path());
        enable_at(&paths, Path::new("/opt/ottrin"), |_| Ok(())).unwrap();
        let record = registration(&paths).unwrap().unwrap();
        write(&record.files[0].0, b"foreign edit").unwrap();
        assert!(disable_at(&paths, |_| panic!("must not stop anything")).is_err());
        assert_eq!(
            fs::read_to_string(&record.config).unwrap(),
            record.installed
        );
        assert_eq!(
            fs::read_to_string(&record.files[0].0).unwrap(),
            "foreign edit"
        );
    }
    #[test]
    fn foreign_activation_and_symlink_preferences_are_retained() {
        let directory = tempfile::tempdir().unwrap();
        let paths = fixture(directory.path());
        let activation = paths.data.join(format!("dbus-1/services/{BUS}.service"));
        write(&activation, b"foreign").unwrap();
        assert!(enable_at(&paths, Path::new("/opt/ottrin"), |_| panic!("no commands")).is_err());
        assert!(!paths.state.exists());
        let target = directory.path().join("target");
        fs::write(&target, "keep").unwrap();
        let link = directory.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(write(&link, b"changed").is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "keep");
    }
}
