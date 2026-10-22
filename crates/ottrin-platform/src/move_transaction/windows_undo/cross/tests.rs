use super::*;
use crate::{ConflictAction, DefaultPlatform, PlatformOps};
use std::path::PathBuf;

struct Fixture {
    primary: crate::tests::TestDir,
    secondary: PathBuf,
    previous: Option<PathBuf>,
}
impl Fixture {
    fn new() -> Self {
        let primary = crate::tests::TestDir::new("native-cross-undo");
        let volume = std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT")
            .expect("owned second volume required");
        let secondary = PathBuf::from(volume).join(primary.path().file_name().unwrap());
        std::fs::create_dir(&secondary).unwrap();
        let previous =
            super::super::super::replace_test_recovery_root(Some(primary.path().join("state")));
        Self {
            primary,
            secondary,
            previous,
        }
    }
    fn moved(&self, kind: u8, keep_both: bool) -> PathMoveUndoReceipt {
        let source = self.primary.path().join("source/entry");
        let destination = self.secondary.join("destination");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::create_dir(&destination).unwrap();
        create(&source, kind, b"original content");
        if keep_both {
            create(&destination.join("entry"), kind, b"untouched occupant");
        }
        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(
                &source,
                &destination,
                if keep_both {
                    ConflictAction::Rename
                } else {
                    ConflictAction::Skip
                },
            )
            .unwrap();
        let receipt = receipt.expect("cross-volume native Undo authority");
        assert!(matches!(
            receipt.windows_undo,
            Some(WindowsPathUndo::Move(WindowsMoveUndoGuard::CrossVolume(_)))
        ));
        assert!(!source.exists());
        if keep_both {
            assert_ne!(receipt.after_path().file_name(), source.file_name());
        }
        receipt
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        super::super::super::replace_test_recovery_root(self.previous.take());
        std::fs::remove_dir_all(&self.secondary).unwrap();
    }
}
fn create(path: &Path, kind: u8, bytes: &[u8]) {
    match kind {
        0 => std::fs::write(path, bytes).unwrap(),
        1 => {
            std::fs::create_dir(path).unwrap();
            std::fs::write(path.join("child"), bytes).unwrap();
        }
        _ => std::os::windows::fs::symlink_file("untouched-raw-link-target", path).unwrap(),
    }
}
fn guard(receipt: &PathMoveUndoReceipt) -> &Guard {
    let Some(WindowsPathUndo::Move(WindowsMoveUndoGuard::CrossVolume(guard))) =
        &receipt.windows_undo
    else {
        panic!("wrong native authority")
    };
    guard
}
fn root() -> PathBuf {
    super::super::super::windows_recovery_root().unwrap()
}
fn terminal_record(receipt: &PathMoveUndoReceipt) -> Record {
    let initial = guard(receipt).inverse.get().unwrap();
    let record = load_windows_cross_volume_record(&windows_cross_volume_journal_path(
        &root(),
        initial.transaction_id().unwrap(),
    ))
    .unwrap();
    assert_eq!(
        status(&root(), &record).unwrap(),
        move_recovery::WindowsMoveUndoStatus::Complete
    );
    record
}
struct HookReset;
impl Drop for HookReset {
    fn drop(&mut self) {
        super::super::super::WINDOWS_MOVE_HOOK.with(|hook| *hook.borrow_mut() = None);
    }
}

