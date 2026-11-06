//! Owned native caller journal controls; not the enabled dispatcher or Undo bridge.
use super::*;
use crate::PlatformOps;
use std::{io::Write, sync::Arc};

struct ResetFault;
impl Drop for ResetFault {
    fn drop(&mut self) {
        FAULT.with(|fault| fault.set(None));
    }
}

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
    let source_pin =
        tree::PinnedTree::open(&Authority::Process, source, 16, &mut |_| Ok(())).unwrap();
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
    let retained =
        RetainedStage::from_snapshot(&parent, manager.root(), snapshot, |_| Ok(())).unwrap();
    assert_eq!(retained.entry_count(), source_evidence.entries.len());
    let recovered_plan = retained.plan_destination_security(|| Ok(())).unwrap();
    assert_eq!(recovered_plan.evidence(), evidence);
    recovered_plan.validate().unwrap();
    retained.revalidate(|_| Ok(())).unwrap();
    drop(recovered_plan);
    let mut publication = PermissionStage::from_retained(retained, None, |_| Ok(())).unwrap();
    publication.apply(|_| Ok(())).unwrap();
    assert_eq!(publication.completed(), source_evidence.entries.len());
    assert!(publication.journal.state.completion_verified);
    drop(publication);
    let snapshot = Snapshot::load(
        &authority,
        &parent,
        manager.root(),
        &name,
        SnapshotAccess::Read,
    )
    .unwrap();
    let retained =
        RetainedStage::from_snapshot(&parent, manager.root(), snapshot, |_| Ok(())).unwrap();
    retained.revalidate(|_| Ok(())).unwrap();
    let final_plan = retained.plan_destination_security(|| Ok(())).unwrap();
    assert_eq!(final_plan.evidence(), evidence.at_root(OsStr::new("copy")));
    drop(final_plan);
    // A fresh writer must re-admit the completed destination without source data.
    let mut replay = PermissionStage::from_retained(retained, None, |_| Ok(())).unwrap();
    replay.apply(|_| Ok(())).unwrap();
    assert!(replay.journal.state.completion_verified);
    drop(replay);
    use crate::PlatformOps;
    assert!(
        discovery::session_receipt(&Authority::Process, &parent, manager.root(), &name, |_| Ok(
            ()
        ))
        .is_err()
    );
    let receipt =
        discovery::session_receipt(&authority, &parent, manager.root(), &name, |_| Ok(())).unwrap();
    assert!(
        receipt
            .refresh_windows_copy_undo(|_| Err(io("owned caller refresh cancellation")))
            .is_err()
    );
    let receipt = receipt.refresh_windows_copy_undo(|_| Ok(())).unwrap();
    crate::DefaultPlatform
        .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
        .unwrap();
    crate::DefaultPlatform
        .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
        .unwrap();
    let snapshot = Snapshot::load(
        &authority,
        &parent,
        manager.root(),
        &name,
        SnapshotAccess::Read,
    )
    .unwrap();
    assert!(snapshot.state.undo_complete());
    assert!(
        !authority
            .identity_at(
                &parent,
                OsStr::new("copy"),
                snapshot.state.final_versions[0].identity
            )
            .unwrap()
    );
    snapshot.revalidate(&parent, manager.root()).unwrap();
    drop((snapshot, receipt));

    // Exercise the actual unified staging/publication/receipt path too; the
    // source and canonical registry admission remain pinned across its lifetime.
    let execute_parent = windows_fs::create_private(
        protected_parent,
        OsStr::new(&format!("caller-execute-{directory}")),
        true,
        sharing,
    )
    .unwrap();
    let destination =
        DestinationAdmission::creation(&authority, &execute_parent, OsStr::new("copy")).unwrap();
    let foreign_registry = windows_fs::create_private(
        protected_parent,
        OsStr::new(&format!("foreign-execute-registry-{directory}")),
        true,
        sharing,
    )
    .unwrap();
    assert!(
        DurableStage::execute_with_receipt(
            source,
            &execute_parent,
            &foreign_registry,
            &destination,
            OsStr::new("caller-source"),
            16,
            |_| Ok(())
        )
        .is_err()
    );
    assert!(
        windows_fs::directory_children_limited(&execute_parent, 1)
            .unwrap()
            .is_empty()
    );
    assert!(
        windows_fs::directory_children_limited(&foreign_registry, 1)
            .unwrap()
            .is_empty()
    );
    let receipt = DurableStage::execute_with_receipt(
        source,
        &execute_parent,
        manager.root(),
        &destination,
        OsStr::new("caller-source"),
        16,
        |_| Ok(()),
    )
    .unwrap();
    let receipt = receipt.refresh_windows_copy_undo(|_| Ok(())).unwrap();
    let identity = receipt.identity.windows;
    assert!(
        authority
            .identity_at(&execute_parent, OsStr::new("copy"), identity)
            .unwrap()
    );
    crate::DefaultPlatform
        .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
        .unwrap();
    assert!(
        !authority
            .identity_at(&execute_parent, OsStr::new("copy"), identity)
            .unwrap()
    );
    drop(receipt);
    println!(
        "caller receipt source_directory={directory}: unified execution, canonical receipt refresh, public Undo and idempotent replay passed"
    );
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
    exercise_caller_resume(
        &authority,
        protected_parent,
        source,
        manager.root(),
        directory,
    );
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

