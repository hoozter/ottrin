//! Read-only admission for the existing Linux retained Move engine.
use super::*;
use crate::directory_watch::DirectoryWatch;
use std::fs::File;
use std::sync::Arc;

fn error(error: impl std::fmt::Display) -> PlatformError {
    PlatformError::Io(format!("Move recovery admission: {error}"))
}

#[derive(Debug)]
struct Directory {
    path: PathBuf,
    identity: move_recovery::LinuxStableIdentity,
    mount: u64,
    watch: DirectoryWatch,
}

impl Directory {
    fn open(path: &Path) -> Result<File, PlatformError> {
        use rustix::fs::{Mode, OFlags, open};
        open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(error)
    }
    fn mount(file: &File) -> Result<u64, PlatformError> {
        crate::local_storage::local_fd_mount_identity(file).map_err(error)
    }
    fn capture(
        path: PathBuf,
        expected: Option<move_recovery::LinuxStableIdentity>,
    ) -> Result<Self, PlatformError> {
        if !path.is_absolute() {
            return Err(error("directory path is not absolute"));
        }
        let file = Self::open(&path)?;
        let identity = stable_identity_from_metadata(&file.metadata().map_err(error)?);
        if expected.is_some_and(|expected| expected != identity) {
            return Err(error("recorded directory identity changed"));
        }
        let result = Self {
            path,
            identity,
            mount: Self::mount(&file)?,
            watch: DirectoryWatch::new(&file).map_err(error)?,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), PlatformError> {
        self.watch.validate().map_err(error)?;
        let file = Self::open(&self.path)?;
        if stable_identity_from_metadata(&file.metadata().map_err(error)?) != self.identity
            || Self::mount(&file)? != self.mount
        {
            return Err(error("directory or mount changed; refresh Activity"));
        }
        self.watch.validate().map_err(error)
    }
}

#[derive(Debug)]
struct Admission {
    root: PathBuf,
    transaction_id: [u8; 16],
    central: Vec<u8>,
    local: Vec<u8>,
    source_parent: PathBuf,
    source_transaction_name: PathBuf,
    source_transaction_identity: move_recovery::LinuxStableIdentity,
    directories: Vec<Directory>,
    paths: Vec<PathBuf>,
}

/// An immutable journal snapshot and short-lived native directory proofs.
/// Capture runs off the UI thread and never creates recovery state. Retaining
/// a plan keeps watches, not directory handles that would prevent unmount.
#[derive(Clone, Debug)]
pub struct LinuxMoveRecoveryPlan(Arc<Admission>);

impl PartialEq for LinuxMoveRecoveryPlan {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for LinuxMoveRecoveryPlan {}

impl LinuxMoveRecoveryPlan {
    pub fn capture(transaction_id: [u8; 16]) -> Result<Self, PlatformError> {
        Self::capture_at(&default_recovery_root()?, transaction_id)
    }
    pub fn capture_at(root: &Path, transaction_id: [u8; 16]) -> Result<Self, PlatformError> {
        let central =
            move_recovery::LinuxJournalDirectory::open_configured_read_only(root).map_err(error)?;
        let bytes = central
            .read_record_bytes(&format!("{}.journal", transaction_id_hex(&transaction_id)))?;
        let (
            source_parent,
            source_parent_identity,
            source_name,
            source_identity,
            destination,
            destination_stage,
        ) = match move_recovery::PendingDestinationRecord::parse(&bytes) {
            Ok(record) => (
                record.source_parent().map_err(error)?,
                record.source_parent_identity(),
                record.source_transaction_name().map_err(error)?,
                record.source_transaction_identity(),
                record.pending_destination_name().map_err(error)?,
                record
                    .destination_transaction_identity()
                    .map(|id| record.destination_transaction_name().map(|name| (name, id)))
                    .transpose()
                    .map_err(error)?,
            ),
            Err(_) => {
                let record = move_recovery::PublishedRecord::parse(&bytes).map_err(error)?;
                (
                    record.source_parent().map_err(error)?,
                    record.source_parent_identity(),
                    record.source_transaction_name().map_err(error)?,
                    record.source_transaction_identity(),
                    record.pending_destination_name().map_err(error)?,
                    record
                        .destination_transaction_identity()
                        .map(|id| record.destination_transaction_name().map(|name| (name, id)))
                        .transpose()
                        .map_err(error)?,
                )
            }
        };
        let parent =
            move_recovery::LinuxSourceParentDirectory::open(&source_parent).map_err(error)?;
        let local = parent
            .open_existing_transaction(&source_name, source_identity)
            .map_err(error)?;
        // Pair validation accepts the existing legitimate central-first gap,
        // while the snapshot still preserves each observed file byte for byte.
        if move_recovery::PendingDestinationRecord::parse(&bytes).is_ok() {
            central
                .load_pending_destination_restart_state(&local, transaction_id)
                .map_err(error)?;
        } else {
            central
                .load_published_restart_state(&local, transaction_id)
                .map_err(error)?;
        }
        let local_bytes = local.read_record_bytes("journal")?;
        let mut directories = vec![
            Directory::capture(root.to_path_buf(), None)?,
            Directory::capture(source_parent.clone(), Some(source_parent_identity))?,
            Directory::capture(source_parent.join(&source_name), Some(source_identity))?,
        ];
        let destination_parent = destination
            .parent()
            .filter(|_| destination.is_absolute())
            .ok_or_else(|| error("destination has no absolute parent"))?;
        directories.push(Directory::capture(destination_parent.to_path_buf(), None)?);
        if let Some((name, identity)) = destination_stage {
            if name.file_name() != Some(name.as_os_str()) {
                return Err(error("invalid destination transaction component"));
            }
            directories.push(Directory::capture(
                destination_parent.join(name),
                Some(identity),
            )?);
        }
        let mut paths = directories
            .iter()
            .map(|d| d.path.clone())
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        let plan = Self(Arc::new(Admission {
            root: root.to_path_buf(),
            transaction_id,
            central: bytes,
            local: local_bytes,
            source_parent,
            source_transaction_name: source_name,
            source_transaction_identity: source_identity,
            directories,
            paths,
        }));
        plan.validate(&central)?;
        Ok(plan)
    }
    pub fn transaction_id(&self) -> [u8; 16] {
        self.0.transaction_id
    }
    pub fn paths(&self) -> &[PathBuf] {
        &self.0.paths
    }
    pub(super) fn root(&self) -> &Path {
        &self.0.root
    }
    pub(super) fn central_bytes(&self) -> &[u8] {
        &self.0.central
    }