#[test]
#[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on an owned second volume"]
fn windows_native_cross_undo_restores_current_file_directory_and_raw_link_to_original_name() {
    for kind in 0..3 {
        let fixture = Fixture::new();
        let receipt = fixture.moved(kind, true);
        if kind == 0 {
            std::fs::write(receipt.after_path(), b"edited after forward Move").unwrap();
        }
        if kind == 1 {
            std::fs::write(
                receipt.after_path().join("child"),
                b"edited after forward Move",
            )
            .unwrap();
            std::fs::write(receipt.after_path().join("added"), b"new child after Move").unwrap();
        }
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
        assert!(std::fs::symlink_metadata(receipt.after_path()).is_err());
        if kind < 2 {
            let content = if kind == 0 {
                std::fs::read(receipt.before_path()).unwrap()
            } else {
                std::fs::read(receipt.before_path().join("child")).unwrap()
            };
            assert_eq!(content, b"edited after forward Move");
            let occupant = receipt.after_path().parent().unwrap().join("entry");
            assert_eq!(
                if kind == 0 {
                    std::fs::read(occupant).unwrap()
                } else {
                    std::fs::read(occupant.join("child")).unwrap()
                },
                b"untouched occupant"
            );
        } else {
            assert_eq!(
                std::fs::read_link(receipt.before_path()).unwrap(),
                Path::new("untouched-raw-link-target")
            );
        }
        if kind == 1 {
            assert_eq!(
                std::fs::read(receipt.before_path().join("added")).unwrap(),
                b"new child after Move"
            );
        }
        terminal_record(&receipt);
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    }
}

#[test]
#[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on an owned second volume"]
fn windows_native_cross_undo_original_name_collision_preserves_both_files_and_retries() {
    let fixture = Fixture::new();
    let receipt = fixture.moved(0, false);
    std::fs::write(receipt.before_path(), b"later original-name occupant").unwrap();
    assert!(matches!(
        DefaultPlatform.undo_move_with_receipt(&receipt),
        Err(PlatformError::DestinationExists(_))
    ));
    assert!(guard(&receipt).inverse.get().is_none());
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"later original-name occupant"
    );
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"original content"
    );
    let parked = fixture.primary.path().join("parked-occupant");
    std::fs::rename(receipt.before_path(), &parked).unwrap();
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    assert_eq!(
        std::fs::read(parked).unwrap(),
        b"later original-name occupant"
    );
    terminal_record(&receipt);
}

#[test]
#[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on an owned second volume"]
fn windows_native_cross_undo_refuses_changed_publication_before_intent() {
    let fixture = Fixture::new();
    let receipt = fixture.moved(0, false);
    let parked = receipt.after_path().with_extension("parked");
    std::fs::rename(receipt.after_path(), &parked).unwrap();
    std::fs::write(receipt.after_path(), b"foreign publication").unwrap();
    assert!(DefaultPlatform.undo_move_with_receipt(&receipt).is_err());
    assert!(guard(&receipt).inverse.get().is_none());
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"foreign publication"
    );
    assert_eq!(std::fs::read(parked).unwrap(), b"original content");
    assert!(!receipt.before_path().exists());
}

#[test]
#[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on an owned second volume"]
fn windows_native_cross_undo_refuses_added_stream_without_removing_current_data() {
    let fixture = Fixture::new();
    let receipt = fixture.moved(0, false);
    let stream = PathBuf::from(format!("{}:private-stream", receipt.after_path().display()));
    std::fs::write(&stream, b"later alternate stream").unwrap();
    assert!(DefaultPlatform.undo_move_with_receipt(&receipt).is_err());
    assert!(guard(&receipt).inverse.get().is_none());
    assert_eq!(std::fs::read(stream).unwrap(), b"later alternate stream");
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"original content"
    );
    assert!(!receipt.before_path().exists());
}

