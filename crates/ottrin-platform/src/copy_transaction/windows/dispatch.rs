//! Path/conflict admission for the native Copy transaction engine.
use super::{io, journal, namespace, security, stable_identity_from_handle, version};
use crate::{ConflictAction, PathMoveUndoReceipt, PlatformError, windows_fs};
use std::{ffi::OsStr, fs::File, path::Path, ptr};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_GENERIC_READ, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    SYNCHRONIZE,
};
use windows_sys::Win32::System::SystemServices::MAXIMUM_ALLOWED;

const MAX_RENAME_CANDIDATES: usize = 10_000;

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
    let name = source
        .file_name()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    windows_fs::valid_component(name).map_err(io)?;
    let source_path = std::path::absolute(source).map_err(io)?;
    let source_parent = windows_fs::open_directory(
        source_path
            .parent()
            .ok_or_else(|| io("Copy source has no parent"))?,
        FILE_GENERIC_READ,
    )
    .map_err(io)?;
    let source_file = namespace::open_source(&source_parent, name, None)?;
    let source_version = version(&source_file)?;
    let parent = windows_fs::open_directory(destination, MAXIMUM_ALLOWED).map_err(io)?;
    let parent_location = windows_fs::location(&parent).map_err(io)?;
    let source_parent_location = windows_fs::location(&source_parent).map_err(io)?;
    let central_location = windows_fs::location(central).map_err(io)?;
    if !stable_identity_from_handle(central)
        .map_err(io)?
        .is_directory_without_reparse()
    {
        return Err(io("Copy registry is not an ordinary directory"));
    }
    let creator = security::current_owner()?;
    let registry_security = security::PrivateSecurity::capture(central, &creator)?;

    // Compare real native IDs, including through differently cased paths and
    // junction ancestors. Lexical prefix comparisons cannot establish this.
    if source_version.identity.is_directory_without_reparse() {
        for location in [&parent_location, &central_location] {
            for ancestor in location.ancestors() {
                progress(0)?;
                let directory =
                    windows_fs::open_directory(ancestor, FILE_READ_ATTRIBUTES | SYNCHRONIZE)
                        .map_err(io)?;
                if stable_identity_from_handle(&directory).map_err(io)? == source_version.identity {
                    return Err(io(
                        "Copy destination and registry must be outside the source tree",
                    ));
                }
            }
        }
    }
    let mut guarded_progress = |bytes| {
        progress(bytes)?;
        if windows_fs::location(&parent).map_err(io)? != parent_location
            || windows_fs::location(&source_parent).map_err(io)? != source_parent_location
            || windows_fs::location(central).map_err(io)? != central_location
        {
            return Err(io("Copy source, destination or registry directory moved"));
        }
        namespace::validate_binding(&source_parent, name, source_version.identity)?;
        registry_security.validate(central, &creator)
    };
    guarded_progress(0)?;
    let existing = entry(&parent, name)?;
    if existing == Some(source_version.identity) {
        return Err(io(
            "Cannot copy an entry onto itself or its hard-link alias",
        ));
    }
    let admission = match (existing, conflict) {
        (Some(_), ConflictAction::Skip) => return Ok(None),
        (Some(_), ConflictAction::Overwrite) => journal::DestinationAdmission::overwrite(
            &parent,
            name,
            max_entries,
            &mut guarded_progress,
        )?,
        (Some(_), ConflictAction::Rename) => {
            let mut chosen = None;
            for number in 1..=MAX_RENAME_CANDIDATES {
                guarded_progress(0)?;
                let mut candidate = source.file_stem().unwrap_or(name).to_os_string();
                candidate.push(format!(" ({number})"));
                if let Some(extension) = source.extension() {
                    candidate.push(".");
                    candidate.push(extension);
                }
                if entry(&parent, &candidate)?.is_none() {
                    chosen = Some(journal::DestinationAdmission::creation(
                        &parent, &candidate,
                    )?);
                    break;
                }
            }
            chosen.ok_or_else(|| io("Copy rename candidate limit reached"))?
        }
        (None, _) => journal::DestinationAdmission::creation(&parent, name)?,
    };
    journal::DurableStage::execute_with_receipt(
        &source_file,
        &parent,
        central,
        &admission,
        name,
        max_entries,
        guarded_progress,
    )
    .map(Some)
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
                        .undo_copy_with_receipt(&receipt)
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
