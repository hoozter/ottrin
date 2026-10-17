//! Configured Copy registry provisioning and external native-identity anchor.
use super::{WindowsStableIdentity, io, namespace, security, stable_identity_from_handle};
use crate::{PlatformError, windows_fs};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs::File,
    io::Read,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::Storage::FileSystem::*;

const ANCHOR: &str = ".ottrin-copy-recovery-root";
const MAX_ANCHOR: u64 = 65_536;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Anchor {
    protocol: String,
    parent: WindowsStableIdentity,
    marker: WindowsStableIdentity,
    root: WindowsStableIdentity,
    name: Vec<u16>,
    creator: Vec<u8>,
    security: security::PrivateSecurity,
}

pub(super) struct ConfiguredRegistry {
    root: File,
    marker: File,
    marker_security: security::PrivateSecurity,
    parent: File,
    record: Anchor,
    parent_location: PathBuf,
    path: PathBuf,
}

pub(super) fn configured_path() -> Result<PathBuf, PlatformError> {
    let local =
        std::env::var_os("LOCALAPPDATA").ok_or_else(|| io("LOCALAPPDATA is unavailable"))?;
    let local = PathBuf::from(local);
    if !local.is_absolute() {
        return Err(io("LOCALAPPDATA must be absolute"));
    }
    Ok(local.join("Ottrin").join("State").join("copies"))
}

fn child(
    parent: &File,
    name: &OsStr,
    directory: bool,
    access: u32,
    sharing: u32,
) -> std::io::Result<File> {
    unsafe {
        windows_fs::open_relative(
            parent,
            name,
            windows_fs::RelativeOptions {
                directory: Some(directory),
                create_new: false,
                access,
                sharing,
                security: ptr::null(),
            },
        )
    }
}

fn parent_directory(path: &Path, initialize: bool) -> Result<Option<File>, PlatformError> {
    let access = if initialize {
        FILE_GENERIC_READ | FILE_GENERIC_WRITE
    } else {
        FILE_GENERIC_READ
    };
    match windows_fs::open_directory(path, access) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !initialize => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or_else(|| io("Registry parent has no existing ancestor"))?;
            let name = path
                .file_name()
                .ok_or_else(|| io("Registry parent has no native name"))?;
            let parent = parent_directory(parent, true)?.unwrap();
            let file = match windows_fs::create_private(
                &parent,
                name,
                true,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => child(
                    &parent,
                    name,
                    true,
                    FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                )
                .map_err(io)?,
                Err(error) => return Err(io(error)),
            };
            let identity = stable_identity_from_handle(&file).map_err(io)?;
            if !identity.is_directory_without_reparse() {
                return Err(io("Registry parent is a reparse entry"));
            }
            namespace::validate_binding(&parent, name, identity)?;
            parent.sync_all().map_err(io)?;
            Ok(Some(file))
        }
        Err(error) => Err(io(error)),
    }
}