/// Real paired states are reopened after every old writer/payload owner closes.
fn exercise_caller_resume(
    authority: &Authority,
    protected_parent: &File,
    source: &File,
    central: &File,
    directory: bool,
) {
    let registry = windows_fs::location(central).unwrap();
    for case in 0..4 {
        let parent = windows_fs::create_private(
            protected_parent,
            OsStr::new(&format!("caller-resume-{directory}-{case}")),
            true,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
        .unwrap();
        let destination =
            DestinationAdmission::creation(authority, &parent, OsStr::new("copy")).unwrap();
        let mut staged = DurableStage::create(
            source,
            &parent,
            central,
            &destination,
            OsStr::new("unavailable-source-label"),
            16,
            |_| Ok(()),
        )
        .unwrap();
        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
        let name = staged.stage.name.clone();
        let id = staged.journal.header.transaction.clone();
        let renamed_sequence = staged.journal.sequence + 1;
        drop(staged);
        if case == 1 {
            let reset = ResetFault;
            FAULT
                .with(|fault| fault.set(Some((renamed_sequence, Step::PublicationRenamed, false))));
            let error = PermissionStage::publish(authority, &parent, central, &name, |_| Ok(()))
                .err()
                .expect("publication rename fault must fire");
            assert!(
                error
                    .to_string()
                    .contains("injected Copy journal flush boundary failure"),
                "{error}"
            );
            drop(reset);
        } else if case >= 2 {
            let mut publication =
                PermissionStage::publish(authority, &parent, central, &name, |_| Ok(())).unwrap();
            assert!(publication.journal.state.published);
            if case == 3 {
                let reset = ResetFault;
                let sequence = publication.journal.sequence + 1;
                FAULT.with(|fault| fault.set(Some((sequence, Step::PermissionsChanged, false))));
                let error = publication.apply(|_| Ok(())).unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains("injected Copy journal flush boundary failure"),
                    "{error}"
                );
                drop(reset);
            }
            drop(publication);
        }
        let before = Snapshot::load(authority, &parent, central, &name, SnapshotAccess::Read)
            .unwrap()
            .bytes;
        assert!(
            RetainedCopyResumePlan::admit(&Authority::Process, &registry, &id, |_| Ok(())).is_err()
        );
        let mut plan =
            RetainedCopyResumePlan::admit(authority, &registry, &id, |_| Ok(())).unwrap();
        assert!(
            plan.resume(|_| Err(io("owned caller resume cancellation")))
                .is_err()
        );
        assert!(!plan.complete());
        assert_eq!(
            Snapshot::load(authority, &parent, central, &name, SnapshotAccess::Read)
                .unwrap()
                .bytes,
            before
        );
        let mut replay = plan.clone();
        plan.resume(|_| Ok(())).unwrap();
        assert!(plan.complete());
        assert!(plan.resume(|_| Ok(())).is_err());
        let completed =
            Snapshot::load(authority, &parent, central, &name, SnapshotAccess::Read).unwrap();
        assert!(completed.state.completion_verified);
        let bytes = completed.bytes.clone();
        drop(completed);
        replay.resume(|_| Ok(())).unwrap();
        assert!(replay.complete());
        assert_eq!(
            Snapshot::load(authority, &parent, central, &name, SnapshotAccess::Read)
                .unwrap()
                .bytes,
            bytes
        );
        let receipt =
            discovery::session_receipt(authority, &parent, central, &name, |_| Ok(())).unwrap();
        use crate::PlatformOps;
        crate::DefaultPlatform
            .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
            .unwrap();
        assert!(
            Snapshot::load(authority, &parent, central, &name, SnapshotAccess::Read)
                .unwrap()
                .state
                .undo_complete()
        );
        println!(
            "caller resume source_directory={directory} case={case}: fresh admission, cancellation preservation, completion, readonly replay and public Undo passed"
        );
    }
}