#[test]
#[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on an owned second volume"]
fn windows_native_cross_undo_raced_collision_preserves_files_and_resumes_bound_intent() {
    let fixture = Fixture::new();
    let receipt = fixture.moved(0, false);
    let before = receipt.before_path().to_path_buf();
    let after = receipt.after_path().to_path_buf();
    let once = std::rc::Rc::new(std::cell::Cell::new(false));
    let observed = once.clone();
    super::super::super::WINDOWS_MOVE_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |boundary, path| {
            if boundary != super::super::super::WindowsMoveBoundary::JournalPublished
                || path.extension() != Some(OsStr::new("cross-journal"))
            {
                return;
            }
            let record = load_windows_cross_volume_record(path).unwrap();
            if record.undo_origin.is_none()
                || record.phase != Phase::Staged
                || observed.replace(true)
            {
                return;
            }
            assert!(
                std::fs::rename(&after, after.with_extension("raced")).is_err(),
                "publication lock pins the selected source"
            );
            std::fs::write(&before, b"raced original occupant").unwrap();
        }))
    });
    let reset = HookReset;
    assert!(matches!(
        DefaultPlatform.undo_move_with_receipt(&receipt),
        Err(PlatformError::CommittedUndoCleanupPending { .. })
    ));
    assert!(once.get());
    drop(reset);
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"raced original occupant"
    );
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"original content"
    );
    let unrelated = fixture.primary.path().join("unrelated");
    std::fs::write(&unrelated, b"untouched unrelated Move").unwrap();
    assert!(
        DefaultPlatform
            .execute_move_with_undo_receipt(
                &unrelated,
                receipt.after_path().parent().unwrap(),
                ConflictAction::Skip
            )
            .is_err()
    );
    assert_eq!(
        std::fs::read(unrelated).unwrap(),
        b"untouched unrelated Move"
    );
    let initial = guard(&receipt).inverse.get().unwrap();
    let central = windows_cross_volume_journal_path(&root(), initial.transaction_id().unwrap());
    let local = initial
        .transaction_path()
        .unwrap()
        .join(WINDOWS_LOCAL_JOURNAL_NAME);
    let saved_central = std::fs::read(&central).unwrap();
    let saved_local = std::fs::read(&local).unwrap();
    let mut forged = load_windows_cross_volume_record(&central).unwrap();
    forged.undo_origin.as_mut().unwrap().forward_transaction_id =
        "00000000000000000000000000000000".into();
    let forged_bytes = serde_json::to_vec(&forged).unwrap();
    std::fs::write(&central, &forged_bytes).unwrap();
    assert!(DefaultPlatform.undo_move_with_receipt(&receipt).is_err());
    assert!(
        super::super::super::recover_retained_windows_move_transaction(
            initial.transaction_id().unwrap()
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&central).unwrap(), forged_bytes);
    assert_eq!(std::fs::read(&local).unwrap(), saved_local);
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"raced original occupant"
    );
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"original content"
    );
    std::fs::write(central, saved_central).unwrap();
    let parked = fixture.primary.path().join("parked-raced-occupant");
    std::fs::rename(receipt.before_path(), &parked).unwrap();
    let clone = receipt.clone();
    DefaultPlatform.undo_move_with_receipt(&clone).unwrap();
    terminal_record(&receipt);
    assert_eq!(std::fs::read(parked).unwrap(), b"raced original occupant");
}

#[test]
#[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on an owned second volume"]
fn windows_native_cross_undo_terminal_proof_preserves_later_names_and_unknown_private_data() {
    let fixture = Fixture::new();
    let receipt = fixture.moved(0, false);
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    let record = terminal_record(&receipt);
    let unknown = record.transaction_path().unwrap().join("user-added");
    std::fs::write(&unknown, b"preserve unknown private data").unwrap();
    let parked = fixture.primary.path().join("restored-parked");
    std::fs::rename(receipt.before_path(), &parked).unwrap();
    std::fs::write(receipt.before_path(), b"later original name").unwrap();
    std::fs::write(receipt.after_path(), b"later publication name").unwrap();
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    let unrelated = fixture.primary.path().join("unrelated");
    std::fs::write(&unrelated, b"unrelated content").unwrap();
    DefaultPlatform
        .execute_move_with_undo_receipt(
            &unrelated,
            receipt.after_path().parent().unwrap(),
            ConflictAction::Skip,
        )
        .unwrap();
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"later original name"
    );
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"later publication name"
    );
    assert_eq!(
        std::fs::read(unknown).unwrap(),
        b"preserve unknown private data"
    );
}