impl ConfiguredRegistry {
    /// Read-only admission never initializes directories, anchors or journals.
    /// Existing unanchored/partial/foreign state is retained and refused.
    pub(super) fn open_at(path: &Path, initialize: bool) -> Result<Option<Self>, PlatformError> {
        if !path.is_absolute()
            || path.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err(io("Copy registry requires an unambiguous absolute path"));
        }
        let name = path
            .file_name()
            .ok_or_else(|| io("Copy registry has no native leaf"))?;
        windows_fs::valid_component(name).map_err(io)?;
        let parent_path = path
            .parent()
            .ok_or_else(|| io("Copy registry has no parent"))?;
        let Some(parent) = parent_directory(parent_path, initialize)? else {
            return Ok(None);
        };
        let parent_location = windows_fs::location(&parent).map_err(io)?;
        let creator = security::current_owner()?;
        let marker = match child(
            &parent,
            OsStr::new(ANCHOR),
            false,
            FILE_GENERIC_READ,
            FILE_SHARE_READ,
        ) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match unsafe {
                    windows_fs::open_relative(
                        &parent,
                        name,
                        windows_fs::RelativeOptions {
                            directory: None,
                            create_new: false,
                            access: FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                            sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                            security: ptr::null(),
                        },
                    )
                } {
                    Ok(_) => {
                        return Err(io(
                            "Copy registry exists without its identity anchor; retained for review",
                        ));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(io(error)),
                }
                if !initialize {
                    return Ok(None);
                }
                // Reserve the external anchor before creating the registry. A
                // partial setup never authorizes adoption or automatic deletion.
                let mut marker =
                    windows_fs::create_private(&parent, OsStr::new(ANCHOR), false, FILE_SHARE_READ)
                        .map_err(io)?;
                let result = (|| {
                    parent.sync_all().map_err(io)?;
                    let root = windows_fs::create_private(
                        &parent,
                        name,
                        true,
                        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    )
                    .map_err(io)?;
                    let record = Anchor {
                        protocol: "OTCRW01".into(),
                        parent: stable_identity_from_handle(&parent).map_err(io)?,
                        marker: stable_identity_from_handle(&marker).map_err(io)?,
                        root: stable_identity_from_handle(&root).map_err(io)?,
                        name: name.encode_wide().collect(),
                        creator: creator.clone(),
                        security: security::PrivateSecurity::capture(&root, &creator)?,
                    };
                    serde_json::to_writer(&mut marker, &record).map_err(io)?;
                    marker.sync_all().map_err(io)?;
                    root.sync_all().map_err(io)?;
                    parent.sync_all().map_err(io)
                })();
                result.map_err(|error| PlatformError::CleanupPending {transaction:parent_location.display().to_string(),message:format!("Copy registry setup needs review: {error}; anchor and any directory retained")})?;
                drop(marker);
                child(
                    &parent,
                    OsStr::new(ANCHOR),
                    false,
                    FILE_GENERIC_READ,
                    FILE_SHARE_READ,
                )
                .map_err(io)?
            }
            Err(error) => return Err(io(error)),
        };
        let marker_id = stable_identity_from_handle(&marker).map_err(io)?;
        if !marker_id.is_regular_file_without_reparse() {
            return Err(io("Copy registry anchor is not an ordinary file"));
        }
        namespace::validate_single_link(&marker)?;
        let marker_security = security::PrivateSecurity::capture(&marker, &creator)?;
        let mut bytes = Vec::new();
        (&marker)
            .take(MAX_ANCHOR + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() as u64 > MAX_ANCHOR {
            return Err(io("Copy registry anchor exceeds its bound"));
        }
        let record: Anchor = serde_json::from_slice(&bytes).map_err(|error| {
            io(format!(
                "Copy registry anchor is incomplete or invalid; retained for review: {error}"
            ))
        })?;
        if record.protocol != "OTCRW01"
            || record.parent != stable_identity_from_handle(&parent).map_err(io)?
            || record.marker != marker_id
            || record.name != name.encode_wide().collect::<Vec<_>>()
            || record.creator != creator
        {
            return Err(io(
                "Copy registry anchor identity or creator changed; retained for review",
            ));
        }
        let access = if initialize {
            FILE_GENERIC_READ | FILE_GENERIC_WRITE
        } else {
            FILE_GENERIC_READ
        };
        let root = child(
            &parent,
            name,
            true,
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )
        .map_err(io)?;
        let result = Self {
            root,
            marker,
            marker_security,
            parent,
            record,
            parent_location: parent_location.clone(),
            path: parent_location.join(name),
        };
        result.validate()?;
        Ok(Some(result))
    }
    pub(super) fn root(&self) -> &File {
        &self.root
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn validate(&self) -> Result<(), PlatformError> {
        if !self.record.root.is_directory_without_reparse()
            || windows_fs::location(&self.parent).map_err(io)? != self.parent_location
            || stable_identity_from_handle(&self.parent).map_err(io)? != self.record.parent
            || stable_identity_from_handle(&self.root).map_err(io)? != self.record.root
            || stable_identity_from_handle(&self.marker).map_err(io)? != self.record.marker
            || security::current_owner()? != self.record.creator
        {
            return Err(io("Configured Copy registry binding changed"));
        }
        self.record
            .security
            .validate(&self.root, &self.record.creator)?;
        self.marker_security
            .validate(&self.marker, &self.record.creator)?;
        namespace::validate_binding(&self.parent, OsStr::new(ANCHOR), self.record.marker)?;
        namespace::validate_binding(
            &self.parent,
            self.path.file_name().unwrap(),
            self.record.root,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn windows_native_copy_registry_provisions_once_and_readers_hold_the_binding() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("profile/Ottrin/State/copies");
        assert!(ConfiguredRegistry::open_at(&path, false).unwrap().is_none());
        assert!(!root.path().join("profile").exists());
        let writer = ConfiguredRegistry::open_at(&path, true).unwrap().unwrap();
        let id = stable_identity_from_handle(writer.root()).unwrap();
        let marker = std::fs::read(path.parent().unwrap().join(ANCHOR)).unwrap();
        let reader = ConfiguredRegistry::open_at(&path, false).unwrap().unwrap();
        assert_eq!(stable_identity_from_handle(reader.root()).unwrap(), id);
        reader.validate().unwrap();
        assert!(std::fs::rename(&path, path.with_file_name("moved")).is_err());
        assert!(!path.with_file_name("moved").exists());
        drop((reader, writer));
        let next = ConfiguredRegistry::open_at(&path, true).unwrap().unwrap();
        assert_eq!(stable_identity_from_handle(next.root()).unwrap(), id);
        assert_eq!(
            std::fs::read(path.parent().unwrap().join(ANCHOR)).unwrap(),
            marker
        );
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
    }

    #[test]
    fn windows_native_copy_registry_rejects_replaced_root_marker_and_parent() {
        for case in ["root", "marker", "parent"] {
            let root = tempfile::tempdir().unwrap();
            let parent = root.path().join("state");
            let path = parent.join("copies");
            drop(ConfiguredRegistry::open_at(&path, true).unwrap().unwrap());
            std::fs::write(path.join("user-data"), b"preserved").unwrap();
            let marker = std::fs::read(parent.join(ANCHOR)).unwrap();
            match case {
                "root" => {
                    std::fs::rename(&path, parent.join("saved")).unwrap();
                    let owner =
                        windows_fs::open_directory(&parent, FILE_GENERIC_READ | FILE_GENERIC_WRITE)
                            .unwrap();
                    drop(
                        windows_fs::create_private(
                            &owner,
                            OsStr::new("copies"),
                            true,
                            FILE_SHARE_READ,
                        )
                        .unwrap(),
                    );
                }
                "marker" => {
                    std::fs::rename(parent.join(ANCHOR), parent.join("saved-marker")).unwrap();
                    let owner =
                        windows_fs::open_directory(&parent, FILE_GENERIC_READ | FILE_GENERIC_WRITE)
                            .unwrap();
                    let mut replacement = windows_fs::create_private(
                        &owner,
                        OsStr::new(ANCHOR),
                        false,
                        FILE_SHARE_READ,
                    )
                    .unwrap();
                    replacement.write_all(&marker).unwrap();
                    replacement.sync_all().unwrap();
                }
                "parent" => {
                    std::fs::rename(&parent, root.path().join("saved-state")).unwrap();
                    std::fs::create_dir(&parent).unwrap();
                    std::fs::rename(root.path().join("saved-state/copies"), &path).unwrap();
                    std::fs::rename(
                        root.path().join("saved-state").join(ANCHOR),
                        parent.join(ANCHOR),
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
            for initialize in [false, true] {
                assert!(
                    ConfiguredRegistry::open_at(&path, initialize).is_err(),
                    "{case}"
                );
            }
            assert_eq!(std::fs::read(parent.join(ANCHOR)).unwrap(), marker);
            let saved = if case == "root" {
                parent.join("saved/user-data")
            } else {
                path.join("user-data")
            };
            assert_eq!(std::fs::read(saved).unwrap(), b"preserved");
        }
    }

    #[test]
    fn windows_native_copy_registry_refuses_unanchored_partial_and_foreign_state_without_repair() {
        for case in ["unanchored", "partial", "missing-root", "permissions"] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("copies");
            if case == "unanchored" {
                std::fs::create_dir(&path).unwrap();
                std::fs::write(path.join("user-data"), b"retained").unwrap();
            } else if case == "partial" {
                let parent =
                    windows_fs::open_directory(root.path(), FILE_GENERIC_READ | FILE_GENERIC_WRITE)
                        .unwrap();
                let mut marker =
                    windows_fs::create_private(&parent, OsStr::new(ANCHOR), false, FILE_SHARE_READ)
                        .unwrap();
                marker.write_all(b"{\"protocol\":").unwrap();
                marker.sync_all().unwrap();
            } else {
                drop(ConfiguredRegistry::open_at(&path, true).unwrap().unwrap());
                if case == "missing-root" {
                    std::fs::rename(&path, root.path().join("saved")).unwrap();
                } else {
                    super::super::tests::set_dacl(&path, "D:P(A;OICI;FA;;;OW)(A;OICI;GR;;;WD)");
                }
            }
            let marker = std::fs::read(root.path().join(ANCHOR)).ok();
            let entries = std::fs::read_dir(root.path()).unwrap().count();
            for initialize in [false, true] {
                assert!(
                    ConfiguredRegistry::open_at(&path, initialize).is_err(),
                    "{case}"
                );
            }
            assert_eq!(std::fs::read(root.path().join(ANCHOR)).ok(), marker);
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), entries);
            if case == "unanchored" {
                assert_eq!(std::fs::read(path.join("user-data")).unwrap(), b"retained");
            }
        }
    }

    #[test]
    fn windows_native_copy_registry_configured_dispatch_discovery_and_source_free_undo() {
        const ROOT: &str = "OTTRIN_TEST_CONFIGURED_COPY_REGISTRY";
        if let Some(root) = std::env::var_os(ROOT) {
            let root = PathBuf::from(root);
            assert_eq!(
                std::env::var_os("LOCALAPPDATA").unwrap(),
                root.join("profile")
            );
            let outside = tempfile::tempdir().unwrap();
            assert!(
                super::super::dispatch::copy_configured(
                    &root,
                    outside.path(),
                    crate::ConflictAction::Overwrite,
                    100,
                    |_| Ok(())
                )
                .is_err()
            );
            assert!(
                !root.join("profile").exists(),
                "Rejected Copy must not provision anything inside its source"
            );
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
            let source = root.join("source.txt");
            let destination = root.join("destination");
            std::fs::write(&source, b"configured copy").unwrap();
            std::fs::create_dir(&destination).unwrap();
            let receipt = super::super::dispatch::copy_configured(
                &source,
                &destination,
                crate::ConflictAction::Overwrite,
                100,
                |_| Ok(()),
            )
            .unwrap()
            .unwrap();
            let entries = crate::discover_retained_copy_transactions().unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(
                Some(entries[0].transaction_id.as_str()),
                receipt.copy_transaction_id()
            );
            std::fs::remove_file(&source).unwrap();
            let mut plan =
                crate::restore_retained_copy_undo_plan(&entries[0].transaction_id).unwrap();
            plan.undo_next().unwrap();
            assert!(!destination.join("source.txt").exists());
            assert_eq!(
                std::fs::read(receipt.before_path()).unwrap(),
                b"configured copy"
            );
            assert_eq!(
                crate::discover_retained_copy_transactions().unwrap().len(),
                1
            );
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let output=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","copy_transaction::windows::registry::tests::windows_native_copy_registry_configured_dispatch_discovery_and_source_free_undo","--nocapture"])
            .env(ROOT,root.path()).env("LOCALAPPDATA",root.path().join("profile")).output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