/// Preserve caller-private existing data through retention, interrupted rename,
/// final publication, public Undo and idempotent restoration.
pub(in super::super) fn exercise_caller_overwrite(
    caller: &Arc<crate::windows_privileged_transport::CallerCopyContext>,
    protected_parent: &File,
    source: &File,
    preferences: &File,
    index: &File,
    mut as_caller: impl FnMut(&mut dyn FnMut()),
) {
    let authority = Authority::Caller(Arc::clone(caller));
    let source_pin = tree::PinnedTree::open(&authority, source, 16, &mut |_| Ok(())).unwrap();
    let source_evidence = source_pin.freeze_existing(|_| Ok(())).unwrap();
    let directory = source_pin.root().directory;
    let source_path = windows_fs::location(source).unwrap();
    let manager = caller.copy_registry(&[source_path], true).unwrap().unwrap();
    let registry_binding = manager.binding().unwrap();
    let observe = || {
        [preferences, index].map(|file| {
            (
                read(file).unwrap(),
                stable_identity_from_handle(file).unwrap(),
                security::owner_group_dacl_digest(file).unwrap(),
            )
        })
    };
    let mut before = None;
    as_caller(&mut || before = Some(observe()));
    let before = before.unwrap();
    use crate::PlatformOps;
    for case in 0..2 {
        let parent = windows_fs::create_private(
            protected_parent,
            OsStr::new(&format!("caller-overwrite-{directory}-{case}")),
            true,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
        .unwrap();
        let mut original = authority
            .create(
                &parent,
                OsStr::new("copy"),
                directory,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            )
            .unwrap();
        if directory {
            let nested = authority
                .create(
                    &original,
                    OsStr::new("Nested"),
                    true,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                )
                .unwrap();
            let mut old = authority
                .create(
                    &nested,
                    OsStr::new("old.txt"),
                    false,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                )
                .unwrap();
            old.write_all(b"original nested payload").unwrap();
            old.sync_all().unwrap();
            drop(old);
            drop(nested);
            let mut old = authority
                .create(
                    &original,
                    OsStr::new("root.txt"),
                    false,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                )
                .unwrap();
            old.write_all(b"original root payload").unwrap();
            old.sync_all().unwrap();
        } else {
            original.write_all(b"original file payload").unwrap();
        }
        original.sync_all().unwrap();
        let baseline = tree::PinnedTree::open(&authority, &original, 16, &mut |_| Ok(()))
            .unwrap()
            .freeze_existing(|_| Ok(()))
            .unwrap();
        let original_id = baseline.entries[0].version.identity;
        drop(original);
        let destination = DestinationAdmission::overwrite(
            &authority,
            &parent,
            OsStr::new("copy"),
            16,
            |_| Ok(()),
        )
        .unwrap();
        let receipt = if case == 0 {
            DurableStage::execute_with_receipt(
                source,
                &parent,
                manager.root(),
                &destination,
                OsStr::new("unavailable-source-label"),
                16,
                |_| Ok(()),
            )
            .unwrap()
        } else {
            let _dependencies = destination
                .guard_overwrite_dependencies(&parent, manager.root(), |_| Ok(()))
                .unwrap();
            let mut staged = DurableStage::create(
                source,
                &parent,
                manager.root(),
                &destination,
                OsStr::new("unavailable-source-label"),
                16,
                |_| Ok(()),
            )
            .unwrap();
            drop(staged.prepare_destination_security(|| Ok(())).unwrap());
            let name = staged.stage.name.clone();
            let id = staged.journal.header.transaction.clone();
            drop(staged);
            let snapshot = Snapshot::load(
                &authority,
                &parent,
                manager.root(),
                &name,
                SnapshotAccess::Write,
            )
            .unwrap();
            let mut journal = Journal::from_snapshot(&parent, manager.root(), snapshot).unwrap();
            journal.prepare_original(16, |_| Ok(())).unwrap();
            let sequence = journal.sequence + 1;
            let reset = ResetFault;
            FAULT.with(|fault| fault.set(Some((sequence, Step::OriginalRenamed, false))));
            let error = journal
                .move_original(None, OriginalMove::Retain, |_| Ok(()))
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("injected Copy journal flush boundary failure"),
                "{error}"
            );
            drop(reset);
            drop(journal);
            let mut plan = RetainedCopyResumePlan::admit(
                &authority,
                &windows_fs::location(manager.root()).unwrap(),
                &id,
                |_| Ok(()),
            )
            .unwrap();
            plan.resume(|_| Ok(())).unwrap();
            assert!(plan.complete());
            discovery::session_receipt(&authority, &parent, manager.root(), &name, |_| Ok(()))
                .unwrap()
        };
        assert!(
            authority
                .identity_at(&parent, OsStr::new("copy"), receipt.identity.windows)
                .unwrap()
        );
        assert_ne!(receipt.identity.windows, original_id);
        let receipt = receipt.refresh_windows_copy_undo(|_| Ok(())).unwrap();
        crate::DefaultPlatform
            .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
            .unwrap();
        crate::DefaultPlatform
            .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
            .unwrap();
        assert!(
            authority
                .identity_at(&parent, OsStr::new("copy"), original_id)
                .unwrap()
        );
        let restored =
            namespace::open_source_with(&authority, &parent, OsStr::new("copy"), None).unwrap();
        let actual = tree::PinnedTree::open(&authority, &restored, 16, &mut |_| Ok(()))
            .unwrap()
            .freeze_existing(|_| Ok(()))
            .unwrap();
        let mut expected = baseline.clone();
        expected.entries[0].version.changed = actual.entries[0].version.changed;
        assert_eq!(
            actual, expected,
            "Original contents, IDs, membership, metadata and permissions must survive both renames"
        );
        manager.validate().unwrap();
        println!(
            "caller overwrite source_directory={directory} case={case}: private original retention, publication, receipt refresh, public Undo and exact restoration passed"
        );
    }
    assert_eq!(manager.binding().unwrap(), registry_binding);
    let mut after = None;
    as_caller(&mut || after = Some(observe()));
    assert_eq!(after.unwrap(), before);
    source_pin
        .validate_existing(&source_evidence, &mut |_| Ok(()))
        .unwrap();
    caller.validate().unwrap();
}

