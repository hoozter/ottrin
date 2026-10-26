use super::*;
use crate::{ConflictAction, DefaultPlatform, PlatformOps};
use std::path::PathBuf;

struct Fixture {
    primary: crate::tests::TestDir,
    secondary: crate::tests::TestDir,
    previous: Option<PathBuf>,
}
impl Fixture {
    fn new() -> Self {
        let primary = crate::tests::TestDir::new("native-cross-replace-undo");
        let volume = PathBuf::from(
            std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").expect("owned second volume"),
        );
        let secondary = crate::tests::TestDir::new_at(&volume, "native-cross-replace-destination");
        let previous = super::super::super::super::replace_test_recovery_root(Some(
            primary.path().join("state"),
        ));
        Self {
            primary,
            secondary,
            previous,
        }
    }
    fn moved(&self, kind: u8) -> PathMoveUndoReceipt {
        let source = self.primary.path().join("source/entry");
        let destination = self.secondary.path().join("destination");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::create_dir(&destination).unwrap();
        for (path, bytes, target) in [
            (&source, b"source bytes".as_slice(), "source-link-target"),
            (
                &destination.join("entry"),
                b"retained bytes".as_slice(),
                "retained-link-target",
            ),
        ] {
            match kind {
                0 => std::fs::write(path, bytes).unwrap(),
                1 => {
                    std::fs::create_dir(path).unwrap();
                    std::fs::write(path.join("child"), bytes).unwrap();
                }
                _ => std::os::windows::fs::symlink_file(target, path).unwrap(),
            }
        }
        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination, ConflictAction::Overwrite)
            .unwrap();
        let receipt = receipt.expect("retained cross-volume publication supplies Undo");
        assert!(origin(&receipt).retained.is_some());
        receipt
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        super::super::super::super::replace_test_recovery_root(self.previous.take());
    }
}
fn root() -> PathBuf {
    super::super::super::super::windows_recovery_root().unwrap()
}

#[test]
#[ignore = "requires an owned second native Windows volume"]
fn windows_native_cross_replace_completion_survives_deleted_parents_and_both_old_journals() {
    let fixture = Fixture::new();
    let receipt = fixture.moved(0);
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    let inverse = guard(&receipt)
        .inverse
        .get()
        .unwrap()
        .transaction_id()
        .unwrap();
    let controller = origin(&receipt).completion_controller().unwrap().unwrap();
    std::fs::remove_dir_all(receipt.before_path().parent().unwrap()).unwrap();
    std::fs::remove_dir_all(receipt.after_path().parent().unwrap()).unwrap();
    for id in [inverse, controller] {
        std::fs::write(
            windows_cross_volume_journal_path(&root(), id),
            b"superseded bytes",
        )
        .unwrap();
        let plan = crate::WindowsMoveRecoveryPlan::capture(id).unwrap();
        assert_eq!(plan.paths(), &[root()]);
        assert_eq!(
            plan.undo_status_for(&receipt),
            Some(move_recovery::WindowsMoveUndoStatus::Complete)
        );
        plan.execute().unwrap();
    }
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    let found = super::super::super::super::discover_retained_windows_move_transactions().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].transaction_id, inverse);
    let source = fixture.primary.path().join("later");
    std::fs::write(&source, b"later independent Move").unwrap();
    let destination = fixture.primary.path().join("later-destination");
    std::fs::create_dir(&destination).unwrap();
    DefaultPlatform
        .execute_move_with_undo_receipt(&source, &destination, ConflictAction::Skip)
        .unwrap();
    assert_eq!(
        std::fs::read(destination.join("later")).unwrap(),
        b"later independent Move"
    );
}

