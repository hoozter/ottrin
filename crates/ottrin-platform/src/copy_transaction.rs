use super::{ConflictAction, PathMoveIdentity, PlatformError};
#[cfg(target_os = "linux")]
use super::{copy_entry, next_available_name, rename_identity};
#[cfg(target_os = "linux")]
use std::ffi::OsStr;
#[cfg(any(target_os = "linux", test))]
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Session identity guards for a published Copy/create transaction's directories.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
pub struct RetainedCopyTransaction(std::sync::Arc<RetainedCopyDirectories>);

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct RetainedCopyDirectories {
    parent_mount: u64,
    stage_mount: u64,
    parent_identity: (u64, u64),
    stage_identity: (u64, u64),
    parent_path: PathBuf,
    stage_path: PathBuf,
    parent_watch: super::directory_watch::DirectoryWatch,
    stage_watch: super::directory_watch::DirectoryWatch,
}

#[cfg(target_os = "linux")]
impl PartialEq for RetainedCopyTransaction {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}
#[cfg(target_os = "linux")]
impl Eq for RetainedCopyTransaction {}

#[cfg(target_os = "linux")]
impl RetainedCopyTransaction {
    fn validate_watches(&self) -> Result<(), PlatformError> {
        self.0
            .parent_watch
            .validate()
            .and_then(|()| self.0.stage_watch.validate())
            .map_err(|_| PlatformError::StalePathMoveUndo(self.0.stage_path.display().to_string()))
    }