/// Re-admit persisted records through independently captured authenticated
/// contexts, rather than cloning the old runtime plan or granting by status row.
pub(in super::super) fn exercise_caller_recovery(
    caller: &Arc<crate::windows_privileged_transport::CallerCopyContext>,
    protected_parent: &File,
    source: &File,
    preferences: &File,
    index: &File,
    mut recapture: impl FnMut() -> Arc<crate::windows_privileged_transport::CallerCopyContext>,
    mut as_caller: impl FnMut(&mut dyn FnMut()),
) {
    let authority = Authority::Caller(Arc::clone(caller));
    let source_pin = tree::PinnedTree::open(&authority, source, 16, &mut |_| Ok(())).unwrap();
    let source_evidence = source_pin.freeze_existing(|_| Ok(())).unwrap();
    let directory = source_pin.root().directory;
    let manager = caller
        .copy_registry(&[windows_fs::location(source).unwrap()], true)
        .unwrap()
        .unwrap();
    let binding = manager.binding().unwrap();
    let observe = || {
        [preferences, index].map(|file| {
            (
                read(file).unwrap(),
                stable_identity_from_handle(file).unwrap(),
                security::owner_group_dacl_digest(file).unwrap(),
            )
        })
    };
    let mut before = None;
    as_caller(&mut || before = Some(observe()));
    let before = before.unwrap();
    let parent = windows_fs::create_private(
        protected_parent,
        OsStr::new(&format!("caller-fresh-recovery-{directory}")),
        true,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )
    .unwrap();
    let destination =
        DestinationAdmission::creation(&authority, &parent, OsStr::new("copy")).unwrap();
    let mut staged = DurableStage::create(
        source,
        &parent,
        manager.root(),
        &destination,
        OsStr::new("unavailable-source-label"),
        16,
        |_| Ok(()),
    )
    .unwrap();
    drop(staged.prepare_destination_security(|| Ok(())).unwrap());
    let name = staged.stage.name.clone();
    let id = staged.journal.header.transaction.clone();
    drop(staged);
    let bytes = Snapshot::load(
        &authority,
        &parent,
        manager.root(),
        &name,
        SnapshotAccess::Read,
    )
    .unwrap()
    .bytes;
    let fresh = recapture();
    assert!(!Arc::ptr_eq(&fresh, caller));
    let fresh_registry = fresh.copy_registry(&[], false).unwrap().unwrap();
    assert!(
        fresh_registry
            .retained_with_progress(|_| Err(io("owned discovery cancellation")))
            .is_err()
    );
    let rows = fresh_registry.retained_with_progress(|_| Ok(())).unwrap();
    let row = rows
        .iter()
        .find(|row| row.transaction_id.as_deref() == Some(&id))
        .unwrap();
    assert_eq!(row.phase, Some(WindowsCopyPhase::ReadyToPublish));
    assert_eq!(row.evidence, WindowsCopyRecoveryEvidence::VerifiedTree);
    assert_eq!(
        Snapshot::load(
            &authority,
            &parent,
            manager.root(),
            &name,
            SnapshotAccess::Read
        )
        .unwrap()
        .bytes,
        bytes
    );
    assert!(
        fresh_registry
            .resume_plan_with_progress("../not-an-id", |_| Ok(()))
            .is_err()
    );
    let mut plan = fresh_registry
        .resume_plan_with_progress(&id, |_| Ok(()))
        .unwrap();
    plan.resume(|_| Ok(())).unwrap();
    assert!(plan.complete());
    drop(plan);
    drop(fresh_registry);
    drop(fresh);
    let fresh = recapture();
    assert!(!Arc::ptr_eq(&fresh, caller));
    let fresh_registry = fresh.copy_registry(&[], false).unwrap().unwrap();
    let rows = fresh_registry.retained_with_progress(|_| Ok(())).unwrap();
    let row = rows
        .iter()
        .find(|row| row.transaction_id.as_deref() == Some(&id))
        .unwrap();
    assert_eq!(row.phase, Some(WindowsCopyPhase::Completed));
    assert_eq!(row.evidence, WindowsCopyRecoveryEvidence::VerifiedTree);
    assert!(
        fresh_registry
            .resume_plan_with_progress(&id, |_| Ok(()))
            .is_err()
    );
    let mut undo = fresh_registry
        .undo_plan_with_progress(&id, |_| Ok(()))
        .unwrap();
    undo.undo_next_with_progress(|_| Ok(())).unwrap();
    assert_eq!(undo.remaining(), 0);
    drop(undo);
    let rows = fresh_registry.retained_with_progress(|_| Ok(())).unwrap();
    let row = rows
        .iter()
        .find(|row| row.transaction_id.as_deref() == Some(&id))
        .unwrap();
    assert_eq!(row.phase, Some(WindowsCopyPhase::Undone));
    assert_eq!(row.evidence, WindowsCopyRecoveryEvidence::VerifiedTree);
    assert!(
        fresh_registry
            .undo_plan_with_progress(&id, |_| Ok(()))
            .is_err()
    );
    assert_eq!(manager.binding().unwrap(), binding);
    let mut after = None;
    as_caller(&mut || after = Some(observe()));
    assert_eq!(after.unwrap(), before);
    source_pin
        .validate_existing(&source_evidence, &mut |_| Ok(()))
        .unwrap();
    caller.validate().unwrap();
    println!(
        "caller fresh recovery source_directory={directory}: recaptured-context discovery, restored Resume, restored Undo, cancellation and readonly evidence passed"
    );
}

