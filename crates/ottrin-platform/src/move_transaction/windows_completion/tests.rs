use super::*;
use crate::{ConflictAction, DefaultPlatform, PlatformOps};

fn moved(source_parent: &Path, destination: &Path) -> crate::PathMoveUndoReceipt {
    std::fs::create_dir(source_parent).unwrap();
    std::fs::create_dir(destination).unwrap();
    let source = source_parent.join("entry");
    std::fs::write(&source, b"selected original").unwrap();
    std::fs::write(destination.join("entry"), b"displaced original").unwrap();
    let (_, receipt) = DefaultPlatform
        .execute_move_with_undo_receipt(&source, destination, ConflictAction::Overwrite)
        .unwrap();
    let receipt = receipt.unwrap();
    DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
    receipt
}
fn check_control(root: &Path, owner: &controls::Owner) {
    let central = match owner.kind {
        WindowsJournalKind::SameVolumeOverwrite => windows_journal_path(root, owner.transaction),
        WindowsJournalKind::CrossVolume => {
            windows_cross_volume_journal_path(root, owner.transaction)
        }
    };
    let directory = owner.directory.decode().unwrap();
    assert_eq!(
        move_recovery::stable_identity_of_entry(&directory).unwrap(),
        owner.identity
    );
    for (file, evidence) in [
        (central, &owner.central),
        (directory.join(WINDOWS_LOCAL_JOURNAL_NAME), &owner.local),
    ] {
        let bytes = std::fs::read(&file).unwrap();
        assert_eq!(
            move_recovery::stable_identity_of_entry(&file).unwrap(),
            evidence.identity
        );
        assert_eq!(bytes.len() as u64, evidence.bytes);
        assert_eq!(<[u8; 32]>::from(Sha256::digest(&bytes)), evidence.sha256);
    }
}
#[test]
fn windows_native_completion_controls_keep_original_file_ids_after_replacement_and_data_change() {
    let fixture = crate::tests::TestDir::new("completion-controls");
    let receipt = moved(
        &fixture.path().join("source"),
        &fixture.path().join("destination"),
    );
    let root = windows_recovery_root().unwrap();
    let done = list(&root).unwrap().pop().unwrap();
    assert_eq!(done.body.version, 2);
    assert_eq!(done.body.controls.len(), 1);
    let owner = &done.body.controls[0];
    check_control(&root, owner);
    let central = windows_journal_path(&root, owner.transaction);
    let bytes = std::fs::read(&central).unwrap();
    std::fs::rename(&central, central.with_extension("held")).unwrap();
    std::fs::write(&central, &bytes).unwrap();
    let foreign = move_recovery::stable_identity_of_entry(&central).unwrap();
    assert_ne!(foreign, owner.central.identity);
    let local = owner
        .directory
        .decode()
        .unwrap()
        .join(WINDOWS_LOCAL_JOURNAL_NAME);
    std::fs::write(&local, b"changed metadata bytes").unwrap();
    let current = find(&root, owner.transaction).unwrap().unwrap();
    assert_eq!(current, done);
    assert!(current.matches(&receipt));
    let plan =
        super::super::windows_recovery_plan::WindowsMoveRecoveryPlan::capture(owner.transaction)
            .unwrap();
    assert_eq!(plan.paths(), &[root]);
    plan.execute().unwrap();
    assert_eq!(std::fs::read(central).unwrap(), bytes);
    assert_eq!(std::fs::read(local).unwrap(), b"changed metadata bytes");
}
#[test]
fn windows_native_completed_plan_publishes_captured_control_ids_after_private_folders_disappear() {
    let fixture = crate::tests::TestDir::new("captured-control-completion");
    let receipt = moved(
        &fixture.path().join("source"),
        &fixture.path().join("destination"),
    );
    let root = windows_recovery_root().unwrap();
    let expected = list(&root).unwrap().pop().unwrap();
    let id = expected.id().unwrap();
    // Model a completed paired Undo interrupted before its receipt publication.
    std::fs::remove_file(root.join(name(id))).unwrap();
    let plan = super::super::windows_recovery_plan::WindowsMoveRecoveryPlan::capture(id).unwrap();
    assert_eq!(plan.paths(), std::slice::from_ref(&root));
    assert_eq!(
        plan.undo_status_for(&receipt),
        Some(move_recovery::WindowsMoveUndoStatus::Complete)
    );
    std::fs::remove_dir_all(receipt.before_path().parent().unwrap()).unwrap();
    std::fs::remove_dir_all(receipt.after_path().parent().unwrap()).unwrap();
    plan.execute().unwrap();
    let done = find(&root, id).unwrap().unwrap();
    assert_eq!(done.body.controls, expected.body.controls);
    assert!(done.matches(&receipt));
    plan.execute().unwrap();
}
#[test]
#[ignore = "requires an owned secondary native Windows volume"]
fn windows_native_cross_completion_captures_both_controllers_and_rejects_control_topology_changes()
{
    let primary = crate::tests::TestDir::new("cross-completion-controls");
    let root = windows_recovery_root().unwrap();
    let volume = PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
    let secondary = crate::tests::TestDir::new_at(&volume, "cross-control-destination");
    struct Restore(Option<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            replace_test_recovery_root(self.0.take());
        }
    }
    let _restore = Restore(replace_test_recovery_root(Some(root.clone())));
    let receipt = moved(
        &primary.path().join("source"),
        &secondary.path().join("destination"),
    );
    let done = list(&root).unwrap().pop().unwrap();
    assert_eq!(done.body.controls.len(), 2);
    assert_eq!(
        done.body
            .controls
            .iter()
            .map(|o| o.identity.volume_serial_number())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2
    );
    for owner in &done.body.controls {
        check_control(&root, owner);
    }
    let path = root.join(name(done.id().unwrap()));
    let bytes = std::fs::read(&path).unwrap();
    for corrupt in 0..4 {
        let mut changed = done.clone();
        match corrupt {
            0 => changed.body.controls[1].identity = changed.body.controls[0].identity,
            1 => {
                changed.body.controls[0].parent_identity = changed.body.controls[1].parent_identity
            }
            2 => changed.body.controls[0].central.sha256[0] ^= 1,
            _ => changed.body.controls.pop().map(|_| ()).unwrap(),
        }
        changed.checksum = checksum(&changed.body).unwrap();
        std::fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(
            find(&root, done.id().unwrap()).is_err(),
            "corruption {corrupt}"
        );
    }
    std::fs::write(path, bytes).unwrap();
    assert!(
        find(&root, done.id().unwrap())
            .unwrap()
            .unwrap()
            .matches(&receipt)
    );
}
