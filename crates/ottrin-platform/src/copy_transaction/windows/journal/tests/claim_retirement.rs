//! Released history remains pinned until every recovery record is absent.
use super::*;

#[test]
fn windows_native_copy_claim_retirement_reuses_only_fully_cleaned_history() {
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
    assert_eq!(next.binding().identities()[4], old_identity);
    assert!(matches!(
        next.abort_unstarted(io("owned test cancellation")),
        PlatformError::Io(_)
    ));
    assert_eq!(std::fs::read(&local).unwrap(), journal);
    assert_eq!(std::fs::read(&mirror).unwrap(), journal);
    let retained_log = std::fs::read(&claim_path).unwrap();
    let user = root.path().join("destination/user-data");
    std::fs::write(&user, b"preserve public sibling").unwrap();
    let cleanup =
        super::super::cleanup_finalize::prepare(&parent, &central, &name, |_| Ok(())).unwrap();
    cleanup.finish(&parent, &central, |_| Ok(())).unwrap();
    assert!(!local.exists());
    assert!(!mirror.exists());
    let fresh =
        claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"c".repeat(32)).unwrap();
    assert_ne!(fresh.binding().identities()[4], old_identity);
    assert!(std::fs::read(&claim_path).unwrap().len() < retained_log.len());
    assert!(matches!(
        fresh.abort_unstarted(io("owned test cancellation")),
        PlatformError::Io(_)
    ));
    assert_eq!(std::fs::read(&user).unwrap(), b"preserve public sibling");
}
