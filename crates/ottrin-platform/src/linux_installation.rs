use gio::prelude::AppInfoExt;
use std::{fs, path::Path, process::Command};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageManager {
    Apt,
    Dnf,
}

impl PackageManager {
    pub fn remove_command(self) -> &'static str {
        match self {
            Self::Apt => "sudo apt remove ottrin",
            Self::Dnf => "sudo dnf remove ottrin",
        }
    }
}

/// Ask the native package database about this exact path. A tool failure is not
/// evidence that a file belongs to a standalone installation.
pub fn package_owner(path: &Path) -> Result<Option<(PackageManager, String)>, String> {
    for (manager, executable, arguments, unowned) in [
        (
            PackageManager::Apt,
            "/usr/bin/dpkg-query",
            &["-S", "--"][..],
            "no path found matching pattern",
        ),
        (
            PackageManager::Dnf,
            "/usr/bin/rpm",
            &["-qf", "--queryformat", "%{NAME}\\n", "--"][..],
            "is not owned by any package",
        ),
    ] {
        if !Path::new(executable).is_file() {
            continue;
        }
        let output = Command::new(executable)
            .env_clear()
            .env("LC_ALL", "C")
            .args(arguments)
            .arg(path)
            .output()
            .map_err(|error| format!("Cannot query the package owner: {error}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if output.status.success() {
            let entry = stdout.trim();
            let name = match manager {
                PackageManager::Apt => entry.split_once(": ").map(|(name, _)| name),
                PackageManager::Dnf => Some(entry),
            }
            .filter(|name| !name.is_empty() && !name.contains(char::is_whitespace))
            .ok_or("Cannot identify a unique package owner")?;
            return Ok(Some((
                manager,
                name.split(':').next().unwrap_or(name).into(),
            )));
        }
        if output.status.code() != Some(1)
            || !(stderr.contains(unowned) || stdout.contains(unowned))
        {
            return Err(format!(
                "Cannot determine the owner of {}: {}",
                path.display(),
                stderr.trim()
            ));
        }
    }
    Ok(None)
}

pub fn terminal_launcher_path() -> std::path::PathBuf {
    ottrin_core::default_home_dir().join(".local/bin/ottrin")
}

/// Instructions for the running copy. Resolve the native package only when the
/// executable is at the native package entry point.
pub fn uninstall_instructions(purge: bool) -> Result<(&'static str, String), String> {
    let target = super::application_launch_path()?;
    if target == Path::new("/usr/bin/ottrin")
        && let Some((manager, name)) = package_owner(&target)?
    {
        if name != "ottrin" {
            return Err(format!(
                "The package manager reports a different owner: {name}"
            ));
        }
        let label = match manager {
            PackageManager::Apt => "DEB package",
            PackageManager::Dnf => "RPM package",
        };
        let command = if purge && manager == PackageManager::Apt {
            "sudo apt purge ottrin"
        } else {
            manager.remove_command()
        };
        return Ok((label, command.into()));
    }
    let label = if std::env::var_os("APPIMAGE").is_some_and(|image| Path::new(&image) == target) {
        "AppImage"
    } else {
        "Standalone build"
    };
    let path = target
        .to_str()
        .ok_or("Cannot copy an uninstall command for a non-UTF-8 executable path")?;
    let args = if purge {
        "--uninstall --purge-config"
    } else {
        "--uninstall"
    };
    Ok((label, format!("'{}' {args}", path.replace('\'', "'\\''"))))
}

pub fn remove_terminal_launcher() -> Result<String, String> {
    let path = terminal_launcher_path();
    let target = super::application_launch_path()?;
    match fs::read_link(&path) {
        Ok(link) if link == target => {
            fs::remove_file(&path)
                .map_err(|error| format!("Cannot remove {}: {error}", path.display()))?;
            Ok("Terminal command uninstalled.".into())
        }
        Ok(_) => Ok("Terminal command belongs to another installation and was kept.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok("No terminal command is installed.".into())
        }
        Err(error) => Err(format!(
            "Cannot confirm ownership of {}: {error}",
            path.display()
        )),
    }
}

pub fn remove_desktop_launcher() -> Result<String, String> {
    let path = super::integration_storage::Directories::current()?
        .data
        .join("applications/org.ottrin.Ottrin.desktop");
    let target = super::application_launch_path()?;
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok("No desktop launcher is installed.".into());
        }
        Err(error) => return Err(format!("Cannot inspect {}: {error}", path.display())),
        Ok(metadata) if !metadata.is_file() => {
            return Err(format!(
                "Unrecognized desktop launcher retained: {}",
                path.display()
            ));
        }
        Ok(_) => {}
    }
    let launcher = gio::DesktopAppInfo::from_filename(&path)
        .ok_or_else(|| format!("Unrecognized desktop launcher retained: {}", path.display()))?;
    if launcher.executable() != target {
        return Ok(format!(
            "A desktop launcher for another executable was kept: {}",
            path.display()
        ));
    }
    fs::remove_file(&path).map_err(|error| format!("Cannot remove {}: {error}", path.display()))?;
    Ok("Desktop launcher uninstalled.".into())
}