#[test]
#[ignore = "requires an owned second native Windows volume"]
fn windows_native_completed_cross_replace_undo_survives_both_parent_renames() {
    let fixture = Fixture::new();
    let receipt = fixture.moved(0);
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    let controller = load_windows_cross_volume_record(&central(&receipt)).unwrap();
    let inverse = load_windows_cross_volume_record(&windows_cross_volume_journal_path(
        &root(),
        guard(&receipt)
            .inverse
            .get()
            .unwrap()
            .transaction_id()
            .unwrap(),
    ))
    .unwrap();
    // Model an interruption before compact completion publication. Once a
    // receipt exists, old journals are superseded and cannot reopen Undo.
    std::fs::remove_file(root().join(format!("{}.move-done", inverse.transaction_id))).unwrap();
    let journals = [
        central(&receipt),
        local(&receipt),
        windows_cross_volume_journal_path(&root(), inverse.transaction_id().unwrap()),
        inverse
            .transaction_path()
            .unwrap()
            .join(WINDOWS_LOCAL_JOURNAL_NAME),
    ];
    let bytes = journals.each_ref().map(|path| std::fs::read(path).unwrap());
    let moved_source = fixture.primary.path().join("renamed-source");
    let moved_destination = fixture.secondary.path().join("renamed-destination");
    std::fs::rename(receipt.before_path().parent().unwrap(), &moved_source).unwrap();
    std::fs::rename(receipt.after_path().parent().unwrap(), &moved_destination).unwrap();
    std::fs::create_dir(receipt.before_path().parent().unwrap()).unwrap();
    std::fs::create_dir(receipt.after_path().parent().unwrap()).unwrap();
    std::fs::write(receipt.before_path(), b"foreign old source").unwrap();
    std::fs::write(receipt.after_path(), b"foreign old destination").unwrap();
    for (record, saved) in [(&controller, &bytes[1]), (&inverse, &bytes[3])] {
        std::fs::create_dir(record.transaction_path().unwrap()).unwrap();
        std::fs::write(
            record
                .transaction_path()
                .unwrap()
                .join(WINDOWS_LOCAL_JOURNAL_NAME),
            saved,
        )
        .unwrap();
        std::fs::write(
            record.candidate_path().unwrap(),
            b"foreign private candidate",
        )
        .unwrap();
    }
    let actual_inverse_local = moved_source
        .join(inverse.transaction_path().unwrap().file_name().unwrap())
        .join(WINDOWS_LOCAL_JOURNAL_NAME);
    let mut gap = inverse.clone();
    gap.phase = Phase::Published;
    let gap_bytes = serde_json::to_vec(&gap).unwrap();
    std::fs::write(&actual_inverse_local, &gap_bytes).unwrap();
    let pending_plan =
        crate::WindowsMoveRecoveryPlan::capture(inverse.transaction_id().unwrap()).unwrap();
    assert_eq!(
        pending_plan.undo_status(),
        move_recovery::WindowsMoveUndoStatus::Pending
    );
    pending_plan
        .execute()
        .expect("converge the original native private journal");
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    assert_eq!(std::fs::read(&journals[2]).unwrap(), bytes[2]);
    assert_eq!(std::fs::read(&actual_inverse_local).unwrap(), bytes[3]);
    assert_eq!(
        std::fs::read(
            inverse
                .transaction_path()
                .unwrap()
                .join(WINDOWS_LOCAL_JOURNAL_NAME)
        )
        .unwrap(),
        bytes[3]
    );
    std::fs::write(&actual_inverse_local, &bytes[3]).unwrap();
    for id in [
        controller.transaction_id().unwrap(),
        inverse.transaction_id().unwrap(),
    ] {
        let plan = crate::WindowsMoveRecoveryPlan::capture(id).unwrap();
        assert_eq!(
            plan.undo_status_for(&receipt),
            Some(move_recovery::WindowsMoveUndoStatus::Complete)
        );
        assert_eq!(plan.paths(), &[root()]);
        plan.execute().unwrap();
    }
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    let found = super::super::super::super::discover_retained_windows_move_transactions().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].undo_status,
        move_recovery::WindowsMoveUndoStatus::Complete
    );
    let unrelated = fixture.primary.path().join("unrelated");
    std::fs::write(&unrelated, b"independent source").unwrap();
    let (_, undo) = DefaultPlatform
        .execute_move_with_undo_receipt(
            &unrelated,
            receipt.after_path().parent().unwrap(),
            ConflictAction::Skip,
        )
        .unwrap();
    DefaultPlatform
        .undo_move_with_receipt(&undo.unwrap())
        .unwrap();
    assert_eq!(std::fs::read(unrelated).unwrap(), b"independent source");
    assert_eq!(
        std::fs::read(moved_source.join("entry")).unwrap(),
        b"source bytes"
    );
    assert_eq!(
        std::fs::read(moved_destination.join("entry")).unwrap(),
        b"retained bytes"
    );
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"foreign old source"
    );
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"foreign old destination"
    );
    for record in [&controller, &inverse] {
        assert_eq!(
            std::fs::read(record.candidate_path().unwrap()).unwrap(),
            b"foreign private candidate"
        );
    }
    assert_eq!(std::fs::read(&journals[0]).unwrap(), bytes[0]);
    assert_eq!(std::fs::read(&journals[2]).unwrap(), bytes[2]);
    for (base, record, saved) in [
        (&moved_destination, &controller, &bytes[1]),
        (&moved_source, &inverse, &bytes[3]),
    ] {
        assert_eq!(
            std::fs::read(
                base.join(record.transaction_path().unwrap().file_name().unwrap())
                    .join(WINDOWS_LOCAL_JOURNAL_NAME)
            )
            .unwrap(),
            *saved
        );
        assert_eq!(
            std::fs::read(
                record
                    .transaction_path()
                    .unwrap()
                    .join(WINDOWS_LOCAL_JOURNAL_NAME)
            )
            .unwrap(),
            *saved
        );
    }
}
fn guard(receipt: &PathMoveUndoReceipt) -> &Guard {
    let Some(WindowsPathUndo::Move(WindowsMoveUndoGuard::CrossVolume(value))) =
        &receipt.windows_undo
    else {
        panic!("wrong Undo authority")
    };
    value
}
fn origin(receipt: &PathMoveUndoReceipt) -> &Origin {
    &guard(receipt).origin
}
fn binding(receipt: &PathMoveUndoReceipt) -> &Binding {
    origin(receipt).retained.as_ref().unwrap()
}
fn central(receipt: &PathMoveUndoReceipt) -> PathBuf {
    let id = super::super::super::super::parse_windows_journal_name(OsStr::new(&format!(
        "{}.cross-journal",
        origin(receipt).forward_transaction_id
    )))
    .unwrap()
    .0;
    windows_cross_volume_journal_path(&root(), id)
}
fn local(receipt: &PathMoveUndoReceipt) -> PathBuf {
    path(&binding(receipt).directory)
        .unwrap()
        .join(WINDOWS_LOCAL_JOURNAL_NAME)
}
fn content(path: &Path, kind: u8) -> Vec<u8> {
    if kind == 0 {
        std::fs::read(path).unwrap()
    } else {
        std::fs::read(path.join("child")).unwrap()
    }
}
fn assert_done(receipt: &PathMoveUndoReceipt) {
    let record = load_windows_cross_volume_record(&central(receipt)).unwrap();
    assert_eq!(record.phase, Phase::Undone);
    let local = load_windows_cross_volume_record(&local(receipt)).unwrap();
    assert_eq!(local, record);
    assert!(!binding(receipt).retained_path().unwrap().exists());
    let visible =
        super::super::super::super::discover_retained_windows_move_transactions().unwrap();
    assert_eq!(
        visible.len(),
        1,
        "one logical recovery row for controller and inverse"
    );
    assert_eq!(
        visible[0].undo_status,
        move_recovery::WindowsMoveUndoStatus::Complete
    );
    assert_eq!(Some(visible[0].transaction_id), record.undo_transaction);
}
struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        super::super::super::super::WINDOWS_MOVE_HOOK.with(|hook| *hook.borrow_mut() = None);
    }
}

