use super::*;
use crate::{ConflictAction, DefaultPlatform, MoveDisposition, PlatformOps};
use std::{cell::Cell, path::PathBuf, rc::Rc};

struct HookReset;
impl Drop for HookReset {
    fn drop(&mut self) {
        super::super::WINDOWS_MOVE_HOOK.with(|hook| *hook.borrow_mut() = None);
    }
}
fn moved(root: &Path) -> PathMoveUndoReceipt {
    let source = root.join("source/entry.txt");
    let destination = root.join("destination");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(&source, b"moved original").unwrap();
    let (disposition, receipt) = DefaultPlatform
        .execute_move_with_undo_receipt(&source, &destination, ConflictAction::Skip)
        .unwrap();
    assert_eq!(disposition, MoveDisposition::Moved);
    receipt.expect("native same-volume Undo ownership")
}

#[test]
fn windows_native_move_undo_restores_file_directory_and_link_after_keep_both() {
    for kind in 0..3 {
        let fixture = crate::tests::TestDir::new("native-move-undo-kind");
        let source = fixture.path().join("entry");
        let destination = fixture.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("entry"), b"untouched occupant").unwrap();
        match kind {
            0 => std::fs::write(&source, b"moved original").unwrap(),
            1 => {
                std::fs::create_dir(&source).unwrap();
                std::fs::write(source.join("child"), b"moved original").unwrap();
            }
            _ => std::os::windows::fs::symlink_file("untouched-link-target", &source).unwrap(),
        }
        let expected = move_recovery::stable_identity_of_entry(&source).unwrap();
        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination, ConflictAction::Rename)
            .unwrap();
        let receipt = receipt.unwrap();
        assert_ne!(receipt.after_path().file_name(), source.file_name());
        assert_eq!(receipt.copy_transaction_id(), None);
        assert!(matches!(
            DefaultPlatform.undo_copy_with_receipt(&receipt, &mut |_| Ok(())),
            Err(PlatformError::InvalidPathMoveUndoReceipt(_))
        ));
        if kind < 2 {
            let content = if kind == 0 {
                receipt.after_path().to_path_buf()
            } else {
                receipt.after_path().join("child")
            };
            std::fs::write(content, b"edited after Move").unwrap();
        }
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
        assert_eq!(
            move_recovery::stable_identity_of_entry(&source).unwrap(),
            expected
        );
        assert!(std::fs::symlink_metadata(receipt.after_path()).is_err());
        assert_eq!(
            std::fs::read(destination.join("entry")).unwrap(),
            b"untouched occupant"
        );
        match kind {
            0 => assert_eq!(std::fs::read(&source).unwrap(), b"edited after Move"),
            1 => assert_eq!(
                std::fs::read(source.join("child")).unwrap(),
                b"edited after Move"
            ),
            _ => assert_eq!(
                std::fs::read_link(&source).unwrap(),
                Path::new("untouched-link-target")
            ),
        }
    }
}

#[test]
fn windows_native_move_undo_refuses_occupied_name_and_retries_without_repeating() {
    let fixture = crate::tests::TestDir::new("native-move-undo-occupied");
    let receipt = moved(fixture.path());
    std::fs::write(receipt.before_path(), b"later source").unwrap();
    assert!(matches!(
        DefaultPlatform.undo_move_with_receipt(&receipt),
        Err(PlatformError::DestinationExists(_))
    ));
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"later source"
    );
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"moved original"
    );
    let parked = fixture.path().join("parked-occupant");
    std::fs::rename(receipt.before_path(), &parked).unwrap();
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"moved original"
    );
    assert_eq!(std::fs::read(&parked).unwrap(), b"later source");
    std::fs::write(receipt.after_path(), b"later destination").unwrap();
    assert!(matches!(
        DefaultPlatform.undo_move_with_receipt(&receipt),
        Err(PlatformError::StalePathMoveUndo(_))
    ));
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"later destination"
    );
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"moved original"
    );
}

#[test]
fn windows_native_move_undo_refuses_changed_entry_or_parent() {
    for mutation in 0..3 {
        let fixture = crate::tests::TestDir::new("native-move-undo-drift");
        let receipt = moved(fixture.path());
        let parked = fixture.path().join("parked");
        match mutation {
            0 => {
                std::fs::rename(receipt.after_path(), &parked).unwrap();
                std::fs::write(receipt.after_path(), b"foreign entry").unwrap();
            }
            1 => {
                std::fs::rename(receipt.before_path().parent().unwrap(), &parked).unwrap();
                std::fs::create_dir(receipt.before_path().parent().unwrap()).unwrap();
            }
            _ => {
                std::fs::rename(receipt.after_path().parent().unwrap(), &parked).unwrap();
                std::fs::create_dir(receipt.after_path().parent().unwrap()).unwrap();
                std::fs::write(receipt.after_path(), b"foreign entry").unwrap();
            }
        }
        assert!(matches!(
            DefaultPlatform.undo_move_with_receipt(&receipt),
            Err(PlatformError::StalePathMoveUndo(_))
        ));
        assert!(!receipt.before_path().exists());
        if mutation == 1 {
            assert_eq!(
                std::fs::read(receipt.after_path()).unwrap(),
                b"moved original"
            );
        } else {
            assert_eq!(
                std::fs::read(receipt.after_path()).unwrap(),
                b"foreign entry"
            );
            assert_eq!(
                std::fs::read(if mutation == 0 {
                    parked
                } else {
                    parked.join("entry.txt")
                })
                .unwrap(),
                b"moved original"
            );
        }
    }
}

