use super::*;
use crate::{ConflictAction, DefaultPlatform, PlatformOps};
use std::{os::windows::fs::OpenOptionsExt, path::PathBuf};

struct Fixture {
    root: crate::tests::TestDir,
    previous: Option<PathBuf>,
}
impl Fixture {
    fn new() -> Self {
        let root = crate::tests::TestDir::new("native-replace-undo");
        let previous =
            super::super::super::replace_test_recovery_root(Some(root.path().join("state")));
        Self { root, previous }
    }
    fn moved(&self, kind: u8) -> (PathMoveUndoReceipt, Record) {
        self.moved_kinds(kind, kind)
    }
    fn moved_kinds(&self, source_kind: u8, destination_kind: u8) -> (PathMoveUndoReceipt, Record) {
        let source = self.root.path().join("source/entry");
        let destination = self.root.path().join("destination");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::create_dir(&destination).unwrap();
        for (path, bytes, kind) in [
            (&source, b"selected original".as_slice(), source_kind),
            (
                &destination.join("entry"),
                b"displaced original".as_slice(),
                destination_kind,
            ),
        ] {
            match kind {
                0 => std::fs::write(path, bytes).unwrap(),
                1 => {
                    std::fs::create_dir(path).unwrap();
                    std::fs::write(path.join("child"), bytes).unwrap();
                }
                _ => std::os::windows::fs::symlink_file(
                    if bytes[0] == b's' {
                        "selected-link-target"
                    } else {
                        "displaced-link-target"
                    },
                    path,
                )
                .unwrap(),
            }
        }
        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination, ConflictAction::Overwrite)
            .unwrap();
        let receipt = receipt.expect("Replace has guarded native Undo");
        let Some(WindowsPathUndo::Move(guard)) = &receipt.windows_undo else {
            panic!("wrong native receipt")
        };
        let record = guard
            .retained
            .as_deref()
            .expect("retained identity authority")
            .clone();
        (receipt, record)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        super::super::super::replace_test_recovery_root(self.previous.take());
    }
}
fn root() -> PathBuf {
    super::super::super::windows_recovery_root().unwrap()
}
fn journal(record: &Record) -> PathBuf {
    windows_journal_path(&root(), record.transaction_id().unwrap())
}
fn local(record: &Record) -> PathBuf {
    record
        .transaction_path()
        .unwrap()
        .join(WINDOWS_LOCAL_JOURNAL_NAME)
}
fn assert_terminal(record: &Record) {
    assert_eq!(
        load_windows_overwrite_record(&journal(record))
            .unwrap()
            .phase,
        Phase::Undone
    );
    assert_eq!(
        load_windows_overwrite_record(&local(record)).unwrap().phase,
        Phase::Undone
    );
    assert!(!record.retained_path().unwrap().exists());
    let found = super::super::super::discover_retained_windows_move_transactions().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].undo_status,
        move_recovery::WindowsMoveUndoStatus::Complete
    );
}

#[test]
fn windows_native_replace_undo_restores_both_entries_and_later_edits() {
    for kind in 0..3 {
        let fixture = Fixture::new();
        let (receipt, record) = fixture.moved(kind);
        if kind < 2 {
            let edited = if kind == 0 {
                receipt.after_path().to_path_buf()
            } else {
                receipt.after_path().join("child")
            };
            std::fs::write(edited, b"edited after Move").unwrap();
            let retained = if kind == 0 {
                record.retained_path().unwrap()
            } else {
                record.retained_path().unwrap().join("child")
            };
            std::fs::write(retained, b"edited retained original").unwrap();
        }
        assert_eq!(receipt.path_pairs().len(), 2);
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
        assert_eq!(
            move_recovery::stable_identity_of_entry(receipt.before_path()).unwrap(),
            record.source_identity
        );
        assert_eq!(
            move_recovery::stable_identity_of_entry(receipt.after_path()).unwrap(),
            record.retained_identity
        );
        if kind < 2 {
            let content = |path: &Path| {
                if kind == 0 {
                    std::fs::read(path).unwrap()
                } else {
                    std::fs::read(path.join("child")).unwrap()
                }
            };
            assert_eq!(content(receipt.before_path()), b"edited after Move");
            assert_eq!(content(receipt.after_path()), b"edited retained original");
        } else {
            assert_eq!(
                std::fs::read_link(receipt.before_path()).unwrap(),
                Path::new("selected-link-target")
            );
            assert_eq!(
                std::fs::read_link(receipt.after_path()).unwrap(),
                Path::new("displaced-link-target")
            );
        }
        assert_terminal(&record);
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    }
}