#[test]
#[ignore = "requires an owned second native Windows volume"]
fn windows_native_cross_replace_undo_restores_current_files_trees_and_raw_links() {
    for kind in 0..3 {
        let fixture = Fixture::new();
        let receipt = fixture.moved(kind);
        let retained = binding(&receipt).retained_path().unwrap();
        if kind < 2 {
            let edited = |p: &Path| {
                if kind == 0 {
                    p.to_path_buf()
                } else {
                    p.join("child")
                }
            };
            std::fs::write(edited(receipt.after_path()), b"source edited after Move").unwrap();
            std::fs::write(edited(&retained), b"retained edited after Move").unwrap();
        }
        let stream = PathBuf::from(format!("{}:retained-stream", retained.display()));
        if kind == 0 {
            std::fs::write(&stream, b"preserve retained alternate stream").unwrap();
        }
        if kind == 1 {
            std::fs::write(receipt.after_path().join("added"), b"added source child").unwrap();
            std::fs::write(retained.join("added"), b"added retained child").unwrap();
        }
        let original_retained_id = binding(&receipt).entry;
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
        assert_eq!(
            move_recovery::stable_identity_of_entry(receipt.after_path()).unwrap(),
            original_retained_id
        );
        if kind < 2 {
            assert_eq!(
                content(receipt.before_path(), kind),
                b"source edited after Move"
            );
            assert_eq!(
                content(receipt.after_path(), kind),
                b"retained edited after Move"
            );
        } else {
            assert_eq!(
                std::fs::read_link(receipt.before_path()).unwrap(),
                Path::new("source-link-target")
            );
            assert_eq!(
                std::fs::read_link(receipt.after_path()).unwrap(),
                Path::new("retained-link-target")
            );
        }
        if kind == 0 {
            assert_eq!(
                std::fs::read(format!(
                    "{}:retained-stream",
                    receipt.after_path().display()
                ))
                .unwrap(),
                b"preserve retained alternate stream"
            );
        }
        if kind == 1 {
            assert_eq!(
                std::fs::read(receipt.before_path().join("added")).unwrap(),
                b"added source child"
            );
            assert_eq!(
                std::fs::read(receipt.after_path().join("added")).unwrap(),
                b"added retained child"
            );
        }
        assert_done(&receipt);
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
        assert_done(&receipt);
    }
}