/// Exercise actual retained payload deletion; metadata/cohort retirement is a
/// separate phase and cannot be inferred from this component control.
pub(in super::super) fn exercise_caller_cleanup(
    caller: &Arc<crate::windows_privileged_transport::CallerCopyContext>,
    protected_parent: &File,
    source: &File,
    preferences: &File,
    index: &File,
    mut recapture: impl FnMut() -> Arc<crate::windows_privileged_transport::CallerCopyContext>,
    mut as_caller: impl FnMut(&mut dyn FnMut()),
) {
    let authority = Authority::Caller(Arc::clone(caller));
    let source_pin = tree::PinnedTree::open(&authority, source, 16, &mut |_| Ok(())).unwrap();
    let source_evidence = source_pin.freeze_existing(|_| Ok(())).unwrap();
    let directory = source_pin.root().directory;
    let manager = caller
        .copy_registry(&[windows_fs::location(source).unwrap()], true)
        .unwrap()
        .unwrap();
    let binding = manager.binding().unwrap();
    let observe = || {
        [preferences, index].map(|file| {
            (
                read(file).unwrap(),
                stable_identity_from_handle(file).unwrap(),
                security::owner_group_dacl_digest(file).unwrap(),
            )
        })
    };
    let mut before = None;
    as_caller(&mut || before = Some(observe()));
    let before = before.unwrap();
    for undo in [false, true] {
        let parent = windows_fs::create_private(
            protected_parent,
            OsStr::new(&format!("caller-cleanup-{directory}-{undo}")),
            true,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
        .unwrap();
        let destination = if undo {
            DestinationAdmission::creation(&authority, &parent, OsStr::new("copy")).unwrap()
        } else {
            let mut original = authority
                .create(
                    &parent,
                    OsStr::new("copy"),
                    directory,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                )
                .unwrap();
            if directory {
                let nested = authority
                    .create(
                        &original,
                        OsStr::new("Nested"),
                        true,
                        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    )
                    .unwrap();
                for (dir, name) in [(&original, "root.txt"), (&nested, "old.txt")] {
                    let mut file = authority
                        .create(
                            dir,
                            OsStr::new(name),
                            false,
                            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                        )
                        .unwrap();
                    file.write_all(b"retained original cleanup payload")
                        .unwrap();
                    file.sync_all().unwrap();
                }
            } else {
                original
                    .write_all(b"retained original cleanup payload")
                    .unwrap();
            }
            original.sync_all().unwrap();
            drop(original);
            DestinationAdmission::overwrite(&authority, &parent, OsStr::new("copy"), 16, |_| Ok(()))
                .unwrap()
        };
        let receipt = DurableStage::execute_with_receipt(
            source,
            &parent,
            manager.root(),
            &destination,
            OsStr::new("unavailable-source-label"),
            16,
            |_| Ok(()),
        )
        .unwrap();
        if undo {
            crate::DefaultPlatform
                .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
                .unwrap();
        }
        drop(receipt);
        let name = windows_fs::directory_children_limited(&parent, 16)
            .unwrap()
            .into_iter()
            .find(|name| name.to_string_lossy().starts_with(".ottrin-copy-"))
            .unwrap();
        let snapshot = Snapshot::load(
            &authority,
            &parent,
            manager.root(),
            &name,
            SnapshotAccess::Read,
        )
        .unwrap();
        let bytes = snapshot.bytes.clone();
        let retained = if undo {
            snapshot.state.header.as_ref().unwrap().candidate_name()
        } else {
            snapshot.state.header.as_ref().unwrap().original_name()
        };
        drop(snapshot);
        let public = if undo {
            None
        } else {
            let file =
                namespace::open_source_with(&authority, &parent, OsStr::new("copy"), None).unwrap();
            Some(
                tree::PinnedTree::open(&authority, &file, 16, &mut |_| Ok(()))
                    .unwrap()
                    .freeze_existing(|_| Ok(()))
                    .unwrap(),
            )
        };
        assert!(cleanup::Admission::observe(&parent, manager.root(), &name, |_| Ok(())).is_err());
        let fresh = Authority::Caller(recapture());
        assert!(
            cleanup::Admission::observe_with(&fresh, &parent, manager.root(), &name, |_| Err(io(
                "owned cancellation"
            )))
            .is_err()
        );
        assert_eq!(
            Snapshot::load(
                &authority,
                &parent,
                manager.root(),
                &name,
                SnapshotAccess::Read
            )
            .unwrap()
            .bytes,
            bytes
        );
        let admission =
            cleanup::Admission::observe_with(&fresh, &parent, manager.root(), &name, |_| Ok(()))
                .unwrap();
        assert!(
            admission
                .reclaim(&parent, manager.root(), |_| Err(io("owned cancellation")))
                .is_err()
        );
        assert_eq!(
            Snapshot::load(
                &authority,
                &parent,
                manager.root(),
                &name,
                SnapshotAccess::Read
            )
            .unwrap()
            .bytes,
            bytes
        );
        let completion = if undo {
            let started = admission
                .start(&parent, manager.root(), |_| Ok(()))
                .unwrap();
            let snapshot = Snapshot::load(
                &authority,
                &parent,
                manager.root(),
                &name,
                SnapshotAccess::Read,
            )
            .unwrap();
            let sequence = snapshot.sequence + 1;
            drop(snapshot);
            let reset = ResetFault;
            FAULT.with(|fault| fault.set(Some((sequence, Step::CleanupHandleClosed, false))));
            let error = started
                .reclaim(&parent, manager.root(), |_| Ok(()))
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("injected Copy journal flush boundary failure"),
                "{error}"
            );
            drop(reset);
            drop(started);
            let fresh = Authority::Caller(recapture());
            let resumed = cleanup::Admission::observe_with(
                &fresh,
                &parent,
                manager.root(),
                &name,
                |_| Ok(()),
            )
            .unwrap();
            assert!(resumed.pending_entry_absent());
            resumed
                .reclaim(&parent, manager.root(), |_| Ok(()))
                .unwrap()
        } else {
            admission
                .reclaim(&parent, manager.root(), |_| Ok(()))
                .unwrap()
        };
        assert!(
            matches!(authority.probe(&parent,&retained),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
        );
        let snapshot = Snapshot::load(
            &authority,
            &parent,
            manager.root(),
            &name,
            SnapshotAccess::Read,
        )
        .unwrap();
        assert!(snapshot.state.cleanup.as_ref().unwrap().payload_removed);
        drop(snapshot);
        let plan = completion
            .begin(&parent, manager.root(), |_| Ok(()))
            .unwrap();
        drop(completion);
        assert!(
            cleanup_finalize::Plan::observe(&parent, manager.root(), &name, |_| Ok(())).is_err()
        );
        let fresh_caller = recapture();
        let fresh_registry = fresh_caller.copy_registry(&[], false).unwrap().unwrap();
        let fresh = Authority::Caller(fresh_caller);
        let metadata_bytes = Snapshot::load(
            &fresh,
            &parent,
            fresh_registry.root(),
            &name,
            SnapshotAccess::Read,
        )
        .unwrap()
        .bytes;
        assert!(
            cleanup_finalize::Plan::observe_with(
                &fresh,
                &parent,
                fresh_registry.root(),
                &name,
                |_| Err(io("owned metadata cancellation"))
            )
            .is_err()
        );
        assert_eq!(
            Snapshot::load(
                &fresh,
                &parent,
                fresh_registry.root(),
                &name,
                SnapshotAccess::Read
            )
            .unwrap()
            .bytes,
            metadata_bytes
        );
        let restored = cleanup_finalize::Plan::observe_with(
            &fresh,
            &parent,
            fresh_registry.root(),
            &name,
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(restored, plan);
        let central = fresh_registry.writable_root().unwrap();
        assert!(restored.finish(&parent, &central, |_| Ok(())).is_err());
        assert_eq!(
            Snapshot::load(&fresh, &parent, &central, &name, SnapshotAccess::Read)
                .unwrap()
                .bytes,
            metadata_bytes
        );
        if undo {
            let sequence = Snapshot::load(&fresh, &parent, &central, &name, SnapshotAccess::Read)
                .unwrap()
                .sequence;
            let reset = ResetFault;
            FAULT.with(|slot| {
                slot.set(Some((
                    sequence,
                    Step::CleanupMetadata {
                        role: CleanupMetadataRole::Local,
                        point: CleanupMetadataPoint::HandleClosed,
                    },
                    false,
                )))
            });
            let error = restored
                .finish_with(&fresh, &parent, &central, |_| Ok(()))
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("injected Copy journal flush boundary failure"),
                "{error}"
            );
            drop(reset);
            let next = Authority::Caller(recapture());
            let resumed =
                cleanup_finalize::Plan::observe_with(&next, &parent, &central, &name, |_| Ok(()))
                    .unwrap();
            resumed
                .finish_with(&next, &parent, &central, |_| Ok(()))
                .unwrap();
            resumed
                .finish_with(&next, &parent, &central, |_| Ok(()))
                .unwrap();
        } else {
            restored
                .finish_with(&fresh, &parent, &central, |_| Ok(()))
                .unwrap();
            restored
                .finish_with(&fresh, &parent, &central, |_| Ok(()))
                .unwrap();
        }
        fresh_registry.validate().unwrap();
        assert!(
            matches!(authority.probe(&parent,&name),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
        );
        if let Some(expected) = public {
            let file =
                namespace::open_source_with(&authority, &parent, OsStr::new("copy"), None).unwrap();
            let current = tree::PinnedTree::open(&authority, &file, 16, &mut |_| Ok(()))
                .unwrap()
                .freeze_existing(|_| Ok(()))
                .unwrap();
            assert_eq!(current, expected);
        } else {
            assert!(
                matches!(authority.probe(&parent,OsStr::new("copy")),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
            );
        }
        println!(
            "caller metadata source_directory={directory} undo={undo}: paired intent, read-only fresh admission, private authority, ordered removal and idempotent replay passed"
        );
        println!(
            "caller cleanup source_directory={directory} undo={undo}: retained payload deletion, cancellation, private authority and public data preservation passed"
        );
    }
    assert_eq!(manager.binding().unwrap(), binding);
    let mut after = None;
    as_caller(&mut || after = Some(observe()));
    assert_eq!(after.unwrap(), before);
    source_pin
        .validate_existing(&source_evidence, &mut |_| Ok(()))
        .unwrap();
    caller.validate().unwrap();
}
