//! Path/conflict admission for the native Copy transaction engine.
use super::{io, journal, namespace, security, stable_identity_from_handle, version};
use crate::{ConflictAction, PathMoveUndoReceipt, PlatformError, windows_fs};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_GENERIC_READ, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    SYNCHRONIZE,
};
use windows_sys::Win32::System::SystemServices::MAXIMUM_ALLOWED;

const MAX_RENAME_CANDIDATES: usize = 10_000;

struct Source {
    parent: File,
    file: File,
    name: OsString,
    version: super::EntryVersion,
    parent_location: PathBuf,
}
impl Source {
    fn open(path: &Path) -> Result<Self, PlatformError> {
        let name = path
            .file_name()
            .ok_or_else(|| PlatformError::InvalidSource(path.display().to_string()))?;
        windows_fs::valid_component(name).map_err(io)?;
        let path = std::path::absolute(path).map_err(io)?;
        let parent = windows_fs::open_directory(
            path.parent()
                .ok_or_else(|| io("Copy source has no parent"))?,
            FILE_GENERIC_READ,
        )
        .map_err(io)?;
        let file = namespace::open_source(&parent, name, None)?;
        let version = version(&file)?;
        let parent_location = windows_fs::location(&parent).map_err(io)?;
        Ok(Self {
            parent,
            file,
            name: name.to_os_string(),
            version,
            parent_location,
        })
    }
    fn validate(&self) -> Result<(), PlatformError> {
        if windows_fs::location(&self.parent).map_err(io)? != self.parent_location {
            return Err(io("Copy source parent moved"));
        }
        namespace::validate_binding(&self.parent, &self.name, self.version.identity)
    }
    fn reject_overlap(
        &self,
        path: &Path,
        allow_missing: bool,
        progress: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if !self.version.identity.is_directory_without_reparse() {
            return Ok(());
        }
        for ancestor in path.ancestors() {
            progress(0)?;
            let directory =
                match windows_fs::open_directory(ancestor, FILE_READ_ATTRIBUTES | SYNCHRONIZE) {
                    Ok(file) => file,
                    Err(error) if allow_missing && error.kind() == std::io::ErrorKind::NotFound => {
                        continue;
                    }
                    Err(error) => return Err(io(error)),
                };
            if stable_identity_from_handle(&directory).map_err(io)? == self.version.identity {
                return Err(io(
                    "Copy destination and registry must be outside the source tree",
                ));
            }
        }
        self.validate()
    }
}

pub(super) fn copy_configured(
    source: &Path,
    destination: &Path,
    conflict: ConflictAction,
    max_entries: usize,
    mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
    progress(0)?;
    let source = Source::open(source)?;
    let registry_path = super::registry::configured_path()?;
    // Provisioning is itself a namespace mutation. Refuse source overlap before
    // creating even the first missing profile/state directory or marker.
    source.reject_overlap(&registry_path, true, &mut progress)?;
    let registry = super::registry::AnchoredRoot::open_at(&registry_path, true)?
        .ok_or_else(|| io("Copy registry initialization returned no registry"))?;
    copy_admitted_source(
        source,
        destination,
        conflict,
        registry.root(),
        max_entries,
        |bytes| {
            progress(bytes)?;
            registry.validate()
        },
    )
}

/// Absence is a native NotFound result, never an arbitrary failed metadata read.
fn entry(
    parent: &File,
    name: &OsStr,
) -> Result<Option<super::WindowsStableIdentity>, PlatformError> {
    match unsafe {
        windows_fs::open_relative(
            parent,
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
        Ok(file) => stable_identity_from_handle(&file).map(Some).map_err(io),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io(error)),
    }
}

/// Execute against a caller-owned private registry. Public workflow routing stays
/// gated until its complete capability and package acceptance requirements pass.
pub(super) fn copy_at(
    source: &Path,
    destination: &Path,
    conflict: ConflictAction,
    central: &File,
    max_entries: usize,
    mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
    progress(0)?;
    copy_admitted_source(
        Source::open(source)?,
        destination,
        conflict,
        central,
        max_entries,
        progress,
    )
}