#[test]
#[ignore = "requires an owned second native Windows volume"]
fn windows_native_cross_replace_undo_original_collision_leaves_intent_and_bytes_unchanged() {
    let fixture = Fixture::new();
    let receipt = fixture.moved(0);
    let before = std::fs::read(central(&receipt)).unwrap();
    let local_before = std::fs::read(local(&receipt)).unwrap();
    std::fs::write(receipt.before_path(), b"later original-name occupant").unwrap();
    assert!(matches!(
        DefaultPlatform.undo_move_with_receipt(&receipt),
        Err(PlatformError::DestinationExists(_))
    ));
    assert!(guard(&receipt).inverse.get().is_none());
    assert_eq!(std::fs::read(central(&receipt)).unwrap(), before);
    assert_eq!(std::fs::read(local(&receipt)).unwrap(), local_before);
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"later original-name occupant"
    );
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"source bytes"
    );
    assert_eq!(
        std::fs::read(binding(&receipt).retained_path().unwrap()).unwrap(),
        b"retained bytes"
    );
    let parked = fixture.primary.path().join("parked-occupant");
    std::fs::rename(receipt.before_path(), &parked).unwrap();
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    assert_done(&receipt);
    assert_eq!(
        std::fs::read(parked).unwrap(),
        b"later original-name occupant"
    );
}

#[test]
#[ignore = "requires an owned second native Windows volume"]
fn windows_native_cross_replace_undo_target_race_retries_without_copying_back_twice() {
    let fixture = Fixture::new();
    let receipt = fixture.moved(0);
    let after = receipt.after_path().to_path_buf();
    let before = receipt.before_path().to_path_buf();
    let seen = std::rc::Rc::new(std::cell::Cell::new(false));
    let observed = seen.clone();
    super::super::super::super::WINDOWS_MOVE_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |boundary, _| {
            if boundary == WindowsMoveBoundary::CrossRetainedBeforeRestore
                && !observed.replace(true)
            {
                assert!(
                    std::fs::rename(&before, before.with_extension("raced")).is_err(),
                    "restored source stays pinned"
                );
                std::fs::write(&after, b"raced destination occupant").unwrap();
            }
        }))
    });
    let reset = Reset;
    assert!(matches!(
        DefaultPlatform.undo_move_with_receipt(&receipt),
        Err(PlatformError::CommittedUndoCleanupPending { .. })
    ));
    assert!(seen.get());
    drop(reset);
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"source bytes"
    );
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"raced destination occupant"
    );
    assert_eq!(
        std::fs::read(binding(&receipt).retained_path().unwrap()).unwrap(),
        b"retained bytes"
    );
    let restored_id = move_recovery::stable_identity_of_entry(receipt.before_path()).unwrap();
    std::fs::write(receipt.before_path(), b"source edited while Undo pending").unwrap();
    let unrelated = fixture.primary.path().join("unrelated");
    std::fs::write(&unrelated, b"untouched unrelated").unwrap();
    assert!(
        DefaultPlatform
            .execute_move_with_undo_receipt(
                &unrelated,
                receipt.after_path().parent().unwrap(),
                ConflictAction::Skip
            )
            .is_err()
    );
    assert_eq!(std::fs::read(unrelated).unwrap(), b"untouched unrelated");
    let parked = fixture.secondary.path().join("parked-target");
    std::fs::rename(receipt.after_path(), &parked).unwrap();
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    assert_eq!(
        move_recovery::stable_identity_of_entry(receipt.before_path()).unwrap(),
        restored_id
    );
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"source edited while Undo pending"
    );
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"retained bytes"
    );
    assert_eq!(
        std::fs::read(parked).unwrap(),
        b"raced destination occupant"
    );
    assert_done(&receipt);
}