    /// Called by the engine under its existing exclusive recovery lock before
    /// journal repair, restoration, publication or cleanup can begin.
    pub(super) fn validate(
        &self,
        central: &move_recovery::LinuxJournalDirectory,
    ) -> Result<(), PlatformError> {
        for directory in &self.0.directories {
            directory.validate()?;
        }
        let parent = move_recovery::LinuxSourceParentDirectory::open(&self.0.source_parent)
            .map_err(error)?;
        let local = parent
            .open_existing_transaction(
                &self.0.source_transaction_name,
                self.0.source_transaction_identity,
            )
            .map_err(error)?;
        if central.read_record_bytes(&format!(
            "{}.journal",
            transaction_id_hex(&self.0.transaction_id)
        ))? != self.0.central
            || local.read_record_bytes("journal")? != self.0.local
        {
            return Err(error("journals changed; refresh Activity"));
        }
        for directory in &self.0.directories {
            directory.validate()?;
        }
        Ok(())
    }
    pub fn execute(&self) -> Result<(), PlatformError> {
        recover_admitted_move(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_recovery_root_is_never_initialized_by_admission() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("missing");
        assert!(LinuxMoveRecoveryPlan::capture_at(&root, [0x93; 16]).is_err());
        assert!(!root.exists());
        assert!(std::fs::read_dir(fixture.path()).unwrap().next().is_none());
    }

    #[test]
    #[ignore = "requires an explicitly owned private user and mount namespace"]
    fn directory_admission_allows_unmount_and_refuses_same_inode_bind_remount() {
        use std::process::Command;
        let parent_namespace = std::env::var("OTTRIN_MOVE_PARENT_MOUNT_NAMESPACE").unwrap();
        assert_ne!(
            std::fs::read_link("/proc/self/ns/mnt")
                .unwrap()
                .to_string_lossy(),
            parent_namespace
        );
        struct Mounted {
            path: PathBuf,
            active: bool,
        }
        impl Mounted {
            fn unmount(mut self) {
                let output = Command::new("umount").arg(&self.path).output().unwrap();
                assert!(output.status.success(), "{:?}", output);
                self.active = false;
            }
        }
        impl Drop for Mounted {
            fn drop(&mut self) {
                if self.active {
                    let output = Command::new("umount").arg(&self.path).output().unwrap();
                    assert!(
                        output.status.success(),
                        "owned mount cleanup failed: {output:?}"
                    );
                }
            }
        }
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("volume");
        std::fs::create_dir(&root).unwrap();
        let output = Command::new("mount")
            .args(["-t", "tmpfs", "tmpfs"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        let mounted = Mounted {
            path: root.clone(),
            active: true,
        };
        let proof = Directory::capture(root.clone(), None).unwrap();
        mounted.unmount();
        assert!(proof.validate().is_err());
        drop(proof);

        let output = Command::new("mount")
            .arg("--bind")
            .arg(&root)
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        let mounted = Mounted {
            path: root.clone(),
            active: true,
        };
        let proof = Directory::capture(root.clone(), None).unwrap();
        let before = std::fs::metadata(&root).unwrap();
        mounted.unmount();
        let output = Command::new("mount")
            .arg("--bind")
            .arg(&root)
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        let mounted = Mounted {
            path: root.clone(),
            active: true,
        };
        let after = std::fs::metadata(&root).unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        assert!(
            proof.validate().is_err(),
            "same inode does not authorize a new mount"
        );
        mounted.unmount();
    }
}