fn copy_admitted_source(
    source: Source,
    destination: &Path,
    conflict: ConflictAction,
    central: &File,
    max_entries: usize,
    mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
    let name = source.name.as_os_str();
    let parent = windows_fs::open_directory(destination, MAXIMUM_ALLOWED).map_err(io)?;
    let parent_location = windows_fs::location(&parent).map_err(io)?;
    let central_location = windows_fs::location(central).map_err(io)?;
    if !stable_identity_from_handle(central)
        .map_err(io)?
        .is_directory_without_reparse()
    {
        return Err(io("Copy registry is not an ordinary directory"));
    }
    let creator = windows_fs::current_owner()?;
    let registry_security = security::PrivateSecurity::capture(central, &creator)?;

    for location in [&parent_location, &central_location] {
        source.reject_overlap(location, false, &mut progress)?;
    }
    let mut guarded_progress = |bytes| {
        progress(bytes)?;
        if windows_fs::location(&parent).map_err(io)? != parent_location
            || windows_fs::location(central).map_err(io)? != central_location
        {
            return Err(io("Copy source, destination or registry directory moved"));
        }
        source.validate()?;
        registry_security.validate(central, &creator)
    };
    guarded_progress(0)?;
    let existing = entry(&parent, name)?;
    if existing == Some(source.version.identity) {
        return Err(io(
            "Cannot copy an entry onto itself or its hard-link alias",
        ));
    }
    let mut number = 0usize;
    loop {
        guarded_progress(0)?;
        let mut target = name.to_os_string();
        if number != 0 {
            if number > MAX_RENAME_CANDIDATES {
                return Err(io("Copy rename candidate limit reached"));
            }
            target = Path::new(name).file_stem().unwrap_or(name).to_os_string();
            target.push(format!(" ({number})"));
            if let Some(extension) = Path::new(name).extension() {
                target.push(".");
                target.push(extension);
            }
        }
        let occupied = entry(&parent, &target)?;
        let admission = match (occupied, conflict) {
            (Some(_), ConflictAction::Skip) => return Ok(None),
            (Some(_), ConflictAction::Rename) => {
                number += 1;
                continue;
            }
            (Some(_), ConflictAction::Overwrite) => journal::DestinationAdmission::overwrite(
                &parent,
                &target,
                max_entries,
                &mut guarded_progress,
            )?,
            (None, _) => journal::DestinationAdmission::creation(&parent, &target)?,
        };
        let _dependencies =
            admission.guard_overwrite_dependencies(&parent, central, &mut guarded_progress)?;
        let result = journal::DurableStage::execute_with_receipt(
            &source.file,
            &parent,
            central,
            &admission,
            name,
            max_entries,
            &mut guarded_progress,
        );
        match result {
            Err(PlatformError::DestinationExists(_)) if conflict == ConflictAction::Rename => {
                number += 1;
            }
            Err(PlatformError::DestinationExists(_)) if conflict == ConflictAction::Skip => {
                return Ok(None);
            }
            other => return other.map(Some),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformOps;

    fn registry(root: &Path) -> File {
        let parent = windows_fs::open_directory(root, MAXIMUM_ALLOWED).unwrap();
        windows_fs::create_private(
            &parent,
            OsStr::new("registry"),
            true,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
        .unwrap()
    }

    #[test]
    fn windows_native_copy_dispatch_preserves_an_outstanding_undo_destination_claim() {
        for (target_name, restored_original) in [
            ("item.txt", false),
            ("ITEM.TXT", false),
            ("item.txt", true),
            ("ITEM.TXT", true),
        ] {
            let root = tempfile::tempdir().unwrap();
            let central = registry(root.path());
            let first = root.path().join("first");
            let second = root.path().join("second");
            let destination = root.path().join("destination");
            for directory in [&first, &second, &destination] {
                std::fs::create_dir(directory).unwrap();
            }
            let first_source = first.join("item.txt");
            let second_source = second.join(target_name);
            std::fs::write(&first_source, b"first publication").unwrap();
            std::fs::write(&second_source, b"second source").unwrap();
            if restored_original {
                std::fs::write(destination.join("item.txt"), b"restored original").unwrap();
            }
            let receipt = copy_at(
                &first_source,
                &destination,
                ConflictAction::Overwrite,
                &central,
                100,
                |_| Ok(()),
            )
            .unwrap()
            .unwrap();
            let journal_bytes = || {
                let mut records: Vec<_> = std::fs::read_dir(root.path().join("registry"))
                    .unwrap()
                    .map(|entry| {
                        let entry = entry.unwrap();
                        (entry.file_name(), std::fs::read(entry.path()).unwrap())
                    })
                    .collect();
                records.sort();
                records
            };
            let before = journal_bytes();
            // No live transaction handle protects this completed Copy. Its retained
            // journals must own the publication until Undo is durably relinquished.
            let attempt = copy_at(
                &second_source,
                &destination,
                ConflictAction::Overwrite,
                &central,
                100,
                |_| Ok(()),
            );
            assert!(
                attempt.is_err(),
                "second Overwrite was admitted while Undo owns the destination; published bytes: {:?}",
                std::fs::read(destination.join("item.txt")).unwrap()
            );
            assert!(
                attempt
                    .unwrap_err()
                    .to_string()
                    .contains("retained Copy transaction")
            );
            assert_eq!(journal_bytes(), before);
            assert_eq!(
                std::fs::read(destination.join("item.txt")).unwrap(),
                b"first publication"
            );
            assert_eq!(std::fs::read(&first_source).unwrap(), b"first publication");
            assert_eq!(std::fs::read(&second_source).unwrap(), b"second source");
            let unrelated = second.join("unrelated.txt");
            std::fs::write(&unrelated, b"independent data").unwrap();
            std::fs::write(destination.join("unrelated.txt"), b"unrelated original").unwrap();
            let transaction = before[0]
                .0
                .to_str()
                .unwrap()
                .strip_suffix(".copy-journal")
                .unwrap();
            let local = destination
                .join(format!(".ottrin-copy-{transaction}"))
                .join("journal");
            let saved = std::fs::read(&local).unwrap();
            let mut divergent = saved.clone();
            divergent.push(0);
            std::fs::write(&local, &divergent).unwrap();
            assert!(
                copy_at(
                    &unrelated,
                    &destination,
                    ConflictAction::Overwrite,
                    &central,
                    100,
                    |_| Ok(()),
                )
                .is_err()
            );
            assert_eq!(std::fs::read(&local).unwrap(), divergent);
            assert_eq!(journal_bytes(), before);
            assert_eq!(
                std::fs::read(destination.join("unrelated.txt")).unwrap(),
                b"unrelated original"
            );
            std::fs::write(&local, saved).unwrap();
            let unrelated_receipt = copy_at(
                &unrelated,
                &destination,
                ConflictAction::Overwrite,
                &central,
                100,
                |_| Ok(()),
            )
            .unwrap()
            .unwrap();
            crate::DefaultPlatform
                .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
                .unwrap();
            let reused = copy_at(
                &second_source,
                &destination,
                ConflictAction::Overwrite,
                &central,
                100,
                |_| Ok(()),
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                std::fs::read(destination.join("item.txt")).unwrap(),
                b"second source"
            );
            crate::DefaultPlatform
                .undo_copy_with_receipt(&reused, &mut |_| Ok(()))
                .unwrap();
            crate::DefaultPlatform
                .undo_copy_with_receipt(&unrelated_receipt, &mut |_| Ok(()))
                .unwrap();
        }
    }

    #[test]
    fn windows_native_copy_dispatch_preserves_creation_times_across_tunneled_names_and_undo() {
        use std::{
            mem::size_of,
            os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
        };
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_BASIC_INFO, FILE_WRITE_ATTRIBUTES, FileBasicInfo, SetFileInformationByHandle,
        };
        const SOURCE_CREATED: i64 = 125_911_584_000_000_000;
        const ORIGINAL_CREATED: i64 = 129_067_776_000_000_000;
        let set_created = |path: &Path, time: i64| {
            let file = std::fs::OpenOptions::new()
                .access_mode(FILE_WRITE_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .open(path)
                .unwrap();
            let info = FILE_BASIC_INFO {
                CreationTime: time,
                ..Default::default()
            };
            assert_ne!(
                unsafe {
                    SetFileInformationByHandle(
                        file.as_raw_handle().cast(),
                        FileBasicInfo,
                        ptr::from_ref(&info).cast(),
                        size_of::<FILE_BASIC_INFO>() as u32,
                    )
                },
                0
            );
        };
        for overwrite in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let central = registry(root.path());
            let source = root.path().join("item.txt");
            let destination = root.path().join("destination");
            std::fs::create_dir(&destination).unwrap();
            let target = destination.join("item.txt");
            std::fs::write(&source, b"copy").unwrap();
            set_created(&source, SOURCE_CREATED);
            std::fs::write(&target, b"original").unwrap();
            set_created(&target, ORIGINAL_CREATED);
            if !overwrite {
                std::fs::remove_file(&target).unwrap();
            }
            let receipt = copy_at(
                &source,
                &destination,
                ConflictAction::Overwrite,
                &central,
                100,
                |_| Ok(()),
            )
            .unwrap()
            .unwrap();
            let parent = windows_fs::open_directory(&destination, FILE_GENERIC_READ).unwrap();
            let published =
                namespace::open_source(&parent, OsStr::new("item.txt"), Some(false)).unwrap();
            assert_eq!(version(&published).unwrap().created, SOURCE_CREATED);
            assert_eq!(std::fs::read(&target).unwrap(), b"copy");
            drop((published, parent));
            crate::DefaultPlatform
                .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
                .unwrap();
            if overwrite {
                let parent = windows_fs::open_directory(&destination, FILE_GENERIC_READ).unwrap();
                let original =
                    namespace::open_source(&parent, OsStr::new("item.txt"), Some(false)).unwrap();
                assert_eq!(version(&original).unwrap().created, ORIGINAL_CREATED);
                assert_eq!(std::fs::read(&target).unwrap(), b"original");
            } else {
                assert!(!target.exists());
            }
            assert_eq!(std::fs::read(&source).unwrap(), b"copy");
        }
    }

    #[test]
    fn windows_native_copy_dispatch_routes_creation_skip_rename_and_overwrite() {
        for directory in [false, true] {
            for conflict in [
                ConflictAction::Skip,
                ConflictAction::Rename,
                ConflictAction::Overwrite,
            ] {
                for occupied in [false, true] {
                    let root = tempfile::tempdir().unwrap();
                    let central = registry(root.path());
                    let source_parent = root.path().join("source");
                    let destination = root.path().join("destination");
                    std::fs::create_dir(&source_parent).unwrap();
                    std::fs::create_dir(&destination).unwrap();
                    let source = source_parent.join("item.txt");
                    if directory {
                        std::fs::create_dir(&source).unwrap();
                        std::fs::write(source.join("child"), b"source data").unwrap();
                    } else {
                        std::fs::write(&source, b"source data").unwrap();
                    }
                    let target = destination.join("item.txt");
                    if occupied {
                        std::fs::write(&target, b"original data").unwrap();
                    }
                    // A dangling link is still an occupied name during Rename.
                    std::os::windows::fs::symlink_file("missing", destination.join("item (1).txt"))
                        .unwrap();
                    let receipt =
                        copy_at(&source, &destination, conflict, &central, 100, |_| Ok(()))
                            .unwrap();
                    if occupied && conflict == ConflictAction::Skip {
                        assert!(receipt.is_none());
                        assert_eq!(std::fs::read(&target).unwrap(), b"original data");
                        assert_eq!(
                            std::fs::read_dir(root.path().join("registry"))
                                .unwrap()
                                .count(),
                            0
                        );
                        continue;
                    }
                    let receipt = receipt.unwrap();
                    let published = if occupied && conflict == ConflictAction::Rename {
                        destination.join("item (2).txt")
                    } else {
                        target.clone()
                    };
                    assert_eq!(
                        receipt.after_path(),
                        windows_fs::location(
                            &windows_fs::open_directory(&destination, FILE_GENERIC_READ).unwrap()
                        )
                        .unwrap()
                        .join(published.file_name().unwrap())
                    );
                    let leaf = if directory {
                        published.join("child")
                    } else {
                        published.clone()
                    };
                    assert_eq!(std::fs::read(&leaf).unwrap(), b"source data");
                    if directory {
                        std::fs::remove_dir_all(&source).unwrap();
                    } else {
                        std::fs::remove_file(&source).unwrap();
                    }
                    crate::DefaultPlatform
                        .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
                        .unwrap();
                    if occupied && conflict == ConflictAction::Overwrite {
                        assert_eq!(std::fs::read(&target).unwrap(), b"original data");
                    } else {
                        assert!(!published.exists());
                    }
                    if occupied && conflict == ConflictAction::Rename {
                        assert_eq!(std::fs::read(&target).unwrap(), b"original data");
                    }
                    assert!(
                        std::fs::symlink_metadata(destination.join("item (1).txt"))
                            .unwrap()
                            .file_type()
                            .is_symlink()
                    );
                }
            }
        }
    }

    #[test]
    fn windows_native_copy_dispatch_refuses_self_descendants_aliases_and_foreign_registry() {
        let root = tempfile::tempdir().unwrap();
        let central = registry(root.path());
        let source = root.path().join("source");
        let nested = source.join("nested");
        let destination = root.path().join("destination");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(source.join("file"), b"unchanged").unwrap();
        for target in [&source, &nested] {
            assert!(
                copy_at(
                    &source,
                    target,
                    ConflictAction::Overwrite,
                    &central,
                    100,
                    |_| Ok(())
                )
                .is_err()
            );
        }
        std::os::windows::fs::symlink_dir(&nested, root.path().join("junction-like-alias"))
            .unwrap();
        assert!(
            copy_at(
                &source,
                &root.path().join("junction-like-alias"),
                ConflictAction::Rename,
                &central,
                100,
                |_| Ok(())
            )
            .is_err()
        );
        std::fs::hard_link(source.join("file"), destination.join("file")).unwrap();
        for conflict in [
            ConflictAction::Skip,
            ConflictAction::Rename,
            ConflictAction::Overwrite,
        ] {
            assert!(
                copy_at(
                    &source.join("file"),
                    &destination,
                    conflict,
                    &central,
                    100,
                    |_| Ok(())
                )
                .is_err()
            );
        }
        let ordinary = windows_fs::open_directory(&destination, MAXIMUM_ALLOWED).unwrap();
        assert!(
            copy_at(
                &source,
                &destination,
                ConflictAction::Rename,
                &ordinary,
                100,
                |_| Ok(())
            )
            .is_err()
        );
        let inner = registry(&nested);
        assert!(
            copy_at(
                &source,
                &destination,
                ConflictAction::Rename,
                &inner,
                100,
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(std::fs::read(source.join("file")).unwrap(), b"unchanged");
        assert_eq!(
            std::fs::read_dir(root.path().join("registry"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            std::fs::read_dir(nested.join("registry")).unwrap().count(),
            0
        );
    }

    #[test]
    fn windows_native_copy_dispatch_preserves_late_occupants_and_original_changes() {
        for conflict in [ConflictAction::Rename, ConflictAction::Overwrite] {
            let root = tempfile::tempdir().unwrap();
            let central = registry(root.path());
            let source = root.path().join("item.txt");
            let destination = root.path().join("destination");
            std::fs::create_dir(&destination).unwrap();
            std::fs::write(&source, b"source data").unwrap();
            std::fs::write(destination.join("item.txt"), b"approved original").unwrap();
            let changed = if conflict == ConflictAction::Rename {
                destination.join("item (1).txt")
            } else {
                destination.join("item.txt")
            };
            let mut injected = false;
            let result = copy_at(&source, &destination, conflict, &central, 100, |bytes| {
                // Overwrite admission itself reads payload bytes under a lock.
                // Wait for durable staging ownership before simulating a change.
                if bytes > 0
                    && !injected
                    && std::fs::read_dir(root.path().join("registry"))
                        .unwrap()
                        .next()
                        .is_some()
                {
                    std::fs::write(&changed, b"external data").unwrap();
                    injected = true;
                }
                Ok(())
            });
            assert!(injected);
            assert!(
                matches!(result, Err(PlatformError::CleanupPending { .. })),
                "{result:?}"
            );
            assert_eq!(std::fs::read(&changed).unwrap(), b"external data");
            assert_eq!(std::fs::read(&source).unwrap(), b"source data");
            assert_eq!(
                std::fs::read_dir(root.path().join("registry"))
                    .unwrap()
                    .count(),
                1
            );
        }
    }
}