#[test]
#[ignore = "requires an owned second native Windows volume"]
fn windows_native_cross_replace_recovery_plan_binds_both_owners_before_mutation() {
    let fixture = Fixture::new();
    let receipt = fixture.moved(0);
    let independently_issued = super::super::issue_if_owned(
        &root(),
        &load_windows_cross_volume_record(&central(&receipt)).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert!(guard(&independently_issued).inverse.get().is_none());
    let before = receipt.before_path().to_path_buf();
    super::super::super::super::WINDOWS_MOVE_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |boundary, _| {
            if boundary == WindowsMoveBoundary::CrossRetainedUndoIntent {
                std::fs::write(&before, b"later original-name occupant").unwrap();
            }
        }))
    });
    let reset = Reset;
    assert!(DefaultPlatform.undo_move_with_receipt(&receipt).is_err());
    drop(reset);
    let inverse_id = guard(&receipt)
        .inverse
        .get()
        .unwrap()
        .transaction_id()
        .unwrap();
    let controller = load_windows_cross_volume_record(&central(&receipt)).unwrap();
    let controller_id = controller.transaction_id().unwrap();
    let first = crate::WindowsMoveRecoveryPlan::capture(inverse_id).unwrap();
    let second = crate::WindowsMoveRecoveryPlan::capture(controller_id).unwrap();
    assert_eq!(
        first.undo_status_for(&receipt),
        Some(move_recovery::WindowsMoveUndoStatus::Pending)
    );
    assert_eq!(
        second.undo_status_for(&receipt),
        first.undo_status_for(&receipt)
    );
    assert_eq!(first.paths(), second.paths());
    for expected in [
        root(),
        receipt.before_path().parent().unwrap().to_owned(),
        receipt.after_path().parent().unwrap().to_owned(),
        controller.transaction_path().unwrap(),
    ] {
        assert!(
            first
                .paths()
                .contains(&std::fs::canonicalize(&expected).unwrap()),
            "missing recovery footprint: {}",
            expected.display()
        );
    }
    let saved = std::fs::read(central(&receipt)).unwrap();
    let mut changed = controller;
    changed.phase = Phase::SourceRemoved;
    changed.undo_transaction = None;
    let changed = serde_json::to_vec(&changed).unwrap();
    std::fs::write(central(&receipt), &changed).unwrap();
    for plan in [&first, &second] {
        assert!(
            plan.execute()
                .unwrap_err()
                .to_string()
                .contains("evidence changed")
        );
        assert_eq!(std::fs::read(central(&receipt)).unwrap(), changed);
        assert_eq!(
            std::fs::read(receipt.before_path()).unwrap(),
            b"later original-name occupant"
        );
        assert_eq!(
            std::fs::read(receipt.after_path()).unwrap(),
            b"source bytes"
        );
        assert_eq!(
            std::fs::read(binding(&receipt).retained_path().unwrap()).unwrap(),
            b"retained bytes"
        );
    }
    std::fs::write(central(&receipt), saved).unwrap();
    let parked = fixture.primary.path().join("parked");
    std::fs::rename(receipt.before_path(), &parked).unwrap();
    first.execute().unwrap();
    assert_done(&receipt);
    let completed_inverse = crate::WindowsMoveRecoveryPlan::capture(inverse_id).unwrap();
    let completed_controller = crate::WindowsMoveRecoveryPlan::capture(controller_id).unwrap();
    for plan in [&completed_inverse, &completed_controller] {
        assert_eq!(
            plan.undo_status_for(&receipt),
            Some(move_recovery::WindowsMoveUndoStatus::Complete)
        );
        assert_eq!(
            plan.undo_status_for(&independently_issued),
            Some(move_recovery::WindowsMoveUndoStatus::Complete),
            "independent inverse completion matches the exact original native publication"
        );
    }
    assert_eq!(
        first.undo_status_for(&receipt),
        Some(move_recovery::WindowsMoveUndoStatus::Pending),
        "cached observations never read or update themselves on the UI thread"
    );
    let parked_registry = root().with_extension("parked");
    std::fs::rename(root(), &parked_registry).unwrap();
    assert_eq!(
        completed_controller.undo_status_for(&independently_issued),
        Some(move_recovery::WindowsMoveUndoStatus::Complete),
        "history reconciliation performs no filesystem access"
    );
    std::fs::rename(parked_registry, root()).unwrap();
    assert_eq!(
        std::fs::read(parked).unwrap(),
        b"later original-name occupant"
    );
    assert!(
        second
            .execute()
            .unwrap_err()
            .to_string()
            .contains("evidence changed")
    );
    crate::WindowsMoveRecoveryPlan::capture(controller_id)
        .unwrap()
        .execute()
        .unwrap();
}

