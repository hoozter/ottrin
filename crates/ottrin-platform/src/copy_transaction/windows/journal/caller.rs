//! Owned native caller journal controls; not the enabled dispatcher or Undo bridge.
use super::*;
use std::{io::Write, sync::Arc};

pub(in super::super) fn exercise_caller_journals(
    caller: &Arc<crate::windows_privileged_transport::CallerCopyContext>,
    protected_parent: &File,
    source: &File,
    preferences: &File,
    index: &File,
    mut as_caller: impl FnMut(&mut dyn FnMut()),
) {
    let authority = Authority::Caller(Arc::clone(caller));
    let source_path = windows_fs::location(source).unwrap();
    let source_pin = tree::PinnedTree::open(source, 16, &mut |_| Ok(())).unwrap();
    let source_evidence = source_pin.freeze_existing(|_| Ok(())).unwrap();
    let directory = source_pin.root().directory;
    let manager = caller
        .copy_registry(std::slice::from_ref(&source_path), true)
        .unwrap_or_else(|error| {
            panic!("caller journal registry directory={directory}, source={source_path:?}: {error}")
        })
        .unwrap();
    let registry_binding = manager.binding().unwrap();
    // Canonical-profile fault testing deliberately invalidates fresh opens.
    // Retain its already-granted readonly observer capabilities across this
    // work; these handles never authorize a journal or payload mutation.
    let observe_user_data = || {
        [preferences, index].map(|file| {
            (
                read(file).unwrap(),
                stable_identity_from_handle(file).unwrap(),
                security::owner_group_dacl_digest(file).unwrap(),
            )
        })
    };
    let mut user_data = None;
    as_caller(&mut || user_data = Some(observe_user_data()));
    let user_data = user_data.unwrap();
    let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    let parent = windows_fs::create_private(
        protected_parent,
        OsStr::new(&format!("caller-durable-staging-{directory}")),
        true,
        sharing,
    )
    .unwrap();
    windows_fs::set_empty_directory_case_policy(&parent, true).unwrap();
    let destination =
        DestinationAdmission::creation(&authority, &parent, OsStr::new("copy")).unwrap();
    manager.validate().unwrap();
    let staged = DurableStage::create(
        source,
        &parent,
        manager.root(),
        &destination,
        OsStr::new("caller-source.txt"),
        16,
        |_| Ok(()),
    )
    .unwrap();
    assert!(staged.journal.state.verified);
    assert_eq!(staged.tree.entry_count(), source_evidence.entries.len());
    assert_eq!(
        staged.journal.header.creator,
        caller.default_file_owner_sid().unwrap()
    );
    assert_ne!(
        staged.journal.header.creator,
        windows_fs::current_owner().unwrap()
    );
    for file in [
        &staged.journal.stage_dir,
        &staged.journal.local,
        &staged.journal.central,
    ] {
        security::PrivateSecurity::capture_for_caller(
            file,
            caller.default_file_owner_sid().unwrap(),
            caller.default_file_group_sid().unwrap(),
        )
        .unwrap();
    }
    let before = read(&staged.journal.local).unwrap();
    assert_eq!(read(&staged.journal.central).unwrap(), before);
    as_caller(&mut || {
        let file = Authority::Process
            .open_child(
                &staged.stage.directory,
                OsStr::new(LOCAL),
                false,
                FILE_GENERIC_READ,
                sharing,
            )
            .unwrap();
        assert_eq!(read(&file).unwrap(), before);
    });
    let name = staged.stage.name.clone();
    assert!(
        Snapshot::load(
            &authority,
            &parent,
            manager.root(),
            &name,
            SnapshotAccess::Read
        )
        .is_err()
    );
    let inspection = Snapshot::load(
        &authority,
        &parent,
        manager.root(),
        &name,
        SnapshotAccess::Inspect,
    )
    .unwrap();
    assert!(inspection.state.verified);
    assert_eq!(inspection.bytes, before);
    let mut readonly = inspection.local.try_clone().unwrap();
    assert_eq!(
        readonly.write_all(b"must not write").unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    drop((readonly, inspection));
    // Retain the actual native projected policy across control reopening. Its
    // descriptors come from the original token, never serialized evidence.
    let plan = staged
        .tree
        .destination_security_plan(&parent, &staged.stage.candidate_name, || Ok(()))
        .unwrap();
    let evidence = plan.evidence();
    drop(staged);
    assert!(
        Snapshot::load(
            &Authority::Process,
            &parent,
            manager.root(),
            &name,
            SnapshotAccess::Read
        )
        .is_err()
    );
    let snapshot = Snapshot::load(
        &authority,
        &parent,
        manager.root(),
        &name,
        SnapshotAccess::Read,
    )
    .unwrap();
    snapshot.revalidate(&parent, manager.root()).unwrap();
    assert_eq!(snapshot.bytes, before);
    drop(snapshot);
    let snapshot = Snapshot::load(
        &authority,
        &parent,
        manager.root(),
        &name,
        SnapshotAccess::Write,
    )
    .unwrap();
    let mut writer = Journal::from_snapshot(&parent, manager.root(), snapshot).unwrap();
    writer.prepare_plan(&evidence, &mut || Ok(())).unwrap();
    writer.bindings().unwrap();
    plan.validate().unwrap();
    let planned = read(&writer.local).unwrap();
    assert!(planned.starts_with(&before) && planned.len() > before.len());
    assert_eq!(read(&writer.central).unwrap(), planned);
    drop((writer, plan));
    let snapshot = Snapshot::load(
        &authority,
        &parent,
        manager.root(),
        &name,
        SnapshotAccess::Read,
    )
    .unwrap();
    assert!(snapshot.state.planned.is_some());
    assert_eq!(snapshot.bytes, planned);
    drop(snapshot);
    // Begin-only records permit reservation release, whether already published
    // or left at the complete preparation name. An intent forbids that release.
    struct ResetFault;
    impl Drop for ResetFault {
        fn drop(&mut self) {
            initialization::FAULT.with(|fault| fault.set(None));
        }
    }
    for (case, unpublished, intent) in [(0, false, false), (1, true, false), (2, false, true)] {
        manager.validate().unwrap();
        let parent = windows_fs::create_private(
            protected_parent,
            OsStr::new(&format!("caller-paired-begin-{directory}-{case}")),
            true,
            sharing,
        )
        .unwrap();
        let id = super::super::super::random_transaction_id().unwrap();
        let mut claim = Some(
            claims::Lease::acquire(&authority, &parent, manager.root(), OsStr::new("copy"), &id)
                .unwrap(),
        );
        let binding = claim.as_ref().unwrap().binding().clone();
        let observed_claim = claim.as_ref().unwrap().observation();
        let prepared = initialization::prepare(&authority, manager.root(), &id).unwrap();
        let stage = namespace::PrivateStage::for_transaction_with_authority(
            &parent,
            &id,
            authority.clone(),
        )
        .unwrap();
        let destination =
            DestinationAdmission::creation(&authority, &parent, OsStr::new("copy")).unwrap();
        let reset = ResetFault;
        if unpublished {
            initialization::FAULT
                .with(|fault| fault.set(Some(initialization::Point::HeadersFlushed)));
        }
        let result = Journal::create(
            &stage,
            manager.root(),
            prepared,
            &destination,
            OsStr::new("caller-source.txt"),
            &mut claim,
        );
        drop(reset);
        let central_name = if unpublished {
            initialization::preparation_name(&id)
        } else {
            OsString::from(format!("{id}.copy-journal"))
        };
        if unpublished {
            assert!(result.is_err());
            assert!(claim.is_some());
            drop(claim.take());
        } else {
            let mut journal = result.unwrap();
            if intent {
                journal
                    .persist(Record::CreateIntent {
                        index: 0,
                        parent: None,
                        name: journal.header.candidate_name().encode_wide().collect(),
                        source: super::super::version(source).unwrap(),
                    })
                    .unwrap();
            }
            drop(journal);
        }
        let stage_name = stage.name.clone();
        drop(stage);
        let setup = Snapshot::read_pair(
            &authority,
            &parent,
            manager.root(),
            &stage_name,
            SnapshotAccess::Read,
            None,
            Some(&central_name),
        )
        .unwrap();
        let bytes = setup.bytes.clone();
        assert_eq!(setup.sequence, if intent { 2 } else { 1 });
        drop(setup);
        let replacement = claims::Lease::acquire(
            &authority,
            &parent,
            manager.root(),
            OsStr::new("copy"),
            &super::super::super::random_transaction_id().unwrap(),
        );
        if intent {
            assert!(matches!(
                replacement,
                Err(PlatformError::DestinationExists(_))
            ));
            let old = claims::Lease::open(&authority, &parent, &binding, false, false).unwrap();
            assert_eq!(old.observation().1, observed_claim.1);
        } else {
            let replacement = replacement.unwrap();
            assert_ne!(
                replacement.binding().identities()[4],
                binding.identities()[4]
            );
            let _ = replacement.abort_unstarted(io("owned fixture admission finished"));
        }
        let retained = Snapshot::read_pair(
            &authority,
            &parent,
            manager.root(),
            &stage_name,
            SnapshotAccess::Read,
            None,
            Some(&central_name),
        )
        .unwrap();
        assert_eq!(retained.bytes, bytes);
        retained
            .revalidate_pair(&parent, manager.root(), &central_name)
            .unwrap();
        manager.validate().unwrap();
    }
    assert_eq!(manager.binding().unwrap(), registry_binding);
    let mut after = None;
    as_caller(&mut || after = Some(observe_user_data()));
    assert_eq!(after.unwrap(), user_data);
    source_pin
        .validate_existing(&source_evidence, &mut |_| Ok(()))
        .unwrap();
    caller.validate().unwrap();
    println!(
        "caller journal source_directory={directory}: staging, reopen, plan and three Begin recovery cases passed"
    );
}