    fn open_directories(&self) -> Result<(std::fs::File, std::fs::File), PlatformError> {
        use std::os::unix::fs::MetadataExt;
        self.validate_watches()?;
        let open = |path: &Path, identity: (u64, u64), mount: u64| {
            let fd = rustix::fs::open(
                path,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(|_| PlatformError::StalePathMoveUndo(path.display().to_string()))?;
            let file = std::fs::File::from(fd);
            let observed = file
                .metadata()
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            if identity != (observed.dev(), observed.ino())
                || super::local_storage::local_path_mount_identity(
                    &std::path::PathBuf::from(format!(
                        "/proc/self/fd/{}",
                        std::os::fd::AsRawFd::as_raw_fd(&file)
                    )),
                    true,
                )
                .map_err(|error| PlatformError::Io(error.to_string()))?
                    != mount
            {
                return Err(PlatformError::StalePathMoveUndo(path.display().to_string()));
            }
            Ok(file)
        };
        let parent = open(
            &self.0.parent_path,
            self.0.parent_identity,
            self.0.parent_mount,
        )?;
        let stage = open(
            &self.0.stage_path,
            self.0.stage_identity,
            self.0.stage_mount,
        )?;
        self.validate_watches()?;
        Ok((parent, stage))
    }

    pub(super) fn validate_path(&self, path: &Path) -> Result<(), PlatformError> {
        use std::os::unix::fs::MetadataExt;
        let parent = path.parent().ok_or_else(|| {
            PlatformError::InvalidPathMoveUndoReceipt("transaction has no parent".into())
        })?;
        self.validate_watches()?;
        for (named, held, mount) in [
            (parent, self.0.parent_identity, self.0.parent_mount),
            (path, self.0.stage_identity, self.0.stage_mount),
        ] {
            let observed = std::fs::symlink_metadata(named)
                .map_err(|_| PlatformError::StalePathMoveUndo(named.display().to_string()))?;
            if !observed.is_dir()
                || held != (observed.dev(), observed.ino())
                || super::local_storage::local_path_mount_identity(named, false)
                    .map_err(|error| PlatformError::Io(error.to_string()))?
                    != mount
            {
                return Err(PlatformError::StalePathMoveUndo(
                    named.display().to_string(),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn validate_publication_parent(
        &self,
        published: &Path,
    ) -> Result<(), PlatformError> {
        use std::os::unix::fs::MetadataExt;
        let parent = published.parent().ok_or_else(|| {
            PlatformError::InvalidPathMoveUndoReceipt("publication has no parent".into())
        })?;
        self.validate_watches()?;
        let held = self.0.parent_identity;
        // Destinations may legitimately be directory symlinks. Follow the alias,
        // but require its actual directory to remain the originally opened one.
        let observed = std::fs::metadata(parent)
            .map_err(|_| PlatformError::StalePathMoveUndo(parent.display().to_string()))?;
        if !observed.is_dir()
            || held != (observed.dev(), observed.ino())
            || super::local_storage::local_path_mount_identity(parent, true)
                .map_err(|error| PlatformError::Io(error.to_string()))?
                != self.0.parent_mount
        {
            return Err(PlatformError::StalePathMoveUndo(
                parent.display().to_string(),
            ));
        }
        Ok(())
    }

    pub(super) fn withdraw_publication(
        &self,
        published: &Path,
        retained: &Path,
        exchange: bool,
    ) -> Result<(), PlatformError> {
        let name = published.file_name().ok_or_else(|| {
            PlatformError::InvalidPathMoveUndoReceipt("publication has no entry name".into())
        })?;
        let flags = if exchange {
            rustix::fs::RenameFlags::EXCHANGE
        } else {
            rustix::fs::RenameFlags::NOREPLACE
        };
        let (parent, stage) = self.open_directories()?;
        rustix::fs::renameat_with(&parent, name, &stage, CANDIDATE, flags).map_err(|error| {
            if error == rustix::io::Errno::EXIST {
                PlatformError::DestinationExists(retained.display().to_string())
            } else {
                PlatformError::Io(error.to_string())
            }
        })
    }
}

#[cfg(target_os = "linux")]
pub(super) const CANDIDATE: &str = "candidate";

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CopyPublication {
    Published {
        retained_transaction: PathBuf,
        #[cfg(target_os = "linux")]
        retained_directory: RetainedCopyTransaction,
        published_path: PathBuf,
        published_identity: PathMoveIdentity,
        displaced_identity: Option<PathMoveIdentity>,
    },
    Skipped {
        retained_transaction: Option<PathBuf>,
    },
}

pub(super) fn copy_entry_transactional(
    source: &Path,
    destination_dir: &Path,
    conflict: ConflictAction,
) -> Result<CopyPublication, PlatformError> {
    #[cfg(target_os = "linux")]
    {
        linux::copy(source, destination_dir, conflict)
    }
    #[cfg(not(target_os = "linux"))]
    {
        portable::copy(source, destination_dir, conflict)
    }
}

#[cfg(target_os = "linux")]
pub(super) fn create_retained_transaction(
    destination_dir: &Path,
) -> Result<PathBuf, PlatformError> {
    linux::create_retained_transaction(destination_dir)
}

#[cfg(target_os = "linux")]
pub(super) fn create_empty_file(
    destination_dir: &Path,
    name: &str,
) -> Result<CopyPublication, PlatformError> {
    linux::create_empty_file(destination_dir, name)
}

#[cfg(target_os = "linux")]
pub(super) fn create_empty_directory(
    destination_dir: &Path,
    name: &str,
) -> Result<CopyPublication, PlatformError> {
    linux::create_empty_directory(destination_dir, name)
}

#[cfg(target_os = "linux")]
pub(super) fn create_symlink(
    destination_dir: &Path,
    name: &OsStr,
    target: &Path,
) -> Result<CopyPublication, PlatformError> {
    linux::create_symlink(destination_dir, name, target)
}

#[cfg(any(target_os = "linux", test))]
fn random_transaction_name() -> Result<OsString, PlatformError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut name = String::from(".ottrin-txn-");
    for byte in random {
        use std::fmt::Write;
        write!(&mut name, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(name.into())
}

fn reject_self_copy(source: &Path, target: &Path) -> Result<(), PlatformError> {
    let Ok(target_metadata) = std::fs::symlink_metadata(target) else {
        return Ok(());
    };
    let source_metadata = std::fs::symlink_metadata(source)
        .map_err(|_| PlatformError::InvalidSource(source.display().to_string()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if source_metadata.dev() == target_metadata.dev()
            && source_metadata.ino() == target_metadata.ino()
        {
            return Err(PlatformError::InvalidDestination(
                target.display().to_string(),
            ));
        }
    }
    #[cfg(windows)]
    {
        let _ = (source_metadata, target_metadata);
        if same_file::is_same_file(source, target).unwrap_or(false) {
            return Err(PlatformError::InvalidDestination(
                target.display().to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn flush_tree_bottom_up(path: &Path) -> Result<(), PlatformError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| PlatformError::Io(error.to_string()))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_file() {
        return std::fs::File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(|error| PlatformError::Io(error.to_string()));
    }
    if metadata.is_dir() {
        for entry in
            std::fs::read_dir(path).map_err(|error| PlatformError::Io(error.to_string()))?
        {
            let entry = entry.map_err(|error| PlatformError::Io(error.to_string()))?;
            flush_tree_bottom_up(&entry.path())?;
        }
        #[cfg(target_os = "linux")]
        {
            return std::fs::File::open(path)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| PlatformError::Io(error.to_string()));
        }
        #[cfg(not(target_os = "linux"))]
        return Ok(());
    }
    Err(PlatformError::InvalidSource(path.display().to_string()))
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use rustix::fd::{AsRawFd, OwnedFd};
    use rustix::fs::{
        AtFlags, CWD, FileType, Mode, OFlags, RenameFlags, fstat, mkdirat, openat, renameat_with,
        statat,
    };
    use rustix::io::Errno;

    struct Transaction {
        parent: OwnedFd,
        stage: OwnedFd,
        name: OsString,
    }

    struct PinnedEntry(OwnedFd);

    impl PinnedEntry {
        fn open(parent: &OwnedFd, name: &OsStr) -> Result<Option<Self>, PlatformError> {
            match openat(
                parent,
                name,
                OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            ) {
                Ok(fd) => Ok(Some(Self(fd))),
                Err(Errno::NOENT) => Ok(None),
                Err(error) => Err(PlatformError::Io(error.to_string())),
            }
        }

        fn matches(&self, parent: &OwnedFd, name: &OsStr) -> Result<bool, PlatformError> {
            let expected = fstat(&self.0).map_err(|error| PlatformError::Io(error.to_string()))?;
            let actual = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            Ok(expected.st_dev == actual.st_dev && expected.st_ino == actual.st_ino)
        }
    }

    impl Transaction {
        fn create(
            destination_dir: &Path,
            hook: &mut impl FnMut(TestPoint) -> Result<(), &'static str>,
        ) -> Result<Self, PlatformError> {
            let canonical = std::fs::canonicalize(destination_dir)
                .map_err(|error| PlatformError::InvalidDestination(error.to_string()))?;
            let parent = openat(
                CWD,
                &canonical,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| PlatformError::InvalidDestination(error.to_string()))?;
            loop {
                let name = random_transaction_name()?;
                match mkdirat(&parent, &name, Mode::RUSR | Mode::WUSR | Mode::XUSR) {
                    Ok(()) => {
                        if let Err(message) = hook(TestPoint::AfterStageCreated) {
                            return Err(PlatformError::CleanupPending {
                                transaction: canonical.join(&name).display().to_string(),
                                message: message.into(),
                            });
                        }
                        let stage = match openat(
                            &parent,
                            &name,
                            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                            Mode::empty(),
                        ) {
                            Ok(stage) => stage,
                            Err(error) => {
                                return Err(PlatformError::CleanupPending {
                                    transaction: canonical.join(&name).display().to_string(),
                                    message: error.to_string(),
                                });
                            }
                        };
                        let validation = (|| {
                            let opened = fstat(&stage)
                                .map_err(|error| PlatformError::Io(error.to_string()))?;
                            let named = statat(&parent, &name, AtFlags::SYMLINK_NOFOLLOW)
                                .map_err(|error| PlatformError::Io(error.to_string()))?;
                            Ok::<_, PlatformError>(
                                FileType::from_raw_mode(opened.st_mode) == FileType::Directory
                                    && Mode::from_raw_mode(opened.st_mode).as_raw_mode() & 0o777
                                        == 0o700
                                    && opened.st_uid == rustix::process::geteuid().as_raw()
                                    && opened.st_dev == named.st_dev
                                    && opened.st_ino == named.st_ino,
                            )
                        })();
                        let valid = match validation {
                            Ok(valid) => valid,
                            Err(error) => {
                                return Err(PlatformError::CleanupPending {
                                    transaction: canonical.join(&name).display().to_string(),
                                    message: error.to_string(),
                                });
                            }
                        };
                        if !valid {
                            return Err(PlatformError::CleanupPending {
                                transaction: canonical.join(&name).display().to_string(),
                                message: "staging directory identity or permissions changed".into(),
                            });
                        }
                        return Ok(Self {
                            parent,
                            stage,
                            name,
                        });
                    }
                    Err(Errno::EXIST) => continue,
                    Err(error) => return Err(PlatformError::Io(error.to_string())),
                }
            }
        }

        fn parent_path(&self, name: &OsStr) -> PathBuf {
            PathBuf::from(format!("/proc/self/fd/{}", self.parent.as_raw_fd())).join(name)
        }

        fn candidate_path(&self) -> PathBuf {
            PathBuf::from(format!("/proc/self/fd/{}", self.stage.as_raw_fd())).join(CANDIDATE)
        }

        fn recovery_path(&self) -> PathBuf {
            std::fs::read_link(format!("/proc/self/fd/{}", self.parent.as_raw_fd()))
                .unwrap_or_else(|_| self.parent_path(OsStr::new("")))
                .join(&self.name)
        }

        fn abandon(self, original: PlatformError) -> PlatformError {
            PlatformError::CleanupPending {
                transaction: self.recovery_path().display().to_string(),
                message: original.to_string(),
            }
        }

        fn retain(self) -> PathBuf {
            self.recovery_path()
        }

        fn retain_for_undo(self) -> Result<(PathBuf, RetainedCopyTransaction), PlatformError> {
            use std::os::unix::fs::MetadataExt;
            let path = self.recovery_path();
            let result = (|| {
                let parent = std::fs::File::from(self.parent);
                let stage = std::fs::File::from(self.stage);
                let parent_info = parent.metadata()?;
                let stage_info = stage.metadata()?;
                let directories = RetainedCopyDirectories {
                    parent_mount: super::super::local_storage::local_path_mount_identity(
                        &PathBuf::from(format!("/proc/self/fd/{}", parent.as_raw_fd())),
                        true,
                    )?,
                    stage_mount: super::super::local_storage::local_path_mount_identity(
                        &PathBuf::from(format!("/proc/self/fd/{}", stage.as_raw_fd())),
                        true,
                    )?,
                    parent_identity: (parent_info.dev(), parent_info.ino()),
                    stage_identity: (stage_info.dev(), stage_info.ino()),
                    parent_path: path.parent().expect("transaction parent").to_path_buf(),
                    stage_path: path.clone(),
                    parent_watch: super::super::directory_watch::DirectoryWatch::new(&parent)?,
                    stage_watch: super::super::directory_watch::DirectoryWatch::new(&stage)?,
                };
                Ok::<_, std::io::Error>(RetainedCopyTransaction(std::sync::Arc::new(directories)))
            })();
            result.map(|guard| (path.clone(), guard)).map_err(|error| PlatformError::CommittedCleanupPending {
                transaction: path.display().to_string(),
                message: format!("publication completed but directory identity watches could not be established: {error}"),
            })
        }
    }

    #[cfg(test)]
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(super) enum TestPoint {
        AfterStageCreated,
        AfterStaged,
        BeforeExchange,
        BeforeDisplacedIdentityCheck,
        AfterExchange,
    }

    #[cfg(not(test))]
    #[derive(Clone, Copy)]
    enum TestPoint {
        AfterStageCreated,
        AfterStaged,
        BeforeExchange,
        BeforeDisplacedIdentityCheck,
        AfterExchange,
    }

    pub(super) fn copy(
        source: &Path,
        destination_dir: &Path,
        conflict: ConflictAction,
    ) -> Result<CopyPublication, PlatformError> {
        copy_impl(source, destination_dir, conflict, |_| Ok(()))
    }

    /// Creates an identity-checked private transaction in `destination_dir` for a caller-owned
    /// cleanup step. The caller must persist its cleanup intent before moving an entry into it.
    pub(super) fn create_retained_transaction(
        destination_dir: &Path,
    ) -> Result<PathBuf, PlatformError> {
        let mut hook = |_| Ok(());
        Transaction::create(destination_dir, &mut hook).map(Transaction::retain)
    }

    pub(super) fn create_empty_file(
        destination_dir: &Path,
        name: &str,
    ) -> Result<CopyPublication, PlatformError> {
        create_empty_entry(destination_dir, OsStr::new(name), "file", |transaction| {
            let candidate = openat(
                &transaction.stage,
                CANDIDATE,
                OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|error| PlatformError::Io(error.to_string()))?;
            rustix::fs::fsync(&candidate).map_err(|error| PlatformError::Io(error.to_string()))
        })
    }

    pub(super) fn create_empty_directory(
        destination_dir: &Path,
        name: &str,
    ) -> Result<CopyPublication, PlatformError> {
        create_empty_entry(destination_dir, OsStr::new(name), "folder", |transaction| {
            mkdirat(&transaction.stage, CANDIDATE, Mode::from_raw_mode(0o777))
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            let candidate = openat(
                &transaction.stage,
                CANDIDATE,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(|error| PlatformError::Io(error.to_string()))?;
            rustix::fs::fsync(&candidate).map_err(|error| PlatformError::Io(error.to_string()))
        })
    }

    pub(super) fn create_symlink(
        destination_dir: &Path,
        name: &OsStr,
        target: &Path,
    ) -> Result<CopyPublication, PlatformError> {
        create_empty_entry(destination_dir, name, "symlink", |transaction| {
            std::os::unix::fs::symlink(target, transaction.candidate_path())
                .map_err(|error| PlatformError::Io(error.to_string()))
        })
    }

    fn create_empty_entry(
        destination_dir: &Path,
        name: &OsStr,
        entry_kind: &str,
        stage_candidate: impl FnOnce(&Transaction) -> Result<(), PlatformError>,
    ) -> Result<CopyPublication, PlatformError> {
        let target = destination_dir.join(name);
        if std::fs::symlink_metadata(&target).is_ok() {
            return Err(PlatformError::DestinationExists(
                target.display().to_string(),
            ));
        }
        let mut hook = |_| Ok(());
        let transaction = Transaction::create(destination_dir, &mut hook)?;
        let stage_result = stage_candidate(&transaction).and_then(|()| {
            rustix::fs::fsync(&transaction.stage)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            rustix::fs::fsync(&transaction.parent)
                .map_err(|error| PlatformError::Io(error.to_string()))
        });
        if let Err(error) = stage_result {
            return Err(transaction.abandon(error));
        }
        let published_identity = match rename_identity(&transaction.candidate_path()) {
            Ok(identity) => identity,
            Err(error) => {
                return Err(transaction.abandon(PlatformError::Io(error.to_string())));
            }
        };
        if let Err(error) = renameat_with(
            &transaction.stage,
            CANDIDATE,
            &transaction.parent,
            name,
            RenameFlags::NOREPLACE,
        ) {
            let publication_error = if error == Errno::EXIST {
                PlatformError::DestinationExists(target.display().to_string())
            } else {
                PlatformError::Io(error.to_string())
            };
            return Err(transaction.abandon(publication_error));
        }
        rustix::fs::fsync(&transaction.parent).map_err(|error| {
            PlatformError::CommittedCleanupPending {
                transaction: transaction.recovery_path().display().to_string(),
                message: format!("created {entry_kind} parent flush failed: {error}"),
            }
        })?;
        let (retained_transaction, retained_directory) = transaction.retain_for_undo()?;
        Ok(CopyPublication::Published {
            retained_transaction,
            retained_directory,
            published_path: target,
            published_identity,
            displaced_identity: None,
        })
    }

    #[cfg(test)]
    pub(super) fn copy_with_test_hook(
        source: &Path,
        destination_dir: &Path,
        conflict: ConflictAction,
        hook: impl FnMut(TestPoint) -> Result<(), &'static str>,
    ) -> Result<CopyPublication, PlatformError> {
        copy_impl(source, destination_dir, conflict, hook)
    }

    fn copy_impl(
        source: &Path,
        destination_dir: &Path,
        conflict: ConflictAction,
        mut hook: impl FnMut(TestPoint) -> Result<(), &'static str>,
    ) -> Result<CopyPublication, PlatformError> {
        let name = source
            .file_name()
            .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
        let initial_target = destination_dir.join(name);
        reject_self_copy(source, &initial_target)?;
        if conflict == ConflictAction::Skip && std::fs::symlink_metadata(&initial_target).is_ok() {
            return Ok(CopyPublication::Skipped {
                retained_transaction: None,
            });
        }
        let transaction = Transaction::create(destination_dir, &mut hook)?;
        let base_target = transaction.parent_path(name);
        if let Err(error) = reject_self_copy(source, &base_target) {
            return Err(transaction.abandon(error));
        }
        let expected_target = if conflict == ConflictAction::Overwrite {
            match PinnedEntry::open(&transaction.parent, name) {
                Ok(expected) => expected,
                Err(error) => return Err(transaction.abandon(error)),
            }
        } else {
            None
        };
        let mut target_name = match conflict {
            ConflictAction::Rename if std::fs::symlink_metadata(&base_target).is_ok() => {
                next_available_name(&base_target)
                    .file_name()
                    .expect("rename candidate has a file name")
                    .to_os_string()
            }
            _ => name.to_os_string(),
        };

        let stage_result = (|| {
            copy_entry(source, &transaction.candidate_path())?;
            flush_tree_bottom_up(&transaction.candidate_path())?;
            std::fs::File::open(
                transaction
                    .candidate_path()
                    .parent()
                    .expect("candidate parent"),
            )
            .and_then(|directory| directory.sync_all())
            .map_err(|error| PlatformError::Io(error.to_string()))?;
            // Persist the staging entry before publication.
            rustix::fs::fsync(&transaction.parent)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            hook(TestPoint::AfterStaged).map_err(|message| PlatformError::Io(message.into()))
        })();
        if let Err(error) = stage_result {
            return Err(transaction.abandon(error));
        }

        let published_identity = match rename_identity(&transaction.candidate_path()) {
            Ok(identity) => identity,
            Err(error) => {
                return Err(transaction.abandon(PlatformError::Io(error.to_string())));
            }
        };
        let mut retained_displaced_identity = None;
        loop {
            match renameat_with(
                &transaction.stage,
                CANDIDATE,
                &transaction.parent,
                &target_name,
                RenameFlags::NOREPLACE,
            ) {
                Ok(()) => break,
                Err(Errno::EXIST) => match conflict {
                    ConflictAction::Skip => {
                        return Ok(CopyPublication::Skipped {
                            retained_transaction: Some(transaction.retain()),
                        });
                    }
                    ConflictAction::Rename => {
                        target_name = next_available_name(&base_target)
                            .file_name()
                            .expect("rename candidate has a file name")
                            .to_os_string();
                    }
                    ConflictAction::Overwrite => {
                        if let Err(message) = hook(TestPoint::BeforeExchange) {
                            return Err(transaction.abandon(PlatformError::Io(message.into())));
                        }
                        match renameat_with(
                            &transaction.stage,
                            CANDIDATE,
                            &transaction.parent,
                            &target_name,
                            RenameFlags::EXCHANGE,
                        ) {
                            Ok(()) => {
                                if let Err(message) = hook(TestPoint::BeforeDisplacedIdentityCheck)
                                {
                                    return Err(PlatformError::CommittedCleanupPending {
                                        transaction: transaction
                                            .recovery_path()
                                            .display()
                                            .to_string(),
                                        message: message.into(),
                                    });
                                }
                                let displaced_identity = expected_target.as_ref().map_or_else(
                                    || Ok(false),
                                    |expected| {
                                        expected.matches(&transaction.stage, OsStr::new(CANDIDATE))
                                    },
                                );
                                match displaced_identity {
                                    Ok(true) => {
                                        retained_displaced_identity = Some(
                                            rename_identity(&transaction.candidate_path()).map_err(
                                                |error| PlatformError::CommittedCleanupPending {
                                                    transaction: transaction
                                                        .recovery_path()
                                                        .display()
                                                        .to_string(),
                                                    message: format!(
                                                        "could not bind displaced destination identity; entry was preserved: {error}"
                                                    ),
                                                },
                                            )?,
                                        );
                                    }
                                    Ok(false) => {
                                        return Err(PlatformError::CommittedCleanupPending {
                                            transaction: transaction
                                                .recovery_path()
                                                .display()
                                                .to_string(),
                                            message: "destination changed concurrently; displaced entry was preserved"
                                                .into(),
                                        });
                                    }
                                    Err(error) => {
                                        return Err(PlatformError::CommittedCleanupPending {
                                            transaction: transaction
                                                .recovery_path()
                                                .display()
                                                .to_string(),
                                            message: format!(
                                                "could not verify displaced destination identity; entry was preserved: {error}"
                                            ),
                                        });
                                    }
                                }
                                if let Err(message) = hook(TestPoint::AfterExchange) {
                                    return Err(PlatformError::CommittedCleanupPending {
                                        transaction: transaction
                                            .recovery_path()
                                            .display()
                                            .to_string(),
                                        message: message.into(),
                                    });
                                }
                                break;
                            }
                            Err(Errno::NOENT) => continue,
                            Err(error) => {
                                return Err(
                                    transaction.abandon(PlatformError::Io(error.to_string()))
                                );
                            }
                        }
                    }
                },
                Err(error) => {
                    return Err(transaction.abandon(PlatformError::Io(error.to_string())));
                }
            }
        }

        rustix::fs::fsync(&transaction.parent).map_err(|error| {
            PlatformError::CommittedCleanupPending {
                transaction: transaction.recovery_path().display().to_string(),
                message: error.to_string(),
            }
        })?;
        let published_path = destination_dir.join(&target_name);
        let (retained_transaction, retained_directory) = transaction.retain_for_undo()?;
        Ok(CopyPublication::Published {
            retained_transaction,
            retained_directory,
            published_path,
            published_identity,
            displaced_identity: retained_displaced_identity,
        })
    }
}

#[cfg(not(target_os = "linux"))]
mod portable {
    use super::*;

    pub(super) fn copy(
        source: &Path,
        destination_dir: &Path,
        _conflict: ConflictAction,
    ) -> Result<CopyPublication, PlatformError> {
        std::fs::symlink_metadata(source)
            .map_err(|_| PlatformError::InvalidSource(source.display().to_string()))?;
        let name = source
            .file_name()
            .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
        let base_target = destination_dir.join(name);
        reject_self_copy(source, &base_target)?;
        Err(PlatformError::UnsupportedSafeCopy(
            source.display().to_string(),
        ))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn fixture(label: &str) -> PathBuf {
            let path = std::env::temp_dir().join(format!(
                "ottrin-portable-{label}-{}",
                random_transaction_name().unwrap().to_string_lossy()
            ));
            std::fs::create_dir(&path).unwrap();
            path
        }

        #[test]
        fn every_copy_mode_is_rejected_without_modification() {
            let root = fixture("overwrite");
            let source_dir = root.join("source");
            let destination = root.join("destination");
            std::fs::create_dir(&source_dir).unwrap();
            std::fs::create_dir(&destination).unwrap();
            let source = source_dir.join("item");
            std::fs::write(&source, "new").unwrap();
            std::fs::write(destination.join("item"), "old").unwrap();

            for conflict in [
                ConflictAction::Skip,
                ConflictAction::Rename,
                ConflictAction::Overwrite,
            ] {
                let result = copy(&source, &destination, conflict);
                assert!(matches!(result, Err(PlatformError::UnsupportedSafeCopy(_))));
                assert_eq!(std::fs::read_to_string(&source).unwrap(), "new");
                assert_eq!(
                    std::fs::read_to_string(destination.join("item")).unwrap(),
                    "old"
                );
            }
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::linux::{TestPoint, copy_with_test_hook};
    use super::*;
    use std::process::Command;

    struct TestDir(PathBuf);
    impl TestDir {
        fn new(label: &str) -> Self {
            loop {
                let suffix = random_transaction_name().unwrap();
                let path = std::env::temp_dir()
                    .join(format!("ottrin-copy-{label}-{}", suffix.to_string_lossy()));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create test directory: {error}"),
                }
            }
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn transactions(directory: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(directory)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(".ottrin-txn-"))
            })
            .collect()
    }

    fn retained_transaction(result: Result<CopyPublication, PlatformError>) -> PathBuf {
        match result {
            Ok(CopyPublication::Published {
                retained_transaction,
                ..
            }) => retained_transaction,
            other => panic!("expected published copy with retained transaction, got {other:?}"),
        }
    }

    #[test]
    fn deterministic_failure_never_publishes_partial_output() {
        let root = TestDir::new("failure");
        let source = root.0.join("item");
        let destination = root.0.join("destination");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(source.join("one"), "complete-one").unwrap();
        std::fs::write(source.join("two"), "complete-two").unwrap();
        let result = copy_with_test_hook(&source, &destination, ConflictAction::Skip, |point| {
            if point == TestPoint::AfterStaged {
                Err("injected")
            } else {
                Ok(())
            }
        });
        assert!(matches!(result, Err(PlatformError::CleanupPending { .. })));
        assert!(!destination.join("item").exists());
        assert_eq!(transactions(&destination).len(), 1);
    }

    #[test]
    fn prepublication_failure_preserves_existing_destination() {
        let root = TestDir::new("failure-existing");
        let source = root.0.join("item");
        let destination = root.0.join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&source, "new-complete").unwrap();
        std::fs::write(destination.join("item"), "old-complete").unwrap();

        let result =
            copy_with_test_hook(&source, &destination, ConflictAction::Overwrite, |point| {
                if point == TestPoint::AfterStaged {
                    Err("injected before publication")
                } else {
                    Ok(())
                }
            });

        assert!(matches!(result, Err(PlatformError::CleanupPending { .. })));
        assert_eq!(
            std::fs::read_to_string(destination.join("item")).unwrap(),
            "old-complete"
        );
        assert_eq!(transactions(&destination).len(), 1);
    }

    #[test]
    fn skip_and_rename_publication_races_do_not_clobber() {
        for conflict in [ConflictAction::Skip, ConflictAction::Rename] {
            let root = TestDir::new("race");
            let source = root.0.join("item");
            let destination = root.0.join("destination");
            std::fs::create_dir(&destination).unwrap();
            std::fs::write(&source, "candidate").unwrap();
            let raced = destination.join("item");
            copy_with_test_hook(&source, &destination, conflict, |point| {
                if point == TestPoint::AfterStaged {
                    std::fs::write(&raced, "racer").unwrap();
                }
                Ok(())
            })
            .unwrap();
            assert_eq!(std::fs::read_to_string(&raced).unwrap(), "racer");
            if conflict == ConflictAction::Rename {
                assert_eq!(
                    std::fs::read_to_string(destination.join("item (1)")).unwrap(),
                    "candidate"
                );
            }
        }
    }

    #[test]
    fn overwrite_is_typed_warning_and_keeps_displaced_entry() {
        let root = TestDir::new("retained-overwrite");
        let source = root.0.join("item");
        let destination = root.0.join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("new"), "new").unwrap();
        std::fs::create_dir(destination.join("item")).unwrap();
        std::fs::write(destination.join("item/old-one"), "old-one").unwrap();
        std::fs::write(destination.join("item/old-two"), "old-two").unwrap();
        let result =
            copy_with_test_hook(&source, &destination, ConflictAction::Overwrite, |_| Ok(()));
        let recovery = retained_transaction(result);
        assert_eq!(
            std::fs::read_to_string(destination.join("item/new")).unwrap(),
            "new"
        );
        assert!(recovery.is_dir());
        let remainder = std::fs::read_dir(recovery.join(CANDIDATE))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(remainder.len(), 2, "the displaced tree remains recoverable");
    }

    #[test]
    fn stage_open_failure_retains_the_just_created_stage() {
        let root = TestDir::new("stage-open-failure");
        let source = root.0.join("item");
        let destination = root.0.join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&source, "new").unwrap();

        let result = copy_with_test_hook(&source, &destination, ConflictAction::Skip, |point| {
            (point != TestPoint::AfterStageCreated)
                .then_some(())
                .ok_or("injected stage open failure")
        });

        assert!(matches!(result, Err(PlatformError::CleanupPending { .. })));
        assert_eq!(transactions(&destination).len(), 1);
        assert!(!destination.join("item").exists());
    }

    #[test]
    fn overwrite_exchange_never_exposes_a_missing_destination() {
        let root = TestDir::new("continuous-destination");
        let source = root.0.join("item");
        let destination = root.0.join("destination");
        let target = destination.join("item");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&source, "new-complete").unwrap();
        std::fs::write(&target, "old-complete").unwrap();

        let result =
            copy_with_test_hook(&source, &destination, ConflictAction::Overwrite, |point| {
                if matches!(point, TestPoint::BeforeExchange | TestPoint::AfterExchange) {
                    let value = std::fs::read_to_string(&target).unwrap();
                    assert!(value == "old-complete" || value == "new-complete");
                }
                Ok(())
            });
        let recovery = retained_transaction(result);
        assert_eq!(std::fs::read_to_string(target).unwrap(), "new-complete");
        assert_eq!(
            std::fs::read_to_string(recovery.join(CANDIDATE)).unwrap(),
            "old-complete"
        );
    }

    #[test]
    fn overwrite_destination_racer_is_preserved_after_exchange() {
        let root = TestDir::new("overwrite-racer");
        let source = root.0.join("item");
        let destination = root.0.join("destination");
        let target = destination.join("item");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&source, "new-complete").unwrap();
        std::fs::write(&target, "old-complete").unwrap();

        let result =
            copy_with_test_hook(&source, &destination, ConflictAction::Overwrite, |point| {
                if point == TestPoint::BeforeExchange {
                    std::fs::remove_file(&target).unwrap();
                    std::fs::write(&target, "racer").unwrap();
                }
                Ok(())
            });

        let recovery = match result {
            Err(PlatformError::CommittedCleanupPending { transaction, .. }) => {
                PathBuf::from(transaction)
            }
            other => panic!("expected committed warning, got {other:?}"),
        };
        assert_eq!(std::fs::read_to_string(target).unwrap(), "new-complete");
        assert_eq!(
            std::fs::read_to_string(recovery.join(CANDIDATE)).unwrap(),
            "racer"
        );
    }

    #[test]
    fn post_exchange_identity_check_failure_retains_displaced_entry() {
        let root = TestDir::new("identity-check-failure");
        let source = root.0.join("item");
        let destination = root.0.join("destination");
        let target = destination.join("item");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&source, "new-complete").unwrap();
        std::fs::write(&target, "old-complete").unwrap();

        let result =
            copy_with_test_hook(&source, &destination, ConflictAction::Overwrite, |point| {
                if point == TestPoint::BeforeDisplacedIdentityCheck {
                    Err("injected displaced identity check failure")
                } else {
                    Ok(())
                }
            });

        let recovery = match result {
            Err(PlatformError::CommittedCleanupPending { transaction, .. }) => {
                PathBuf::from(transaction)
            }
            other => panic!("expected committed warning, got {other:?}"),
        };
        assert_eq!(std::fs::read_to_string(target).unwrap(), "new-complete");
        assert_eq!(
            std::fs::read_to_string(recovery.join(CANDIDATE)).unwrap(),
            "old-complete"
        );
    }

    #[test]
    fn transaction_directory_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let root = TestDir::new("private-stage");
        let source = root.0.join("item");
        let destination = root.0.join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&source, "new").unwrap();
        copy_with_test_hook(&source, &destination, ConflictAction::Skip, |point| {
            if point == TestPoint::AfterStaged {
                let transaction = transactions(&destination);
                assert_eq!(transaction.len(), 1);
                let mode = std::fs::metadata(&transaction[0])
                    .unwrap()
                    .permissions()
                    .mode();
                assert_eq!(mode & 0o777, 0o700);
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn anchored_parent_path_rename_and_replacement_cannot_redirect_publish() {
        let root = TestDir::new("anchor");
        let source = root.0.join("item");
        let destination = root.0.join("destination");
        let moved = root.0.join("moved");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&source, "new").unwrap();
        copy_with_test_hook(&source, &destination, ConflictAction::Skip, |point| {
            if point == TestPoint::AfterStaged {
                std::fs::rename(&destination, &moved).unwrap();
                std::fs::create_dir(&destination).unwrap();
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read_to_string(moved.join("item")).unwrap(), "new");
        assert!(!destination.join("item").exists());
    }

    #[test]
    fn overwrite_supports_all_file_directory_symlink_kind_pairs() {
        use std::os::unix::fs::symlink;
        for source_kind in 0..3 {
            for destination_kind in 0..3 {
                let root = TestDir::new("kinds");
                let source = root.0.join("source/item");
                let destination = root.0.join("destination");
                std::fs::create_dir(source.parent().unwrap()).unwrap();
                std::fs::create_dir(&destination).unwrap();
                let target = destination.join("item");
                match source_kind {
                    0 => std::fs::write(&source, "new-file").unwrap(),
                    1 => {
                        std::fs::create_dir(&source).unwrap();
                        std::fs::write(source.join("child"), "new-dir").unwrap();
                    }
                    _ => symlink("missing-new", &source).unwrap(),
                }
                match destination_kind {
                    0 => std::fs::write(&target, "old-file").unwrap(),
                    1 => {
                        std::fs::create_dir(&target).unwrap();
                        std::fs::write(target.join("child"), "old-dir").unwrap();
                    }
                    _ => symlink("missing-old", &target).unwrap(),
                }
                let result = super::copy_entry_transactional(
                    &source,
                    &destination,
                    ConflictAction::Overwrite,
                );
                let recovery = retained_transaction(result);
                let metadata = std::fs::symlink_metadata(&target).unwrap();
                assert_eq!(metadata.is_file(), source_kind == 0);
                assert_eq!(metadata.is_dir(), source_kind == 1);
                assert_eq!(metadata.file_type().is_symlink(), source_kind == 2);
                let displaced = std::fs::symlink_metadata(recovery.join(CANDIDATE)).unwrap();
                assert_eq!(displaced.is_file(), destination_kind == 0);
                assert_eq!(displaced.is_dir(), destination_kind == 1);
                assert_eq!(displaced.file_type().is_symlink(), destination_kind == 2);
            }
        }
    }

    #[test]
    fn non_utf8_rename_candidate_is_lossless() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let root = TestDir::new("nonutf8");
        let source_dir = root.0.join("source");
        let destination = root.0.join("destination");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::create_dir(&destination).unwrap();
        let name = OsString::from_vec(b"item\x80.txt".to_vec());
        let source = source_dir.join(&name);
        std::fs::write(&source, "new").unwrap();
        std::fs::write(destination.join(&name), "old").unwrap();
        super::copy_entry_transactional(&source, &destination, ConflictAction::Rename).unwrap();
        assert_eq!(
            std::fs::read_to_string(destination.join(OsStr::from_bytes(b"item\x80 (1).txt")))
                .unwrap(),
            "new"
        );
    }

    #[test]
    fn sigkill_immediately_before_and_after_exchange_preserves_both_entries() {
        if let (Ok(root), Ok(point)) = (
            std::env::var("OTTRIN_SIGKILL_ROOT"),
            std::env::var("OTTRIN_SIGKILL_POINT"),
        ) {
            let root = PathBuf::from(root);
            let kill_point = if point == "before" {
                TestPoint::BeforeExchange
            } else {
                TestPoint::AfterExchange
            };
            let _ = copy_with_test_hook(
                &root.join("item"),
                &root.join("destination"),
                ConflictAction::Overwrite,
                |current| {
                    if current == kill_point {
                        let _ = Command::new("kill")
                            .args(["-KILL", &std::process::id().to_string()])
                            .status();
                    }
                    Ok(())
                },
            );
            panic!("SIGKILL hook returned");
        }
        for point in ["before", "after"] {
            let root = TestDir::new("sigkill");
            let destination = root.0.join("destination");
            std::fs::create_dir(&destination).unwrap();
            std::fs::write(root.0.join("item"), "new-complete").unwrap();
            std::fs::write(destination.join("item"), "old-complete").unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "copy_transaction::tests::sigkill_immediately_before_and_after_exchange_preserves_both_entries", "--nocapture"])
                .env("OTTRIN_SIGKILL_ROOT", &root.0).env("OTTRIN_SIGKILL_POINT", point).status().unwrap();
            assert!(!status.success());
            let final_value = std::fs::read_to_string(destination.join("item")).unwrap();
            let staged_value =
                std::fs::read_to_string(transactions(&destination)[0].join(CANDIDATE)).unwrap();
            let expected = if point == "before" {
                ("old-complete", "new-complete")
            } else {
                ("new-complete", "old-complete")
            };
            assert_eq!((final_value.as_str(), staged_value.as_str()), expected);
        }
    }
}