#[test]
#[ignore = "requires an owned second native Windows volume"]
fn windows_native_cross_replace_undo_refuses_changed_retained_entry_or_private_parent() {
    for changed_parent in [false, true] {
        let fixture = Fixture::new();
        let receipt = fixture.moved(0);
        let retained = binding(&receipt).retained_path().unwrap();
        let replaced = if changed_parent {
            path(&binding(&receipt).directory).unwrap()
        } else {
            retained.clone()
        };
        let parked = fixture.secondary.path().join("parked-original");
        std::fs::rename(&replaced, &parked).unwrap();
        if changed_parent {
            std::fs::create_dir(&replaced).unwrap();
            std::fs::write(
                replaced.join(WINDOWS_LOCAL_JOURNAL_NAME),
                std::fs::read(parked.join(WINDOWS_LOCAL_JOURNAL_NAME)).unwrap(),
            )
            .unwrap();
        }
        std::fs::write(&retained, b"foreign private entry").unwrap();
        assert!(DefaultPlatform.undo_move_with_receipt(&receipt).is_err());
        assert!(!receipt.before_path().exists());
        assert_eq!(
            std::fs::read(receipt.after_path()).unwrap(),
            b"source bytes"
        );
        assert_eq!(std::fs::read(retained).unwrap(), b"foreign private entry");
        assert_eq!(
            std::fs::read(if changed_parent {
                parked.join(WINDOWS_RETAINED_NAME)
            } else {
                parked
            })
            .unwrap(),
            b"retained bytes"
        );
    }
}

#[test]
#[ignore = "requires an owned second native Windows volume"]
fn windows_native_cross_replace_undo_rejects_noncausal_and_relinked_controller_records() {
    let fixture = Fixture::new();
    let receipt = fixture.moved(0);
    let before = receipt.before_path().to_path_buf();
    super::super::super::super::WINDOWS_MOVE_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |boundary, _| {
            if boundary == WindowsMoveBoundary::CrossRetainedUndoIntent {
                std::fs::write(&before, b"pause at original collision").unwrap();
            }
        }))
    });
    let reset = Reset;
    assert!(DefaultPlatform.undo_move_with_receipt(&receipt).is_err());
    drop(reset);
    let saved_central = std::fs::read(central(&receipt)).unwrap();
    let saved_local = std::fs::read(local(&receipt)).unwrap();
    let inverse_id = guard(&receipt)
        .inverse
        .get()
        .unwrap()
        .transaction_id()
        .unwrap();
    for relink in [false, true] {
        let mut central_record: Record = serde_json::from_slice(&saved_central).unwrap();
        let mut local_record: Record = serde_json::from_slice(&saved_local).unwrap();
        if relink {
            central_record.undo_transaction = Some([0x91; 16]);
            local_record.undo_transaction = Some([0x91; 16]);
        } else {
            central_record.phase = Phase::SourceRemoved;
            central_record.undo_transaction = None;
        }
        let changed_central = serde_json::to_vec(&central_record).unwrap();
        let changed_local = serde_json::to_vec(&local_record).unwrap();
        std::fs::write(central(&receipt), &changed_central).unwrap();
        std::fs::write(local(&receipt), &changed_local).unwrap();
        assert!(DefaultPlatform.undo_move_with_receipt(&receipt).is_err());
        assert!(
            super::super::super::super::recover_retained_windows_move_transaction(inverse_id)
                .is_err()
        );
        assert_eq!(std::fs::read(central(&receipt)).unwrap(), changed_central);
        assert_eq!(std::fs::read(local(&receipt)).unwrap(), changed_local);
        assert_eq!(
            std::fs::read(receipt.before_path()).unwrap(),
            b"pause at original collision"
        );
        assert_eq!(
            std::fs::read(receipt.after_path()).unwrap(),
            b"source bytes"
        );
        assert_eq!(
            std::fs::read(binding(&receipt).retained_path().unwrap()).unwrap(),
            b"retained bytes"
        );
        std::fs::write(central(&receipt), &saved_central).unwrap();
        std::fs::write(local(&receipt), &saved_local).unwrap();
    }
    std::fs::rename(
        receipt.before_path(),
        fixture.primary.path().join("parked-collision"),
    )
    .unwrap();
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    assert_done(&receipt);
}

