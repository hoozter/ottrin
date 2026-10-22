use super::*;
use crate::move_transaction as moves;
use crate::{ConflictAction, DefaultPlatform, PlatformOps};

struct Fixture {
    root: crate::tests::TestDir,
    previous: Option<PathBuf>,
}
impl Fixture {
    fn new() -> Self {
        let root = crate::tests::TestDir::new("native-move-parent-names");
        let previous = moves::replace_test_recovery_root(Some(root.path().join("state")));
        Self { root, previous }
    }
    fn names(&self) -> (PathBuf, PathBuf) {
        (
            self.root.path().join("source"),
            self.root.path().join("destination"),
        )
    }
    fn renamed(&self) -> (PathBuf, PathBuf) {
        let (source, destination) = self.names();
        let before = self.root.path().join("renamed-source");
        let after = self.root.path().join("renamed-destination");
        std::fs::rename(&source, &before).unwrap();
        std::fs::rename(&destination, &after).unwrap();
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(source.join("entry"), b"foreign-source").unwrap();
        std::fs::write(destination.join("entry"), b"foreign-destination").unwrap();
        (before, after)
    }
    fn foreign_unchanged(&self) {
        let (before, after) = self.names();
        assert_eq!(
            std::fs::read(before.join("entry")).unwrap(),
            b"foreign-source"
        );
        assert_eq!(
            std::fs::read(after.join("entry")).unwrap(),
            b"foreign-destination"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        moves::replace_test_recovery_root(self.previous.take());
    }
}
fn create(path: &Path, kind: u8, content: &[u8]) {
    match kind {
        0 => std::fs::write(path, content).unwrap(),
        1 => {
            std::fs::create_dir(path).unwrap();
            std::fs::write(path.join("child"), content).unwrap();
        }
        _ => {
            std::os::windows::fs::symlink_file(std::str::from_utf8(content).unwrap(), path).unwrap()
        }
    }
}
fn contents(path: &Path, kind: u8, content: &[u8]) {
    match kind {
        0 => assert_eq!(std::fs::read(path).unwrap(), content),
        1 => assert_eq!(std::fs::read(path.join("child")).unwrap(), content),
        _ => assert_eq!(
            std::fs::read_link(path).unwrap(),
            Path::new(std::str::from_utf8(content).unwrap())
        ),
    }
}
#[test]
fn windows_native_atomic_undo_admits_renamed_parents_and_refuses_a_stale_plan() {
    let f = Fixture::new();
    let (before, after) = f.names();
    std::fs::create_dir(&before).unwrap();
    std::fs::create_dir(&after).unwrap();
    std::fs::write(before.join("entry"), b"selected").unwrap();
    let (_, receipt) = DefaultPlatform
        .execute_move_with_undo_receipt(&before.join("entry"), &after, ConflictAction::Skip)
        .unwrap();
    let receipt = receipt.unwrap();
    let stale = WindowsMoveUndoPlan::capture(&receipt).unwrap();
    let (current_before, current_after) = f.renamed();
    assert!(stale.execute().is_err());
    assert_eq!(
        std::fs::read(current_after.join("entry")).unwrap(),
        b"selected"
    );
    let plan = WindowsMoveUndoPlan::capture(&receipt).unwrap();
    assert!(
        plan.paths()
            .contains(&std::fs::canonicalize(&current_before).unwrap())
    );
    assert!(
        plan.paths()
            .contains(&std::fs::canonicalize(&current_after).unwrap())
    );
    plan.execute().unwrap();
    assert_eq!(
        std::fs::read(current_before.join("entry")).unwrap(),
        b"selected"
    );
    assert!(!current_after.join("entry").exists());
    plan.execute().unwrap();
    f.foreign_unchanged();
}
#[test]
fn windows_native_replace_undo_admits_current_parents_without_rebinding_foreign_names() {
    for kind in 0..3 {
        let f = Fixture::new();
        let (before, after) = f.names();
        std::fs::create_dir(&before).unwrap();
        std::fs::create_dir(&after).unwrap();
        create(&before.join("entry"), kind, b"selected");
        create(&after.join("entry"), kind, b"displaced");
        let selected = move_recovery::stable_identity_of_entry(&before.join("entry")).unwrap();
        let displaced = move_recovery::stable_identity_of_entry(&after.join("entry")).unwrap();
        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(
                &before.join("entry"),
                &after,
                ConflictAction::Overwrite,
            )
            .unwrap();
        let receipt = receipt.unwrap();
        let stale = WindowsMoveUndoPlan::capture(&receipt).unwrap();
        let (current_before, current_after) = f.renamed();
        assert!(stale.execute().is_err());
        let plan = WindowsMoveUndoPlan::capture(&receipt).unwrap();
        plan.execute().unwrap();
        assert_eq!(
            move_recovery::stable_identity_of_entry(&current_before.join("entry")).unwrap(),
            selected
        );
        assert_eq!(
            move_recovery::stable_identity_of_entry(&current_after.join("entry")).unwrap(),
            displaced
        );
        contents(&current_before.join("entry"), kind, b"selected");
        contents(&current_after.join("entry"), kind, b"displaced");
        f.foreign_unchanged();
        // Completed history and cleanup remain independent of the old public labels.
        let Some(WindowsPathUndo::Move(WindowsMoveUndoGuard::Replace(record))) =
            &receipt.windows_undo
        else {
            unreachable!()
        };
        let recovery =
            crate::WindowsMoveRecoveryPlan::capture(record.transaction_id().unwrap()).unwrap();
        assert_eq!(recovery.paths(), &[f.root.path().join("state")]);
        let cleanup =
            crate::WindowsMoveRecoveryPlan::capture_cleanup(record.transaction_id().unwrap())
                .unwrap()
                .unwrap();
        cleanup.execute_cleanup().unwrap();
        f.foreign_unchanged();
    }
}
#[test]
fn windows_native_replace_recovery_follows_parent_ids_after_process_exit() {
    if let Some(root) = std::env::var_os("OTTRIN_PARENT_MOVE_CHILD") {
        let root = PathBuf::from(root);
        moves::replace_test_recovery_root(Some(root.join("state")));
        let wanted = std::env::var("OTTRIN_PARENT_MOVE_BOUNDARY").unwrap();
        moves::WINDOWS_MOVE_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |boundary, path| {
                if format!("{boundary:?}") == wanted
                    && (wanted != "JournalPublished"
                        || path.extension() == Some(OsStr::new("journal")))
                {
                    std::process::exit(87);
                }
            }))
        });
        DefaultPlatform
            .execute_move_with_undo_receipt(
                &root.join("source/entry"),
                &root.join("destination"),
                ConflictAction::Overwrite,
            )
            .unwrap();
        panic!("expected process interruption");
    }
    for kind in 0..3 {
        for boundary in [
            "JournalPublished",
            if kind == 0 {
                "OverwritePublished"
            } else {
                "OverwriteDisplacedRetained"
            },
        ] {
            let f = Fixture::new();
            let (before, after) = f.names();
            std::fs::create_dir(&before).unwrap();
            std::fs::create_dir(&after).unwrap();
            create(&before.join("entry"), kind, b"selected");
            create(&after.join("entry"), kind, b"displaced");
            let selected = move_recovery::stable_identity_of_entry(&before.join("entry")).unwrap();
            let displaced = move_recovery::stable_identity_of_entry(&after.join("entry")).unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "move_transaction::windows_undo::plan::tests::windows_native_replace_recovery_follows_parent_ids_after_process_exit", "--nocapture"])
                .env("OTTRIN_PARENT_MOVE_CHILD", f.root.path()).env("OTTRIN_PARENT_MOVE_BOUNDARY", boundary).status().unwrap();
            assert_eq!(status.code(), Some(87), "{kind} {boundary}");
            let (current_before, current_after) = f.renamed();
            let root = f.root.path().join("state");
            let rows = moves::discover_retained_windows_move_transactions().unwrap();
            assert_eq!(rows.len(), 1);
            let id = rows[0].transaction_id;
            let central = moves::windows_journal_path(&root, id);
            let record = moves::load_windows_overwrite_record(&central).unwrap();
            let foreign = record.transaction_path().unwrap();
            std::fs::create_dir(&foreign).unwrap();
            std::fs::write(
                foreign.join(moves::WINDOWS_LOCAL_JOURNAL_NAME),
                b"foreign-journal",
            )
            .unwrap();
            std::fs::write(
                foreign.join(moves::WINDOWS_REPLACEMENT_NAME),
                b"foreign-candidate",
            )
            .unwrap();
            let snapshot = std::fs::read(&central).unwrap();
            let plan = crate::WindowsMoveRecoveryPlan::capture(id).unwrap();
            assert!(
                plan.paths()
                    .contains(&std::fs::canonicalize(&current_before).unwrap())
            );
            assert!(
                plan.paths()
                    .contains(&std::fs::canonicalize(&current_after).unwrap())
            );
            assert_eq!(
                std::fs::read(&central).unwrap(),
                snapshot,
                "admission is read-only"
            );
            plan.execute().unwrap();
            assert!(!current_before.join("entry").exists());
            assert_eq!(
                move_recovery::stable_identity_of_entry(&current_after.join("entry")).unwrap(),
                selected
            );
            let completed = moves::load_windows_overwrite_record(&central).unwrap();
            let receipt = overwrite::issue(&completed).unwrap();
            WindowsMoveUndoPlan::capture(&receipt)
                .unwrap()
                .execute()
                .unwrap();
            assert_eq!(
                move_recovery::stable_identity_of_entry(&current_before.join("entry")).unwrap(),
                selected
            );
            assert_eq!(
                move_recovery::stable_identity_of_entry(&current_after.join("entry")).unwrap(),
                displaced
            );
            contents(&current_before.join("entry"), kind, b"selected");
            contents(&current_after.join("entry"), kind, b"displaced");
            f.foreign_unchanged();
            assert_eq!(
                std::fs::read(foreign.join(moves::WINDOWS_LOCAL_JOURNAL_NAME)).unwrap(),
                b"foreign-journal"
            );
            assert_eq!(
                std::fs::read(foreign.join(moves::WINDOWS_REPLACEMENT_NAME)).unwrap(),
                b"foreign-candidate"
            );
        }
    }
}

