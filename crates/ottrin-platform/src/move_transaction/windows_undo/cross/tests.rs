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
fn windows_native_cross_undo_refuses_changed_publication_or_parent_before_intent() {
    for changed in 0..3 {
        let fixture = Fixture::new();
        let receipt = fixture.moved(0, false);
        let replaced = match changed {
            0 => receipt.after_path().to_path_buf(),
            1 => receipt.after_path().parent().unwrap().to_path_buf(),
            _ => receipt.before_path().parent().unwrap().to_path_buf(),
        };
        let parked = replaced.with_extension("parked");
        std::fs::rename(&replaced, &parked).unwrap();
        if changed == 0 {
            std::fs::write(&replaced, b"foreign publication").unwrap();
        } else {
            std::fs::create_dir(&replaced).unwrap();
            if changed == 1 {
                std::fs::write(receipt.after_path(), b"foreign publication").unwrap();
            }
        }
        assert!(DefaultPlatform.undo_move_with_receipt(&receipt).is_err());
        assert!(guard(&receipt).inverse.get().is_none());
        if changed < 2 {
            assert_eq!(
                std::fs::read(receipt.after_path()).unwrap(),
                b"foreign publication"
            );
            let old_publication = if changed == 0 {
                parked.clone()
            } else {
                parked.join(receipt.after_path().file_name().unwrap())
            };
            assert_eq!(std::fs::read(old_publication).unwrap(), b"original content");
        } else {
            assert_eq!(
                std::fs::read(receipt.after_path()).unwrap(),
                b"original content"
            );
        }
        assert!(!receipt.before_path().exists());
    }
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
            super::super::super::recover_retained_windows_move_transaction(id).unwrap();
            assert_eq!(
                if kind == 0 {
                    std::fs::read(receipt.before_path()).unwrap()
                } else {
                    std::fs::read(receipt.before_path().join("child")).unwrap()
                },
                b"original content"
            );
            assert!(std::fs::symlink_metadata(receipt.after_path()).is_err());
            let transactions =
                super::super::super::discover_retained_windows_move_transactions().unwrap();
            assert_eq!(
                transactions[0].undo_status,
                move_recovery::WindowsMoveUndoStatus::Complete
            );
            super::super::super::recover_retained_windows_move_transaction(id).unwrap();
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
