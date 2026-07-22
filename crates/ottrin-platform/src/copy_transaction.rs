use super::{ConflictAction, PlatformError, copy_entry, next_available_name};
#[cfg(target_os = "linux")]
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;
#[cfg(any(target_os = "linux", test))]
use std::path::PathBuf;

const CANDIDATE: &str = "candidate";

pub(super) fn copy_entry_transactional(
    source: &Path,
    destination_dir: &Path,
    conflict: ConflictAction,
) -> Result<(), PlatformError> {
    #[cfg(target_os = "linux")]
    {
        linux::copy(source, destination_dir, conflict)
    }
    #[cfg(not(target_os = "linux"))]
    {
        portable::copy(source, destination_dir, conflict)
    }
}

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
    use rustix::fd::{AsFd, AsRawFd, OwnedFd};
    use rustix::fs::{
        AtFlags, CWD, FileType, Mode, OFlags, RenameFlags, fstat, mkdirat, openat, renameat_with,
        statat, unlinkat,
    };
    use rustix::io::Errno;

    struct Transaction {
        parent: OwnedFd,
        stage: OwnedFd,
        name: OsString,
        committed: bool,
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
                            let cleanup = unlinkat(&parent, &name, AtFlags::REMOVEDIR);
                            return Err(PlatformError::Io(match cleanup {
                                Ok(()) => message.into(),
                                Err(error) => format!(
                                    "{message}; failed to remove unopened stage {}: {error}",
                                    canonical.join(&name).display()
                                ),
                            }));
                        }
                        let stage = match openat(
                            &parent,
                            &name,
                            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                            Mode::empty(),
                        ) {
                            Ok(stage) => stage,
                            Err(error) => {
                                let cleanup = unlinkat(&parent, &name, AtFlags::REMOVEDIR);
                                return Err(PlatformError::Io(match cleanup {
                                    Ok(()) => error.to_string(),
                                    Err(cleanup_error) => format!(
                                        "{error}; failed to remove unopened stage {}: {cleanup_error}",
                                        canonical.join(&name).display()
                                    ),
                                }));
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
                                drop(stage);
                                let cleanup = unlinkat(&parent, &name, AtFlags::REMOVEDIR);
                                return Err(match cleanup {
                                    Ok(()) => error,
                                    Err(cleanup_error) => PlatformError::CleanupPending {
                                        transaction: canonical.join(&name).display().to_string(),
                                        message: format!("{error}; {cleanup_error}"),
                                    },
                                });
                            }
                        };
                        if !valid {
                            drop(stage);
                            let cleanup = unlinkat(&parent, &name, AtFlags::REMOVEDIR);
                            return Err(PlatformError::Io(match cleanup {
                                Ok(()) => {
                                    "staging directory identity or permissions changed".into()
                                }
                                Err(error) => format!(
                                    "staging directory validation failed and cleanup is pending at {}: {error}",
                                    canonical.join(&name).display()
                                ),
                            }));
                        }
                        return Ok(Self {
                            parent,
                            stage,
                            name,
                            committed: false,
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

        fn remove_stage(&self) -> Result<(), PlatformError> {
            unlinkat(&self.parent, &self.name, AtFlags::REMOVEDIR)
                .map_err(|error| PlatformError::Io(error.to_string()))
        }

        fn abort_cleanup(&mut self) -> Result<(), PlatformError> {
            let candidate = self.candidate_path();
            let result = if std::fs::symlink_metadata(&candidate).is_ok() {
                super::super::delete_permanent(&candidate)
            } else {
                Ok(())
            }
            .and_then(|()| self.remove_stage())
            .and_then(|()| {
                rustix::fs::fsync(&self.parent)
                    .map_err(|error| PlatformError::Io(error.to_string()))
            });
            // An explicit result path now owns any cleanup error/recovery path.
            // Do not let Drop silently retry and potentially erase that evidence.
            self.committed = true;
            result
        }

        fn abort(mut self, original: PlatformError) -> PlatformError {
            let recovery = self.recovery_path();
            match self.abort_cleanup() {
                Ok(()) => original,
                Err(cleanup) => PlatformError::CleanupPending {
                    transaction: recovery.display().to_string(),
                    message: format!("{original}; {cleanup}"),
                },
            }
        }

        fn cancel(mut self) -> Result<(), PlatformError> {
            let recovery = self.recovery_path();
            self.abort_cleanup()
                .map_err(|error| PlatformError::CleanupPending {
                    transaction: recovery.display().to_string(),
                    message: error.to_string(),
                })
        }

        fn cleanup(
            mut self,
            displaced: bool,
            hook: &mut impl FnMut(TestPoint) -> Result<(), &'static str>,
        ) -> Result<(), PlatformError> {
            if displaced {
                remove_displaced(&self.candidate_path(), hook)?;
            }
            self.remove_stage()?;
            hook(TestPoint::BeforeFinalParentSync)
                .map_err(|message| PlatformError::Io(message.into()))?;
            rustix::fs::fsync(&self.parent)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            self.committed = false;
            Ok(())
        }
    }

    fn remove_displaced(
        path: &Path,
        hook: &mut impl FnMut(TestPoint) -> Result<(), &'static str>,
    ) -> Result<(), PlatformError> {
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            for entry in
                std::fs::read_dir(path).map_err(|error| PlatformError::Io(error.to_string()))?
            {
                let entry = entry.map_err(|error| PlatformError::Io(error.to_string()))?;
                remove_displaced(&entry.path(), hook)?;
            }
            hook(TestPoint::DuringCleanup).map_err(|message| PlatformError::Io(message.into()))?;
            std::fs::remove_dir(path).map_err(|error| PlatformError::Io(error.to_string()))
        } else {
            hook(TestPoint::DuringCleanup).map_err(|message| PlatformError::Io(message.into()))?;
            std::fs::remove_file(path).map_err(|error| PlatformError::Io(error.to_string()))
        }
    }

    impl Drop for Transaction {
        fn drop(&mut self) {
            if !self.committed {
                let candidate = self.candidate_path();
                if std::fs::symlink_metadata(&candidate).is_ok() {
                    let _ = super::super::delete_permanent(&candidate);
                }
                let _ = self.remove_stage();
            }
        }
    }

    #[cfg(test)]
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(super) enum TestPoint {
        AfterStageCreated,
        AfterStaged,
        BeforeExchange,
        AfterExchange,
        BeforeCleanup,
        DuringCleanup,
        BeforeFinalParentSync,
    }

    #[cfg(not(test))]
    #[derive(Clone, Copy)]
    enum TestPoint {
        AfterStageCreated,
        AfterStaged,
        BeforeExchange,
        AfterExchange,
        BeforeCleanup,
        DuringCleanup,
        BeforeFinalParentSync,
    }

    pub(super) fn copy(
        source: &Path,
        destination_dir: &Path,
        conflict: ConflictAction,
    ) -> Result<(), PlatformError> {
        copy_impl(source, destination_dir, conflict, |_| Ok(()))
    }

    #[cfg(test)]
    pub(super) fn copy_with_test_hook(
        source: &Path,
        destination_dir: &Path,
        conflict: ConflictAction,
        hook: impl FnMut(TestPoint) -> Result<(), &'static str>,
    ) -> Result<(), PlatformError> {
        copy_impl(source, destination_dir, conflict, hook)
    }

    fn copy_impl(
        source: &Path,
        destination_dir: &Path,
        conflict: ConflictAction,
        mut hook: impl FnMut(TestPoint) -> Result<(), &'static str>,
    ) -> Result<(), PlatformError> {
        let transaction = Transaction::create(destination_dir, &mut hook)?;
        let name = match source.file_name() {
            Some(name) => name,
            None => {
                return Err(
                    transaction.abort(PlatformError::InvalidSource(source.display().to_string()))
                );
            }
        };
        let base_target = transaction.parent_path(name);
        if let Err(error) = reject_self_copy(source, &base_target) {
            return Err(transaction.abort(error));
        }

        if conflict == ConflictAction::Skip && std::fs::symlink_metadata(&base_target).is_ok() {
            return transaction.cancel();
        }
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
            return Err(transaction.abort(error));
        }

        let mut displaced = false;
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
                    ConflictAction::Skip => return transaction.cancel(),
                    ConflictAction::Rename => {
                        target_name = next_available_name(&transaction.parent_path(&target_name))
                            .file_name()
                            .expect("rename candidate has a file name")
                            .to_os_string();
                    }
                    ConflictAction::Overwrite => {
                        if let Err(message) = hook(TestPoint::BeforeExchange) {
                            return Err(transaction.abort(PlatformError::Io(message.into())));
                        }
                        match renameat_with(
                            &transaction.stage,
                            CANDIDATE,
                            &transaction.parent,
                            &target_name,
                            RenameFlags::EXCHANGE,
                        ) {
                            Ok(()) => {
                                displaced = true;
                                if let Err(message) = hook(TestPoint::AfterExchange) {
                                    let mut transaction = transaction;
                                    transaction.committed = true;
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
                                return Err(transaction.abort(PlatformError::Io(error.to_string())));
                            }
                        }
                    }
                },
                Err(error) => {
                    return Err(transaction.abort(PlatformError::Io(error.to_string())));
                }
            }
        }

        transaction.parent.as_fd();
        let mut transaction = transaction;
        transaction.committed = true;
        rustix::fs::fsync(&transaction.parent).map_err(|error| {
            PlatformError::CommittedCleanupPending {
                transaction: transaction.recovery_path().display().to_string(),
                message: error.to_string(),
            }
        })?;
        let transaction_name = transaction.recovery_path().display().to_string();
        if let Err(message) = hook(TestPoint::BeforeCleanup) {
            return Err(PlatformError::CommittedCleanupPending {
                transaction: transaction_name,
                message: message.into(),
            });
        }
        transaction.cleanup(displaced, &mut hook).map_err(|error| {
            PlatformError::CommittedCleanupPending {
                transaction: transaction_name,
                message: error.to_string(),
            }
        })
    }
}