#[test]
fn windows_native_replace_undo_collision_is_non_mutating_and_retryable() {
    let fixture = Fixture::new();
    let (receipt, record) = fixture.moved(0);
    let central = std::fs::read(journal(&record)).unwrap();
    let local_before = std::fs::read(local(&record)).unwrap();
    std::fs::write(receipt.before_path(), b"later occupant").unwrap();
    assert!(matches!(
        DefaultPlatform.undo_move_with_receipt(&receipt),
        Err(PlatformError::DestinationExists(_))
    ));
    assert_eq!(std::fs::read(journal(&record)).unwrap(), central);
    assert_eq!(std::fs::read(local(&record)).unwrap(), local_before);
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"later occupant"
    );
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"selected original"
    );
    assert_eq!(
        std::fs::read(record.retained_path().unwrap()).unwrap(),
        b"displaced original"
    );
    let parked = fixture.root.path().join("parked-later");
    std::fs::rename(receipt.before_path(), &parked).unwrap();
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    assert_terminal(&record);
    assert_eq!(std::fs::read(parked).unwrap(), b"later occupant");
}

#[test]
fn windows_native_replace_undo_refuses_replaced_retained_entry_and_parent() {
    for parent in [false, true] {
        let fixture = Fixture::new();
        let (receipt, record) = fixture.moved(0);
        let retained = record.retained_path().unwrap();
        let original = if parent {
            record.transaction_path().unwrap()
        } else {
            retained.clone()
        };
        let parked = fixture.root.path().join("parked-original");
        std::fs::rename(&original, &parked).unwrap();
        if parent {
            std::fs::create_dir(&original).unwrap();
            std::fs::write(
                original.join(WINDOWS_LOCAL_JOURNAL_NAME),
                std::fs::read(parked.join(WINDOWS_LOCAL_JOURNAL_NAME)).unwrap(),
            )
            .unwrap();
            std::fs::write(&retained, b"foreign private entry").unwrap();
        } else {
            std::fs::write(&original, b"foreign private entry").unwrap();
        }
        assert!(DefaultPlatform.undo_move_with_receipt(&receipt).is_err());
        assert!(!receipt.before_path().exists());
        assert_eq!(
            std::fs::read(receipt.after_path()).unwrap(),
            b"selected original"
        );
        assert_eq!(std::fs::read(retained).unwrap(), b"foreign private entry");
        let old = if parent {
            parked.join(WINDOWS_RETAINED_NAME)
        } else {
            parked
        };
        assert_eq!(std::fs::read(old).unwrap(), b"displaced original");
    }
}

#[test]
fn windows_native_replace_undo_sharing_refuses_before_intent_and_retries() {
    let fixture = Fixture::new();
    let (receipt, record) = fixture.moved(0);
    let blocked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(record.retained_path().unwrap())
        .unwrap();
    assert!(DefaultPlatform.undo_move_with_receipt(&receipt).is_err());
    assert_eq!(
        load_windows_overwrite_record(&journal(&record))
            .unwrap()
            .phase,
        Phase::Published
    );
    assert!(!receipt.before_path().exists());
    drop(blocked);
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    assert_terminal(&record);
}

#[test]
fn windows_native_replace_undo_terminal_history_releases_later_public_names() {
    let fixture = Fixture::new();
    let (receipt, record) = fixture.moved(0);
    let unknown = record.transaction_path().unwrap().join("user-added");
    std::fs::write(&unknown, b"preserve unknown private content").unwrap();
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    std::fs::rename(
        receipt.before_path(),
        fixture.root.path().join("selected-parked"),
    )
    .unwrap();
    std::fs::rename(
        receipt.after_path(),
        fixture.root.path().join("displaced-parked"),
    )
    .unwrap();
    std::fs::write(receipt.before_path(), b"later source").unwrap();
    std::fs::write(receipt.after_path(), b"later target").unwrap();
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    let unrelated = fixture.root.path().join("unrelated");
    std::fs::write(&unrelated, b"unrelated Move").unwrap();
    DefaultPlatform
        .execute_move_with_undo_receipt(
            &unrelated,
            receipt.after_path().parent().unwrap(),
            ConflictAction::Skip,
        )
        .unwrap();
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"later source"
    );
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"later target"
    );
    assert_eq!(
        std::fs::read(unknown).unwrap(),
        b"preserve unknown private content"
    );
    assert_terminal(&record);
}

#[test]
fn windows_native_replace_undo_rejects_noncausal_or_changed_journals() {
    for altered_identity in [false, true] {
        let fixture = Fixture::new();
        let (receipt, mut record) = fixture.moved(0);
        if altered_identity {
            record.destination_identity = record.source_identity;
        } else {
            record.phase = Phase::Undoing;
        }
        std::fs::write(journal(&record), serde_json::to_vec(&record).unwrap()).unwrap();
        let central = std::fs::read(journal(&record)).unwrap();
        let local_before = std::fs::read(local(&record)).unwrap();
        assert!(DefaultPlatform.undo_move_with_receipt(&receipt).is_err());
        assert!(
            super::super::super::recover_retained_windows_move_transaction(
                record.transaction_id().unwrap()
            )
            .is_err()
        );
        assert_eq!(std::fs::read(journal(&record)).unwrap(), central);
        assert_eq!(std::fs::read(local(&record)).unwrap(), local_before);
        assert!(!receipt.before_path().exists());
        assert_eq!(
            std::fs::read(receipt.after_path()).unwrap(),
            b"selected original"
        );
        assert_eq!(
            std::fs::read(record.retained_path().unwrap()).unwrap(),
            b"displaced original"
        );
    }
}

