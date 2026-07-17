use ottrin_core::default_home_dir;
use std::path::PathBuf;

fn launcher_target_path() -> Option<PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE").map(PathBuf::from)
        && appimage.exists()
    {
        return Some(appimage);
    }
    std::env::current_exe().ok()
}

fn launcher_link_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        default_home_dir().join("ottrin.bat")
    }
    #[cfg(not(target_os = "windows"))]
    {
        default_home_dir().join(".local").join("bin").join("ottrin")
    }
}

pub(crate) fn launcher_status() -> Option<String> {
    let link = launcher_link_path();
    if link.exists() {
        Some(format!("Installed at {}", link.display()))
    } else {
        Some(format!("Not installed. Suggested path: {}", link.display()))
    }
}

pub(crate) fn install_terminal_launcher() -> Result<String, String> {
    let link = launcher_link_path();
    let target = launcher_target_path()
        .ok_or_else(|| "Could not determine the current Ottrin executable path.".to_string())?;
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    if link.exists() {
        std::fs::remove_file(&link)
            .map_err(|e| format!("Failed to replace {}: {}", link.display(), e))?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, &link).map_err(|e| {
            format!(
                "Failed to create symlink {} -> {}: {}",
                link.display(),
                target.display(),
                e
            )
        })?;
        Ok(format!(
            "Installed terminal launcher at {}. Make sure ~/.local/bin is in PATH.",
            link.display()
        ))
    }
    #[cfg(target_os = "windows")]
    {
        std::fs::write(
            &link,
            format!("@echo off\r\n\"{}\" %*\r\n", target.display()),
        )
        .map_err(|e| format!("Failed to write {}: {}", link.display(), e))?;
        Ok(format!("Installed launcher script at {}.", link.display()))
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let _ = (target, link);
        Err("Terminal launcher installation is not supported on this platform yet.".to_string())
    }
}

pub(crate) fn remove_terminal_launcher() -> Result<String, String> {
    let link = launcher_link_path();
    match std::fs::remove_file(&link) {
        Ok(()) => Ok(format!("Removed terminal launcher at {}.", link.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok("No terminal launcher was installed.".to_string())
        }
        Err(err) => Err(format!("Failed to remove {}: {}", link.display(), err)),
    }
}
