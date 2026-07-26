#[cfg(target_os = "linux")]
use super::move_recovery;
use super::{
    ConflictAction, PlatformError, copy_entry, next_available_name,
    reject_directory_copy_into_itself,
};
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
use std::path::Path;

#[cfg(target_os = "linux")]
const CROSS_DEVICE_STAGED_NAME: &str = "candidate";

/// Move a single entry with safe runtime semantics on supported platforms.
#[cfg(not(target_os = "linux"))]
pub(super) fn move_entry_transactional(
    source: &Path,
    destination_dir: &Path,
    _conflict: ConflictAction,
) -> Result<(), PlatformError> {
    std::fs::symlink_metadata(source)
        .map_err(|_| PlatformError::InvalidSource(source.display().to_string()))?;
    reject_directory_copy_into_itself(source, destination_dir)?;
    Err(PlatformError::UnsupportedSafeMove(
        source.display().to_string(),
    ))
}

#[cfg(target_os = "linux")]
pub(super) fn move_entry_transactional(
    source: &Path,
    destination_dir: &Path,
    conflict: ConflictAction,
) -> Result<(), PlatformError> {
    move_with_hooks(source, destination_dir, conflict, |_| Ok(()))
}

#[cfg(any(test, target_os = "linux"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TestPoint {
    Prepare,
    Claim,
    ClassifyCrossDevice,
    PendingDestination,
    Publication,
    BeforeSourceCleanup,
}

