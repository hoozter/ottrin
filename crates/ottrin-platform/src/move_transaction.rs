#![cfg_attr(not(test), allow(dead_code))]

// The Linux journal implementation below remains compiled for recovery-test coverage while
// user-facing Move is deliberately disabled. Remove this allowance only when the runtime gate
// is reopened with the documented platform and crash evidence.
#[cfg(target_os = "linux")]
use super::move_recovery;
#[cfg(target_os = "windows")]
use super::move_recovery;
use super::{ConflictAction, MoveDisposition, PlatformError, reject_directory_copy_into_itself};
#[cfg(target_os = "linux")]
use super::{copy_entry, next_available_name, rename_without_replacement};
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
use std::path::Path;

#[cfg(target_os = "linux")]
const CROSS_DEVICE_STAGED_NAME: &str = "candidate";

#[cfg(all(test, target_os = "linux"))]
static TEST_MOVE_SERIALIZER: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(all(test, target_os = "linux"))]
static TEST_MOVE_ENV_SERIALIZER: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Runtime Move stays fail-closed except where the exact Linux-safe implementation is proven.
#[cfg(target_os = "linux")]
pub(super) fn move_entry_transactional(
    source: &Path,
    destination_dir: &Path,
    conflict: ConflictAction,
) -> Result<MoveDisposition, PlatformError> {
    let source_metadata = std::fs::symlink_metadata(source)
        .map_err(|_| PlatformError::InvalidSource(source.display().to_string()))?;
    let destination_dir = std::fs::canonicalize(destination_dir)
        .map_err(|_| PlatformError::InvalidDestination(destination_dir.display().to_string()))?;
    let destination_metadata = std::fs::symlink_metadata(&destination_dir)
        .map_err(|_| PlatformError::InvalidDestination(destination_dir.display().to_string()))?;
    reject_directory_copy_into_itself(source, &destination_dir)?;

    if source_metadata.dev() != destination_metadata.dev() {
        return Err(PlatformError::UnsupportedSafeCrossDeviceMove(
            source.display().to_string(),
        ));
    }

    let target = destination_dir.join(
        source
            .file_name()
            .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?,
    );
    match conflict {
        ConflictAction::Skip => match rename_without_replacement(source, &target) {
            Ok(()) => Ok(MoveDisposition::Moved),
            Err(PlatformError::DestinationExists(_)) => Ok(MoveDisposition::Skipped),
            Err(error) => Err(error),
        },
        ConflictAction::Rename => {
            let mut candidate = target;
            loop {
                match rename_without_replacement(source, &candidate) {
                    Ok(()) => return Ok(MoveDisposition::Moved),
                    Err(PlatformError::DestinationExists(_)) => {
                        candidate = next_available_name(&candidate);
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        ConflictAction::Overwrite => std::fs::rename(source, target)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::CrossesDevices {
                    PlatformError::UnsupportedSafeCrossDeviceMove(source.display().to_string())
                } else {
                    PlatformError::Io(error.to_string())
                }
            })
            .map(|_| MoveDisposition::Moved),
    }
}

#[cfg(target_os = "windows")]
pub(super) fn move_entry_transactional(
    source: &Path,
    destination_dir: &Path,
    _conflict: ConflictAction,
) -> Result<MoveDisposition, PlatformError> {
    std::fs::symlink_metadata(source)
        .map_err(|_| PlatformError::InvalidSource(source.display().to_string()))?;
    std::fs::symlink_metadata(destination_dir)
        .map_err(|_| PlatformError::InvalidDestination(destination_dir.display().to_string()))?;
    reject_directory_copy_into_itself(source, destination_dir)?;

    let _source_identity = move_recovery::stable_identity_of_entry(source)
        .map_err(|error| PlatformError::Io(format!("source identity probe failed: {error}")))?;
    let _destination_identity =
        move_recovery::stable_identity_of_entry(destination_dir).map_err(|error| {
            PlatformError::Io(format!("destination identity probe failed: {error}"))
        })?;

    Err(PlatformError::UnsupportedSafeMove(
        source.display().to_string(),
    ))
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub(super) fn move_entry_transactional(
    source: &Path,
    destination_dir: &Path,
    _conflict: ConflictAction,
) -> Result<MoveDisposition, PlatformError> {
    std::fs::symlink_metadata(source)
        .map_err(|_| PlatformError::InvalidSource(source.display().to_string()))?;
    reject_directory_copy_into_itself(source, destination_dir)?;
    Err(PlatformError::UnsupportedSafeMove(
        source.display().to_string(),
    ))
}

#[cfg(target_os = "linux")]
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
) -> Result<MoveDisposition, PlatformError> {
    #[cfg(test)]
    let _test_move_guard = TEST_MOVE_SERIALIZER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let source_name = source
        .file_name()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;

    let source_parent_path = source
        .parent()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    let source_parent_path = std::fs::canonicalize(source_parent_path)
        .map_err(|_| PlatformError::InvalidSource(source_parent_path.display().to_string()))?;
    let destination_dir = std::fs::canonicalize(destination_dir)
        .map_err(|_| PlatformError::InvalidDestination(destination_dir.display().to_string()))?;
    let source_parent = move_recovery::LinuxSourceParentDirectory::open(&source_parent_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let source_entry_metadata = std::fs::symlink_metadata(source).ok();
    let source_entry_identity = source_entry_metadata
        .as_ref()
        .map(stable_identity_from_metadata);
    let source_parent_metadata = std::fs::symlink_metadata(&source_parent_path)
        .map_err(|_| PlatformError::InvalidSource(source_parent_path.display().to_string()))?;
    let source_parent_identity = move_recovery::LinuxStableIdentity::new(
        source_parent_metadata.dev(),
        source_parent_metadata.ino(),
        if source_parent_metadata.is_dir() {
            move_recovery::LinuxFileType::Directory
        } else {
            move_recovery::LinuxFileType::Other
        },
    );

    let destination_parent_metadata = std::fs::symlink_metadata(&destination_dir)
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

    let destination_parent = move_recovery::LinuxSourceParentDirectory::open(&destination_dir)
        .map_err(|_| PlatformError::InvalidDestination(destination_dir.display().to_string()))?;

    if !is_cross_device
        && (conflict == ConflictAction::Skip || conflict == ConflictAction::Rename)
        && let Some(disposition) = recover_same_filesystem_pending_destination(
            source,
            &destination_dir,
            source_name,
            &source_parent,
            &source_parent_path,
            source_parent_identity,
            source_entry_identity,
            &destination_parent,
            conflict,
        )?
    {
        return Ok(disposition);
    }
    if is_cross_device
        && (conflict == ConflictAction::Skip || conflict == ConflictAction::Rename)
        && let Some(disposition) = recover_cross_filesystem_pending_destination(
            source,
            &destination_dir,
            source_name,
            &source_parent,
            &source_parent_path,
            source_parent_identity,
            source_entry_identity,
            &destination_parent,
            conflict,
        )?
    {
        return Ok(disposition);
    }

    reject_directory_copy_into_itself(source, &destination_dir)?;

    let source_metadata = source_entry_metadata
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    if !is_cross_device
        && destination_dir.join(source_name).exists()
        && conflict == ConflictAction::Skip
    {
        return Ok(MoveDisposition::Skipped);
    }
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

    // Cross-device Overwrite has no durable displaced-entry lifecycle yet. Decide that before
    // creating a recovery root or claiming the source so an unsupported request is truly
    // non-mutating for both the source and the occupied destination.
    if is_cross_device && conflict == ConflictAction::Overwrite {
        return Err(PlatformError::UnsupportedSafeCrossDeviceMove(
            source.display().to_string(),
        ));
    }

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
        &source_parent_path,
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
    let claimed_path = local_path.join("claimed");
    let mut destination_name = source_name.to_os_string();
    let mut destination_target = destination_dir.join(&destination_name);

    if is_cross_device {
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
                        expected_displaced_destination_identity: None,
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
            std::fs::File::open(&destination_dir)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("destination parent flush failed: {error}"),
                })?;

            let destination_transaction_directory =
                move_recovery::LinuxJournalDirectory::open_configured(
                    destination_transaction_path.as_path(),
                )
                .map_err(|error| PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("destination transaction reopen failed: {error}"),
                })?;
            match publish_claimed_to_destination_with_no_replace(
                &destination_transaction_directory,
                &destination_parent,
                std::ffi::OsStr::new(CROSS_DEVICE_STAGED_NAME),
                destination_name.as_os_str(),
            ) {
                Ok(()) => {
                    break;
                }
                Err(publish_error) if publish_error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if conflict == ConflictAction::Skip {
                        restore_claimed_to_source(
                            &source_parent,
                            &local,
                            source_name,
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
                        return Ok(MoveDisposition::Skipped);
                    }
                    if conflict == ConflictAction::Overwrite {
                        publish_claimed_to_destination_with_exchange(
                            &destination_transaction_directory,
                            &destination_parent,
                            std::ffi::OsStr::new(CROSS_DEVICE_STAGED_NAME),
                            destination_name.as_os_str(),
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
        return Ok(MoveDisposition::Moved);
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

        match publish_claimed_to_destination_with_no_replace(
            &local,
            &destination_parent,
            std::ffi::OsStr::new("claimed"),
            destination_name.as_os_str(),
        ) {
            Ok(()) => {
                break;
            }
            Err(publish_error) if publish_error.kind() == std::io::ErrorKind::AlreadyExists => {
                if conflict == ConflictAction::Skip {
                    restore_claimed_to_source(
                        &source_parent,
                        &local,
                        source_name,
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
                    return Ok(MoveDisposition::Skipped);
                }

                if conflict == ConflictAction::Overwrite {
                    publish_claimed_to_destination_with_exchange(
                        &local,
                        &destination_parent,
                        std::ffi::OsStr::new("claimed"),
                        destination_name.as_os_str(),
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
    Ok(MoveDisposition::Moved)
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn recover_same_filesystem_pending_destination(
    source: &Path,
    destination_dir: &Path,
    source_name: &std::ffi::OsStr,
    source_parent: &move_recovery::LinuxSourceParentDirectory,
    source_parent_path: &Path,
    source_parent_identity: move_recovery::LinuxStableIdentity,
    source_entry_identity: Option<move_recovery::LinuxStableIdentity>,
    destination_parent: &move_recovery::LinuxSourceParentDirectory,
    conflict: ConflictAction,
) -> Result<Option<MoveDisposition>, PlatformError> {
    let recovery_root = default_recovery_root()?;
    let discovered = discover_retained_move_transactions_at(&recovery_root)?;
    if !discovered
        .iter()
        .any(|entry| entry.phase == move_recovery::JournalPhase::PendingDestination)
    {
        return Ok(None);
    }

    let expected_source_name = Path::new(source_name);
    let central = move_recovery::LinuxJournalDirectory::open_configured_read_only(&recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut recovery = central
        .try_lock_recovery()
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    for entry in discovered {
        if entry.phase != move_recovery::JournalPhase::PendingDestination {
            continue;
        }

        let local_path = source_parent_path.join(format!(
            ".ottrin-move-{}",
            transaction_id_hex(&entry.transaction_id)
        ));
        let local =
            match move_recovery::LinuxJournalDirectory::open_configured_read_only(&local_path) {
                Ok(local) => local,
                Err(_) => continue,
            };

        let state =
            match central.load_pending_destination_restart_state(&local, entry.transaction_id) {
                Ok(state) => state,
                Err(_) => continue,
            };
        let pending = match state {
            move_recovery::LinuxPendingDestinationRestartState::MatchingPair(pending) => pending,
            move_recovery::LinuxPendingDestinationRestartState::CentralPendingDestinationLocalClaimed {
                pending,
                ..
            } => {
                recovery
                    .resume_pending_destination_with_local(&local, entry.transaction_id)
                    .map_err(|error| PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!(
                            "pending-destination restart persistence failed during recovery: {error}"
                        ),
                    })?;
                pending
            }
        };

        if pending.source_parent_identity() != source_parent_identity {
            continue;
        }
        if !pending.original_name_matches_path(expected_source_name) {
            continue;
        }
        if let Some(expected_identity) = source_entry_identity
            && pending.expected_source_identity() != expected_identity
        {
            continue;
        }
        if pending.has_cross_device_destination_state() {
            continue;
        }
        if let Some(disposition) = replay_same_filesystem_pending_destination_with_record(
            source,
            source_parent,
            destination_parent,
            destination_dir,
            &recovery_root,
            entry.transaction_id,
            &local_path,
            &local,
            &mut recovery,
            pending,
            conflict,
        )? {
            return Ok(Some(disposition));
        }
    }

    Ok(None)
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn recover_cross_filesystem_pending_destination(
    source: &Path,
    destination_dir: &Path,
    source_name: &std::ffi::OsStr,
    source_parent: &move_recovery::LinuxSourceParentDirectory,
    source_parent_path: &Path,
    source_parent_identity: move_recovery::LinuxStableIdentity,
    source_entry_identity: Option<move_recovery::LinuxStableIdentity>,
    destination_parent: &move_recovery::LinuxSourceParentDirectory,
    conflict: ConflictAction,
) -> Result<Option<MoveDisposition>, PlatformError> {
    let recovery_root = default_recovery_root()?;
    let discovered = discover_retained_move_transactions_at(&recovery_root)?;
    if !discovered
        .iter()
        .any(|entry| entry.phase == move_recovery::JournalPhase::PendingDestination)
    {
        return Ok(None);
    }

    let expected_source_name = Path::new(source_name);
    let central = move_recovery::LinuxJournalDirectory::open_configured_read_only(&recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut recovery = central
        .try_lock_recovery()
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    for entry in discovered {
        if entry.phase != move_recovery::JournalPhase::PendingDestination {
            continue;
        }

        let local_path = source_parent_path.join(format!(
            ".ottrin-move-{}",
            transaction_id_hex(&entry.transaction_id)
        ));
        let local =
            match move_recovery::LinuxJournalDirectory::open_configured_read_only(&local_path) {
                Ok(local) => local,
                Err(_) => continue,
            };

        let state =
            match central.load_pending_destination_restart_state(&local, entry.transaction_id) {
                Ok(state) => state,
                Err(_) => continue,
            };
        let pending = match state {
            move_recovery::LinuxPendingDestinationRestartState::MatchingPair(pending) => pending,
            move_recovery::LinuxPendingDestinationRestartState::CentralPendingDestinationLocalClaimed {
                pending,
                ..
            } => {
                recovery
                    .resume_pending_destination_with_local(&local, entry.transaction_id)
                    .map_err(|error| PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!(
                            "pending-destination restart persistence failed during recovery: {error}"
                        ),
                    })?;
                pending
            }
        };

        if pending.source_parent_identity() != source_parent_identity {
            continue;
        }
        if !pending.original_name_matches_path(expected_source_name) {
            continue;
        }
        if let Some(expected_identity) = source_entry_identity
            && pending.expected_source_identity() != expected_identity
        {
            continue;
        }
        if !pending.has_cross_device_destination_state() {
            continue;
        }
        if let Some(disposition) = replay_cross_filesystem_pending_destination_with_record(
            source,
            source_parent,
            destination_parent,
            destination_dir,
            &recovery_root,
            entry.transaction_id,
            &local_path,
            &local,
            &mut recovery,
            pending,
            conflict,
        )? {
            return Ok(Some(disposition));
        }
    }

    Ok(None)
}

#[cfg(target_os = "linux")]
pub(super) fn recover_retained_move_pending_destination(
    transaction_id: [u8; 16],
) -> Result<(), PlatformError> {
    let recovery_root = default_recovery_root()?;
    recover_retained_move_pending_destination_at(&recovery_root, transaction_id)
}

#[cfg(target_os = "linux")]
pub(super) fn recover_retained_move_pending_destination_at(
    recovery_root: &Path,
    transaction_id: [u8; 16],
) -> Result<(), PlatformError> {
    let transaction_id_hex = transaction_id_hex(&transaction_id);
    let discovered = discover_retained_move_transactions_at(recovery_root)?;
    if !discovered.iter().any(|entry| {
        entry.transaction_id == transaction_id
            && entry.phase == move_recovery::JournalPhase::PendingDestination
    }) {
        return Err(PlatformError::Io(format!(
            "retained Move transaction {transaction_id_hex} is not in pending destination state",
        )));
    }

    let central = move_recovery::LinuxJournalDirectory::open_configured(recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut recovery = central
        .try_lock_recovery()
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    let central_journal_path = recovery_root.join(format!("{transaction_id_hex}.journal"));
    let central_journal = std::fs::read(&central_journal_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let pending =
        move_recovery::PendingDestinationRecord::parse(&central_journal).map_err(|error| {
            PlatformError::Io(format!("failed to parse retained Move journal: {error}"))
        })?;

    let source_parent_path = pending.source_parent().map_err(|error| {
        PlatformError::Io(format!("failed to decode source parent hint: {error}"))
    })?;

    let source_parent_metadata = std::fs::symlink_metadata(&source_parent_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let source_parent_identity = move_recovery::LinuxStableIdentity::new(
        source_parent_metadata.dev(),
        source_parent_metadata.ino(),
        if source_parent_metadata.is_dir() {
            move_recovery::LinuxFileType::Directory
        } else {
            move_recovery::LinuxFileType::Other
        },
    );
    if source_parent_identity != pending.source_parent_identity() {
        return Err(PlatformError::Io(format!(
            "source parent identity changed for retained Move transaction {transaction_id_hex}"
        )));
    }

    let source_parent = move_recovery::LinuxSourceParentDirectory::open(&source_parent_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let local_path = source_parent_path.join(format!(".ottrin-move-{transaction_id_hex}"));
    let local = match move_recovery::LinuxJournalDirectory::open_configured(&local_path) {
        Ok(local) => local,
        Err(error) => {
            return Err(PlatformError::Io(format!(
                "unable to open source transaction directory for {transaction_id_hex}: {error}"
            )));
        }
    };

    let pending = match central
        .load_pending_destination_restart_state(&local, transaction_id)
        .map_err(|error| PlatformError::Io(format!("failed to load pending destination state: {error}")))?
    {
        move_recovery::LinuxPendingDestinationRestartState::MatchingPair(pending) => pending,
        move_recovery::LinuxPendingDestinationRestartState::CentralPendingDestinationLocalClaimed {
            pending,
            ..
        } => {
            recovery
                .resume_pending_destination_with_local(&local, transaction_id)
                .map_err(|error| PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("recovery restart persistence failed: {error}"),
                })?;
            pending
        }
    };

    let source_name = pending
        .original_name()
        .map_err(|error| PlatformError::Io(format!("failed to decode original name: {error}")))?;
    let source_name = source_name.file_name().ok_or_else(|| {
        PlatformError::Io("invalid original source path for retained Move entry".into())
    })?;
    let claimed_path = local_path.join("claimed");
    let claimed_metadata = std::fs::symlink_metadata(&claimed_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    if stable_identity_from_metadata(&claimed_metadata) != pending.claimed_source_identity() {
        return Err(PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: "claimed identity no longer matches retained recovery evidence".to_string(),
        });
    }

    restore_claimed_to_source(&source_parent, &local, source_name).map_err(|error| {
        PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: format!("restore claimed source failed: {error}"),
        }
    })?;

    let _ = cleanup_transaction_journals(recovery_root, &local_path, &transaction_id);
    Ok(())
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn replay_cross_filesystem_pending_destination_with_record(
    source: &Path,
    source_parent: &move_recovery::LinuxSourceParentDirectory,
    destination_parent: &move_recovery::LinuxSourceParentDirectory,
    destination_dir: &Path,
    recovery_root: &Path,
    transaction_id: [u8; 16],
    local_path: &Path,
    local: &move_recovery::LinuxJournalDirectory,
    recovery: &mut move_recovery::LinuxRecoveryLock<'_>,
    pending: move_recovery::PendingDestinationRecord,
    conflict: ConflictAction,
) -> Result<Option<MoveDisposition>, PlatformError> {
    let source_name = pending
        .original_name()
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let claimed_source_identity = pending.claimed_source_identity();
    let destination_transaction_name = pending.destination_transaction_name().map_err(|error| {
        PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: format!("cross-device destination transaction name decode failed: {error}"),
        }
    })?;
    let destination_transaction_identity =
        pending.destination_transaction_identity().ok_or_else(|| {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: "cross-device destination transaction identity is missing".to_string(),
            }
        })?;
    let source_tree_snapshot =
        pending
            .source_tree_snapshot()
            .ok_or_else(|| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: "cross-device source-tree snapshot is missing".to_string(),
            })?;
    let staged_destination_identity = pending.staged_destination_identity().ok_or_else(|| {
        PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: "cross-device staged destination identity is missing".to_string(),
        }
    })?;
    let mut destination_name = pending
        .pending_destination_name()
        .map_err(|error| PlatformError::Io(error.to_string()))?
        .file_name()
        .expect("pending destination has a file name")
        .to_os_string();
    let mut destination_target = destination_dir.join(&destination_name);
    let destination_transaction_path = destination_dir.join(&destination_transaction_name);
    let claimed_path = local_path.join("claimed");

    let pending_destination_metadata = move_recovery::PendingDestinationMetadata {
        destination_transaction_name: Some(destination_transaction_name.clone()),
        destination_transaction_identity: Some(destination_transaction_identity),
        source_tree_snapshot: Some(source_tree_snapshot.clone()),
        staged_destination_identity: Some(staged_destination_identity),
        expected_displaced_destination_identity: None,
    };

    loop {
        match std::fs::symlink_metadata(&claimed_path) {
            Ok(metadata) => {
                if stable_identity_from_metadata(&metadata) != claimed_source_identity {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message:
                            "recovered cross-device claimed identity changed before publication"
                                .to_string(),
                    });
                }
                let destination_transaction_metadata =
                    std::fs::symlink_metadata(&destination_transaction_path).map_err(|error| {
                        PlatformError::CommittedCleanupPending {
                            transaction: local_path.display().to_string(),
                            message: format!(
                                "recovery destination transaction metadata failed: {error}"
                            ),
                        }
                    })?;
                if stable_identity_from_metadata(&destination_transaction_metadata)
                    != destination_transaction_identity
                {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message:
                            "recovery destination transaction identity changed before publication"
                                .to_string(),
                    });
                }
                let staged_destination_metadata = std::fs::symlink_metadata(
                    destination_transaction_path.join(CROSS_DEVICE_STAGED_NAME),
                )
                .map_err(|error| PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!(
                        "recovery staged destination metadata unavailable before publication: {error}"
                    ),
                })?;
                if stable_identity_from_metadata(&staged_destination_metadata)
                    != staged_destination_identity
                {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: "recovery staged destination identity changed before publication"
                            .to_string(),
                    });
                }

                let destination_transaction_directory =
                    move_recovery::LinuxJournalDirectory::open_configured(
                        &destination_transaction_path,
                    )
                    .map_err(|error| {
                        PlatformError::Io(format!(
                            "recovery could not reopen destination transaction directory: {error}"
                        ))
                    })?;
                match publish_claimed_to_destination_with_no_replace(
                    &destination_transaction_directory,
                    destination_parent,
                    std::ffi::OsStr::new(CROSS_DEVICE_STAGED_NAME),
                    destination_name.as_os_str(),
                ) {
                    Ok(()) => {
                        let destination_metadata = std::fs::symlink_metadata(&destination_target)
                            .map_err(|error| {
                            PlatformError::CommittedCleanupPending {
                                transaction: local_path.display().to_string(),
                                message: format!(
                                    "cross-device destination identity check failed: {error}"
                                ),
                            }
                        })?;
                        if stable_identity_from_metadata(&destination_metadata)
                            != staged_destination_identity
                        {
                            return Err(PlatformError::CommittedCleanupPending {
                                transaction: local_path.display().to_string(),
                                message:
                                    "cross-device destination identity changed after publication"
                                        .to_string(),
                            });
                        }
                        if !verify_source_tree_snapshot(&claimed_path, &source_tree_snapshot)
                            .map_err(|error| PlatformError::Io(error.to_string()))?
                        {
                            return Err(PlatformError::CommittedCleanupPending {
                                transaction: local_path.display().to_string(),
                                message: "cross-device source snapshot changed before cleanup"
                                    .to_string(),
                            });
                        }

                        recovery
                            .advance_to_published_with_local(local, transaction_id)
                            .map_err(|error| PlatformError::CommittedCleanupPending {
                                transaction: local_path.display().to_string(),
                                message: format!(
                                    "cross-device publication durable state failed: {error}"
                                ),
                            })?;
                        remove_source_entry(&claimed_path).map_err(|error| {
                            PlatformError::CommittedCleanupPending {
                                transaction: local_path.display().to_string(),
                                message: format!("cross-device source cleanup failed: {error}"),
                            }
                        })?;
                        let _ = std::fs::remove_dir_all(&destination_transaction_path);
                        let _ = cleanup_transaction_journals(
                            recovery_root,
                            local_path,
                            &transaction_id,
                        );
                        return Ok(Some(MoveDisposition::Moved));
                    }
                    Err(publish_error)
                        if publish_error.kind() == std::io::ErrorKind::AlreadyExists =>
                    {
                        let destination_metadata = match std::fs::symlink_metadata(
                            &destination_target,
                        ) {
                            Ok(metadata) => metadata,
                            Err(error) => {
                                return Err(PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "cross-device destination conflict check failed: {error}"
                                    ),
                                });
                            }
                        };
                        if stable_identity_from_metadata(&destination_metadata)
                            == staged_destination_identity
                        {
                            if !verify_source_tree_snapshot(&claimed_path, &source_tree_snapshot)
                                .map_err(|error| PlatformError::Io(error.to_string()))?
                            {
                                return Err(PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: "cross-device source snapshot changed before cleanup"
                                        .to_string(),
                                });
                            }
                            recovery
                                .advance_to_published_with_local(local, transaction_id)
                                .map_err(|error| PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "cross-device publication durable state failed: {error}"
                                    ),
                                })?;
                            remove_source_entry(&claimed_path).map_err(|error| {
                                PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!("cross-device source cleanup failed: {error}"),
                                }
                            })?;
                            let _ = std::fs::remove_dir_all(&destination_transaction_path);
                            let _ = cleanup_transaction_journals(
                                recovery_root,
                                local_path,
                                &transaction_id,
                            );
                            return Ok(Some(MoveDisposition::Moved));
                        }

                        if conflict == ConflictAction::Skip {
                            if source.exists() {
                                return Err(PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message:
                                        "source reappeared before recovery skip-mode restoration"
                                            .to_string(),
                                });
                            }

                            restore_claimed_to_source(
                                source_parent,
                                local,
                                source_name.file_name().unwrap_or_default(),
                            )
                            .map_err(|restore_error| {
                                PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "skip-mode recovery restoration failed: {restore_error}"
                                    ),
                                }
                            })?;
                            let _ = cleanup_transaction_journals(
                                recovery_root,
                                local_path,
                                &transaction_id,
                            );
                            return Ok(Some(MoveDisposition::Skipped));
                        }

                        if conflict == ConflictAction::Rename {
                            destination_name = next_available_name(&destination_target)
                                .file_name()
                                .expect("candidate has a file name")
                                .to_os_string();
                            destination_target = destination_dir.join(&destination_name);
                            let displaced_destination_identity =
                                stable_identity_from_metadata(&destination_metadata);
                            recovery
                                .advance_to_pending_destination_with_local_with_metadata(
                                    local,
                                    transaction_id,
                                    destination_name.as_ref(),
                                    move_recovery::PendingDestinationMetadata {
                                        expected_displaced_destination_identity: Some(
                                            displaced_destination_identity,
                                        ),
                                        ..pending_destination_metadata.clone()
                                    },
                                )
                                .map_err(|error| PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "recovery pending-destination advancement failed: {error}"
                                    ),
                                })?;
                            continue;
                        }

                        return Err(PlatformError::CommittedCleanupPending {
                            transaction: local_path.display().to_string(),
                            message: "recovery publication conflict for unsupported mode"
                                .to_string(),
                        });
                    }
                    Err(publish_error) => {
                        return Err(PlatformError::CommittedCleanupPending {
                            transaction: local_path.display().to_string(),
                            message: format!("recovery publication failed: {publish_error}"),
                        });
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: "recovery found no claimed source after cross-device publication"
                        .to_string(),
                });
            }
            Err(error) => {
                return Err(PlatformError::Io(error.to_string()));
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn replay_same_filesystem_pending_destination_with_record(
    source: &Path,
    source_parent: &move_recovery::LinuxSourceParentDirectory,
    destination_parent: &move_recovery::LinuxSourceParentDirectory,
    destination_dir: &Path,
    recovery_root: &Path,
    transaction_id: [u8; 16],
    local_path: &Path,
    local: &move_recovery::LinuxJournalDirectory,
    recovery: &mut move_recovery::LinuxRecoveryLock<'_>,
    pending: move_recovery::PendingDestinationRecord,
    conflict: ConflictAction,
) -> Result<Option<MoveDisposition>, PlatformError> {
    let source_name = pending
        .original_name()
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let claimed_source_identity = pending.claimed_source_identity();
    let mut destination_name = pending
        .pending_destination_name()
        .map_err(|error| PlatformError::Io(error.to_string()))?
        .file_name()
        .expect("pending destination has a file name")
        .to_os_string();
    let mut destination_target = destination_dir.join(&destination_name);
    let claimed_path = local_path.join("claimed");

    loop {
        match std::fs::symlink_metadata(&claimed_path) {
            Ok(metadata) => {
                if stable_identity_from_metadata(&metadata) != claimed_source_identity {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: "recovered claimed identity changed before publication"
                            .to_string(),
                    });
                }

                match publish_claimed_to_destination_with_no_replace(
                    local,
                    destination_parent,
                    std::ffi::OsStr::new("claimed"),
                    destination_name.as_os_str(),
                ) {
                    Ok(()) => {
                        recovery
                            .advance_to_published_with_local(local, transaction_id)
                            .map_err(|error| PlatformError::CommittedCleanupPending {
                                transaction: local_path.display().to_string(),
                                message: format!(
                                    "recovery publication durable state failed: {error}"
                                ),
                            })?;
                        let _ = cleanup_transaction_journals(
                            recovery_root,
                            local_path,
                            &transaction_id,
                        );
                        return Ok(Some(MoveDisposition::Moved));
                    }
                    Err(publish_error)
                        if publish_error.kind() == std::io::ErrorKind::AlreadyExists =>
                    {
                        let destination_metadata =
                            match std::fs::symlink_metadata(&destination_target) {
                                Ok(metadata) => metadata,
                                Err(error) => {
                                    return Err(PlatformError::CommittedCleanupPending {
                                        transaction: local_path.display().to_string(),
                                        message: format!(
                                            "recovery destination conflict check failed: {error}"
                                        ),
                                    });
                                }
                            };
                        if stable_identity_from_metadata(&destination_metadata)
                            == claimed_source_identity
                        {
                            recovery
                                .advance_to_published_with_local(local, transaction_id)
                                .map_err(|error| PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "recovery publication durable state failed: {error}"
                                    ),
                                })?;
                            let _ = cleanup_transaction_journals(
                                recovery_root,
                                local_path,
                                &transaction_id,
                            );
                            return Ok(Some(MoveDisposition::Moved));
                        }

                        if conflict == ConflictAction::Skip {
                            if source.exists() {
                                return Err(PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message:
                                        "source reappeared before recovery skip-mode restoration"
                                            .to_string(),
                                });
                            }

                            restore_claimed_to_source(
                                source_parent,
                                local,
                                source_name.file_name().unwrap_or_default(),
                            )
                            .map_err(|restore_error| {
                                PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "skip-mode recovery restoration failed: {restore_error}"
                                    ),
                                }
                            })?;
                            let _ = cleanup_transaction_journals(
                                recovery_root,
                                local_path,
                                &transaction_id,
                            );
                            return Ok(Some(MoveDisposition::Skipped));
                        }

                        if conflict == ConflictAction::Rename {
                            destination_name = next_available_name(&destination_target)
                                .file_name()
                                .expect("candidate has a file name")
                                .to_os_string();
                            destination_target = destination_dir.join(&destination_name);
                            recovery
                                .advance_to_pending_destination_with_local(
                                    local,
                                    transaction_id,
                                    &destination_target,
                                )
                                .map_err(|error| PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "recovery pending-destination advancement failed: {error}"
                                    ),
                                })?;
                            continue;
                        }

                        return Err(PlatformError::CommittedCleanupPending {
                            transaction: local_path.display().to_string(),
                            message: "recovery publication conflict for unsupported mode"
                                .to_string(),
                        });
                    }
                    Err(publish_error) => {
                        return Err(PlatformError::CommittedCleanupPending {
                            transaction: local_path.display().to_string(),
                            message: format!("recovery publication failed: {publish_error}"),
                        });
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let destination_metadata = match std::fs::symlink_metadata(&destination_target) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        return Err(PlatformError::CommittedCleanupPending {
                            transaction: local_path.display().to_string(),
                            message: format!(
                                "recovery destination check failed after publication: {error}"
                            ),
                        });
                    }
                };
                if stable_identity_from_metadata(&destination_metadata) != claimed_source_identity {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message:
                            "recovery destination identity does not match claimed source identity"
                                .to_string(),
                    });
                }
                recovery
                    .advance_to_published_with_local(local, transaction_id)
                    .map_err(|error| PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!("recovery publication durable state failed: {error}"),
                    })?;
                let _ = cleanup_transaction_journals(recovery_root, local_path, &transaction_id);
                return Ok(Some(MoveDisposition::Moved));
            }
            Err(error) => {
                return Err(PlatformError::Io(error.to_string()));
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn transaction_id_hex(transaction_id: &[u8; 16]) -> String {
    transaction_id
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(target_os = "linux")]
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
    claimed: &move_recovery::LinuxJournalDirectory,
    destination_parent: &move_recovery::LinuxSourceParentDirectory,
    source_name: &std::ffi::OsStr,
    destination_name: &std::ffi::OsStr,
) -> std::io::Result<()> {
    claimed.rename_to_parent_no_replace(source_name, destination_parent, destination_name)
}