#[test]
#[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on an owned second volume"]
fn windows_native_cross_undo_process_exit_recovers_without_session_receipt() {
    if let Ok(encoded) = std::env::var("OTTRIN_CROSS_UNDO_CHILD") {
        let origin: Origin = serde_json::from_str(&encoded).unwrap();
        let before = path(&origin.original).unwrap();
        let after = path(&origin.publication).unwrap();
        let identity = origin.publication_identity;
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
            displaced_identity: None,
            retained_displaced_path: None,
            requirement: PathMoveUndoRequirement::None,
        };
        let stop = std::env::var("OTTRIN_CROSS_UNDO_STOP").unwrap();
        super::super::super::WINDOWS_MOVE_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |boundary, path| {
                if stop == format!("{boundary:?}") {
                    std::process::exit(87);
                }
                if boundary != super::super::super::WindowsMoveBoundary::JournalPublished {
                    return;
                }
                let record = load_windows_cross_volume_record(path).unwrap();
                if record.undo_origin.is_none() {
                    return;
                }
                let side = if path.extension() == Some(OsStr::new("cross-journal")) {
                    "central"
                } else {
                    "local"
                };
                if stop == format!("{:?}-{side}", record.phase) {
                    std::process::exit(87);
                }
            }))
        });
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
        panic!("missed cross-volume inverse exit boundary");
    }
    for kind in 0..2 {
        for renamed in [false, true] {
            for stop in [
                "Prepared-central",
                "Prepared-local",
                "Staged-central",
                "Published-central",
                "BeforeSourceCleanup",
                "SourceRemoved-central",
                "SourceRemoved-local",
            ] {
                let fixture = Fixture::new();
                let receipt = fixture.moved(kind, false);
                let status=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","move_transaction::windows_undo::cross::tests::windows_native_cross_undo_process_exit_recovers_without_session_receipt","--include-ignored","--nocapture"])
                .env("OTTRIN_CROSS_UNDO_CHILD",serde_json::to_string(&guard(&receipt).origin).unwrap()).env("OTTRIN_CROSS_UNDO_STOP",stop).env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT",root()).status().unwrap();
                assert_eq!(status.code(), Some(87), "{kind}/{stop}");
                let transactions =
                    super::super::super::discover_retained_windows_move_transactions().unwrap();
                assert_eq!(transactions.len(), 1);
                let id = transactions[0].transaction_id;
                let (before, after) = if renamed {
                    let current_before = fixture.primary.path().join("renamed-source");
                    let current_after = fixture.secondary.join("renamed-destination");
                    std::fs::rename(receipt.before_path().parent().unwrap(), &current_before)
                        .unwrap();
                    std::fs::rename(receipt.after_path().parent().unwrap(), &current_after)
                        .unwrap();
                    std::fs::create_dir(receipt.before_path().parent().unwrap()).unwrap();
                    std::fs::create_dir(receipt.after_path().parent().unwrap()).unwrap();
                    std::fs::write(receipt.before_path(), b"foreign source").unwrap();
                    std::fs::write(receipt.after_path(), b"foreign destination").unwrap();
                    (current_before.join("entry"), current_after.join("entry"))
                } else {
                    (
                        receipt.before_path().to_path_buf(),
                        receipt.after_path().to_path_buf(),
                    )
                };
                crate::WindowsMoveRecoveryPlan::capture(id)
                    .unwrap()
                    .execute()
                    .unwrap();
                assert_eq!(
                    if kind == 0 {
                        std::fs::read(&before).unwrap()
                    } else {
                        std::fs::read(before.join("child")).unwrap()
                    },
                    b"original content"
                );
                assert!(std::fs::symlink_metadata(&after).is_err());
                let transactions =
                    super::super::super::discover_retained_windows_move_transactions().unwrap();
                assert_eq!(
                    transactions[0].undo_status,
                    move_recovery::WindowsMoveUndoStatus::Complete
                );
                super::super::super::recover_retained_windows_move_transaction(id).unwrap();
                if renamed {
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
        }
    }
}

#[test]
#[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on an owned second volume"]
fn windows_native_cross_completed_cleanup_does_not_report_success_for_a_later_source() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let source = fixture.primary.path().join("source/entry");
    let destination = fixture.secondary.join("destination");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(&source, b"completed predecessor").unwrap();
    let held = std::rc::Rc::new(std::cell::RefCell::new(None));
    let capture = held.clone();
    super::super::super::WINDOWS_MOVE_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |boundary, _| {
            if boundary != super::super::super::WindowsMoveBoundary::TransactionDirectoryRemoved
                || capture.borrow().is_some()
            {
                return;
            }
            let central = std::fs::read_dir(root())
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.extension() == Some(OsStr::new("cross-journal")))
                .unwrap();
            *capture.borrow_mut() = Some(
                std::fs::OpenOptions::new()
                    .read(true)
                    .share_mode(FILE_SHARE_READ)
                    .open(central)
                    .unwrap(),
            );
        }))
    });
    let reset = HookReset;
    assert!(matches!(
        DefaultPlatform.execute_move_with_undo_receipt(
            &source,
            &destination,
            ConflictAction::Rename
        ),
        Err(PlatformError::CommittedCleanupPending { .. })
    ));
    assert!(held.borrow().is_some());
    assert!(!source.exists());
    assert_eq!(
        std::fs::read(destination.join("entry")).unwrap(),
        b"completed predecessor"
    );
    drop(reset);
    drop(held.borrow_mut().take());
    std::fs::write(&source, b"later file must actually move").unwrap();
    let (disposition, receipt) = DefaultPlatform
        .execute_move_with_undo_receipt(&source, &destination, ConflictAction::Rename)
        .unwrap();
    assert_eq!(disposition, crate::MoveDisposition::Moved);
    let receipt = receipt.expect("new file has its own publication authority");
    assert!(
        !source.exists(),
        "the new operation must not just report its predecessor's success"
    );
    assert_ne!(receipt.after_path(), destination.join("entry"));
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"later file must actually move"
    );
    assert_eq!(
        std::fs::read(destination.join("entry")).unwrap(),
        b"completed predecessor"
    );
}

