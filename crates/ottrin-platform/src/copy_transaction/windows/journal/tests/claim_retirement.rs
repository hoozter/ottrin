//! Released reservations can be replaced while paired recovery records remain.
use super::*;

#[test]
fn windows_native_copy_claim_retirement_keeps_terminal_cleanup_independent_of_new_owners() {
    let root = tempfile::tempdir().unwrap();
    let (name, _, _cleanup) = completed_undo_fixture(root.path(), true, false);
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let header = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read)
        .unwrap()
        .state
        .header
        .unwrap();
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    let claim_path = root
        .path()
        .join("destination")
        .join(header.claim.test_relative_path());
    let local = root.path().join("destination").join(&name).join(LOCAL);
    let mirror = root.path().join("central").join(header.central_name());
    let journal = std::fs::read(&local).unwrap();
    let old_identity = header.claim.identities()[4];
    let next =
        claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"b".repeat(32)).unwrap();
    assert_ne!(next.binding().identities()[4], old_identity);
    assert_eq!(std::fs::read(&local).unwrap(), journal);
    assert_eq!(std::fs::read(&mirror).unwrap(), journal);
    let retained_log = std::fs::read(&claim_path).unwrap();
    let terminal = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).unwrap();
    assert!(terminal.claim.is_none());
    drop(terminal);
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    assert_eq!(std::fs::read(&claim_path).unwrap(), retained_log);
    let user = root.path().join("destination/user-data");
    std::fs::write(&user, b"preserve public sibling").unwrap();
    let cleanup =
        super::super::cleanup_finalize::prepare(&parent, &central, &name, |_| Ok(())).unwrap();
    cleanup.finish(&parent, &central, |_| Ok(())).unwrap();
    assert!(!local.exists());
    assert!(!mirror.exists());
    next.validate(false).unwrap();
    assert_eq!(std::fs::read(&claim_path).unwrap(), retained_log);
    assert!(matches!(
        next.abort_unstarted(io("owned test cancellation")),
        PlatformError::Io(_)
    ));
    let fresh =
        claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"c".repeat(32)).unwrap();
    assert_ne!(fresh.binding().identities()[4], old_identity);
    assert_ne!(std::fs::read(&claim_path).unwrap(), retained_log);
    assert!(matches!(
        fresh.abort_unstarted(io("owned test cancellation")),
        PlatformError::Io(_)
    ));
    assert_eq!(std::fs::read(&user).unwrap(), b"preserve public sibling");
}

#[test]
fn windows_native_copy_terminal_claim_discharge_requires_paired_state_and_never_adopts_foreign_controls()
 {
    let root = tempfile::tempdir().unwrap();
    let (name, _, _cleanup) = completed_undo_fixture(root.path(), false, false);
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let header = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read)
        .unwrap()
        .state
        .header
        .unwrap();
    let claim = root
        .path()
        .join("destination")
        .join(header.claim.test_relative_path());
    let displaced = claim.with_extension("held");
    let local = root.path().join("destination").join(&name).join(LOCAL);
    let mirror = root.path().join("central").join(header.central_name());
    let active = std::fs::read(&local).unwrap();
    std::fs::rename(&claim, &displaced).unwrap();
    assert!(Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).is_err());
    std::fs::write(&claim, b"foreign control bytes").unwrap();
    assert!(Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).is_err());
    assert_eq!(std::fs::read(&claim).unwrap(), b"foreign control bytes");
    std::fs::remove_file(&claim).unwrap();
    std::fs::rename(&displaced, &claim).unwrap();
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    remove_undo_fixture_source(root.path());
    let terminal = std::fs::read(&local).unwrap();
    std::fs::remove_file(&claim).unwrap();
    assert!(
        Snapshot::load(&parent, &central, &name, SnapshotAccess::Read)
            .unwrap()
            .claim
            .is_none()
    );
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    assert!(!claim.exists());
    std::fs::write(&claim, b"different owner; deliberately not a claim log").unwrap();
    let snapshot = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).unwrap();
    assert!(snapshot.claim.is_none());
    drop(snapshot);
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    assert_eq!(
        std::fs::read(&claim).unwrap(),
        b"different owner; deliberately not a claim log"
    );
    // An absent old claim never turns a lone terminal journal into authority.
    std::fs::write(&mirror, &active).unwrap();
    assert!(Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).is_err());
    assert!(undo_completed_copy(&parent, &central, &name, |_| Ok(())).is_err());
    assert_eq!(std::fs::read(&local).unwrap(), terminal);
    assert_eq!(std::fs::read(&mirror).unwrap(), active);
    assert_eq!(
        std::fs::read(&claim).unwrap(),
        b"different owner; deliberately not a claim log"
    );
}
