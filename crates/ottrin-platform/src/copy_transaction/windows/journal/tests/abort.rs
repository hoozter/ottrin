use super::cleanup::namespace_contents;
use super::*;
use std::path::PathBuf;

fn interrupted(root: &Path) -> (PathBuf, Header) {
    let registry = root.join("profile/Ottrin/State/copies");
    partial_stage_at(root, &registry, &root.join("destination"), false);
    let name = stage_name(&root.join("destination"));
    let header = decode(&std::fs::read(root.join("destination").join(name).join(LOCAL)).unwrap())
        .unwrap()
        .state
        .header
        .unwrap();
    (registry, header)
}

pub(super) fn partial_stage_at(root: &Path, registry: &Path, destination: &Path, overwrite: bool) {
    fixture(root);
    if destination != root.join("destination") {
        std::fs::remove_dir(root.join("destination")).unwrap();
        std::fs::create_dir(destination).unwrap();
    }
    drop(
        super::super::super::registry::AnchoredRoot::open_at(registry, true)
            .unwrap()
            .unwrap(),
    );
    if overwrite {
        std::fs::write(destination.join("copy"), b"unrelated original destination").unwrap();
    }
    let original = vec![37u8; super::super::super::BUFFER_BYTES * 4 + 17];
    std::fs::write(root.join("source/nested/file"), &original).unwrap();
    {
        let input = source(&root.join("source"));
        let parent = owner(destination);
        let central = owner(registry);
        let admission = if overwrite {
            DestinationAdmission::overwrite(&parent, OsStr::new("copy"), 100, |_| Ok(())).unwrap()
        } else {
            DestinationAdmission::creation(&parent, OsStr::new("copy")).unwrap()
        };
        let error = DurableStage::create(
            &input,
            &parent,
            &central,
            &admission,
            OsStr::new("source"),
            100,
            |bytes| {
                if bytes > 0 {
                    Err(io("owned staging stop"))
                } else {
                    Ok(())
                }
            },
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("owned staging stop"));
    }
    assert_eq!(
        std::fs::read(root.join("source/nested/file")).unwrap(),
        original
    );
    let parent = destination;
    let name = stage_name(parent);
    let state = decode(&std::fs::read(parent.join(&name).join(LOCAL)).unwrap())
        .unwrap()
        .state;
    assert!(!state.verified && state.nodes.iter().any(|node| node.staged.is_none()));
    let header = state.header.unwrap();
    remove_undo_fixture_source(root);
    std::fs::write(parent.join("foreign"), b"unrelated public data").unwrap();
    let payload = parent.join(header.candidate_name());
    std::fs::write(payload.join("nested/file"), b"later retained edits").unwrap();
    std::fs::write(payload.join("nested/file:keep"), b"later named stream").unwrap();
    std::fs::write(payload.join("unrecognized"), b"unexpected retained file").unwrap();
}

#[test]
fn windows_native_copy_end_staging_preserves_all_data_and_releases_only_its_name() {
    let root = tempfile::tempdir().unwrap();
    let (registry, header) = interrupted(root.path());
    let parent_path = root.path().join("destination");
    let next = super::super::super::super::random_transaction_id().unwrap();
    {
        let parent = owner(&parent_path);
        let central = owner(&registry);
        assert!(claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &next).is_err());
    }
    let before = namespace_contents(root.path());
    let mut plan =
        crate::restore_retained_copy_abort_plan_at(&registry, &header.transaction).unwrap();
    let mut stale = plan.clone();
    assert!(!plan.complete());
    assert_eq!(plan.transaction_id(), header.transaction);
    assert_eq!(namespace_contents(root.path()), before);
    assert!(plan.end_attempt(|_| Err(io("owned cancellation"))).is_err());
    assert_eq!(namespace_contents(root.path()), before);
    let payload = parent_path.join(header.candidate_name());
    let payload_before = namespace_contents(&payload);
    // An exclusive payload-root lease proves ending does not open that tree.
    let held = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&payload)
        .unwrap();
    plan.end_attempt(|_| Ok(())).unwrap();
    assert!(plan.complete());
    drop(held);
    assert_eq!(namespace_contents(&payload), payload_before);
    assert_eq!(
        std::fs::read(payload.join("nested/file:keep")).unwrap(),
        b"later named stream"
    );
    assert_eq!(
        std::fs::read(parent_path.join("foreign")).unwrap(),
        b"unrelated public data"
    );
    assert!(!parent_path.join("copy").exists() && !root.path().join("source").exists());
    assert!(crate::restore_retained_copy_resume_plan_at(&registry, &header.transaction).is_err());
    {
        let parent = owner(&parent_path);
        let central = owner(&registry);
        assert!(
            PermissionStage::publish(&parent, &central, &header.stage_name(), |_| Ok(())).is_err()
        );
    }
    let status = discovery::discover_at(&registry).unwrap();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].phase, Some(discovery::WindowsCopyPhase::Aborted));
    let later_source = root.path().join("later-source");
    std::fs::create_dir(&later_source).unwrap();
    std::fs::write(later_source.join("file"), b"later copy data").unwrap();
    {
        let input = source(&later_source);
        let parent = owner(&parent_path);
        let central = owner(&registry);
        let later = stage(&input, &parent, &central).unwrap();
        assert_ne!(later.journal.header.transaction, header.transaction);
        drop(later); // Durable staging retains its reservation after handles close.
    }
    let with_later = namespace_contents(root.path());
    assert!(stale.end_attempt(|_| Ok(())).is_err());
    assert_eq!(namespace_contents(root.path()), with_later);
    let parent = owner(&parent_path);
    let central = owner(&registry);
    assert!(
        claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"b".repeat(32)).is_err()
    );
}