#[test]
#[ignore = "requires an owned second native Windows volume"]
fn windows_native_cross_undo_admits_renamed_parents_and_refuses_stale_queue_plan() {
    for kind in 0..3 {
        for keep_both in [false, true] {
            let fixture = Fixture::new();
            let receipt = fixture.moved(kind, keep_both);
            let stale = crate::WindowsMoveUndoPlan::capture(&receipt).unwrap();
            let before_parent = receipt.before_path().parent().unwrap();
            let after_parent = receipt.after_path().parent().unwrap();
            let current_before = before_parent.with_extension("renamed");
            let current_after = after_parent.with_extension("renamed");
            std::fs::rename(before_parent, &current_before).unwrap();
            std::fs::rename(after_parent, &current_after).unwrap();
            std::fs::create_dir(before_parent).unwrap();
            std::fs::create_dir(after_parent).unwrap();
            std::fs::write(receipt.before_path(), b"foreign source").unwrap();
            std::fs::write(receipt.after_path(), b"foreign publication").unwrap();
            assert!(stale.execute().is_err());
            assert!(guard(&receipt).inverse.get().is_none());
            let plan = crate::WindowsMoveUndoPlan::capture(&receipt).unwrap();
            for parent in [&current_before, &current_after] {
                assert!(
                    plan.paths()
                        .contains(&std::fs::canonicalize(parent).unwrap())
                );
            }
            plan.execute().unwrap();
            let restored = current_before.join(receipt.before_path().file_name().unwrap());
            match kind {
                0 => assert_eq!(std::fs::read(&restored).unwrap(), b"original content"),
                1 => assert_eq!(
                    std::fs::read(restored.join("child")).unwrap(),
                    b"original content"
                ),
                _ => assert_eq!(
                    std::fs::read_link(&restored).unwrap(),
                    Path::new("untouched-raw-link-target")
                ),
            }
            assert!(
                std::fs::symlink_metadata(
                    current_after.join(receipt.after_path().file_name().unwrap())
                )
                .is_err()
            );
            assert_eq!(
                std::fs::read(receipt.before_path()).unwrap(),
                b"foreign source"
            );
            assert_eq!(
                std::fs::read(receipt.after_path()).unwrap(),
                b"foreign publication"
            );
            terminal_record(&receipt);
            // Once completion is durable, repeat Undo does not touch either namespace.
            crate::WindowsMoveUndoPlan::capture(&receipt)
                .unwrap()
                .execute()
                .unwrap();
            assert_eq!(
                std::fs::read(receipt.before_path()).unwrap(),
                b"foreign source"
            );
            assert_eq!(
                std::fs::read(receipt.after_path()).unwrap(),
                b"foreign publication"
            );
        }
    }
}

