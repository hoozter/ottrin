//! Native package maintenance restores only registrations owned by /usr/bin/ottrin.
//! Run with the owning user's HOME/XDG and identity, never as root for another user.
use gio::glib::variant::ToVariant;
use std::{fs, os::unix::fs::MetadataExt, path::Path};

/// Check is non-mutating; removal preserves preferences and generated search data.
/// The caller establishes whether this user's systemd manager is online.
pub fn maintain(check_only: bool, online: bool) -> Result<(), String> {
    super::portal_integration::native_package_owned(true, online)?;
    super::folder_integration::native_package_owned(true, online)?;
    if !check_only {
        super::portal_integration::native_package_owned(false, online)?;
        super::folder_integration::native_package_owned(false, online)?;
    }
    Ok(())
}

enum Owner {
    Absent,
    Foreign,
    Native(String),
}

fn native_bus_owner(bus: &gio::DBusConnection, name: &str) -> Result<Owner, String> {
    let call = |destination: &str,
                path: &str,
                interface: &str,
                method: &str,
                parameters: Option<&gio::glib::Variant>| {
        bus.call_sync(
            Some(destination),
            path,
            interface,
            method,
            parameters,
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            2000,
            gio::Cancellable::NONE,
        )
    };
    let owner = match call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetNameOwner",
        Some(&(name,).to_variant()),
    ) {
        Ok(reply) => {
            reply
                .get::<(String,)>()
                .ok_or("Invalid file manager owner")?
                .0
        }
        Err(error)
            if gio::DBusError::remote_error(&error).as_deref()
                == Some("org.freedesktop.DBus.Error.NameHasNoOwner") =>
        {
            return Ok(Owner::Absent);
        }
        Err(error) => return Err(error.to_string()),
    };
    let (pid,) = call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetConnectionUnixProcessID",
        Some(&(owner.as_str(),).to_variant()),
    )
    .map_err(|e| e.to_string())?
    .get::<(u32,)>()
    .ok_or("Invalid file manager process")?;
    let (uid,) = call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetConnectionUnixUser",
        Some(&(owner.as_str(),).to_variant()),
    )
    .map_err(|e| e.to_string())?
    .get::<(u32,)>()
    .ok_or("Invalid desktop peer identity")?;
    if uid != unsafe { libc::geteuid() } {
        return Err("Desktop peer is not owned by this user".into());
    }
    let expected = Path::new("/usr/bin/ottrin");
    let executable = fs::read_link(format!("/proc/{pid}/exe")).map_err(|e| e.to_string())?;
    if executable != expected && executable != Path::new("/usr/bin/ottrin (deleted)") {
        return Ok(Owner::Foreign);
    }
    // The same spelling inside an independent mount namespace is not ownership.
    let installed = fs::metadata(expected).map_err(|e| e.to_string())?;
    let peer_installed =
        fs::metadata(format!("/proc/{pid}/root/usr/bin/ottrin")).map_err(|e| e.to_string())?;
    if !installed.is_file()
        || installed.dev() != peer_installed.dev()
        || installed.ino() != peer_installed.ino()
    {
        return Ok(Owner::Foreign);
    }
    Ok(Owner::Native(owner))
}

pub(crate) fn check_native_portal() -> Result<(), String> {
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .map_err(|e| e.to_string())?;
    if matches!(
        native_bus_owner(&bus, "org.freedesktop.impl.portal.desktop.ottrin")?,
        Owner::Foreign
    ) {
        return Err(
            "An independent Ottrin picker is running; close it before removing the native package"
                .into(),
        );
    }
    Ok(())
}

pub(crate) fn stop_native_file_manager() -> Result<(), String> {
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .map_err(|e| e.to_string())?;
    let Owner::Native(owner) = native_bus_owner(&bus, "org.freedesktop.FileManager1")? else {
        return Ok(());
    };
    // Pin the bus's unique owner, never send to a newly acquired well-known name.
    bus.call_sync(
        Some(&owner),
        "/org/freedesktop/FileManager1",
        "org.ottrin.FileManager",
        "Stop",
        None,
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        2000,
        gio::Cancellable::NONE,
    )
    .map_err(|e| format!("Cannot stop the native file manager: {e}"))?;
    Ok(())
}