#[test]
fn windows_native_replace_undo_process_exit_recovers_from_paired_intent() {
    if let Ok(encoded) = std::env::var("OTTRIN_REPLACE_UNDO_CHILD") {
        let record: Record = serde_json::from_str(&encoded).unwrap();
        let receipt = issue(&record).unwrap();
        let stop = std::env::var("OTTRIN_REPLACE_UNDO_STOP").unwrap();
        super::super::super::WINDOWS_MOVE_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |boundary, path| {
                let name = format!("{boundary:?}");
                let local_phase = if name == "JournalPublished"
                    && path.file_name() == Some(OsStr::new(WINDOWS_LOCAL_JOURNAL_NAME))
                {
                    Some(load_windows_overwrite_record(path).unwrap().phase)
                } else {
                    None
                };
                if name == stop
                    || (stop == "local-intent" && local_phase == Some(Phase::Undoing))
                    || (stop == "local-complete" && local_phase == Some(Phase::Undone))
                {
                    std::process::exit(87);
                }
            }))
        });
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
        panic!("child missed interruption boundary");
    }
    for stop in [
        "local-intent",
        "OverwriteUndoIntent",
        "OverwriteUndoSourceRestored",
        "OverwriteUndoDisplacedRestored",
        "local-complete",
        "OverwriteUndoComplete",
    ] {
        let fixture = Fixture::new();
        let (receipt, record) = fixture.moved(0);
        let status=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","move_transaction::windows_undo::overwrite::tests::windows_native_replace_undo_process_exit_recovers_from_paired_intent","--nocapture"])
            .env("OTTRIN_REPLACE_UNDO_CHILD",serde_json::to_string(&record).unwrap()).env("OTTRIN_REPLACE_UNDO_STOP",stop)
            .env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT",root()).status().unwrap();
        assert_eq!(status.code(), Some(87), "{stop}");
        let central = load_windows_overwrite_record(&journal(&record)).unwrap();
        let local_record = load_windows_overwrite_record(&local(&record)).unwrap();
        if !completed(&root(), &central, &local_record).unwrap() {
            let unrelated = fixture.root.path().join("unrelated");
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
            let found = super::super::super::discover_retained_windows_move_transactions().unwrap();
            assert_eq!(
                found[0].undo_status,
                move_recovery::WindowsMoveUndoStatus::Pending
            );
        }
        super::super::super::recover_retained_windows_move_transaction(
            record.transaction_id().unwrap(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(receipt.before_path()).unwrap(),
            b"selected original"
        );
        assert_eq!(
            std::fs::read(receipt.after_path()).unwrap(),
            b"displaced original"
        );
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
        assert_terminal(&record);
    }
}

#[test]
fn windows_native_replace_undo_restores_mixed_entry_kinds() {
    for source_kind in 0..3 {
        for destination_kind in 0..3 {
            if source_kind == destination_kind {
                continue;
            }
            let fixture = Fixture::new();
            let (receipt, record) = fixture.moved_kinds(source_kind, destination_kind);
            DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
            assert_eq!(
                move_recovery::stable_identity_of_entry(receipt.before_path()).unwrap(),
                record.source_identity
            );
            assert_eq!(
                move_recovery::stable_identity_of_entry(receipt.after_path()).unwrap(),
                record.retained_identity
            );
            assert_terminal(&record);
        }
    }
}

#[test]
fn windows_native_replace_undo_pins_names_and_preserves_a_raced_original_collision() {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            super::super::super::WINDOWS_MOVE_HOOK.with(|hook| *hook.borrow_mut() = None);
        }
    }
    let fixture = Fixture::new();
    let (receipt, record) = fixture.moved(0);
    let before = receipt.before_path().to_path_buf();
    let after = receipt.after_path().to_path_buf();
    let retained = record.retained_path().unwrap();
    let observed = std::rc::Rc::new(std::cell::Cell::new(false));
    let observed_hook = observed.clone();
    super::super::super::WINDOWS_MOVE_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |boundary, _| {
            if boundary != super::super::super::WindowsMoveBoundary::OverwriteUndoIntent {
                return;
            }
            for path in [after.parent().unwrap(), after.as_path(), retained.as_path()] {
                assert!(std::fs::rename(path, path.with_extension("raced-rename")).is_err());
            }
            std::fs::write(&before, b"raced original occupant").unwrap();
            observed_hook.set(true);
        }))
    });
    let reset = Reset;
    assert!(matches!(
        DefaultPlatform.undo_move_with_receipt(&receipt),
        Err(PlatformError::CommittedUndoCleanupPending { .. })
    ));
    assert!(observed.get());
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"raced original occupant"
    );
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"selected original"
    );
    assert_eq!(
        std::fs::read(record.retained_path().unwrap()).unwrap(),
        b"displaced original"
    );
    drop(reset);
    let parked = fixture.root.path().join("parked-raced-occupant");
    std::fs::rename(receipt.before_path(), &parked).unwrap();
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    assert_eq!(std::fs::read(parked).unwrap(), b"raced original occupant");
    assert_terminal(&record);
}
