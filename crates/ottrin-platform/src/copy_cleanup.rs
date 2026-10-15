//! Admission for the retained-data side of Copy's lifecycle. A changed visible
//! publication is not evidence that its private backup may be deleted.

use super::*;
use crate::move_recovery::LinuxJournalDirectory;
use rustix::fs::{Mode, OFlags};
use std::collections::{BTreeMap, BTreeSet};

/// Original-record-bound cleanup selection. No handles are kept between calls,
/// so inspecting cleanup eligibility does not prevent ordinary unmount.
/// Deletion still requires durable intent and per-entry mutation authority.
#[derive(Debug)]
pub struct RetainedCopyCleanupPlan {
    root: PathBuf,
    records: BTreeMap<String, CopyRecord>,
}

impl RetainedCopyCleanupPlan {
    pub fn transaction_ids(&self) -> Vec<&str> {
        self.records.keys().map(String::as_str).collect()
    }

    /// Rechecks original records, complete operation membership, dependencies,
    /// namespace identity and retained contents. Never adopts a newer record.
    pub fn revalidate(&self) -> Result<(), PlatformError> {
        let central =
            LinuxJournalDirectory::open_copy_registry(&self.root, false).map_err(journal_error)?;
        let all = registry_records(&central)?;
        for (id, expected) in &self.records {
            if all.get(id) != Some(expected) {
                return Err(refusal("Copy journal changed after cleanup admission"));
            }
            for (other_id, other) in &all {
                if other.operation.id == expected.operation.id
                    && !self.records.contains_key(other_id)
                {
                    return Err(refusal(
                        "Cleanup requires the complete recorded Copy operation",
                    ));
                }
            }
            validate_retained(&central, expected)?;
            if let Some((_, objects)) = retained_evidence(expected)? {
                for (owner_id, owner) in &all {
                    if owner_id != id
                        && !self.records.contains_key(owner_id)
                        && possible_reference(owner, objects)?
                    {
                        return Err(refusal(
                            "Retained Copy data is still referenced by another operation",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

fn possible_reference(owner: &CopyRecord, retained: &[TreeObject]) -> Result<bool, PlatformError> {
    // An ancestor overwrite can contain an older transaction itself, including
    // its backup even when that older publication has already been deleted.
    // Whole-tree publication comparisons cannot establish absence of a reference.
    let mut roots = vec![
        owner.parent_handle.as_ref(),
        owner.transaction_handle.as_ref(),
    ];
    if owner.phase != CopyPhase::Undone {
        roots.push(
            owner
                .publication_objects
                .first()
                .and_then(|object| object.handle.as_ref()),
        );
    }
    for root in roots {
        let root = root.ok_or_else(|| refusal("Copy dependency object evidence is unavailable"))?;
        // A generation handle alone is not a global filesystem identity. A
        // possible match across namespaces is conservatively retained, never
        // used to grant mutation authority or adopt an alternate pathname.
        if retained
            .iter()
            .any(|object| object.handle.as_ref() == Some(root))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn plan_cleanup(
    captured: &[RetainedCopyStatus],
) -> Result<RetainedCopyCleanupPlan, PlatformError> {
    if captured.is_empty() {
        return Err(refusal("No retained Copy transactions were selected"));
    }
    let mut records = BTreeMap::new();
    let mut captured_groups = BTreeMap::<String, BTreeSet<String>>::new();
    let mut ordering = BTreeMap::<String, (usize, BTreeSet<usize>)>::new();
    for status in captured {
        let observed = status
            .observation
            .as_ref()
            .ok_or_else(|| refusal("Copy cleanup requires an original journal observation"))?;
        let record = &observed.record;
        if !status.matches_observation()?
            || records
                .insert(status.transaction_id.clone(), record.clone())
                .is_some()
        {
            return Err(refusal(
                "Copy observation was changed or selected more than once",
            ));
        }
        retained_evidence(record)?;
        let (count, positions) = ordering
            .entry(record.operation.id.clone())
            .or_insert_with(|| (record.operation.item_count, BTreeSet::new()));
        if *count != record.operation.item_count || !positions.insert(record.operation.item_index) {
            return Err(refusal("Copy operation ordering is inconsistent"));
        }
        captured_groups
            .entry(record.operation.id.clone())
            .or_default()
            .insert(status.transaction_id.clone());
    }
    for status in captured {
        let observed = status.observation.as_ref().expect("validated observation");
        let group = &captured_groups[&observed.record.operation.id];
        if group.iter().ne(observed.members.iter()) {
            return Err(refusal(
                "Cleanup requires the complete captured Copy operation",
            ));
        }
    }
    let plan = RetainedCopyCleanupPlan {
        root: configured_root()?,
        records,
    };
    plan.revalidate()?;
    Ok(plan)
}

fn refusal(message: &str) -> PlatformError {
    PlatformError::CopyCleanupRefused(message.into())
}

fn registry_records(
    central: &LinuxJournalDirectory,
) -> Result<BTreeMap<String, CopyRecord>, PlatformError> {
    central
        .discover_copy_records()
        .map_err(journal_error)?
        .into_iter()
        .map(|(id, record)| {
            let record = record?;
            if record.transaction_id()? != id {
                return Err(refusal("Copy registry filename differs from its record"));
            }
            Ok((id, record))
        })
        .collect()
}

fn retained_evidence(
    record: &CopyRecord,
) -> Result<Option<(&SourceTreeSnapshot, &[TreeObject])>, PlatformError> {
    match record.phase {
        CopyPhase::Published => Ok(record
            .displaced_content
            .as_ref()
            .zip(record.displaced_objects.as_deref())),
        CopyPhase::Undone => Ok(Some((
            &record.publication_content,
            &record.publication_objects,
        ))),
        CopyPhase::PublicationIntent | CopyPhase::UndoIntent => Err(refusal(
            "Interrupted Copy publication or Undo must be reconciled before cleanup",
        )),
    }
}

fn validate_retained(
    central: &LinuxJournalDirectory,
    record: &CopyRecord,
) -> Result<(), PlatformError> {
    record.require_object_evidence()?;
    record.validate_namespace()?;
    let transaction = record.transaction.decode().map_err(journal_error)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let parent = rustix::fs::open(
        transaction.parent().expect("validated parent"),
        flags,
        Mode::empty(),
    )
    .map_err(journal_error)?;
    let stage = rustix::fs::openat(
        &parent,
        transaction.file_name().expect("validated stage name"),
        flags,
        Mode::empty(),
    )
    .map_err(journal_error)?;
    record.validate_opened_directory(&parent, true)?;
    record.validate_opened_directory(&stage, false)?;
    let local = LinuxJournalDirectory::for_copy_stage(&stage).map_err(journal_error)?;
    let _lock = local.try_lock_recovery().map_err(journal_error)?;
    let id = record.transaction_id()?;
    if local.read_copy_record("copy-journal")? != *record
        || central.read_copy_record(&format!("{id}.journal"))? != *record
    {
        return Err(refusal(
            "Copy journals differ or changed during cleanup admission",
        ));
    }
    let mut directory = rustix::fs::Dir::read_from(&stage).map_err(journal_error)?;
    while let Some(entry) = directory.read() {
        let entry = entry.map_err(journal_error)?;
        if !matches!(
            entry.file_name().to_bytes(),
            b"." | b".." | b"candidate" | b"copy-journal"
        ) {
            return Err(refusal(
                "Copy transaction contains an unrecognized retained entry",
            ));
        }
    }
    match record.phase {
        CopyPhase::Published => record.validate_retained_backup(&stage)?,
        CopyPhase::Undone => record.validate_completed_data()?,
        _ => return Err(refusal("Copy transaction is not ready for cleanup")),
    }
    record.validate_namespace()?;
    record.validate_opened_directory(&parent, true)?;
    record.validate_opened_directory(&stage, false)?;
    if local.read_copy_record("copy-journal")? != *record
        || central.read_copy_record(&format!("{id}.journal"))? != *record
    {
        return Err(refusal(
            "Copy journals changed while retained contents were checked",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConflictAction, DefaultPlatform, PlatformOps};

    struct Registry(Option<PathBuf>);
    impl Registry {
        fn new(root: &Path) -> Self {
            Self(crate::move_transaction::replace_test_recovery_root(Some(
                root.join("moves"),
            )))
        }
    }
    impl Drop for Registry {
        fn drop(&mut self) {
            crate::move_transaction::replace_test_recovery_root(self.0.take());
        }
    }

    fn copy(root: &Path, version: &str) -> crate::PathMoveUndoReceipt {
        let directory = root.join(version);
        std::fs::create_dir(&directory).unwrap();
        let source = directory.join("item");
        std::fs::write(&source, version).unwrap();
        let destination = root.join("destination");
        std::fs::create_dir_all(&destination).unwrap();
        DefaultPlatform
            .execute_copy_with_undo_receipt(&source, &destination, ConflictAction::Overwrite)
            .unwrap()
            .unwrap()
    }

    fn statuses(root: &Path) -> Vec<RetainedCopyStatus> {
        discover_at(&root.join("copies")).unwrap()
    }

    fn stage(receipt: &crate::PathMoveUndoReceipt) -> PathBuf {
        receipt.before_path().parent().unwrap().to_path_buf()
    }

    #[test]
    fn published_cleanup_uses_retained_data_without_reading_changed_publication() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let destination = root.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("item"), "old destination").unwrap();
        let receipt = copy(root.path(), "new");
        let visible = vec![b'x'; 4 * 1024 * 1024];
        std::fs::write(destination.join("item"), &visible).unwrap();
        let captured = statuses(root.path());
        assert!(matches!(
            captured[0].evidence,
            CopyRecoveryEvidence::NeedsReview { .. }
        ));
        let local = stage(&receipt).join("copy-journal");
        let before = std::fs::read(&local).unwrap();
        let read_chars = || {
            std::fs::read_to_string("/proc/thread-self/io")
                .unwrap()
                .lines()
                .find_map(|line| {
                    line.strip_prefix("rchar: ")
                        .map(|n| n.parse::<u64>().unwrap())
                })
                .unwrap()
        };
        let reads_before = read_chars();
        let plan = plan_cleanup(&captured).unwrap();
        plan.revalidate().unwrap();
        assert!(
            read_chars() - reads_before < 1024 * 1024,
            "Cleanup admission read the unrelated 4 MiB visible publication"
        );
        assert_eq!(std::fs::read(&local).unwrap(), before);
        assert_eq!(
            std::fs::read(receipt.before_path()).unwrap(),
            b"old destination"
        );
        assert_eq!(std::fs::read(destination.join("item")).unwrap(), visible);
        std::fs::write(receipt.before_path(), "changed retained data").unwrap();
        assert!(plan.revalidate().is_err());
        assert!(plan_cleanup(&statuses(root.path())).is_err());
        assert_eq!(
            std::fs::read(receipt.before_path()).unwrap(),
            b"changed retained data"
        );
    }

    #[test]
    fn completed_undo_cleanup_does_not_adopt_the_reused_destination() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let receipt = copy(root.path(), "new");
        DefaultPlatform.undo_copy_with_receipt(&receipt).unwrap();
        std::fs::write(root.path().join("destination/item"), "later work").unwrap();
        let plan = plan_cleanup(&statuses(root.path())).unwrap();
        plan.revalidate().unwrap();
        assert_eq!(std::fs::read(receipt.before_path()).unwrap(), b"new");
        assert_eq!(
            std::fs::read(root.path().join("destination/item")).unwrap(),
            b"later work"
        );
        std::fs::write(stage(&receipt).join("unrelated"), "preserve me").unwrap();
        assert!(plan.revalidate().is_err());
        assert_eq!(
            std::fs::read(stage(&receipt).join("unrelated")).unwrap(),
            b"preserve me"
        );
    }

    #[test]
    fn cleanup_requires_the_whole_batch_and_protects_external_copy_dependencies() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let first = copy(root.path(), "first");
        let second = copy(root.path(), "second");
        let captured = statuses(root.path());
        let second_only = captured
            .iter()
            .filter(|entry| entry.transaction.as_ref() == Some(&stage(&second)))
            .map(|entry| entry.transaction_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(second_only.len(), 1);
        let selected = statuses(root.path())
            .into_iter()
            .filter(|entry| second_only.contains(&entry.transaction_id))
            .collect::<Vec<_>>();
        assert!(
            plan_cleanup(&selected)
                .unwrap_err()
                .to_string()
                .contains("another operation")
        );
        let plan = plan_cleanup(&captured).unwrap();
        assert_eq!(plan.transaction_ids().len(), 2);
        plan.revalidate().unwrap();
        assert_eq!(std::fs::read(second.before_path()).unwrap(), b"first");
        assert!(first.before_path().parent().unwrap().exists());

        let sources = ["batch-a", "batch-b"].map(|version| {
            let directory = root.path().join(version);
            std::fs::create_dir(&directory).unwrap();
            let path = directory.join("batch-item");
            std::fs::write(&path, version).unwrap();
            path
        });
        let batch = DefaultPlatform.execute_copy_batch_with_undo_receipts(
            &sources,
            &root.path().join("destination"),
            ConflictAction::Overwrite,
        );
        assert!(batch.error.is_none());
        let batch_stages: Vec<_> = batch.undo_receipts.iter().map(stage).collect();
        let selected = statuses(root.path())
            .into_iter()
            .filter(|entry| batch_stages.contains(entry.transaction.as_ref().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 2);
        assert!(plan_cleanup(&selected[..1]).is_err());
        plan_cleanup(&selected).unwrap().revalidate().unwrap();
    }

    #[test]
    fn cleanup_never_adopts_changed_observations_journals_or_stage_paths() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let receipt = copy(root.path(), "new");
        let mut captured = statuses(root.path());
        captured[0].publication = Some(root.path().join("unrelated"));
        assert!(plan_cleanup(&captured).is_err());
        let plan = plan_cleanup(&statuses(root.path())).unwrap();
        let local = stage(&receipt).join("copy-journal");
        let before = std::fs::read(&local).unwrap();
        std::fs::write(&local, b"unrelated journal").unwrap();
        assert!(plan.revalidate().is_err());
        assert_eq!(std::fs::read(&local).unwrap(), b"unrelated journal");
        std::fs::write(&local, &before).unwrap();
        let original = stage(&receipt);
        let renamed = root.path().join("old-stage");
        std::fs::rename(&original, &renamed).unwrap();
        std::fs::create_dir(&original).unwrap();
        std::fs::write(original.join("user-file"), "preserve me").unwrap();
        assert!(plan.revalidate().is_err());
        assert_eq!(
            std::fs::read(original.join("user-file")).unwrap(),
            b"preserve me"
        );
        assert_eq!(std::fs::read(renamed.join("copy-journal")).unwrap(), before);
    }

    #[test]
    fn cleanup_checks_nested_backup_contents_and_symlink_entries_without_following_targets() {
        for kind in ["directory", "symlink"] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let destination = root.path().join("destination");
            let outside = root.path().join("outside");
            std::fs::create_dir(&destination).unwrap();
            std::fs::create_dir(&outside).unwrap();
            std::fs::write(outside.join("user-file"), "outside data").unwrap();
            let target = destination.join("item");
            if kind == "directory" {
                std::fs::create_dir_all(target.join("nested")).unwrap();
                std::fs::write(target.join("nested/leaf"), "retained data").unwrap();
            } else {
                std::os::unix::fs::symlink(&outside, &target).unwrap();
            }
            let receipt = copy(root.path(), "new");
            std::fs::remove_file(&target).unwrap();
            std::os::unix::fs::symlink(&outside, &target).unwrap();
            let plan = plan_cleanup(&statuses(root.path())).unwrap();
            if kind == "directory" {
                std::fs::write(receipt.before_path().join("nested/leaf"), "changed data").unwrap();
            } else {
                std::fs::remove_file(receipt.before_path()).unwrap();
                std::os::unix::fs::symlink(root.path().join("other-target"), receipt.before_path())
                    .unwrap();
            }
            assert!(plan.revalidate().is_err());
            assert_eq!(
                std::fs::read(outside.join("user-file")).unwrap(),
                b"outside data"
            );
            assert_eq!(std::fs::read_link(&target).unwrap(), outside);
        }
    }

    #[test]
    fn cleanup_rejects_changed_central_records_and_interrupted_phases() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let receipt = copy(root.path(), "new");
        let captured = statuses(root.path());
        let id = captured[0].transaction_id.clone();
        let plan = plan_cleanup(&captured).unwrap();
        let central_path = root.path().join("copies").join(format!("{id}.journal"));
        let original = std::fs::read(&central_path).unwrap();
        let record = CopyRecord::parse(&original).unwrap();
        let local_path = stage(&receipt).join("copy-journal");
        for phase in [CopyPhase::PublicationIntent, CopyPhase::UndoIntent] {
            let mut changed = record.clone();
            changed.phase = phase;
            let bytes = serde_json::to_vec(&changed).unwrap();
            std::fs::write(&central_path, &bytes).unwrap();
            assert!(plan.revalidate().is_err());
            std::fs::write(&local_path, &bytes).unwrap();
            let observed = statuses(root.path());
            assert!(
                plan_cleanup(&observed)
                    .unwrap_err()
                    .to_string()
                    .contains("Interrupted")
            );
            assert_eq!(std::fs::read(&central_path).unwrap(), bytes);
            assert_eq!(std::fs::read(&local_path).unwrap(), bytes);
        }
        assert_eq!(
            std::fs::read(root.path().join("destination/item")).unwrap(),
            b"new"
        );
    }

    #[test]
    fn ancestor_overwrite_cannot_discard_a_nested_transactions_private_backup() {
        for keep_publication in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let destination = root.path().join("destination");
            let outer = destination.join("outer");
            std::fs::create_dir_all(&outer).unwrap();
            std::fs::write(outer.join("item"), "before first copy").unwrap();
            let source = root.path().join("source");
            std::fs::create_dir(&source).unwrap();
            std::fs::write(source.join("item"), "first copy").unwrap();
            let first = DefaultPlatform
                .execute_copy_with_undo_receipt(
                    &source.join("item"),
                    &outer,
                    ConflictAction::Overwrite,
                )
                .unwrap()
                .unwrap();
            if !keep_publication {
                std::fs::remove_file(outer.join("item")).unwrap();
            }
            let replacement = root.path().join("replacement/outer");
            std::fs::create_dir_all(&replacement).unwrap();
            std::fs::write(replacement.join("new"), "second copy").unwrap();
            let second = DefaultPlatform
                .execute_copy_with_undo_receipt(
                    &replacement,
                    &destination,
                    ConflictAction::Overwrite,
                )
                .unwrap()
                .unwrap();
            let nested_backup = second
                .before_path()
                .join(stage(&first).file_name().unwrap())
                .join("candidate");
            assert_eq!(std::fs::read(&nested_backup).unwrap(), b"before first copy");
            let selected = statuses(root.path())
                .into_iter()
                .filter(|entry| entry.transaction.as_ref() == Some(&stage(&second)))
                .collect::<Vec<_>>();
            assert_eq!(selected.len(), 1);
            assert!(
                plan_cleanup(&selected)
                    .unwrap_err()
                    .to_string()
                    .contains("another operation")
            );
            assert_eq!(std::fs::read(nested_backup).unwrap(), b"before first copy");
            assert_eq!(std::fs::read(outer.join("new")).unwrap(), b"second copy");
        }
    }
}
