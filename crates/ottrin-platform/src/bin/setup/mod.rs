use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

const POLICY: &[u8] = include_bytes!("../../../../../packaging/linux/org.ottrin.filesystem.policy");

pub fn install() -> Result<(), String> {
    // Read the running executable, rather than reopening a replaceable package pathname.
    let mut helper = Vec::new();
    fs::File::open("/proc/self/exe")
        .and_then(|mut file| file.read_to_end(&mut helper))
        .map_err(|error| format!("Cannot read the running helper: {error}"))?;
    install_at(Path::new("/"), &helper, 0).map_err(|error| error.to_string())
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

fn install_at(root: &Path, helper: &[u8], owner: u32) -> std::io::Result<()> {
    protected_directory(root, owner)?;
    for relative in [
        "usr",
        "usr/libexec",
        "usr/libexec/ottrin",
        "usr/share",
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
        &root.join("usr/share/polkit-1/actions/org.ottrin.filesystem.policy"),
        POLICY,
        0o644,
        owner,
    )
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
        install_at(&root, b"first", owner).unwrap();
        let helper = root.join("usr/libexec/ottrin/ottrin-priv-helper");
        fs::write(root.join("usr/libexec/ottrin/other"), "keep").unwrap();
        install_at(&root, b"second", owner).unwrap();
        assert_eq!(fs::read(&helper).unwrap(), b"second");
        assert_eq!(fs::metadata(helper).unwrap().mode() & 0o777, 0o755);
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
        install_at(&root, b"original", owner).unwrap();
        let helper = root.join("usr/libexec/ottrin/ottrin-priv-helper");
        let other = root.join("keep");
        fs::write(&other, b"preserve").unwrap();
        fs::remove_file(&helper).unwrap();
        std::os::unix::fs::symlink(&other, helper).unwrap();
        assert!(install_at(&root, b"replacement", owner).is_err());
        assert_eq!(fs::read(other).unwrap(), b"preserve");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_symlink_and_writable_namespaces() {
        let root = fixture();
        let owner = fs::metadata(&root).unwrap().uid();
        std::os::unix::fs::symlink("/usr", root.join("usr")).unwrap();
        assert!(install_at(&root, b"test", owner).is_err());
        fs::remove_file(root.join("usr")).unwrap();
        fs::create_dir(root.join("usr")).unwrap();
        fs::set_permissions(root.join("usr"), fs::Permissions::from_mode(0o777)).unwrap();
        assert!(install_at(&root, b"test", owner).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