#[test]
fn windows_native_copy_end_staging_refuses_live_changed_and_verified_controls() {
    let root = tempfile::tempdir().unwrap();
    let (registry, header) = interrupted(root.path());
    let parent = owner(&root.path().join("destination"));
    let central = owner(&registry);
    let snapshot = Snapshot::load(
        &parent,
        &central,
        &header.stage_name(),
        SnapshotAccess::Write,
    )
    .unwrap();
    drop((parent, central));
    assert!(crate::restore_retained_copy_abort_plan_at(&registry, &header.transaction).is_err());
    drop(snapshot);
    let mut plan =
        crate::restore_retained_copy_abort_plan_at(&registry, &header.transaction).unwrap();
    let local = root
        .path()
        .join("destination")
        .join(header.stage_name())
        .join(LOCAL);
    let mirror = registry.join(header.central_name());
    let original = std::fs::read(&local).unwrap();
    for path in [&local, &mirror] {
        let mut bytes = original.clone();
        bytes.push(7);
        std::fs::write(path, bytes).unwrap();
    }
    let changed = namespace_contents(root.path());
    assert!(plan.end_attempt(|_| Ok(())).is_err());
    assert_eq!(namespace_contents(root.path()), changed);
    let missing = root.path().join("missing");
    assert!(crate::restore_retained_copy_abort_plan_at(&missing, "../invalid").is_err());
    assert!(!missing.exists());

    let verified = tempfile::tempdir().unwrap();
    fixture(verified.path());
    let registry = verified.path().join("profile/Ottrin/State/copies");
    drop(
        super::super::super::registry::AnchoredRoot::open_at(&registry, true)
            .unwrap()
            .unwrap(),
    );
    let header = {
        let input = source(&verified.path().join("source"));
        let parent = owner(&verified.path().join("destination"));
        let central = owner(&registry);
        let staged = stage(&input, &parent, &central).unwrap();
        staged.journal.header.clone()
    };
    let before = namespace_contents(verified.path());
    assert!(crate::restore_retained_copy_abort_plan_at(&registry, &header.transaction).is_err());
    assert_eq!(namespace_contents(verified.path()), before);
}

#[test]
fn windows_native_copy_end_staging_paired_prefix_recovers_and_divergence_stays_reserved() {
    for step in [Step::LocalFlushed, Step::CentralFlushed] {
        let root = tempfile::tempdir().unwrap();
        let (registry, header) = interrupted(root.path());
        let parent_path = root.path().join("destination");
        let payload = parent_path.join(header.candidate_name());
        let before = namespace_contents(&payload);
        let local = parent_path.join(header.stage_name()).join(LOCAL);
        let sequence = decode(&std::fs::read(&local).unwrap()).unwrap().sequence;
        let mut plan =
            crate::restore_retained_copy_abort_plan_at(&registry, &header.transaction).unwrap();
        FAULT.with(|fault| fault.set(Some((sequence, step, false))));
        let result = plan.end_attempt(|_| Ok(()));
        FAULT.with(|fault| fault.set(None));
        assert!(result.is_err() && !plan.complete());
        assert_eq!(namespace_contents(&payload), before);
        assert_eq!(
            std::fs::read(payload.join("nested/file:keep")).unwrap(),
            b"later named stream"
        );
        let parent = owner(&parent_path);
        let central = owner(&registry);
        let id = super::super::super::super::random_transaction_id().unwrap();
        assert!(claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &id).is_err());
        drop((parent, central));
        let fresh =
            crate::restore_retained_copy_name_release_plan_at(&registry, &header.transaction);
        if step == Step::CentralFlushed {
            let mut release = fresh.unwrap();
            assert_eq!(
                release.admitted_phase(),
                discovery::WindowsCopyPhase::Aborted
            );
            release.finish(|_| Ok(())).unwrap();
            let parent = owner(&parent_path);
            let central = owner(&registry);
            let later = claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &id).unwrap();
            assert!(matches!(
                later.abort_unstarted(io("owned retry cancelled")),
                PlatformError::Io(_)
            ));
        } else {
            assert!(fresh.is_err());
            let uncertain = namespace_contents(root.path());
            assert!(
                crate::restore_retained_copy_abort_plan_at(&registry, &header.transaction).is_err()
            );
            assert_eq!(namespace_contents(root.path()), uncertain);
        }
        assert_eq!(namespace_contents(&payload), before);
    }
}
