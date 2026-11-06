//! Authenticated helper routing. Wire IDs select fresh canonical journal plans;
//! neither request paths nor response history carry recovery authority.
use super::{copy_response, failure, selected};
use crate::windows_privileged_transport::CallerCopyContext;
use crate::{PlatformError, WindowsCopyPhase, WindowsCopyRecoveryEvidence, WindowsCopyRegistry};
use ottrin_core::{
    FileCommand, PrivilegedCommand, PrivilegedCopyHistoryEntry, PrivilegedCopyHistoryState,
    PrivilegedCopyReceipt, PrivilegedCopyRecoveryAction, PrivilegedPayload, PrivilegedResponse,
    PrivilegedStatus,
};
use std::{collections::BTreeSet, sync::Arc};

pub(crate) fn execute(
    caller: Arc<CallerCopyContext>,
    command: &PrivilegedCommand,
    progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
) -> PrivilegedResponse {
    let result = match command {
        PrivilegedCommand::File(FileCommand::Copy {
            sources,
            destination,
            conflict,
        }) => {
            if sources.is_empty() || sources.len() > crate::copy_transaction::MAX_COPY_ENTRIES {
                return failure(
                    "Administrator Copy needs a bounded nonempty source selection".into(),
                );
            }
            return copy_response(crate::execute_copy_batch_with(sources, |source| {
                progress(0)?;
                caller.copy_entry_with_progress(
                    source,
                    destination,
                    *conflict,
                    crate::copy_transaction::MAX_COPY_ENTRIES,
                    &mut *progress,
                )
            }));
        }
        PrivilegedCommand::DiscoverCopyHistory => history(&caller, progress)
            .map(PrivilegedPayload::CopyHistory)
            .map(Some),
        PrivilegedCommand::UndoCopy { transactions } => {
            undo(&caller, transactions, progress).map(|()| None)
        }
        PrivilegedCommand::CleanupCopy { transactions } => {
            cleanup(&caller, transactions, progress).map(|()| None)
        }
        PrivilegedCommand::RecoverCopy {
            transaction,
            action,
        } => recover(&caller, transaction, *action, progress).map(|()| None),
        _ => return failure("Command is not a Copy operation".into()),
    };
    match result {
        Ok(payload) => PrivilegedResponse {
            status: PrivilegedStatus::Success,
            message: None,
            payload,
        },
        Err(error) => failure(error.to_string()),
    }
}

/// A damaged record is an inspectable history state. A failed progress callback
/// ends the request instead; it must not become a misleading NeedsReview row.
fn inspect_history<T>(
    progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
    inspect: impl FnOnce(&mut dyn FnMut(u64) -> Result<(), PlatformError>) -> Result<T, PlatformError>,
) -> Result<Result<T, PlatformError>, PlatformError> {
    let mut interrupted = false;
    let result = inspect(&mut |bytes| {
        let result = progress(bytes);
        interrupted |= result.is_err();
        result
    });
    if interrupted {
        Err(result.err().unwrap_or_else(|| {
            PlatformError::Io(
                "Copy history inspection did not propagate interrupted progress".into(),
            )
        }))
    } else {
        Ok(result)
    }
}

fn registry(caller: &Arc<CallerCopyContext>) -> Result<WindowsCopyRegistry, PlatformError> {
    caller
        .copy_registry(&[], false)
        .map_err(|error| PlatformError::Io(error.to_string()))?
        .ok_or_else(|| {
            PlatformError::InvalidPathMoveUndoReceipt("Caller Copy registry is missing".into())
        })
}