#[cfg(target_os = "linux")]
fn move_with_hooks(
    source: &Path,
    destination_dir: &Path,
    conflict: ConflictAction,
    mut hook: impl FnMut(TestPoint) -> Result<(), &'static str>,
) -> Result<(), PlatformError> {
    std::fs::symlink_metadata(source)
        .map_err(|_| PlatformError::InvalidSource(source.display().to_string()))?;
    let source_name = source
        .file_name()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    reject_directory_copy_into_itself(source, destination_dir)?;

    if destination_dir.join(source_name).exists() && conflict == ConflictAction::Skip {
        return Ok(());
    }

    let source_parent_path = source
        .parent()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    let source_parent = move_recovery::LinuxSourceParentDirectory::open(source_parent_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let source_metadata = std::fs::symlink_metadata(source)
        .map_err(|_| PlatformError::InvalidSource(source.display().to_string()))?;
    let source_parent_metadata = std::fs::symlink_metadata(source_parent_path)
        .map_err(|_| PlatformError::InvalidDestination(source_parent_path.display().to_string()))?;
    let source_parent_identity = move_recovery::LinuxStableIdentity::new(
        source_parent_metadata.dev(),
        source_parent_metadata.ino(),
        if source_parent_metadata.is_dir() {
            move_recovery::LinuxFileType::Directory
        } else {
            move_recovery::LinuxFileType::Other
        },
    );
    let source_entry_identity = move_recovery::LinuxStableIdentity::new(
        source_metadata.dev(),
        source_metadata.ino(),
        if source_metadata.is_file() {
            move_recovery::LinuxFileType::Regular
        } else if source_metadata.is_dir() {
            move_recovery::LinuxFileType::Directory
        } else if source_metadata.file_type().is_symlink() {
            move_recovery::LinuxFileType::Symlink
        } else {
            move_recovery::LinuxFileType::Other
        },
    );

    let recovery_root = default_recovery_root()?;
    std::fs::create_dir_all(&recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut recovery_transaction_id = [0_u8; 16];
    getrandom::fill(&mut recovery_transaction_id)
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    let local_transaction = source_parent
        .create_transaction(recovery_transaction_id)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let source_transaction_identity = local_transaction.observed_identity();
    let local_path = source_parent_path.join(local_transaction.name());
    let local = move_recovery::LinuxJournalDirectory::open_configured(&local_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    let prepared = move_recovery::PreparedRecord::new_linux(
        recovery_transaction_id,
        source_parent_path,
        Path::new(source_name),
        source_parent_identity,
        source_entry_identity,
        local_transaction.name(),
        source_transaction_identity,
    )
    .map_err(|error| PlatformError::Io(error.to_string()))?;

    let central = move_recovery::LinuxJournalDirectory::open_configured(&recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut recovery = central
        .try_lock_recovery()
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    recovery
        .prepare_with_local(&local, &prepared)
        .map_err(|error| {
            PlatformError::Io(format!(
                "prepare durable state failed before source claim: {error}"
            ))
        })?;
    hook(TestPoint::Prepare)
        .map_err(|message| PlatformError::Io(format!("move test hook failed: {message}")))?;

    recovery
        .advance_to_claim_intent_with_local(&local, &prepared)
        .map_err(|error| {
            PlatformError::Io(format!(
                "claim-intent durable state failed before source claim: {error}"
            ))
        })?;

    let _ = recovery
        .claim_source_after_intent(&source_parent, &local, recovery_transaction_id)
        .map_err(|error| PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: format!("source claim failed: {error}"),
        })?;

    recovery
        .advance_to_claimed_with_local(&local, recovery_transaction_id)
        .map_err(|error| PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: format!("claimed durable state failed: {error}"),
        })?;
    hook(TestPoint::Claim)
        .map_err(|message| PlatformError::Io(format!("move test hook failed: {message}")))?;
    let destination_parent_metadata = std::fs::symlink_metadata(destination_dir)
        .map_err(|_| PlatformError::InvalidDestination(destination_dir.display().to_string()))?;
    let is_cross_device = match hook(TestPoint::ClassifyCrossDevice) {
        Ok(()) => source_parent_metadata.dev() != destination_parent_metadata.dev(),
        Err("force cross-device move") => true,
        Err(message) => {
            return Err(PlatformError::Io(format!(
                "move test hook failed: {message}"
            )));
        }
    };
    if is_cross_device && conflict == ConflictAction::Overwrite {
        return Err(PlatformError::UnsupportedSafeCrossDeviceMove(
            source.display().to_string(),
        ));
    }

    let claimed_path = local_path.join("claimed");
    let mut destination_name = source_name.to_os_string();
    let mut destination_target = destination_dir.join(&destination_name);

    if is_cross_device {
        let destination_parent = move_recovery::LinuxSourceParentDirectory::open(destination_dir)
            .map_err(|error| PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: format!("destination transaction parent open failed: {error}"),
        })?;
        let destination_transaction = destination_parent
            .create_transaction(recovery_transaction_id)
            .map_err(|error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("destination transaction creation failed: {error}"),
            })?;
        let destination_transaction_identity = destination_transaction.observed_identity();
        let destination_transaction_path = destination_dir.join(destination_transaction.name());
        let mut staged_destination_identity = None;
        let destination_tree_snapshot = source_tree_snapshot_for_path(&claimed_path)?;

        loop {
            if staged_destination_identity.is_none() {
                let staged_destination_path =
                    destination_transaction_path.join(CROSS_DEVICE_STAGED_NAME);
                copy_entry(&claimed_path, &staged_destination_path).map_err(|error| {
                    PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!("cross-device stage copy failed: {error}"),
                    }
                })?;
                flush_tree_bottom_up(&staged_destination_path).map_err(|error| {
                    PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!("cross-device stage flush failed: {error}"),
                    }
                })?;
                let staged_metadata = std::fs::symlink_metadata(&staged_destination_path)
                    .map_err(|error| PlatformError::Io(error.to_string()))?;
                staged_destination_identity = Some(stable_identity_from_metadata(&staged_metadata));
            }

            let destination_transaction_path_path = destination_transaction_path.as_path();
            let staged_identity = staged_destination_identity
                .ok_or_else(|| PlatformError::InvalidSource(claimed_path.display().to_string()))?;
            recovery
                .advance_to_pending_destination_with_local_with_metadata(
                    &local,
                    recovery_transaction_id,
                    destination_name.as_ref(),
                    move_recovery::PendingDestinationMetadata {
                        destination_transaction_name: Some(
                            destination_transaction.name().to_path_buf(),
                        ),
                        destination_transaction_identity: Some(destination_transaction_identity),
                        source_tree_snapshot: Some(destination_tree_snapshot.clone()),
                        staged_destination_identity: Some(staged_identity),
                    },
                )
                .map_err(|error| PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("pending destination persistence failed: {error}"),
                })?;
            hook(TestPoint::PendingDestination).map_err(|message| {
                PlatformError::Io(format!("move test hook failed: {message}"))
            })?;
            flush_tree_bottom_up(destination_transaction_path_path).map_err(|error| {
                PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("cross-device transaction flush failed: {error}"),
                }
            })?;
            std::fs::File::open(destination_dir)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("destination parent flush failed: {error}"),
                })?;

            let staged_candidate = destination_transaction_path.join(CROSS_DEVICE_STAGED_NAME);
            match publish_claimed_to_destination_with_no_replace(
                &staged_candidate,
                &destination_target,
            ) {
                Ok(()) => {
                    break;
                }
                Err(publish_error) if publish_error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if conflict == ConflictAction::Skip {
                        restore_claimed_to_source(
                            source_parent_path,
                            source_name,
                            &claimed_path,
                            &local_path,
                        )
                        .map_err(|restore_error| PlatformError::CommittedCleanupPending {
                            transaction: local_path.display().to_string(),
                            message: format!(
                                "skip-mode publication conflict but claim could not be restored: {restore_error}"
                            ),
                        })?;
                        let _ = cleanup_transaction_journals(
                            &recovery_root,
                            &local_path,
                            &recovery_transaction_id,
                        );
                        let _ = std::fs::remove_dir_all(destination_transaction_path_path);
                        return Ok(());
                    }
                    if conflict == ConflictAction::Overwrite {
                        publish_claimed_to_destination_with_exchange(
                            &staged_candidate,
                            &destination_target,
                        )
                        .map_err(|publish_error| {
                            PlatformError::CommittedCleanupPending {
                                transaction: local_path.display().to_string(),
                                message: format!("publication failed: {publish_error}"),
                            }
                        })?;
                        break;
                    }
                    destination_name = next_available_name(&destination_target)
                        .file_name()
                        .expect("candidate has a file name")
                        .to_os_string();
                    destination_target = destination_dir.join(&destination_name);
                    if conflict == ConflictAction::Rename {
                        continue;
                    }
                    return Err(PlatformError::CleanupPending {
                        transaction: local_path.display().to_string(),
                        message: "publication conflict occurred for unsupported conflict mode"
                            .to_string(),
                    });
                }
                Err(publish_error) => {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!("publication failed: {publish_error}"),
                    });
                }
            }
        }

        hook(TestPoint::Publication)
            .map_err(|message| PlatformError::Io(format!("move test hook failed: {message}")))?;

        let destination_identity = staged_destination_identity
            .ok_or_else(|| PlatformError::InvalidSource(claimed_path.display().to_string()))?;
        let destination_metadata =
            std::fs::symlink_metadata(&destination_target).map_err(|error| {
                PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("cross-device destination identity check failed: {error}"),
                }
            })?;
        if destination_identity != stable_identity_from_metadata(&destination_metadata) {
            return Err(PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: "cross-device destination identity changed before source cleanup"
                    .to_string(),
            });
        }
        if !verify_source_tree_snapshot(&claimed_path, &destination_tree_snapshot)
            .map_err(|error| PlatformError::Io(error.to_string()))?
        {
            return Err(PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: "cross-device source snapshot changed before cleanup".to_string(),
            });
        }

        recovery
            .advance_to_published_with_local(&local, recovery_transaction_id)
            .map_err(|error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("publication durable state failed: {error}"),
            })?;
        hook(TestPoint::BeforeSourceCleanup).map_err(|message| {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("move test hook failed: {message}"),
            }
        })?;

        remove_source_entry(&claimed_path).map_err(|error| {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("cross-device source cleanup failed: {error}"),
            }
        })?;

        let _ = std::fs::remove_dir_all(destination_transaction_path);
        let _ = cleanup_transaction_journals(&recovery_root, &local_path, &recovery_transaction_id);
        return Ok(());
    }

    loop {
        recovery
            .advance_to_pending_destination_with_local(
                &local,
                recovery_transaction_id,
                destination_name.as_ref(),
            )
            .map_err(|error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("pending destination persistence failed: {error}"),
            })?;
        hook(TestPoint::PendingDestination)
            .map_err(|message| PlatformError::Io(format!("move test hook failed: {message}")))?;

        match publish_claimed_to_destination_with_no_replace(&claimed_path, &destination_target) {
            Ok(()) => {
                break;
            }
            Err(publish_error) if publish_error.kind() == std::io::ErrorKind::AlreadyExists => {
                if conflict == ConflictAction::Skip {
                    restore_claimed_to_source(
                        source_parent_path,
                        source_name,
                        &claimed_path,
                        &local_path,
                    )
                    .map_err(|restore_error| PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!(
                            "skip-mode publication conflict but claim could not be restored: {restore_error}"
                        ),
                    })?;
                    let _ = cleanup_transaction_journals(
                        &recovery_root,
                        &local_path,
                        &recovery_transaction_id,
                    );
                    return Ok(());
                }

                if conflict == ConflictAction::Overwrite {
                    publish_claimed_to_destination_with_exchange(
                        &claimed_path,
                        &destination_target,
                    )
                    .map_err(|publish_error| {
                        PlatformError::CommittedCleanupPending {
                            transaction: local_path.display().to_string(),
                            message: format!("publication failed: {publish_error}"),
                        }
                    })?;
                    break;
                }

                destination_name = next_available_name(&destination_target)
                    .file_name()
                    .expect("candidate has a file name")
                    .to_os_string();
                destination_target = destination_dir.join(&destination_name);
                if conflict == ConflictAction::Rename {
                    continue;
                }
                return Err(PlatformError::CleanupPending {
                    transaction: local_path.display().to_string(),
                    message: "publication conflict occurred for unsupported conflict mode"
                        .to_string(),
                });
            }
            Err(publish_error) => {
                return Err(PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("publication failed: {publish_error}"),
                });
            }
        }
    }

    hook(TestPoint::Publication)
        .map_err(|message| PlatformError::Io(format!("move test hook failed: {message}")))?;

    recovery
        .advance_to_published_with_local(&local, recovery_transaction_id)
        .map_err(|error| PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: format!("publication durable state failed: {error}"),
        })?;

    let _ = cleanup_transaction_journals(&recovery_root, &local_path, &recovery_transaction_id);
    Ok(())
}

