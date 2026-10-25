use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

const HTML_POLICY: &[u8] =
    include_bytes!("../../../../../packaging/linux/ottrin-html-render.apparmor");

const POLICY: &[u8] = include_bytes!("../../../../../packaging/linux/org.ottrin.filesystem.policy");

pub fn install() -> Result<(), String> {
    // Read the running executable, rather than reopening a replaceable package pathname.
    let mut helper = Vec::new();
    fs::File::open("/proc/self/exe")
        .and_then(|mut file| file.read_to_end(&mut helper))
        .map_err(|error| format!("Cannot read the running helper: {error}"))?;
    let path = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("Helper has no installation directory")?
        .join("ottrin-html-render");
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|e| format!("Cannot read bundled HTML sandbox: {e}"))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Bundled HTML sandbox is not a regular file".into());
    }
    let mut html = Vec::new();
    file.read_to_end(&mut html).map_err(|e| e.to_string())?;
    install_at(Path::new("/"), &helper, &html, 0).map_err(|error| error.to_string())?;
    configure_html_sandbox()
}

fn protected_directory(path: &Path, owner: u32) -> std::io::Result<()> {
    if !path.exists() {
        fs::create_dir(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != owner || metadata.mode() & 0o022 != 0 {
        return Err(std::io::Error::other(format!(
            "Unsafe installation directory: {}",
            path.display()
        )));
    }
    Ok(())
}

fn write_protected(path: &Path, bytes: &[u8], mode: u32, owner: u32) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if !metadata.is_file() || metadata.uid() != owner || metadata.mode() & 0o022 != 0 =>
        {
            return Err(std::io::Error::other(format!(
                "Unsafe installation target: {}",
                path.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut created = false;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&temporary)?;
        created = true;
        file.write_all(bytes)?;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::File::open(path.parent().expect("fixed installation parent"))?.sync_all()
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn install_at(root: &Path, helper: &[u8], html: &[u8], owner: u32) -> std::io::Result<()> {
    protected_directory(root, owner)?;
    for relative in [
        "usr",
        "usr/libexec",
        "usr/libexec/ottrin",
        "usr/share",
        "usr/share/ottrin",
        "usr/share/polkit-1",
        "usr/share/polkit-1/actions",
    ] {
        protected_directory(&root.join(relative), owner)?;
    }
    write_protected(
        &root.join("usr/libexec/ottrin/ottrin-priv-helper"),
        helper,
        0o755,
        owner,
    )?;
    write_protected(
        &root.join("usr/libexec/ottrin/ottrin-html-render"),
        html,
        0o755,
        owner,
    )?;
    write_protected(
        &root.join("usr/share/ottrin/ottrin-html-render.apparmor"),
        HTML_POLICY,
        0o644,
        owner,
    )?;
    write_protected(
        &root.join("usr/share/polkit-1/actions/org.ottrin.filesystem.policy"),
        POLICY,
        0o644,
        owner,
    )
}

/// Activate only our policy on hosts that need/support AppArmor 4. Hosts without
/// namespace restrictions still use the same helper and namespace plan.
pub fn configure_html_sandbox() -> Result<(), String> {
    // SAFETY: read-only process identity inspection.
    if unsafe { libc::geteuid() } != 0 {
        return Err("System integration requires administrator permission".into());
    }
    let restricted = fs::read_to_string("/proc/sys/kernel/apparmor_restrict_unprivileged_userns")
        .is_ok_and(|s| s.trim() == "1");
    let enabled = fs::read_to_string("/sys/module/apparmor/parameters/enabled")
        .is_ok_and(|s| s.trim() == "Y");
    if !enabled && !restricted {
        return Ok(());
    }
    if !Path::new("/etc/apparmor.d/abi/4.0").is_file() {
        return if restricted {
            Err("HTML sandbox setup requires AppArmor 4 policy support".into())
        } else {
            Ok(())
        };
    }
    protected_directory(Path::new("/etc"), 0).map_err(|e| e.to_string())?;
    protected_directory(Path::new("/etc/apparmor.d"), 0).map_err(|e| e.to_string())?;
    let path = Path::new("/etc/apparmor.d/ottrin-html-render");
    write_protected(path, HTML_POLICY, 0o644, 0).map_err(|e| e.to_string())?;
    let parser = ["/usr/sbin/apparmor_parser", "/sbin/apparmor_parser"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .ok_or("Install the AppArmor policy loader to finish HTML sandbox setup")?;
    let output = std::process::Command::new(parser)
        .arg("-r")
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "HTML sandbox policy could not be activated: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

pub fn remove_html_policy() -> Result<(), String> {
    // SAFETY: read-only process identity inspection.
    if unsafe { libc::geteuid() } != 0 {
        return Err("System integration removal requires administrator permission".into());
    }
    let path = Path::new("/etc/apparmor.d/ottrin-html-render");
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
        Ok(m) if !m.is_file() || m.uid() != 0 || m.mode() & 0o022 != 0 => {
            return Err("Cannot establish ownership of HTML sandbox policy".into());
        }
        Ok(_) => {}
    }
    if fs::read(path).map_err(|e| e.to_string())? != HTML_POLICY {
        println!("Modified HTML sandbox policy retained.");
        return Ok(());
    }
    fs::remove_file(path).map_err(|e| e.to_string())?;
    // Do not unload confinement from previews already running. The bounded
    // jobs finish under the cached policy; no new entry is possible after the
    // package removes its root-owned executable. A later setup replaces it.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ottrin-setup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        root
    }

    #[test]
    fn install_and_update_preserve_other_files_and_set_permissions() {
        let root = fixture();
        let owner = fs::metadata(&root).unwrap().uid();
        install_at(&root, b"first", b"HTML sandbox", owner).unwrap();
        let helper = root.join("usr/libexec/ottrin/ottrin-priv-helper");
        fs::write(root.join("usr/libexec/ottrin/other"), "keep").unwrap();
        install_at(&root, b"second", b"HTML sandbox", owner).unwrap();
        assert_eq!(fs::read(&helper).unwrap(), b"second");
        assert_eq!(fs::metadata(helper).unwrap().mode() & 0o777, 0o755);
        let html = root.join("usr/libexec/ottrin/ottrin-html-render");
        assert_eq!(fs::read(&html).unwrap(), b"HTML sandbox");
        assert_eq!(fs::metadata(html).unwrap().mode() & 0o777, 0o755);
        let html_policy = root.join("usr/share/ottrin/ottrin-html-render.apparmor");
        assert_eq!(fs::read(&html_policy).unwrap(), HTML_POLICY);
        assert_eq!(fs::metadata(html_policy).unwrap().mode() & 0o777, 0o644);
        let policy = root.join("usr/share/polkit-1/actions/org.ottrin.filesystem.policy");
        assert_eq!(fs::read(&policy).unwrap(), POLICY);
        assert_eq!(fs::metadata(policy).unwrap().mode() & 0o777, 0o644);
        assert_eq!(
            fs::read_to_string(root.join("usr/libexec/ottrin/other")).unwrap(),
            "keep"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_symlink_target_without_touching_destination() {
        let root = fixture();
        let owner = fs::metadata(&root).unwrap().uid();
        install_at(&root, b"original", b"HTML sandbox", owner).unwrap();
        let helper = root.join("usr/libexec/ottrin/ottrin-priv-helper");
        let other = root.join("keep");
        fs::write(&other, b"preserve").unwrap();
        fs::remove_file(&helper).unwrap();
        std::os::unix::fs::symlink(&other, helper).unwrap();
        assert!(install_at(&root, b"replacement", b"HTML sandbox", owner).is_err());
        assert_eq!(fs::read(other).unwrap(), b"preserve");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_symlink_and_writable_namespaces() {
        let root = fixture();
        let owner = fs::metadata(&root).unwrap().uid();
        std::os::unix::fs::symlink("/usr", root.join("usr")).unwrap();
        assert!(install_at(&root, b"test", b"HTML sandbox", owner).is_err());
        fs::remove_file(root.join("usr")).unwrap();
        fs::create_dir(root.join("usr")).unwrap();
        fs::set_permissions(root.join("usr"), fs::Permissions::from_mode(0o777)).unwrap();
        assert!(install_at(&root, b"test", b"HTML sandbox", owner).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