fn history(
    caller: &Arc<CallerCopyContext>,
    progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
) -> Result<Vec<PrivilegedCopyHistoryEntry>, PlatformError> {
    progress(0)?;
    let Some(registry) = caller
        .copy_registry(&[], false)
        .map_err(|error| PlatformError::Io(error.to_string()))?
    else {
        return Ok(Vec::new());
    };
    let pending = registry.pending_cleanup_plan_with_progress(&mut *progress)?;
    let pending_ids = pending
        .as_ref()
        .map(|plan| {
            plan.transaction_ids()
                .into_iter()
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let mut history = Vec::new();
    for entry in registry.retained_with_progress(&mut *progress)? {
        progress(0)?;
        if entry
            .transaction_id
            .as_ref()
            .is_some_and(|id| pending_ids.contains(id))
        {
            continue;
        }
        let recovery = match (&entry.transaction_id, &entry.evidence) {
            (_, WindowsCopyRecoveryEvidence::NeedsReview { reason }) => {
                PrivilegedCopyHistoryState::NeedsReview {
                    reason: reason.clone(),
                }
            }
            (Some(id), WindowsCopyRecoveryEvidence::PendingNameRelease) => {
                match inspect_history(progress, |progress| {
                    registry.name_release_plan_with_progress(id, progress)
                })? {
                    Ok(_) => PrivilegedCopyHistoryState::RecoveryAvailable {
                        action: PrivilegedCopyRecoveryAction::ReleaseName,
                    },
                    Err(error) => PrivilegedCopyHistoryState::NeedsReview {
                        reason: error.to_string(),
                    },
                }
            }
            (Some(_), _) if entry.phase == Some(WindowsCopyPhase::Aborted) => {
                PrivilegedCopyHistoryState::AttemptEnded
            }
            (Some(id), WindowsCopyRecoveryEvidence::UnverifiedStage) => {
                match inspect_history(progress, |progress| {
                    registry.abort_plan_with_progress(id, progress)
                })? {
                    Ok(_) => PrivilegedCopyHistoryState::RecoveryAvailable {
                        action: PrivilegedCopyRecoveryAction::EndAttempt,
                    },
                    Err(error) => PrivilegedCopyHistoryState::NeedsReview {
                        reason: error.to_string(),
                    },
                }
            }
            (Some(id), WindowsCopyRecoveryEvidence::VerifiedTree)
                if !matches!(
                    entry.phase,
                    Some(
                        WindowsCopyPhase::Completed
                            | WindowsCopyPhase::UndoPending
                            | WindowsCopyPhase::Undone
                            | WindowsCopyPhase::CleaningUp
                    )
                ) =>
            {
                match inspect_history(progress, |progress| {
                    registry.resume_plan_with_progress(id, progress)
                })? {
                    Ok(_) => PrivilegedCopyHistoryState::RecoveryAvailable {
                        action: PrivilegedCopyRecoveryAction::Resume,
                    },
                    Err(error) => PrivilegedCopyHistoryState::NeedsReview {
                        reason: error.to_string(),
                    },
                }
            }
            (
                Some(_),
                WindowsCopyRecoveryEvidence::VerifiedTree
                | WindowsCopyRecoveryEvidence::VerifiedOriginal,
            ) if entry.phase == Some(WindowsCopyPhase::Undone) => {
                PrivilegedCopyHistoryState::Undone
            }
            (Some(id), _) => match inspect_history(progress, |progress| {
                registry.undo_plan_with_progress(id, progress)
            })? {
                Ok(_) => PrivilegedCopyHistoryState::UndoAvailable,
                Err(error) => PrivilegedCopyHistoryState::NeedsReview {
                    reason: error.to_string(),
                },
            },
            (None, _) => PrivilegedCopyHistoryState::NeedsReview {
                reason: "Unrecognized caller Copy recovery record; retained for review".into(),
            },
        };
        history.push(PrivilegedCopyHistoryEntry {
            reference: PrivilegedCopyReceipt {
                completed_sources: 0,
                transactions: entry.transaction_id.into_iter().collect(),
                paths: entry.retained.zip(entry.publication).into_iter().collect(),
            },
            recovery,
        });
    }
    if !pending_ids.is_empty() {
        history.push(PrivilegedCopyHistoryEntry {
            reference: PrivilegedCopyReceipt {
                completed_sources: 0,
                transactions: pending_ids.into_iter().collect(),
                paths: Vec::new(),
            },
            recovery: PrivilegedCopyHistoryState::CleanupPending,
        });
    }
    registry.validate()?;
    Ok(history)
}

fn undo(
    caller: &Arc<CallerCopyContext>,
    transactions: &[String],
    progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    progress(0)?;
    let ids = selected(transactions)?;
    let registry = registry(caller)?;
    let observed = registry.retained_with_progress(&mut *progress)?;
    if ids.iter().any(|id| {
        !observed
            .iter()
            .any(|entry| entry.transaction_id.as_ref() == Some(id))
    }) {
        return Err(PlatformError::InvalidPathMoveUndoReceipt(
            "Caller Copy recovery references are missing".into(),
        ));
    }
    // Preserve original batch order, reversing dependent publications. Re-admit
    // each inverse after its successor restores the state it depends upon.
    for id in transactions.iter().rev() {
        let mut plan = registry.undo_plan_with_progress(id, &mut *progress)?;
        while plan.remaining() > 0 {
            plan.undo_next_with_progress(&mut *progress)?;
        }
    }
    registry.validate()
}

fn cleanup(
    caller: &Arc<CallerCopyContext>,
    transactions: &[String],
    progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    progress(0)?;
    let mut ids = selected(transactions)?;
    let registry = registry(caller)?;
    if let Some(mut pending) = registry.pending_cleanup_plan_with_progress(&mut *progress)? {
        let members = pending
            .transaction_ids()
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        if !members.is_disjoint(&ids) {
            if !members.is_subset(&ids) {
                return Err(PlatformError::CopyCleanupRefused(
                    "Selection omits part of pending caller Copy finalization".into(),
                ));
            }
            pending.reclaim(&mut *progress)?;
            ids.retain(|id| !members.contains(id));
        }
    }
    if !ids.is_empty() {
        registry
            .cleanup_plan_with_progress(&ids.into_iter().collect::<Vec<_>>(), &mut *progress)?
            .reclaim(&mut *progress)?;
    }
    registry.validate()
}

fn recover(
    caller: &Arc<CallerCopyContext>,
    transaction: &str,
    action: PrivilegedCopyRecoveryAction,
    progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    progress(0)?;
    selected(&[transaction.to_owned()])?;
    let registry = registry(caller)?;
    match action {
        PrivilegedCopyRecoveryAction::Resume => registry
            .resume_plan_with_progress(transaction, &mut *progress)?
            .resume(&mut *progress)?,
        PrivilegedCopyRecoveryAction::EndAttempt => registry
            .abort_plan_with_progress(transaction, &mut *progress)?
            .end_attempt(&mut *progress)?,
        PrivilegedCopyRecoveryAction::ReleaseName => registry
            .name_release_plan_with_progress(transaction, &mut *progress)?
            .finish(&mut *progress)?,
    }
    registry.validate()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_interruption_is_a_request_error_instead_of_a_damaged_record() {
        let result = inspect_history(
            &mut |_| Err(PlatformError::Io("Stopped".into())),
            |progress| {
                progress(17)?;
                Ok(())
            },
        );
        assert!(matches!(result, Err(PlatformError::Io(message)) if message == "Stopped"));
    }

    #[test]
    fn damaged_history_record_remains_inspectable_without_hiding_progress_errors() {
        let result = inspect_history::<()>(&mut |_| Ok(()), |progress| {
            progress(17)?;
            Err(PlatformError::Io("Changed journal".into()))
        })
        .unwrap();
        assert!(matches!(result, Err(PlatformError::Io(message)) if message == "Changed journal"));
        let result = inspect_history(&mut |_| Ok(()), |progress| {
            progress(17)?;
            Ok(42)
        })
        .unwrap()
        .unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn ignored_history_progress_failure_never_returns_success() {
        let result = inspect_history(
            &mut |_| Err(PlatformError::Io("Stopped".into())),
            |progress| {
                let _ = progress(17);
                Ok(())
            },
        );
        assert!(
            matches!(result, Err(PlatformError::Io(message)) if message.contains("did not propagate"))
        );
    }
}