#[cfg(target_os = "linux")]
fn publish_claimed_to_destination_with_exchange(
    claimed: &move_recovery::LinuxJournalDirectory,
    destination_parent: &move_recovery::LinuxSourceParentDirectory,
    source_name: &std::ffi::OsStr,
    destination_name: &std::ffi::OsStr,
) -> std::io::Result<()> {
    claimed.rename_to_parent_exchange(source_name, destination_parent, destination_name)
}

#[cfg(target_os = "linux")]
fn restore_claimed_to_source(
    source_parent: &move_recovery::LinuxSourceParentDirectory,
    claimed: &move_recovery::LinuxJournalDirectory,
    source_name: &std::ffi::OsStr,
) -> Result<(), String> {
    claimed
        .rename_to_parent_no_replace(std::ffi::OsStr::new("claimed"), source_parent, source_name)
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

#[cfg(target_os = "linux")]
fn default_recovery_root() -> Result<std::path::PathBuf, PlatformError> {
    #[cfg(test)]
    if let Some(root) = TEST_RECOVERY_ROOT.with(|root| root.borrow().clone()) {
        return Ok(root);
    }

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

#[cfg(all(target_os = "linux", test))]
thread_local! {
    static TEST_RECOVERY_ROOT: std::cell::RefCell<Option<std::path::PathBuf>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(all(target_os = "linux", test))]
pub(super) fn set_test_recovery_root(root: Option<std::path::PathBuf>) {
    TEST_RECOVERY_ROOT.with(|configured_root| *configured_root.borrow_mut() = root);
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
) -> Result<MoveDisposition, PlatformError> {
    move_with_hooks(source, destination_dir, conflict, hook)
}

#[cfg(all(test, target_os = "linux"))]
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn with_move_env_serializer<T>(work: impl FnOnce() -> T) -> T {
        let _guard = TEST_MOVE_ENV_SERIALIZER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        work()
    }

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

    fn is_move_transaction_directory(name: &std::ffi::OsStr) -> bool {
        let name = name.to_string_lossy();
        const PREFIX: &str = ".ottrin-move-";
        name.len() == PREFIX.len() + 32
            && name.starts_with(PREFIX)
            && name[PREFIX.len()..]
                .chars()
                .all(|char| char.is_ascii_hexdigit())
    }

    fn has_local_transaction_dir(root: &Path) -> bool {
        std::fs::read_dir(root)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.path().is_dir() && is_move_transaction_directory(&entry.file_name()))
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

    #[test]
    fn cross_filesystem_overwrite_is_rejected_before_claiming_source_or_destination() {
        let dir = TestDir::new("overwrite-rejected-before-claim");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "source payload").unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("source"), "displaced destination payload").unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Overwrite,
            |point| match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                _ => Ok(()),
            },
        );

        assert!(matches!(
            result,
            Err(PlatformError::UnsupportedSafeCrossDeviceMove(_))
        ));
        assert_eq!(std::fs::read(&source).unwrap(), b"source payload");
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"displaced destination payload"
        );
        assert!(std::fs::read_dir(&dir.0).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".ottrin-move-")
        }));
    }

    #[test]
    fn same_filesystem_publication_uses_verified_destination_parent_after_destination_alias_retarget()
     {
        let dir = TestDir::new("destination-handle-rebind");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        let retired = dir.0.join("destination-retired");
        let attacker = dir.0.join("attacker");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::create_dir(&retired).unwrap();
        std::fs::create_dir(&attacker).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| {
                if point == TestPoint::PendingDestination {
                    std::fs::rename(&destination, &retired).unwrap();
                    std::fs::rename(&attacker, &destination).unwrap();
                }
                Ok(())
            },
        );
        assert!(result.is_ok(), "move failed: {result:?}");
        assert_eq!(std::fs::read(retired.join("source")).unwrap(), b"payload");
        assert!(!destination.join("source").exists());
        assert!(!source.exists());
    }

    #[test]
    fn same_filesystem_restore_uses_verified_source_parent_after_source_alias_retarget() {
        let dir = TestDir::new("source-handle-rebind");
        let source_parent_real = dir.0.join("source-parent");
        let source_parent_alias = dir.0.join("source-parent-link");
        let source = source_parent_alias.join("source");
        let destination = dir.0.join("destination");
        let attacker = dir.0.join("attacker");
        std::fs::create_dir(&source_parent_real).unwrap();
        std::fs::create_dir(&attacker).unwrap();
        std::fs::create_dir(&destination).unwrap();
        use std::os::unix::fs::symlink;
        symlink(&source_parent_real, &source_parent_alias).unwrap();
        std::fs::write(&source, "payload").unwrap();
        std::fs::write(destination.join("source"), "occupied").unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| {
                if point == TestPoint::PendingDestination {
                    let _ = std::fs::remove_file(&source_parent_alias);
                    symlink(&attacker, &source_parent_alias).unwrap();
                }
                Ok(())
            },
        );
        assert!(result.is_ok());
        assert_eq!(
            std::fs::read(source_parent_real.join("source")).unwrap(),
            b"payload"
        );
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"occupied"
        );
        assert!(!attacker.join("source").exists());
    }

    #[cfg(target_os = "linux")]
    fn run_publication_sigkill_child(root: &Path) {
        use rustix::process;

        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir(&destination).unwrap();
        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| {
                if point == TestPoint::Publication {
                    let _ = process::kill_process(process::getpid(), process::Signal::Kill);
                    panic!("publication child was not terminated");
                } else {
                    Ok(())
                }
            },
        );
        panic!("publication move returned {result:?} without being terminated");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn same_filesystem_publication_kill_child_process_after_publication_preserves_volatile_state() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        with_move_env_serializer(|| {
            if std::env::var_os("OTTRIN_MOVE_PUBLICATION_SIGKILL_CHILD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                let root = std::env::var_os("OTTRIN_MOVE_PUBLICATION_SIGKILL_ROOT").unwrap();
                run_publication_sigkill_child(Path::new(&root));
                panic!("publication kill child returned without being terminated");
            }

            let root = std::env::temp_dir().join(format!(
                "ottrin-move-publication-kill-{}",
                getrandom::u64().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let state_root = root.join("state");
            std::fs::create_dir_all(&state_root).unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_transaction::tests::same_filesystem_publication_kill_child_process_after_publication_preserves_volatile_state",
                "--test-threads=1",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_PUBLICATION_SIGKILL_CHILD", "1")
            .env("OTTRIN_MOVE_PUBLICATION_SIGKILL_ROOT", &root)
            .env("XDG_STATE_HOME", &state_root)
            .status()
            .unwrap();
            assert_eq!(status.signal(), Some(9));
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"payload"
            );
            assert!(!std::fs::exists(root.join("source")).unwrap_or_default());
            let has_central_journal_after_kill =
                std::fs::read_dir(state_root.join("ottrin").join("moves"))
                    .unwrap()
                    .filter_map(Result::ok)
                    .any(|entry| {
                        entry
                            .path()
                            .extension()
                            .is_some_and(|ext| ext == std::ffi::OsStr::new("journal"))
                    });
            assert!(has_central_journal_after_kill);
            let previous_state_home = std::env::var_os("XDG_STATE_HOME");
            unsafe {
                std::env::set_var("XDG_STATE_HOME", &state_root);
            }
            assert!(has_local_transaction_dir(&root));

            let recovery = move_entry_transactional_with_test_hook(
                &root.join("source"),
                &root.join("destination"),
                ConflictAction::Skip,
                |_| Ok(()),
            );
            assert!(recovery.is_ok(), "recovery retry failed: {recovery:?}");
            if let Some(previous_state_home) = previous_state_home {
                unsafe {
                    std::env::set_var("XDG_STATE_HOME", previous_state_home);
                }
            } else {
                unsafe {
                    std::env::remove_var("XDG_STATE_HOME");
                }
            }
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"payload"
            );
            assert!(!std::fs::exists(root.join("source")).unwrap_or_default());
            assert!(!has_local_transaction_dir(&root));

            let has_central_journal = std::fs::read_dir(state_root.join("ottrin").join("moves"))
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext == std::ffi::OsStr::new("journal"))
                });
            assert!(!has_central_journal);
        });
    }

    #[cfg(target_os = "linux")]
    fn run_pending_destination_sigkill_child(root: &Path) {
        use rustix::process;

        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        std::fs::write(destination.join("source"), b"occupied").unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Rename,
            |point| {
                if point == TestPoint::PendingDestination {
                    let _ = process::kill_process(process::getpid(), process::Signal::Kill);
                    panic!("pending destination child was not terminated");
                } else {
                    Ok(())
                }
            },
        );
        panic!("pending destination move returned {result:?} without being terminated");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn same_filesystem_rename_pending_destination_kill_child_process_replays_to_next_available_name()
     {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        with_move_env_serializer(|| {
            if std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_CHILD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                let root =
                    std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_ROOT").unwrap();
                run_pending_destination_sigkill_child(Path::new(&root));
                panic!("pending destination kill child returned without being terminated");
            }

            let root = std::env::temp_dir().join(format!(
                "ottrin-move-pending-destination-kill-{}",
                getrandom::u64().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let state_root = root.join("state");
            std::fs::create_dir_all(&state_root).unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_transaction::tests::same_filesystem_rename_pending_destination_kill_child_process_replays_to_next_available_name",
                "--test-threads=1",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_CHILD", "1")
            .env("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_ROOT", &root)
            .env("XDG_STATE_HOME", &state_root)
            .status()
            .unwrap();
            assert_eq!(status.signal(), Some(9));
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"occupied"
            );
            assert!(!std::fs::exists(root.join("source")).unwrap_or_default());
            let previous_state_home = std::env::var_os("XDG_STATE_HOME");
            unsafe {
                std::env::set_var("XDG_STATE_HOME", &state_root);
            }

            let recovery = move_entry_transactional_with_test_hook(
                &root.join("source"),
                &root.join("destination"),
                ConflictAction::Rename,
                |_| Ok(()),
            );
            assert!(recovery.is_ok(), "recovery retry failed: {recovery:?}");
            if let Some(previous_state_home) = previous_state_home {
                unsafe {
                    std::env::set_var("XDG_STATE_HOME", previous_state_home);
                }
            } else {
                unsafe {
                    std::env::remove_var("XDG_STATE_HOME");
                }
            }
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"occupied"
            );
            assert_eq!(
                std::fs::read(root.join("destination").join("source (1)")).unwrap(),
                b"payload"
            );

            assert!(!has_local_transaction_dir(&root));

            let has_central_journal = std::fs::read_dir(state_root.join("ottrin").join("moves"))
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext == std::ffi::OsStr::new("journal"))
                });
            assert!(!has_central_journal);
        });
    }

    #[cfg(target_os = "linux")]
    fn run_cross_filesystem_pending_destination_sigkill_child(
        root: &Path,
        conflict: ConflictAction,
    ) {
        use rustix::process;

        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        std::fs::write(destination.join("source"), b"occupied").unwrap();

        let result =
            move_entry_transactional_with_test_hook(&source, &destination, conflict, |point| {
                if point == TestPoint::ClassifyCrossDevice {
                    Err("force cross-device move")
                } else if point == TestPoint::PendingDestination {
                    let _ = process::kill_process(process::getpid(), process::Signal::Kill);
                    panic!("cross-device pending destination child was not terminated");
                } else {
                    Ok(())
                }
            });
        panic!(
            "cross-device pending destination move returned {result:?} without being terminated"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_filesystem_rename_pending_destination_kill_child_process_replays_to_next_available_name()
     {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        with_move_env_serializer(|| {
            if std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_CHILD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                let root =
                    std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_ROOT").unwrap();
                run_cross_filesystem_pending_destination_sigkill_child(
                    Path::new(&root),
                    ConflictAction::Rename,
                );
                panic!(
                    "cross-device pending destination kill child returned without being terminated"
                );
            }

            let root = std::env::temp_dir().join(format!(
                "ottrin-move-cross-pending-destination-kill-{}",
                getrandom::u64().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let state_root = root.join("state");
            std::fs::create_dir_all(&state_root).unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_transaction::tests::cross_filesystem_rename_pending_destination_kill_child_process_replays_to_next_available_name",
                "--test-threads=1",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_CHILD", "1")
            .env("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_ROOT", &root)
            .env("XDG_STATE_HOME", &state_root)
            .status()
            .unwrap();
            assert_eq!(status.signal(), Some(9));
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"occupied"
            );
            assert!(!std::fs::exists(root.join("source")).unwrap_or_default());
            let previous_state_home = std::env::var_os("XDG_STATE_HOME");
            unsafe {
                std::env::set_var("XDG_STATE_HOME", &state_root);
            }

            let recovery = move_entry_transactional_with_test_hook(
                &root.join("source"),
                &root.join("destination"),
                ConflictAction::Rename,
                |point| {
                    if point == TestPoint::ClassifyCrossDevice {
                        Err("force cross-device move")
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(recovery.is_ok(), "recovery retry failed: {recovery:?}");
            if let Some(previous_state_home) = previous_state_home {
                unsafe {
                    std::env::set_var("XDG_STATE_HOME", previous_state_home);
                }
            } else {
                unsafe {
                    std::env::remove_var("XDG_STATE_HOME");
                }
            }
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"occupied"
            );
            assert_eq!(
                std::fs::read(root.join("destination").join("source (1)")).unwrap(),
                b"payload"
            );
            assert!(!std::fs::exists(root.join("source")).unwrap_or_default());
            assert!(!has_local_transaction_dir(&root));

            let has_central_journal = std::fs::read_dir(state_root.join("ottrin").join("moves"))
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext == std::ffi::OsStr::new("journal"))
                });
            assert!(!has_central_journal);
        });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_filesystem_skip_pending_destination_kill_child_process_restores_source_on_recovery() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        with_move_env_serializer(|| {
            if std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_SKIP_SIGKILL_CHILD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                let root =
                    std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_SKIP_SIGKILL_ROOT").unwrap();
                run_cross_filesystem_pending_destination_sigkill_child(
                    Path::new(&root),
                    ConflictAction::Skip,
                );
                panic!(
                    "cross-device skip pending destination kill child returned without being terminated"
                );
            }

            let root = std::env::temp_dir().join(format!(
                "ottrin-move-cross-pending-destination-skip-kill-{}",
                getrandom::u64().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let state_root = root.join("state");
            std::fs::create_dir_all(&state_root).unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_transaction::tests::cross_filesystem_skip_pending_destination_kill_child_process_restores_source_on_recovery",
                "--test-threads=1",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_PENDING_DESTINATION_SKIP_SIGKILL_CHILD", "1")
            .env("OTTRIN_MOVE_PENDING_DESTINATION_SKIP_SIGKILL_ROOT", &root)
            .env("XDG_STATE_HOME", &state_root)
            .status()
            .unwrap();
            assert_eq!(status.signal(), Some(9));
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"occupied"
            );
            assert!(!std::fs::exists(root.join("source")).unwrap_or_default());

            let previous_state_home = std::env::var_os("XDG_STATE_HOME");
            unsafe {
                std::env::set_var("XDG_STATE_HOME", &state_root);
            }
            let recovery = move_entry_transactional_with_test_hook(
                &root.join("source"),
                &root.join("destination"),
                ConflictAction::Skip,
                |point| {
                    if point == TestPoint::ClassifyCrossDevice {
                        Err("force cross-device move")
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(recovery.is_ok(), "recovery retry failed: {recovery:?}");
            if let Some(previous_state_home) = previous_state_home {
                unsafe {
                    std::env::set_var("XDG_STATE_HOME", previous_state_home);
                }
            } else {
                unsafe {
                    std::env::remove_var("XDG_STATE_HOME");
                }
            }
            assert_eq!(std::fs::read(root.join("source")).unwrap(), b"payload");
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"occupied"
            );
            assert!(!has_local_transaction_dir(&root));

            let has_central_journal = std::fs::read_dir(state_root.join("ottrin").join("moves"))
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext == std::ffi::OsStr::new("journal"))
                });
            assert!(!has_central_journal);
        });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_filesystem_recovery_requires_destination_transaction() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        with_move_env_serializer(|| {
            if std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_VANISHED_SIGKILL_CHILD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                let root =
                    std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_VANISHED_SIGKILL_ROOT")
                        .unwrap();
                run_cross_filesystem_pending_destination_sigkill_child(
                    Path::new(&root),
                    ConflictAction::Rename,
                );
                panic!(
                    "cross-device vanished destination tx kill child returned without being terminated"
                );
            }

            let root = std::env::temp_dir().join(format!(
                "ottrin-move-cross-pending-destination-vanish-{}",
                getrandom::u64().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let state_root = root.join("state");
            std::fs::create_dir_all(&state_root).unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_transaction::tests::cross_filesystem_recovery_requires_destination_transaction",
                "--test-threads=1",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_PENDING_DESTINATION_VANISHED_SIGKILL_CHILD", "1")
            .env("OTTRIN_MOVE_PENDING_DESTINATION_VANISHED_SIGKILL_ROOT", &root)
            .env("XDG_STATE_HOME", &state_root)
            .status()
            .unwrap();
            assert_eq!(status.signal(), Some(9));

            for entry in std::fs::read_dir(root.join("destination"))
                .unwrap()
                .filter_map(Result::ok)
            {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".ottrin-move-")
                {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }

            let previous_state_home = std::env::var_os("XDG_STATE_HOME");
            unsafe {
                std::env::set_var("XDG_STATE_HOME", &state_root);
            }
            let recovery = move_entry_transactional_with_test_hook(
                &root.join("source"),
                &root.join("destination"),
                ConflictAction::Rename,
                |point| {
                    if point == TestPoint::ClassifyCrossDevice {
                        Err("force cross-device move")
                    } else {
                        Ok(())
                    }
                },
            );
            if let Some(previous_state_home) = previous_state_home {
                unsafe {
                    std::env::set_var("XDG_STATE_HOME", previous_state_home);
                }
            } else {
                unsafe {
                    std::env::remove_var("XDG_STATE_HOME");
                }
            }
            assert!(matches!(
                recovery,
                Err(PlatformError::CommittedCleanupPending { .. })
            ));
            assert!(!std::fs::exists(root.join("source")).unwrap_or_default());
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"occupied"
            );
            assert!(has_local_transaction_dir(&root));
        });
    }
}
