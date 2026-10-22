//! Reclaim only complete, native-bound, unstarted preparations after their writer
//! disappears. Names select candidates; they never grant deletion authority.
use super::*;

pub(super) fn collect(
    parent: &File,
    root: &AnchoredRoot,
    central: &File,
) -> Result<(), PlatformError> {
    root.validate()?;
    let central_identity = stable_identity_from_handle(central).map_err(io)?;
    // Bound admission work even if a damaged/private namespace is flooded.
    for name in windows_fs::directory_children_limited(root.root(), 4096).map_err(io)? {
        let Some(transaction) = name.to_str().and_then(|s| s.strip_prefix(".preparing-")) else {
            continue;
        };
        if transaction.len() != 32
            || !transaction
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            continue;
        }
        let file = match open_file(
            root.root(),
            &name,
            FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE,
        ) {
            Ok(file) => file,
            // Active writers, directories, inaccessible and disappearing entries
            // are not cleanup capabilities. Leave them to their owner.
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                ) || error.raw_os_error()
                    == Some(windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION as i32) =>
            {
                continue;
            }
            Err(error) => return Err(io(error)),
        };
        let size = file.metadata().map_err(io)?.len();
        if size == 0 || size > (FRAME_HEADER + MAX_FRAME) as u64 {
            continue;
        }
        let bytes = read(&file)?;
        let Ok(log) = Log::decode(&bytes) else {
            // Partial or foreign bytes establish no authority, even in a private
            // directory and even when their names look like our preparations.
            continue;
        };
        let generation = log.owner.as_ref().unwrap();
        let claim = &generation.binding.claim;
        if log.sequence != 1
            || generation.released
            || claim.transaction != transaction
            || claim.central != central_identity
            || claim.parent != root.parent_identity()
            || claim.root != stable_identity_from_handle(root.root()).map_err(io)?
            || claim.anchor != root.anchor_identity()
            || claim.file != stable_identity_from_handle(&file).map_err(io)?
            || claim.creator != windows_fs::current_owner()?
        {
            continue;
        }
        let Some(bucket) = AnchoredRoot::claim_bucket(
            root.root(),
            ledger_key(&OsString::from_wide(&claim.target)).0,
            false,
        )?
        else {
            continue;
        };
        if claim.bucket != stable_identity_from_handle(bucket.root()).map_err(io)?
            || claim.bucket_anchor != bucket.anchor_identity()
            || bucket.parent_identity() != claim.root
            || !records_absent(parent, central, claim)?
        {
            continue;
        }
        root.validate()?;
        bucket.validate()?;
        namespace::validate_binding(root.root(), &name, claim.file)?;
        // Complete matching records still do not authorize losing added streams,
        // hard links or altered security. Surface that conflict before deletion.
        cleanup_finalize::metadata(
            &file,
            claim.file,
            &claim.security,
            &claim.creator,
            Some(&bytes),
            &mut |_| Ok(()),
        )?;
        cleanup_tree::dispose(&file)?;
        drop(file);
        root.writable_root()?.sync_all().map_err(io)?;
    }
    root.validate()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn directory(path: &Path) -> File {
        windows_fs::open_directory(
            path,
            windows_sys::Win32::System::SystemServices::MAXIMUM_ALLOWED,
        )
        .unwrap()
    }
    struct Fixture {
        _temp: tempfile::TempDir,
        parent: File,
        central: File,
        path: PathBuf,
        preparation: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().to_path_buf();
            std::fs::create_dir(path.join("destination")).unwrap();
            std::fs::create_dir(path.join("central")).unwrap();
            std::fs::write(path.join("source"), b"original source").unwrap();
            std::fs::write(path.join("destination/public"), b"unrelated public data").unwrap();
            Self {
                parent: directory(&path.join("destination")),
                central: directory(&path.join("central")),
                preparation: path
                    .join("destination/.ottrin-copy-target-claims")
                    .join(format!(".preparing-{}", "d".repeat(32))),
                _temp: temp,
                path,
            }
        }
        fn prepare(&self) -> File {
            let root = AnchoredRoot::claims(&self.parent, true).unwrap().unwrap();
            let bucket = AnchoredRoot::claim_bucket(root.root(), true, true)
                .unwrap()
                .unwrap();
            let name = self.preparation.file_name().unwrap();
            let mut file =
                windows_fs::create_private(root.root(), name, false, FILE_SHARE_READ).unwrap();
            let claim = Claim::capture(
                &self.parent,
                &self.central,
                &root,
                &bucket,
                &file,
                OsStr::new("copy"),
                &"d".repeat(32),
            )
            .unwrap();
            Log::empty()
                .append(
                    &mut file,
                    Entry::Claim {
                        claim: Box::new(claim),
                    },
                )
                .unwrap();
            file.sync_all().unwrap();
            root.writable_root().unwrap().sync_all().unwrap();
            file
        }
        fn admit(&self, id: char) -> Result<Lease, PlatformError> {
            Lease::acquire(
                &self.parent,
                &self.central,
                OsStr::new("copy"),
                &id.to_string().repeat(32),
            )
        }
        fn preserved(&self) {
            assert_eq!(
                std::fs::read(self.path.join("source")).unwrap(),
                b"original source"
            );
            assert_eq!(
                std::fs::read(self.path.join("destination/public")).unwrap(),
                b"unrelated public data"
            );
        }
    }

    #[test]
    fn windows_native_copy_preparation_cleanup_waits_for_writer_and_reclaims_complete_orphan() {
        let f = Fixture::new();
        let writer = f.prepare();
        let next = f.admit('1').unwrap();
        assert!(
            f.preparation.exists(),
            "Live preparation must remain owned by its writer"
        );
        next.release_after_terminal().unwrap();
        drop(writer);
        let next = f.admit('2').unwrap();
        assert!(!f.preparation.exists());
        next.validate(false).unwrap();
        next.release_after_terminal().unwrap();
        f.preserved();
    }

    #[test]
    fn windows_native_copy_preparation_cleanup_preserves_foreign_partial_replaced_and_misnamed_files()
     {
        let f = Fixture::new();
        drop(f.prepare());
        let bytes = std::fs::read(&f.preparation).unwrap();
        // Same complete record on another native object conveys no authority.
        let copied = f
            .preparation
            .with_file_name(format!(".preparing-{}", "a".repeat(32)));
        std::fs::write(&copied, &bytes).unwrap();
        let partial = f
            .preparation
            .with_file_name(format!(".preparing-{}", "b".repeat(32)));
        std::fs::write(&partial, b"OTC").unwrap();
        let foreign = f
            .preparation
            .with_file_name(format!(".preparing-{}", "f".repeat(32)));
        std::fs::write(&foreign, b"foreign data").unwrap();
        let misplaced = f
            .preparation
            .with_file_name(format!(".preparing-{}", "c".repeat(32)));
        std::fs::rename(&f.preparation, &misplaced).unwrap();
        // Replacement retains the transaction filename but has a new file ID.
        std::fs::write(&f.preparation, &bytes).unwrap();
        let next = f.admit('1').unwrap();
        for path in [&f.preparation, &misplaced, &copied] {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        assert_eq!(std::fs::read(&partial).unwrap(), b"OTC");
        assert_eq!(std::fs::read(&foreign).unwrap(), b"foreign data");
        next.release_after_terminal().unwrap();
        f.preserved();
    }

    #[test]
    fn windows_native_copy_preparation_cleanup_preserves_retained_stage_and_registry_records() {
        for local in [true, false] {
            let f = Fixture::new();
            drop(f.prepare());
            let before = std::fs::read(&f.preparation).unwrap();
            let record = if local {
                f.path
                    .join(format!("destination/.ottrin-copy-{}", "d".repeat(32)))
            } else {
                f.path
                    .join(format!("central/{}.copy-journal", "d".repeat(32)))
            };
            std::fs::write(&record, b"retained recovery data").unwrap();
            f.admit('1').unwrap().release_after_terminal().unwrap();
            assert_eq!(std::fs::read(&f.preparation).unwrap(), before);
            assert_eq!(std::fs::read(&record).unwrap(), b"retained recovery data");
            std::fs::remove_file(&record).unwrap();
            f.admit('2').unwrap().release_after_terminal().unwrap();
            assert!(!f.preparation.exists());
            f.preserved();
        }
    }

    #[test]
    fn windows_native_copy_preparation_cleanup_refuses_added_streams_and_hard_links() {
        for hard_link in [true, false] {
            let f = Fixture::new();
            drop(f.prepare());
            let before = std::fs::read(&f.preparation).unwrap();
            let extra = if hard_link {
                let extra = f.path.join("external-alias");
                std::fs::hard_link(&f.preparation, &extra).unwrap();
                extra
            } else {
                let extra = f.preparation.with_file_name(format!(
                    "{}:extra",
                    f.preparation.file_name().unwrap().to_string_lossy()
                ));
                std::fs::write(&extra, b"added data").unwrap();
                extra
            };
            assert!(f.admit('1').is_err());
            assert_eq!(std::fs::read(&f.preparation).unwrap(), before);
            assert_eq!(
                std::fs::read(&extra).unwrap(),
                if hard_link {
                    before.as_slice()
                } else {
                    b"added data"
                }
            );
            std::fs::remove_file(extra).unwrap();
            f.admit('2').unwrap().release_after_terminal().unwrap();
            assert!(!f.preparation.exists());
            f.preserved();
        }
    }
}