#[test]
#[ignore = "requires an owned second native Windows volume"]
fn windows_native_cross_replace_undo_terminal_history_releases_names_and_preserves_unknown_content()
{
    let fixture = Fixture::new();
    let receipt = fixture.moved(0);
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    assert_done(&receipt);
    let inverse = guard(&receipt).inverse.get().unwrap();
    let unknown_before = inverse.transaction_path().unwrap().join("unknown-before");
    let unknown_after = path(&binding(&receipt).directory)
        .unwrap()
        .join("unknown-after");
    std::fs::write(&unknown_before, b"preserve unknown before").unwrap();
    std::fs::write(&unknown_after, b"preserve unknown after").unwrap();
    let paths = [
        central(&receipt),
        local(&receipt),
        windows_cross_volume_journal_path(&root(), inverse.transaction_id().unwrap()),
        inverse
            .transaction_path()
            .unwrap()
            .join(WINDOWS_LOCAL_JOURNAL_NAME),
    ];
    let snapshots: Vec<_> = paths
        .iter()
        .map(|path| std::fs::read(path).unwrap())
        .collect();
    std::fs::rename(
        receipt.before_path(),
        fixture.primary.path().join("source-parked"),
    )
    .unwrap();
    std::fs::rename(
        receipt.after_path(),
        fixture.secondary.path().join("retained-parked"),
    )
    .unwrap();
    std::fs::write(receipt.before_path(), b"later before name").unwrap();
    std::fs::write(receipt.after_path(), b"later after name").unwrap();
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    let other = fixture.primary.path().join("unrelated");
    std::fs::write(&other, b"unrelated Move").unwrap();
    DefaultPlatform
        .execute_move_with_undo_receipt(
            &other,
            receipt.after_path().parent().unwrap(),
            ConflictAction::Skip,
        )
        .unwrap();
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"later before name"
    );
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"later after name"
    );
    assert_eq!(
        std::fs::read(unknown_before).unwrap(),
        b"preserve unknown before"
    );
    assert_eq!(
        std::fs::read(unknown_after).unwrap(),
        b"preserve unknown after"
    );
    for (path, saved) in paths.iter().zip(snapshots) {
        assert_eq!(std::fs::read(path).unwrap(), saved);
    }
}

#[test]
#[ignore = "requires an owned second native Windows volume"]
fn windows_native_cross_replace_undo_process_exits_resume_either_owner_without_session_receipt() {
    if let Ok(encoded) = std::env::var("OTTRIN_CROSS_REPLACE_UNDO_CHILD") {
        let origin: Origin = serde_json::from_str(&encoded).unwrap();
        let before = path(&origin.original).unwrap();
        let after = path(&origin.publication).unwrap();
        let identity = origin.publication_identity;
        let binding = origin.retained.as_ref().unwrap();
        let displaced_identity = Some(PathMoveIdentity {
            windows: binding.entry,
        });
        let retained_displaced_path = Some(binding.retained_path().unwrap());
        let receipt = PathMoveUndoReceipt {
            windows_undo: Some(WindowsPathUndo::Move(WindowsMoveUndoGuard::CrossVolume(
                Box::new(Guard {
                    origin,
                    inverse: Arc::new(OnceLock::new()),
                }),
            ))),
            before_path: before,
            after_path: after,
            identity: PathMoveIdentity { windows: identity },
            displaced_identity,
            retained_displaced_path,
            requirement: PathMoveUndoRequirement::None,
        };
        let stop = std::env::var("OTTRIN_CROSS_REPLACE_UNDO_STOP").unwrap();
        super::super::super::super::WINDOWS_MOVE_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |boundary, path| {
                if stop == format!("{boundary:?}") {
                    std::process::exit(87);
                }
                if boundary != WindowsMoveBoundary::JournalPublished {
                    return;
                }
                let record = load_windows_cross_volume_record(path).unwrap();
                let role = if record.undo_origin.is_some() {
                    "inverse"
                } else {
                    "controller"
                };
                let side = if path.extension() == Some(OsStr::new("cross-journal")) {
                    "central"
                } else {
                    "local"
                };
                if stop == format!("{role}-{:?}-{side}", record.phase) {
                    std::process::exit(87);
                }
            }))
        });
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
        panic!("missed retained inverse exit boundary");
    }
    for kind in 0..2 {
        for (index, stop) in [
            "inverse-Prepared-central",
            "controller-Undoing-central",
            "controller-Undoing-local",
            "CrossRetainedUndoIntent",
            "inverse-Published-central",
            "inverse-SourceRemoved-central",
            "CrossRetainedBeforeRestore",
            "CrossRetainedRestored",
            "controller-Undone-central",
            "controller-Undone-local",
            "CrossRetainedUndoComplete",
        ]
        .into_iter()
        .enumerate()
        {
            let fixture = Fixture::new();
            let receipt = fixture.moved(kind);
            let retained = binding(&receipt).retained_path().unwrap();
            let source = if kind == 0 {
                receipt.after_path().to_path_buf()
            } else {
                receipt.after_path().join("child")
            };
            let original = if kind == 0 {
                retained
            } else {
                retained.join("child")
            };
            std::fs::write(source, b"current source at interruption").unwrap();
            std::fs::write(original, b"current retained at interruption").unwrap();
            let result=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","move_transaction::windows_undo::cross::retained::tests::windows_native_cross_replace_undo_process_exits_resume_either_owner_without_session_receipt","--include-ignored","--nocapture"])
                .env("OTTRIN_CROSS_REPLACE_UNDO_CHILD",serde_json::to_string(origin(&receipt)).unwrap()).env("OTTRIN_CROSS_REPLACE_UNDO_STOP",stop).env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT",root()).status().unwrap();
            assert_eq!(result.code(), Some(87), "{kind}/{stop}");
            let visible =
                super::super::super::super::discover_retained_windows_move_transactions().unwrap();
            assert_eq!(visible.len(), 1, "{kind}/{stop}");
            let controller = load_windows_cross_volume_record(&central(&receipt)).unwrap();
            let id = if index % 2 == 0 && controller.undo_transaction.is_some() {
                controller.transaction_id().unwrap()
            } else {
                visible[0].transaction_id
            };
            super::super::super::super::recover_retained_windows_move_transaction(id).unwrap();
            assert_eq!(
                content(receipt.before_path(), kind),
                b"current source at interruption"
            );
            assert_eq!(
                content(receipt.after_path(), kind),
                b"current retained at interruption"
            );
            let controller = load_windows_cross_volume_record(&central(&receipt)).unwrap();
            assert_eq!(controller.phase, Phase::Undone);
            let visible =
                super::super::super::super::discover_retained_windows_move_transactions().unwrap();
            assert_eq!(visible.len(), 1);
            assert_eq!(
                visible[0].undo_status,
                move_recovery::WindowsMoveUndoStatus::Complete
            );
            super::super::super::super::recover_retained_windows_move_transaction(id).unwrap();
        }
    }
}