#[cfg(not(target_os = "linux"))]
mod portable {
    use super::*;

    /// Conservative fallback used outside Linux.
    ///
    /// The staging directory is path-based and inherits platform ACL defaults; it is
    /// deliberately not described as private or handle-anchored. Native Windows ACL
    /// and handle anchoring are deferred. Existing-target overwrite is rejected, and
    /// publication of an initially absent target uses a no-replace rename so a racer
    /// cannot be clobbered or deleted.
    pub(super) fn copy(
        source: &Path,
        destination_dir: &Path,
        conflict: ConflictAction,
    ) -> Result<(), PlatformError> {
        let name = source
            .file_name()
            .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
        let base_target = destination_dir.join(name);
        reject_self_copy(source, &base_target)?;
        let base_exists = std::fs::symlink_metadata(&base_target).is_ok();
        if base_exists && conflict == ConflictAction::Skip {
            return Ok(());
        }
        if base_exists && conflict == ConflictAction::Overwrite {
            return Err(PlatformError::UnsupportedSafeOverwrite(
                base_target.display().to_string(),
            ));
        }
        let target = if base_exists && conflict == ConflictAction::Rename {
            next_available_name(&base_target)
        } else {
            base_target
        };

        let stage = loop {
            let candidate = destination_dir.join(random_transaction_name()?);
            match std::fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(PlatformError::Io(error.to_string())),
            }
        };
        let candidate = stage.join(CANDIDATE);
        let result = (|| {
            copy_entry(source, &candidate)?;
            flush_tree_bottom_up(&candidate)?;
            super::super::rename_without_replacement(&candidate, &target)
        })();
        let cleanup = std::fs::remove_dir_all(&stage);
        match (result, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Ok(()), Err(error)) => Err(PlatformError::CommittedCleanupPending {
                transaction: stage.display().to_string(),
                message: error.to_string(),
            }),
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(cleanup)) => Err(PlatformError::CleanupPending {
                transaction: stage.display().to_string(),
                message: format!("{error}; {cleanup}"),
            }),
        }
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
        fn existing_target_overwrite_is_rejected_without_modification() {
            let root = fixture("overwrite");
            let source_dir = root.join("source");
            let destination = root.join("destination");
            std::fs::create_dir(&source_dir).unwrap();
            std::fs::create_dir(&destination).unwrap();
            let source = source_dir.join("item");
            std::fs::write(&source, "new").unwrap();
            std::fs::write(destination.join("item"), "old").unwrap();

            let result = copy(&source, &destination, ConflictAction::Overwrite);

            assert!(matches!(
                result,
                Err(PlatformError::UnsupportedSafeOverwrite(_))
            ));
            assert_eq!(
                std::fs::read_to_string(destination.join("item")).unwrap(),
                "old"
            );
            std::fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn absent_target_publication_does_not_clobber_a_racer() {
            let root = fixture("no-clobber");
            let candidate = root.join("candidate");
            let target = root.join("target");
            std::fs::write(&candidate, "candidate").unwrap();
            std::fs::write(&target, "racer").unwrap();

            let result = super::super::super::rename_without_replacement(&candidate, &target);

            assert!(result.is_err());
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "racer");
            assert_eq!(std::fs::read_to_string(&candidate).unwrap(), "candidate");
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
        assert!(matches!(result, Err(PlatformError::Io(_))));
        assert!(!destination.join("item").exists());
        assert!(transactions(&destination).is_empty());
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

        assert!(matches!(result, Err(PlatformError::Io(_))));
        assert_eq!(
            std::fs::read_to_string(destination.join("item")).unwrap(),
            "old-complete"
        );
        assert!(transactions(&destination).is_empty());
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
    fn cleanup_failure_is_typed_and_keeps_displaced_entry() {
        let root = TestDir::new("cleanup");
        let source = root.0.join("item");
        let destination = root.0.join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("new"), "new").unwrap();
        std::fs::create_dir(destination.join("item")).unwrap();
        std::fs::write(destination.join("item/old-one"), "old-one").unwrap();
        std::fs::write(destination.join("item/old-two"), "old-two").unwrap();
        let mut cleanup_steps = 0;
        let result =
            copy_with_test_hook(&source, &destination, ConflictAction::Overwrite, |point| {
                if point == TestPoint::DuringCleanup {
                    cleanup_steps += 1;
                    if cleanup_steps == 2 {
                        return Err("permission denied while removing displaced data");
                    }
                    Ok(())
                } else {
                    Ok(())
                }
            });
        let recovery = match result {
            Err(PlatformError::CommittedCleanupPending { transaction, .. }) => {
                PathBuf::from(transaction)
            }
            other => panic!("expected committed cleanup warning, got {other:?}"),
        };
        assert_eq!(
            std::fs::read_to_string(destination.join("item/new")).unwrap(),
            "new"
        );
        assert!(recovery.is_dir());
        let remainder = std::fs::read_dir(recovery.join(CANDIDATE))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            remainder.len(),
            1,
            "one displaced child remains recoverable"
        );
    }

    #[test]
    fn stage_open_failure_removes_the_just_created_stage() {
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

        assert!(matches!(result, Err(PlatformError::Io(_))));
        assert!(transactions(&destination).is_empty());
        assert!(!destination.join("item").exists());
    }

    #[test]
    fn cleanup_success_syncs_parent_after_removing_transaction() {
        let root = TestDir::new("cleanup-parent-sync");
        let source = root.0.join("item");
        let destination = root.0.join("destination");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("new"), "new").unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::create_dir(destination.join("item")).unwrap();
        std::fs::write(destination.join("item/old"), "old").unwrap();
        let mut reached_final_sync = false;

        copy_with_test_hook(&source, &destination, ConflictAction::Overwrite, |point| {
            if point == TestPoint::BeforeFinalParentSync {
                assert!(transactions(&destination).is_empty());
                reached_final_sync = true;
            }
            Ok(())
        })
        .unwrap();

        assert!(reached_final_sync);
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

        copy_with_test_hook(&source, &destination, ConflictAction::Overwrite, |point| {
            if matches!(point, TestPoint::BeforeExchange | TestPoint::AfterExchange) {
                let value = std::fs::read_to_string(&target).unwrap();
                assert!(value == "old-complete" || value == "new-complete");
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read_to_string(target).unwrap(), "new-complete");
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
                result.unwrap();
                assert!(transactions(&destination).is_empty());
                let metadata = std::fs::symlink_metadata(&target).unwrap();
                assert_eq!(metadata.is_file(), source_kind == 0);
                assert_eq!(metadata.is_dir(), source_kind == 1);
                assert_eq!(metadata.file_type().is_symlink(), source_kind == 2);
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