fn transaction_id_hex(transaction_id: &[u8; 16]) -> String {
    transaction_id
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn cleanup_transaction_journals(
    recovery_root: &Path,
    local_path: &Path,
    transaction_id: &[u8; 16],
) -> Result<(), PlatformError> {
    let transaction_id = transaction_id_hex(transaction_id);
    let central_journal = recovery_root.join(format!("{transaction_id}.journal"));
    let local_journal = local_path.join("journal");
    std::fs::remove_file(central_journal).ok();
    std::fs::remove_file(local_journal).ok();

    #[cfg(test)]
    {
        std::fs::remove_dir_all(local_path)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
    }

    #[cfg(not(test))]
    {
        let _ = std::fs::remove_dir(local_path);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn publish_claimed_to_destination_with_no_replace(
    claimed: &Path,
    destination: &Path,
) -> std::io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    use rustix::io::Errno;

    match renameat_with(CWD, claimed, CWD, destination, RenameFlags::NOREPLACE) {
        Ok(()) => Ok(()),
        Err(error) => {
            if error == Errno::EXIST {
                Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
            } else {
                Err(std::io::Error::other(error.to_string()))
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn publish_claimed_to_destination_with_exchange(
    claimed: &Path,
    destination: &Path,
) -> std::io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    renameat_with(CWD, claimed, CWD, destination, RenameFlags::EXCHANGE)
        .map_err(|error| std::io::Error::other(error.to_string()))
}

#[cfg(target_os = "linux")]
fn restore_claimed_to_source(
    source_parent: &Path,
    source_name: &std::ffi::OsStr,
    claimed: &Path,
    _local_path: &Path,
) -> Result<(), String> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    let original_target = source_parent.join(source_name);
    renameat_with(
        CWD,
        claimed,
        CWD,
        original_target.as_os_str(),
        RenameFlags::NOREPLACE,
    )
    .map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
fn stable_identity_from_metadata(
    metadata: &std::fs::Metadata,
) -> move_recovery::LinuxStableIdentity {
    let file_type = metadata.file_type();
    move_recovery::LinuxStableIdentity::new(
        metadata.dev(),
        metadata.ino(),
        if metadata.is_file() {
            move_recovery::LinuxFileType::Regular
        } else if metadata.is_dir() {
            move_recovery::LinuxFileType::Directory
        } else if file_type.is_symlink() {
            move_recovery::LinuxFileType::Symlink
        } else {
            move_recovery::LinuxFileType::Other
        },
    )
}

#[cfg(target_os = "linux")]
fn source_tree_snapshot_for_path(
    source_path: &Path,
) -> Result<move_recovery::SourceTreeSnapshot, PlatformError> {
    let mut entries = Vec::new();
    collect_source_tree_snapshot(source_path, source_path, &mut entries)?;
    entries.sort_by(|(left, _), (right, _)| left.cmp(right));
    Ok(move_recovery::SourceTreeSnapshot::from_entries(entries))
}

#[cfg(target_os = "linux")]
fn collect_source_tree_snapshot(
    source_path: &Path,
    root: &Path,
    entries: &mut Vec<(std::path::PathBuf, move_recovery::LinuxStableIdentity)>,
) -> Result<(), PlatformError> {
    let metadata = std::fs::symlink_metadata(source_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let relative = source_path
        .strip_prefix(root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    entries.push((
        relative.to_path_buf(),
        stable_identity_from_metadata(&metadata),
    ));

    if metadata.is_dir() {
        for child in
            std::fs::read_dir(source_path).map_err(|error| PlatformError::Io(error.to_string()))?
        {
            let child = child.map_err(|error| PlatformError::Io(error.to_string()))?;
            collect_source_tree_snapshot(&child.path(), root, entries)?;
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn verify_source_tree_snapshot(
    source_path: &Path,
    snapshot: &move_recovery::SourceTreeSnapshot,
) -> Result<bool, PlatformError> {
    let current = source_tree_snapshot_for_path(source_path)?;
    Ok(current == *snapshot)
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
        return std::fs::File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| PlatformError::Io(error.to_string()));
    }
    Err(PlatformError::InvalidSource(path.display().to_string()))
}

#[cfg(target_os = "linux")]
fn remove_source_entry(claimed_path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(claimed_path).map_err(|error| error.to_string())?;
    if metadata.is_file() || metadata.file_type().is_symlink() {
        std::fs::remove_file(claimed_path).map_err(|error| error.to_string())?;
        return Ok(());
    }
    if metadata.is_dir() {
        std::fs::remove_dir_all(claimed_path).map_err(|error| error.to_string())?;
        return Ok(());
    }
    Err("unsupported source tree entry type".into())
}

fn default_recovery_root() -> Result<std::path::PathBuf, PlatformError> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                std::path::Path::new(&home)
                    .join(".local")
                    .join("state")
                    .to_path_buf()
                    .to_string_lossy()
                    .into_owned()
                    .into()
            })
        })
        .map(std::path::PathBuf::from)
        .ok_or_else(|| PlatformError::Io("XDG_STATE_HOME and HOME are unavailable".to_string()))?;

    Ok(base.join("ottrin").join("moves"))
}

/// Reads retained Move recovery status from an existing configured recovery root.
///
/// This is intentionally incapable of creating an anchor, journal, transaction directory, or
/// cleanup side effect. A missing root simply means there are no durable Move transactions yet.
#[cfg(target_os = "linux")]
pub(super) fn discover_retained_move_transactions()
-> Result<Vec<move_recovery::LinuxDiscoveredMoveTransaction>, PlatformError> {
    discover_retained_move_transactions_at(&default_recovery_root()?)
}

/// Reads retained Move recovery status from an existing configured recovery root.
///
/// This is intentionally incapable of creating an anchor, journal, transaction directory, or
/// cleanup side effect. A missing root simply means there are no durable Move transactions yet.
#[cfg(target_os = "linux")]
pub(super) fn discover_retained_move_transactions_at(
    recovery_root: &Path,
) -> Result<Vec<move_recovery::LinuxDiscoveredMoveTransaction>, PlatformError> {
    match std::fs::symlink_metadata(recovery_root) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(PlatformError::Io(error.to_string())),
    }
    let central = move_recovery::LinuxJournalDirectory::open_configured_read_only(recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    central
        .discover_move_transactions()
        .map_err(|error| PlatformError::Io(error.to_string()))
}

#[cfg(target_os = "linux")]
#[cfg(test)]
pub(super) fn move_entry_transactional_with_test_hook(
    source: &Path,
    destination_dir: &Path,
    conflict: ConflictAction,
    hook: impl FnMut(TestPoint) -> Result<(), &'static str>,
) -> Result<(), PlatformError> {
    move_with_hooks(source, destination_dir, conflict, hook)
}

#[cfg(not(target_os = "linux"))]
#[cfg(test)]
#[allow(dead_code)]
pub(super) fn move_entry_transactional_with_test_hook(
    source: &Path,
    destination_dir: &Path,
    conflict: ConflictAction,
    _hook: impl FnMut(TestPoint) -> Result<(), &'static str>,
) -> Result<(), PlatformError> {
    move_entry_transactional(source, destination_dir, conflict)
}

#[cfg(all(test, target_os = "linux"))]
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TestDir(PathBuf);
    impl TestDir {
        fn new(label: &str) -> Self {
            loop {
                let mut random = [0_u8; 8];
                getrandom::fill(&mut random).unwrap();
                let suffix = random
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                let path = std::env::temp_dir()
                    .join(format!("ottrin-move-crossfs-metadata-{label}-{suffix}"));
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

    #[test]
    fn cross_filesystem_move_publishes_destination_metadata_with_snapshot() {
        let dir = TestDir::new("metadata");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                TestPoint::Publication => Err("publication hook failure"),
                _ => Ok(()),
            },
        );
        assert!(matches!(
            result,
            Err(PlatformError::Io(message))
                if message == "move test hook failed: publication hook failure"
        ));

        let local_transaction = std::fs::read_dir(dir.0.clone())
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry.path().is_dir()
                    && entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".ottrin-move-")
            })
            .expect("local transaction must exist");
        let local_transaction_name = local_transaction.file_name();
        let local_transaction_name = local_transaction_name.to_string_lossy();
        let transaction_id = local_transaction_name.trim_start_matches(".ottrin-move-");
        let local_path = local_transaction.path();

        let local_journal = serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(local_path.join("journal")).unwrap(),
        )
        .unwrap();
        let recovery_root = default_recovery_root().unwrap();
        let central_journal = serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(recovery_root.join(format!("{transaction_id}.journal"))).unwrap(),
        )
        .unwrap();

        assert_eq!(local_journal["phase"], "pending-destination");
        assert_eq!(central_journal["phase"], "pending-destination");
        assert!(!local_journal["destination_transaction_name"].is_null());
        assert!(!local_journal["source_tree_snapshot"].is_null());
        assert!(!local_journal["staged_destination_identity"].is_null());
        assert!(!central_journal["destination_transaction_name"].is_null());
        assert!(!central_journal["source_tree_snapshot"].is_null());
        assert!(!central_journal["staged_destination_identity"].is_null());
    }

    #[test]
    fn cross_filesystem_move_durably_records_publication_before_source_cleanup() {
        let dir = TestDir::new("published-before-cleanup");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                TestPoint::BeforeSourceCleanup => Err("stop before source cleanup"),
                _ => Ok(()),
            },
        );
        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { message, .. })
                if message == "move test hook failed: stop before source cleanup"
        ));

        let local_transaction = std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry.path().is_dir()
                    && entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".ottrin-move-")
            })
            .expect("local transaction must remain for recovery");
        let local_path = local_transaction.path();
        let transaction_id = local_transaction
            .file_name()
            .to_string_lossy()
            .trim_start_matches(".ottrin-move-")
            .to_owned();
        let local_journal: serde_json::Value =
            serde_json::from_slice(&std::fs::read(local_path.join("journal")).unwrap()).unwrap();
        let central_journal: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                default_recovery_root()
                    .unwrap()
                    .join(format!("{transaction_id}.journal")),
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(local_journal["phase"], "published");
        assert_eq!(central_journal["phase"], "published");
        assert_eq!(
            std::fs::read(local_path.join("claimed")).unwrap(),
            b"payload"
        );
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"payload"
        );
    }
}