#[test]
fn windows_native_move_undo_adverse_sharing_preserves_data_and_retries() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = crate::tests::TestDir::new("native-move-undo-sharing");
    let receipt = moved(fixture.path());
    let reader = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(receipt.after_path())
        .unwrap();
    assert!(DefaultPlatform.undo_move_with_receipt(&receipt).is_err());
    assert!(!receipt.before_path().exists());
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"moved original"
    );
    drop(reader);
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"moved original"
    );
}

#[test]
fn windows_native_move_undo_pins_parents_and_publication_during_name_race() {
    let fixture = crate::tests::TestDir::new("native-move-undo-race");
    let receipt = moved(fixture.path());
    let before = receipt.before_path().to_path_buf();
    let after = receipt.after_path().to_path_buf();
    let parked_parent = fixture.path().join("parked-parent");
    let parked_entry = fixture.path().join("parked-entry");
    let observed = Rc::new(Cell::new(0));
    let counts = observed.clone();
    let reset = HookReset;
    super::super::WINDOWS_MOVE_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |boundary, _| {
            use super::super::WindowsMoveBoundary;
            if boundary == WindowsMoveBoundary::UndoParentsOpened {
                assert!(std::fs::rename(before.parent().unwrap(), &parked_parent).is_err());
                assert!(std::fs::rename(after.parent().unwrap(), &parked_parent).is_err());
                counts.set(counts.get() + 1);
            }
            if boundary == WindowsMoveBoundary::BeforeUndoRename {
                assert!(std::fs::rename(&after, &parked_entry).is_err());
                std::fs::write(&before, b"raced source occupant").unwrap();
                counts.set(counts.get() + 1);
            }
        }))
    });
    assert!(matches!(
        DefaultPlatform.undo_move_with_receipt(&receipt),
        Err(PlatformError::DestinationExists(_))
    ));
    assert_eq!(observed.get(), 2);
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"raced source occupant"
    );
    assert_eq!(
        std::fs::read(receipt.after_path()).unwrap(),
        b"moved original"
    );
    drop(reset);
}

#[test]
fn windows_native_move_undo_receipt_cannot_adopt_publication_handoff_replacement() {
    let fixture = crate::tests::TestDir::new("native-move-undo-handoff");
    let source = fixture.path().join("entry.txt");
    let destination = fixture.path().join("destination");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(&source, b"selected original").unwrap();
    let parked = fixture.path().join("parked-original");
    let parked_hook = parked.clone();
    let observed = Rc::new(Cell::new(false));
    let reached = observed.clone();
    let _reset = HookReset;
    super::super::WINDOWS_MOVE_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |boundary, path| {
            if boundary == super::super::WindowsMoveBoundary::BeforeUndoReceipt {
                std::fs::rename(path, &parked_hook).unwrap();
                std::fs::write(path, b"foreign replacement").unwrap();
                reached.set(true);
            }
        }))
    });
    assert!(matches!(
        DefaultPlatform.execute_move_with_undo_receipt(&source, &destination, ConflictAction::Skip),
        Err(PlatformError::CommittedCleanupPending { .. })
    ));
    assert!(observed.get());
    assert!(!source.exists());
    assert_eq!(std::fs::read(parked).unwrap(), b"selected original");
    assert_eq!(
        std::fs::read(destination.join("entry.txt")).unwrap(),
        b"foreign replacement"
    );
}

#[test]
fn windows_native_move_undo_retries_after_inverse_process_exit() {
    if let Ok(encoded) = std::env::var("OTTRIN_ATOMIC_MOVE_UNDO_CHILD") {
        let (before, after, identity, before_parent, after_parent): (
            PathBuf,
            PathBuf,
            WindowsStableIdentity,
            WindowsStableIdentity,
            WindowsStableIdentity,
        ) = serde_json::from_str(&encoded).unwrap();
        let receipt = PathMoveUndoReceipt {
            windows_undo: Some(WindowsPathUndo::Move(WindowsMoveUndoGuard {
                before_parent,
                after_parent,
            })),
            before_path: before,
            after_path: after,
            identity: PathMoveIdentity { windows: identity },
            displaced_identity: None,
            retained_displaced_path: None,
            requirement: PathMoveUndoRequirement::None,
        };
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
        panic!("inverse did not reach exit boundary");
    }
    let fixture = crate::tests::TestDir::new("native-move-undo-exit");
    let receipt = moved(fixture.path());
    let Some(WindowsPathUndo::Move(guard)) = &receipt.windows_undo else {
        panic!("wrong receipt kind")
    };
    let encoded = serde_json::to_string(&(
        receipt.before_path(),
        receipt.after_path(),
        receipt.identity.windows,
        guard.before_parent,
        guard.after_parent,
    ))
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "move_transaction::windows_undo::tests::windows_native_move_undo_retries_after_inverse_process_exit", "--nocapture"])
        .env("OTTRIN_ATOMIC_MOVE_UNDO_CHILD", encoded)
        .env("OTTRIN_TEST_WINDOWS_MOVE_EXIT_BOUNDARY", "UndoRenamed")
        .env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT", super::super::windows_recovery_root().unwrap())
        .status().unwrap();
    assert_eq!(status.code(), Some(87));
    assert!(!receipt.after_path().exists());
    assert_eq!(
        std::fs::read(receipt.before_path()).unwrap(),
        b"moved original"
    );
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
}
