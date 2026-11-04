//! Elevated Copy uses the same durable transactions as ordinary Copy. References
//! returned over IPC are lookup keys, never deserialized filesystem authority.
use crate::{DefaultPlatform, PlatformError, PlatformOps};
use ottrin_core::{
    ConflictAction, PrivilegedCopyReceipt, PrivilegedPayload, PrivilegedResponse, PrivilegedStatus,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub fn copy(
    platform: &DefaultPlatform,
    sources: &[PathBuf],
    destination: &Path,
    conflict: ConflictAction,
) -> PrivilegedResponse {
    let batch =
        platform
            .execute_copy_batch_with_undo_receipts(sources, destination, conflict, &mut |_| Ok(()));
    copy_response(batch)
}

fn copy_response(batch: crate::CopyBatchResult) -> PrivilegedResponse {
    let mut references = PrivilegedCopyReceipt {
        completed_sources: batch.completed_sources,
        transactions: Vec::new(),
        paths: Vec::new(),
    };
    for receipt in &batch.undo_receipts {
        let Some(id) = receipt.copy_transaction_id() else {
            return failure("Committed Copy has no durable transaction reference; retained data requires administrator review".into());
        };
        references.transactions.push(id.to_owned());
        references.paths.push((
            receipt.before_path().to_owned(),
            receipt.after_path().to_owned(),
        ));
    }
    PrivilegedResponse {
        status: if batch.error.is_some() {
            PrivilegedStatus::Failed
        } else {
            PrivilegedStatus::Success
        },
        message: batch.error.map(|error| error.to_string()),
        // Include completed receipts even when a later batch item fails.
        payload: Some(PrivilegedPayload::Copy(references)),
    }
}

fn failure(message: String) -> PrivilegedResponse {
    PrivilegedResponse {
        status: PrivilegedStatus::Failed,
        message: Some(message),
        payload: None,
    }
}

fn selected(ids: &[String]) -> Result<BTreeSet<String>, PlatformError> {
    let selected = ids.iter().cloned().collect::<BTreeSet<_>>();
    if selected.is_empty()
        || selected.len() != ids.len()
        || selected
            .iter()
            .any(|id| id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(PlatformError::InvalidPathMoveUndoReceipt(
            "Invalid privileged Copy transaction selection".into(),
        ));
    }
    Ok(selected)
}

pub fn undo(ids: &[String]) -> Result<(), PlatformError> {
    let ids = selected(ids)?;
    let observed = crate::discover_retained_copy_transactions()?
        .into_iter()
        .filter(|entry| ids.contains(&entry.transaction_id))
        .collect::<Vec<_>>();
    if observed.len() != ids.len() {
        return Err(PlatformError::InvalidPathMoveUndoReceipt(
            "Privileged Copy recovery references are missing".into(),
        ));
    }
    let mut plan = crate::restore_retained_copy_undo_plan(&observed)?;
    while plan.remaining() > 0 {
        plan.undo_next()?;
    }
    Ok(())
}

/// Discover only the elevated account's canonical registry. A successful scan
/// does not retain descriptors or mutation plans across the IPC boundary.
pub fn discover_history() -> Result<Vec<ottrin_core::PrivilegedCopyHistoryEntry>, PlatformError> {
    use ottrin_core::PrivilegedCopyHistoryEntry;
    let finalizations = crate::discover_retained_copy_finalizations()?;
    let finalized_ids = finalizations
        .iter()
        .flat_map(|plan| plan.transaction_ids().into_iter().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let mut groups = BTreeMap::<String, Vec<crate::RetainedCopyStatus>>::new();
    for entry in crate::discover_retained_copy_transactions()? {
        if finalized_ids.contains(&entry.transaction_id) {
            continue;
        }
        let key = entry
            .operation
            .as_ref()
            .map(|operation| operation.id.clone())
            .unwrap_or_else(|| entry.transaction_id.clone());
        groups.entry(key).or_default().push(entry);
    }
    let mut history = Vec::new();
    for mut entries in groups.into_values() {
        entries.sort_by_key(|entry| {
            entry
                .operation
                .as_ref()
                .map(|operation| operation.item_index)
        });
        let undo_unavailable_reason = match crate::restore_retained_copy_undo_plan(&entries) {
            Ok(plan) if plan.remaining() > 0 => None,
            Ok(_) => {
                Some("Already undone. Retained data can be cleared after confirmation.".into())
            }
            Err(error) => Some(error.to_string()),
        };
        let paths = entries
            .iter()
            .filter_map(|entry| Some((entry.source.clone()?, entry.publication.clone()?)))
            .collect();
        history.push(PrivilegedCopyHistoryEntry {
            reference: PrivilegedCopyReceipt {
                // Discovery cannot establish how many originally requested
                // sources completed: skipped/failed items may have no journal.
                completed_sources: 0,
                transactions: entries
                    .into_iter()
                    .map(|entry| entry.transaction_id)
                    .collect(),
                paths,
            },
            undo_unavailable_reason,
        });
    }
    for plan in finalizations {
        history.push(PrivilegedCopyHistoryEntry {
            reference: PrivilegedCopyReceipt {
                completed_sources: 0,
                transactions: plan
                    .transaction_ids()
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                paths: Vec::new(),
            },
            undo_unavailable_reason: Some(
                "Copy cleanup was interrupted. Undo is revoked; finish clearing retained data."
                    .into(),
            ),
        });
    }
    Ok(history)
}

pub fn cleanup(ids: &[String]) -> Result<(), PlatformError> {
    let mut ids = selected(ids)?;
    // A previous helper may have stopped after removing some central journals.
    // Resume its exact durable finalization, never adopt a new deletion plan.
    for finalization in crate::discover_retained_copy_finalizations()? {
        let members = finalization
            .transaction_ids()
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        if members.is_disjoint(&ids) {
            continue;
        }
        if !members.is_subset(&ids) {
            return Err(PlatformError::CopyCleanupRefused(
                "Selection omits part of pending privileged Copy finalization".into(),
            ));
        }
        finalization.finish()?;
        ids.retain(|id| !members.contains(id));
    }
    if ids.is_empty() {
        return Ok(());
    }
    let ids = ids.into_iter().collect::<Vec<_>>();
    let observed = crate::observe_retained_copy_cleanup(&ids)?;
    let pending = observed.iter().all(|entry| {
        matches!(
            entry.phase,
            Some(crate::CopyPhase::CleanupPending | crate::CopyPhase::CleanupComplete)
        )
    });
    let mut plan = if pending {
        crate::resume_retained_copy_cleanup(&ids)?
    } else {
        crate::plan_retained_copy_cleanup(&observed)?
    };
    while plan.reclaim_payload_next()? {}
    plan.begin_finalization()?.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: tempfile::TempDir,
        previous: Option<PathBuf>,
    }
    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let previous = crate::move_transaction::replace_test_recovery_root(Some(
                root.path().join("state/moves"),
            ));
            std::fs::create_dir(root.path().join("input")).unwrap();
            std::fs::create_dir(root.path().join("output")).unwrap();
            Self { root, previous }
        }
        fn path(&self, name: &str) -> PathBuf {
            self.root.path().join(name)
        }
        fn copy(&self, names: &[&str]) -> (PrivilegedResponse, PrivilegedCopyReceipt) {
            let response = copy(
                &DefaultPlatform,
                &names.iter().map(|n| self.path(n)).collect::<Vec<_>>(),
                &self.path("output"),
                ConflictAction::Overwrite,
            );
            let Some(PrivilegedPayload::Copy(reference)) = &response.payload else {
                panic!("missing recovery payload: {response:?}");
            };
            let reference = reference.clone();
            (response, reference)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            crate::move_transaction::replace_test_recovery_root(self.previous.take());
        }
    }

    #[test]
    fn privileged_copy_returns_durable_batch_references_for_guarded_undo_and_cleanup() {
        let f = Fixture::new();
        std::fs::write(f.path("input/a"), "new a").unwrap();
        std::fs::write(f.path("input/b"), "new b").unwrap();
        std::fs::write(f.path("output/a"), "old a").unwrap();
        let (response, reference) = f.copy(&["input/a", "input/b"]);
        assert_eq!(response.status, PrivilegedStatus::Success);
        assert_eq!(reference.completed_sources, 2);
        assert_eq!(reference.transactions.len(), 2);
        assert_eq!(
            crate::discover_retained_copy_transactions().unwrap().len(),
            2
        );
        let history = discover_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].reference.transactions, reference.transactions);
        assert!(history[0].undo_unavailable_reason.is_none());
        assert_eq!(std::fs::read(f.path("output/a")).unwrap(), b"new a");
        // References survive the loss of live receipts and use canonical journals.
        undo(&history[0].reference.transactions).unwrap();
        assert_eq!(std::fs::read(f.path("output/a")).unwrap(), b"old a");
        assert!(!f.path("output/b").exists());
        let undone = discover_history().unwrap();
        assert_eq!(undone.len(), 1);
        assert!(undone[0].undo_unavailable_reason.is_some());
        cleanup(&reference.transactions).unwrap();
        assert!(
            crate::discover_retained_copy_transactions()
                .unwrap()
                .is_empty()
        );
        assert_eq!(std::fs::read(f.path("input/a")).unwrap(), b"new a");
        assert_eq!(std::fs::read_dir(f.path("output")).unwrap().count(), 1);
        assert!(discover_history().unwrap().is_empty());
    }

    #[test]
    fn privileged_copy_refuses_changed_publication_and_incomplete_selection() {
        let f = Fixture::new();
        std::fs::write(f.path("input/a"), "a").unwrap();
        std::fs::write(f.path("input/b"), "b").unwrap();
        let (_, reference) = f.copy(&["input/a", "input/b"]);
        assert!(undo(&reference.transactions[..1]).is_err());
        assert!(cleanup(&reference.transactions[..1]).is_err());
        std::fs::write(f.path("output/a"), "user edited").unwrap();
        let history = discover_history().unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].undo_unavailable_reason.is_some());
        assert!(undo(&reference.transactions).is_err());
        assert_eq!(std::fs::read(f.path("output/a")).unwrap(), b"user edited");
        // Explicit discard validates retained data and never deletes the publication.
        cleanup(&reference.transactions).unwrap();
        assert_eq!(std::fs::read(f.path("output/a")).unwrap(), b"user edited");
        assert_eq!(std::fs::read(f.path("output/b")).unwrap(), b"b");
    }

    #[test]
    fn privileged_copy_keeps_completed_receipts_when_a_later_item_fails() {
        let f = Fixture::new();
        std::fs::write(f.path("input/a"), "a").unwrap();
        std::fs::write(f.path("input/b"), "b").unwrap();
        let mut removed = false;
        let batch = DefaultPlatform.execute_copy_batch_with_undo_receipts(
            &[f.path("input/a"), f.path("input/b")],
            &f.path("output"),
            ConflictAction::Overwrite,
            &mut |_| {
                // Remove only an owned second source after the first publication.
                if !removed && f.path("output/a").exists() {
                    std::fs::remove_file(f.path("input/b")).unwrap();
                    removed = true;
                }
                Ok(())
            },
        );
        assert!(removed);
        let response = copy_response(batch);
        let Some(PrivilegedPayload::Copy(reference)) = &response.payload else {
            panic!("missing completed receipt");
        };
        assert_eq!(response.status, PrivilegedStatus::Failed);
        assert_eq!(reference.completed_sources, 1);
        assert_eq!(reference.transactions.len(), 1);
        let history = discover_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].reference.transactions, reference.transactions);
        assert!(history[0].undo_unavailable_reason.is_none());
        assert_eq!(std::fs::read(f.path("output/a")).unwrap(), b"a");
        assert!(!f.path("output/b").exists());
        undo(&reference.transactions).unwrap();
        cleanup(&reference.transactions).unwrap();
        assert!(!f.path("output/a").exists());
    }

    #[test]
    fn privileged_copy_cleanup_resumes_original_finalization_after_helper_exit() {
        let f = Fixture::new();
        std::fs::write(f.path("input/a"), "a").unwrap();
        let (_, reference) = f.copy(&["input/a"]);
        let observed = crate::observe_retained_copy_cleanup(&reference.transactions).unwrap();
        let mut plan = crate::plan_retained_copy_cleanup(&observed).unwrap();
        while plan.reclaim_payload_next().unwrap() {}
        let original = plan.begin_finalization().unwrap();
        let id = original.id().to_owned();
        drop(original);
        drop(plan);
        let history = discover_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].reference.transactions, reference.transactions);
        assert!(history[0].reference.paths.is_empty());
        assert!(history[0].undo_unavailable_reason.is_some());
        assert!(
            crate::discover_retained_copy_finalizations()
                .unwrap()
                .iter()
                .any(|p| p.id() == id)
        );
        cleanup(&reference.transactions).unwrap();
        assert!(
            crate::discover_retained_copy_finalizations()
                .unwrap()
                .is_empty()
        );
        assert_eq!(std::fs::read(f.path("output/a")).unwrap(), b"a");
    }

    #[test]
    fn privileged_copy_rejects_unknown_duplicate_and_malformed_references() {
        let f = Fixture::new();
        std::fs::write(f.path("input/a"), "a").unwrap();
        let (_, reference) = f.copy(&["input/a"]);
        for ids in [
            Vec::new(),
            vec!["../outside".into()],
            vec!["0".repeat(32)],
            vec![reference.transactions[0].clone(); 2],
        ] {
            assert!(undo(&ids).is_err());
            assert!(cleanup(&ids).is_err());
            assert_eq!(std::fs::read(f.path("output/a")).unwrap(), b"a");
        }
        cleanup(&reference.transactions).unwrap();
    }
}
