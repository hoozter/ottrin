use super::cleanup::namespace_contents;
use super::*;
use std::path::PathBuf;

fn replace(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}
fn prepared(root: &Path, undone: bool) -> (PathBuf, Header, PathBuf, Vec<u8>, ReadonlyCleanup) {
    let registry = root.join("profile/Ottrin/State/copies");
    let (name, _, cleanup) = completed_undo_fixture_at(
        root,
        &root.join("destination"),
        true,
        false,
        Some(&registry),
    );
    let parent = owner(&root.join("destination"));
    let central = owner(&registry);
    let header = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read)
        .unwrap()
        .state
        .header
        .unwrap();
    if undone {
        undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    } else {
        drop(
            super::super::cleanup::Admission::observe(&parent, &central, &name, |_| Ok(()))
                .unwrap()
                .start(&parent, &central, |_| Ok(()))
                .unwrap(),
        );
    }
    let claim = root
        .join("destination")
        .join(header.claim.test_relative_path());
    let full = std::fs::read(&claim).unwrap();
    let end = FRAME_HEADER + u32::from_le_bytes(full[8..12].try_into().unwrap()) as usize;
    replace(&claim, &full[..end + 12]);
    (registry, header, claim, full, cleanup)
}

#[test]
fn windows_native_copy_name_release_admission_finishes_without_touching_retained_payload() {
    let root = tempfile::tempdir().unwrap();
    let (registry, header, claim, full, _cleanup) = prepared(root.path(), true);
    let retained = root
        .path()
        .join("destination")
        .join(header.candidate_name());
    std::fs::write(retained.join("nested/file"), b"retained user edits").unwrap();
    std::fs::write(retained.join("unrecognized"), b"keep additional file").unwrap();
    let before = namespace_contents(root.path());
    let status = discovery::discover_at(&registry).unwrap();
    assert_eq!(status.len(), 1);
    assert_eq!(
        status[0].evidence,
        discovery::WindowsCopyRecoveryEvidence::PendingNameRelease
    );
    assert_eq!(status[0].phase, Some(discovery::WindowsCopyPhase::Undone));
    assert_eq!(namespace_contents(root.path()), before);
    let mut plan =
        crate::restore_retained_copy_name_release_plan_at(&registry, &header.transaction).unwrap();
    assert_eq!(plan.transaction_id(), header.transaction);
    assert_eq!(
        plan.destination(),
        windows_fs::location(&owner(&root.path().join("destination"))).unwrap()
    );
    assert!(!plan.complete());
    assert_eq!(plan.admitted_phase(), discovery::WindowsCopyPhase::Undone);
    assert_eq!(namespace_contents(root.path()), before);
    assert!(
        plan.finish(|_| Err(io("owned cancellation before release")))
            .is_err()
    );
    assert_eq!(namespace_contents(root.path()), before);
    plan.finish(|_| Ok(())).unwrap();
    assert!(plan.complete());
    let after = namespace_contents(root.path());
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    let relative_claim = claim.strip_prefix(root.path()).unwrap();
    for (path, bytes) in &before {
        if path != relative_claim {
            assert_eq!(after.get(path), Some(bytes), "{}", path.display());
        }
    }
    assert_eq!(std::fs::read(&claim).unwrap(), full);
    assert_eq!(
        std::fs::read(retained.join("nested/file")).unwrap(),
        b"retained user edits"
    );
    assert_eq!(
        std::fs::read(retained.join("unrecognized")).unwrap(),
        b"keep additional file"
    );
    assert!(
        !discovery::discover_at(&registry)
            .unwrap()
            .iter()
            .any(|entry| entry.evidence
                == discovery::WindowsCopyRecoveryEvidence::PendingNameRelease)
    );
    assert!(
        crate::restore_retained_copy_name_release_plan_at(&registry, &header.transaction).is_err()
    );
    assert!(plan.finish(|_| Ok(())).is_err());
    assert_eq!(namespace_contents(root.path()), after);

    let cleaning = tempfile::tempdir().unwrap();
    let (registry, header, _, _, _cleanup) = prepared(cleaning.path(), false);
    let before = namespace_contents(cleaning.path());
    let mut plan =
        crate::restore_retained_copy_name_release_plan_at(&registry, &header.transaction).unwrap();
    assert_eq!(
        plan.admitted_phase(),
        discovery::WindowsCopyPhase::CleaningUp
    );
    plan.finish(|_| Ok(())).unwrap();
    let after = namespace_contents(cleaning.path());
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    let claim = PathBuf::from("destination").join(header.claim.test_relative_path());
    for (path, bytes) in &before {
        if path != &claim {
            assert_eq!(after.get(path), Some(bytes), "{}", path.display());
        }
    }
    assert_eq!(
        std::fs::read(cleaning.path().join("destination/copy/nested/file")).unwrap(),
        b"original data"
    );
}

#[test]
fn windows_native_copy_name_release_admission_rejects_stale_claim_and_registry_changes() {
    let root = tempfile::tempdir().unwrap();
    let (registry, header, claim, full, _cleanup) = prepared(root.path(), true);
    let mut stale =
        crate::restore_retained_copy_name_release_plan_at(&registry, &header.transaction).unwrap();
    let prefix = FRAME_HEADER + u32::from_le_bytes(full[8..12].try_into().unwrap()) as usize;
    replace(&claim, &full[..prefix + 13]);
    let changed = namespace_contents(root.path());
    assert!(stale.finish(|_| Ok(())).is_err());
    assert_eq!(namespace_contents(root.path()), changed);
    let mut current =
        crate::restore_retained_copy_name_release_plan_at(&registry, &header.transaction).unwrap();
    current.finish(|_| Ok(())).unwrap();
    let parent = owner(&root.path().join("destination"));
    let central = owner(&registry);
    let transaction = super::super::super::super::random_transaction_id().unwrap();
    let later =
        claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &transaction).unwrap();
    let later_state = namespace_contents(root.path());
    assert!(stale.finish(|_| Ok(())).is_err());
    assert_eq!(namespace_contents(root.path()), later_state);
    assert!(matches!(
        later.abort_unstarted(io("owned later admission cancellation")),
        PlatformError::Io(_)
    ));
    drop((parent, central));

    let other = tempfile::tempdir().unwrap();
    let (registry, header, _, _, _cleanup) = prepared(other.path(), true);
    let mut plan =
        crate::restore_retained_copy_name_release_plan_at(&registry, &header.transaction).unwrap();
    let anchor = registry
        .parent()
        .unwrap()
        .join(".ottrin-copy-recovery-root");
    let saved = std::fs::read(&anchor).unwrap();
    std::fs::remove_file(&anchor).unwrap();
    std::fs::write(&anchor, saved).unwrap();
    let changed = namespace_contents(other.path());
    assert!(plan.finish(|_| Ok(())).is_err());
    assert_eq!(namespace_contents(other.path()), changed);
    let missing = other.path().join("missing/registry");
    assert!(crate::restore_retained_copy_name_release_plan_at(&missing, "../invalid").is_err());
    assert!(!missing.exists());
}