#[test]
#[ignore = "requires an owned second native Windows volume"]
fn windows_native_cross_replace_undo_follows_both_parent_ids_before_inverse_creation() {
    for kind in 0..3 {
        let fixture = Fixture::new();
        let receipt = fixture.moved(kind);
        let retained_identity = receipt.displaced_identity.as_ref().unwrap().windows;
        let stale = crate::WindowsMoveUndoPlan::capture(&receipt, &[]).unwrap();
        let current_before = fixture.primary.path().join("renamed-source");
        let current_after = fixture.secondary.path().join("renamed-destination");
        std::fs::rename(receipt.before_path().parent().unwrap(), &current_before).unwrap();
        std::fs::rename(receipt.after_path().parent().unwrap(), &current_after).unwrap();
        std::fs::create_dir(receipt.before_path().parent().unwrap()).unwrap();
        std::fs::create_dir(receipt.after_path().parent().unwrap()).unwrap();
        std::fs::write(receipt.before_path(), b"foreign source").unwrap();
        std::fs::write(receipt.after_path(), b"foreign destination").unwrap();
        assert!(stale.execute().is_err());
        assert!(guard(&receipt).inverse.get().is_none());
        let plan = crate::WindowsMoveUndoPlan::capture(&receipt, &[]).unwrap();
        plan.execute().unwrap();
        let restored = current_before.join("entry");
        let displaced = current_after.join("entry");
        assert_eq!(
            move_recovery::stable_identity_of_entry(&displaced).unwrap(),
            retained_identity
        );
        match kind {
            0 => {
                assert_eq!(std::fs::read(&restored).unwrap(), b"source bytes");
                assert_eq!(std::fs::read(&displaced).unwrap(), b"retained bytes");
            }
            1 => {
                assert_eq!(
                    std::fs::read(restored.join("child")).unwrap(),
                    b"source bytes"
                );
                assert_eq!(
                    std::fs::read(displaced.join("child")).unwrap(),
                    b"retained bytes"
                );
            }
            _ => {
                assert_eq!(
                    std::fs::read_link(&restored).unwrap(),
                    Path::new("source-link-target")
                );
                assert_eq!(
                    std::fs::read_link(&displaced).unwrap(),
                    Path::new("retained-link-target")
                );
            }
        }
        crate::WindowsMoveUndoPlan::capture(&receipt, &[])
            .unwrap()
            .execute()
            .unwrap();
        assert_eq!(
            std::fs::read(receipt.before_path()).unwrap(),
            b"foreign source"
        );
        assert_eq!(
            std::fs::read(receipt.after_path()).unwrap(),
            b"foreign destination"
        );
    }
}