#[test]
fn windows_native_move_undo_keeps_ancestor_names_stable_and_releases_its_pins() {
    use std::{cell::Cell, rc::Rc};
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            moves::WINDOWS_MOVE_HOOK.with(|hook| *hook.borrow_mut() = None);
        }
    }
    let f = Fixture::new();
    let outer = f.root.path().join("outer");
    let moved_outer = f.root.path().join("moved-outer");
    let source = outer.join("nested/source");
    let destination = outer.join("nested/destination");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(source.join("entry"), b"selected").unwrap();
    let (_, receipt) = DefaultPlatform
        .execute_move_with_undo_receipt(&source.join("entry"), &destination, ConflictAction::Skip)
        .unwrap();
    let receipt = receipt.unwrap();
    // Prove the ordinary owner can rename this ancestor without an active pin.
    std::fs::rename(&outer, &moved_outer).unwrap();
    std::fs::rename(&moved_outer, &outer).unwrap();
    let plan = WindowsMoveUndoPlan::capture(&receipt).unwrap();
    let called = Rc::new(Cell::new(false));
    let seen = called.clone();
    let original = outer.clone();
    let moved = moved_outer.clone();
    let reset = Reset;
    moves::WINDOWS_MOVE_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |boundary, _| {
            if boundary == moves::WindowsMoveBoundary::UndoParentsOpened {
                seen.set(true);
                let result = std::fs::rename(&original, &moved);
                if result.is_ok() {
                    std::fs::rename(&moved, &original).unwrap();
                }
                assert!(
                    result.is_err(),
                    "a live inverse must keep ancestor path reservations stable"
                );
            }
        }))
    });
    plan.execute().unwrap();
    drop(reset);
    assert!(called.get());
    assert_eq!(std::fs::read(source.join("entry")).unwrap(), b"selected");
    std::fs::rename(&outer, &moved_outer).unwrap();
    assert_eq!(
        std::fs::read(moved_outer.join("nested/source/entry")).unwrap(),
        b"selected"
    );
}