#[test]
#[ignore = "requires an owned second native Windows volume"]
fn windows_native_cross_recovery_admits_renamed_parents_after_process_exit() {
    use crate::move_transaction as moves;
    if let Some(source) = std::env::var_os("OTTRIN_CROSS_PARENT_SOURCE") {
        let source = PathBuf::from(source);
        let destination =
            PathBuf::from(std::env::var_os("OTTRIN_CROSS_PARENT_DESTINATION").unwrap());
        let stop = std::env::var("OTTRIN_CROSS_PARENT_PHASE").unwrap();
        moves::WINDOWS_MOVE_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |boundary, path| {
                if boundary == moves::WindowsMoveBoundary::JournalPublished
                    && path.extension() == Some(OsStr::new("cross-journal"))
                    && format!(
                        "{:?}",
                        load_windows_cross_volume_record(path).unwrap().phase
                    ) == stop
                {
                    std::process::exit(87);
                }
            }));
        });
        let conflict = match std::env::var("OTTRIN_CROSS_PARENT_CONFLICT")
            .unwrap()
            .as_str()
        {
            "Skip" => ConflictAction::Skip,
            "Rename" => ConflictAction::Rename,
            "Overwrite" => ConflictAction::Overwrite,
            _ => panic!("unknown fixture conflict"),
        };
        DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination, conflict)
            .unwrap();
        panic!("missed durable cross-volume interruption");
    }
    for kind in 0..3 {
        for conflict in [
            ConflictAction::Skip,
            ConflictAction::Rename,
            ConflictAction::Overwrite,
        ] {
            for phase in ["Prepared", "Staged", "Published", "SourceRemoved"] {
                let fixture = Fixture::new();
                let before = fixture.primary.path().join("source");
                let after = fixture.secondary.join("destination");
                std::fs::create_dir(&before).unwrap();
                std::fs::create_dir(&after).unwrap();
                create(&before.join("entry"), kind, b"original content");
                let occupant = if conflict != ConflictAction::Skip {
                    create(&after.join("entry"), kind, b"untouched occupant");
                    Some(move_recovery::stable_identity_of_entry(&after.join("entry")).unwrap())
                } else {
                    None
                };
                let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "move_transaction::windows_undo::cross::tests::windows_native_cross_recovery_admits_renamed_parents_after_process_exit", "--include-ignored", "--nocapture"])
                .env("OTTRIN_CROSS_PARENT_SOURCE", before.join("entry"))
                .env("OTTRIN_CROSS_PARENT_DESTINATION", &after)
                .env("OTTRIN_CROSS_PARENT_PHASE", phase)
                .env("OTTRIN_CROSS_PARENT_CONFLICT", format!("{conflict:?}"))
                .env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT", root())
                .status().unwrap();
                assert_eq!(child.code(), Some(87), "{kind}/{phase}");
                let transactions = moves::discover_retained_windows_move_transactions().unwrap();
                assert_eq!(transactions.len(), 1);
                let id = transactions[0].transaction_id;
                let record = load_windows_cross_volume_record(&windows_cross_volume_journal_path(
                    &root(),
                    id,
                ))
                .unwrap();
                let stale = crate::WindowsMoveRecoveryPlan::capture(id).unwrap();
                let current_before = before.with_extension("renamed");
                let current_after = after.with_extension("renamed");
                std::fs::rename(&before, &current_before).unwrap();
                std::fs::rename(&after, &current_after).unwrap();
                std::fs::create_dir(&before).unwrap();
                std::fs::create_dir(&after).unwrap();
                std::fs::write(before.join("entry"), b"foreign source").unwrap();
                std::fs::write(after.join("entry"), b"foreign destination").unwrap();
                // Plausible copied journals at the historical name must never select
                // a different private directory, even if its records are byte-identical.
                let foreign_private = record.transaction_path().unwrap();
                std::fs::create_dir(&foreign_private).unwrap();
                let foreign_journal = foreign_private.join(WINDOWS_LOCAL_JOURNAL_NAME);
                let central_bytes =
                    std::fs::read(windows_cross_volume_journal_path(&root(), id)).unwrap();
                std::fs::write(&foreign_journal, &central_bytes).unwrap();
                std::fs::write(foreign_private.join("foreign"), b"unrelated private data").unwrap();
                assert!(stale.execute().is_err());
                let plan = crate::WindowsMoveRecoveryPlan::capture(id).unwrap();
                for parent in [&current_before, &current_after] {
                    assert!(
                        plan.paths()
                            .contains(&std::fs::canonicalize(parent).unwrap())
                    );
                }
                let current_private =
                    current_after.join(record.transaction_path().unwrap().file_name().unwrap());
                let parked_private = current_after.with_extension("parked-private");
                std::fs::rename(&current_private, &parked_private).unwrap();
                assert!(plan.execute().is_err(), "captured private name changed");
                assert!(
                    crate::WindowsMoveRecoveryPlan::capture(id).is_err(),
                    "a private directory cannot move independently of its bound parent"
                );
                assert!(moves::recover_retained_windows_move_transaction(id).is_err());
                assert_eq!(
                    std::fs::read(windows_cross_volume_journal_path(&root(), id)).unwrap(),
                    central_bytes
                );
                std::fs::rename(&parked_private, &current_private).unwrap();
                crate::WindowsMoveRecoveryPlan::capture(id)
                    .unwrap()
                    .execute()
                    .unwrap_or_else(|error| panic!("{kind}/{conflict:?}/{phase}: {error:?}"));
                assert!(std::fs::symlink_metadata(current_before.join("entry")).is_err());
                let published = std::fs::read_dir(&current_after)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .filter(|path| {
                        !path
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with(".ottrin-move-")
                    })
                    .filter(|path| {
                        conflict != ConflictAction::Rename
                            || Some(move_recovery::stable_identity_of_entry(path).unwrap())
                                != occupant
                    })
                    .collect::<Vec<_>>();
                assert_eq!(published.len(), 1, "one selected public entry");
                let published = &published[0];
                if let Some(identity) = record.candidate_identity {
                    assert_eq!(
                        move_recovery::stable_identity_of_entry(published).unwrap(),
                        identity
                    );
                }
                if let Some(identity) = occupant {
                    let retained = if conflict == ConflictAction::Overwrite {
                        current_private.join(WINDOWS_RETAINED_NAME)
                    } else {
                        current_after.join("entry")
                    };
                    assert_eq!(
                        move_recovery::stable_identity_of_entry(&retained).unwrap(),
                        identity
                    );
                    if kind < 2 {
                        assert_eq!(
                            std::fs::read(if kind == 0 {
                                retained
                            } else {
                                retained.join("child")
                            })
                            .unwrap(),
                            b"untouched occupant"
                        );
                    }
                }
                match kind {
                    0 => assert_eq!(std::fs::read(published).unwrap(), b"original content"),
                    1 => assert_eq!(
                        std::fs::read(published.join("child")).unwrap(),
                        b"original content"
                    ),
                    _ => assert_eq!(
                        std::fs::read_link(published).unwrap(),
                        Path::new("untouched-raw-link-target")
                    ),
                }
                assert_eq!(
                    std::fs::read(before.join("entry")).unwrap(),
                    b"foreign source"
                );
                assert_eq!(
                    std::fs::read(after.join("entry")).unwrap(),
                    b"foreign destination"
                );
                assert_eq!(std::fs::read(foreign_journal).unwrap(), central_bytes);
                assert_eq!(
                    std::fs::read(foreign_private.join("foreign")).unwrap(),
                    b"unrelated private data"
                );
            }
        }
    }
}
