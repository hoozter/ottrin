//! Caller-bound terminal recovery; payloads remain observations, never authority.
use super::*;
use crate::PlatformOps;
use std::{io::Write, sync::Arc};

struct ResetFaults;
impl Drop for ResetFaults {
    fn drop(&mut self) {
        FAULT.with(|fault| fault.set(None));
        claims::RELEASE_FAULT.with(|fault| fault.set(None));
    }
}

fn frozen(authority: &Authority, file: &File) -> tree::ExistingTreeEvidence {
    tree::PinnedTree::open(authority, file, 16, &mut |_| Ok(()))
        .unwrap()
        .freeze_existing(|_| Ok(()))
        .unwrap()
}

fn controls(authority: &Authority, parent: &File, central: &File, header: &Header) -> [Vec<u8>; 2] {
    let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    let stage = authority
        .child(
            parent,
            &header.stage_name(),
            true,
            FILE_GENERIC_READ,
            sharing,
        )
        .unwrap();
    let local = authority
        .child(&stage, OsStr::new(LOCAL), false, FILE_GENERIC_READ, sharing)
        .unwrap();
    let mirror = authority
        .child(
            central,
            &header.central_name(),
            false,
            FILE_GENERIC_READ,
            sharing,
        )
        .unwrap();
    assert_eq!(
        stable_identity_from_handle(&local).unwrap(),
        header.local_file
    );
    assert_eq!(
        stable_identity_from_handle(&mirror).unwrap(),
        header.central_file
    );
    [read(&local).unwrap(), read(&mirror).unwrap()]
}

fn later_edits(
    authority: &Authority,
    parent: &File,
    name: &OsStr,
    directory: bool,
) -> tree::ExistingTreeEvidence {
    let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    let mut root = authority
        .open_entry(
            parent,
            name,
            Some(directory),
            windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_WRITE | FILE_GENERIC_READ,
            sharing,
        )
        .unwrap();
    if directory {
        let mut unknown = authority
            .create(&root, OsStr::new("unknown-user-data"), false, sharing)
            .unwrap();
        unknown.write_all(b"retain unknown user data").unwrap();
        unknown.sync_all().unwrap();
    } else {
        root.write_all(b"later retained user edits").unwrap();
        root.sync_all().unwrap();
    }
    frozen(authority, &root)
}

fn claim_reserved(
    authority: &Authority,
    parent: &File,
    central: &File,
    target: &OsStr,
    reserved: bool,
) {
    let id = super::super::super::random_transaction_id().unwrap();
    let claim = claims::Lease::acquire(authority, parent, central, target, &id);
    if reserved {
        assert!(
            claim.is_err(),
            "An uncertain or unterminated attempt must keep its name"
        );
    } else {
        assert!(matches!(
            claim
                .unwrap()
                .abort_unstarted(io("owned next admission cancelled")),
            PlatformError::Io(_)
        ));
    }
}

pub(in super::super) fn exercise_caller_terminal_recovery(
    caller: &Arc<crate::windows_privileged_transport::CallerCopyContext>,
    protected_parent: &File,
    sources: [&File; 2],
    preferences: &File,
    index: &File,
    mut recapture: impl FnMut() -> Arc<crate::windows_privileged_transport::CallerCopyContext>,
    mut as_caller: impl FnMut(&mut dyn FnMut()),
) {
    // Complete both sources' public terminal phases before constructing
    // deliberately divergent prefixes in this shared canonical registry.
    for end_attempt in [false, true] {
        for source in sources {
            let authority = Authority::Caller(Arc::clone(caller));
            let source_path = windows_fs::location(source).unwrap();
            let source_name = source_path.file_name().unwrap();
            let source_pin =
                tree::PinnedTree::open(&authority, source, 16, &mut |_| Ok(())).unwrap();
            let source_evidence = source_pin.freeze_existing(|_| Ok(())).unwrap();
            let directory = source_pin.root().directory;
            let manager = caller
                .copy_registry(std::slice::from_ref(&source_path), true)
                .unwrap()
                .unwrap();
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
            let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

            // Cover both non-abort terminal phases before leaving deliberately
            // uncertain staging records in the shared registry below.
            if !end_attempt {
                for undo in [true, false] {
                    let parent = windows_fs::create_private(
                        protected_parent,
                        OsStr::new(&format!("caller-name-release-{directory}-{undo}")),
                        true,
                        sharing,
                    )
                    .unwrap();
                    if !undo {
                        let mut original = authority
                            .create(&parent, source_name, false, sharing)
                            .unwrap();
                        original
                            .write_all(b"retained original for name release")
                            .unwrap();
                        original.sync_all().unwrap();
                    }
                    let receipt = caller
                        .copy_entry_with_progress(
                            &source_path,
                            &windows_fs::location(&parent).unwrap(),
                            crate::ConflictAction::Overwrite,
                            16,
                            |_| Ok(()),
                        )
                        .unwrap()
                        .unwrap();
                    let id = receipt.copy_transaction_id().unwrap().to_owned();
                    let header = discovery::RegisteredJournal::open(
                        &authority,
                        manager.root(),
                        &OsString::from(format!("{id}.copy-journal")),
                    )
                    .unwrap()
                    .state
                    .header
                    .unwrap();
                    let reset = ResetFaults;
                    claims::RELEASE_FAULT
                        .with(|fault| fault.set(Some((claims::ReleasePoint::BeforeWrite, false))));
                    let error = if undo {
                        crate::DefaultPlatform
                            .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
                            .unwrap_err()
                    } else {
                        cleanup::Admission::observe_with(
                            &authority,
                            &parent,
                            manager.root(),
                            &header.stage_name(),
                            |_| Ok(()),
                        )
                        .unwrap()
                        .start(&parent, manager.root(), |_| Ok(()))
                        .err()
                        .unwrap()
                    };
                    assert!(
                        error.to_string().contains("injected Copy claim release"),
                        "{error}"
                    );
                    drop(reset);
                    drop(receipt);
                    let retained = if undo {
                        header.candidate_name()
                    } else {
                        header.original_name()
                    };
                    let payload_before =
                        later_edits(&authority, &parent, &retained, undo && directory);
                    let journal_before = controls(&authority, &parent, manager.root(), &header);
                    let held = authority
                        .open_entry(
                            &parent,
                            &retained,
                            Some(undo && directory),
                            FILE_GENERIC_READ,
                            0,
                        )
                        .unwrap();
                    claim_reserved(&authority, &parent, manager.root(), source_name, true);
                    assert!(
                        name_release::RetainedCopyNameReleasePlan::admit(
                            manager.path(),
                            &id,
                            |_| Ok(())
                        )
                        .is_err()
                    );
                    let fresh = recapture();
                    assert!(!Arc::ptr_eq(&fresh, caller));
                    let registry = fresh.copy_registry(&[], false).unwrap().unwrap();
                    assert!(
                        registry
                            .name_release_plan_with_progress(&id, |_| Err(io(
                                "owned name-release admission cancellation"
                            )))
                            .is_err()
                    );
                    let mut plan = registry
                        .name_release_plan_with_progress(&id, |_| Ok(()))
                        .unwrap();
                    assert_eq!(
                        plan.admitted_phase(),
                        if undo {
                            WindowsCopyPhase::Undone
                        } else {
                            WindowsCopyPhase::CleaningUp
                        }
                    );
                    assert_eq!(plan.transaction_id(), id);
                    let mut stale = plan.clone();
                    assert!(
                        plan.finish(|_| Err(io("owned name-release cancellation")))
                            .is_err()
                    );
                    assert_eq!(
                        controls(&authority, &parent, manager.root(), &header),
                        journal_before
                    );
                    plan.finish(|_| Ok(())).unwrap();
                    assert!(plan.complete());
                    assert!(plan.finish(|_| Ok(())).is_err());
                    assert!(stale.finish(|_| Ok(())).is_err());
                    assert!(
                        registry
                            .name_release_plan_with_progress(&id, |_| Ok(()))
                            .is_err()
                    );
                    assert_eq!(
                        controls(&authority, &parent, manager.root(), &header),
                        journal_before
                    );
                    drop(held);
                    let payload =
                        namespace::open_source_with(&authority, &parent, &retained, None).unwrap();
                    assert_eq!(frozen(&authority, &payload), payload_before);
                    claim_reserved(&authority, &parent, manager.root(), source_name, false);
                    println!(
                        "caller terminal name release source_directory={directory} undo={undo}: readonly admission, cancellation, exclusive edited payload preservation, stale refusal and released-name reuse passed"
                    );
                }
            } else {
                for overwrite in [false, true] {
                    // Direct terminal write, paired terminal prefix, divergent local prefix.
                    for mode in 0..3 {
                        let parent = windows_fs::create_private(
                            protected_parent,
                            OsStr::new(&format!(
                                "caller-end-attempt-{directory}-{overwrite}-{mode}"
                            )),
                            true,
                            sharing,
                        )
                        .unwrap();
                        let original = if overwrite {
                            let mut file = authority
                                .create(&parent, OsStr::new("copy"), false, sharing)
                                .unwrap();
                            file.write_all(b"unchanged public original").unwrap();
                            file.sync_all().unwrap();
                            Some(frozen(&authority, &file))
                        } else {
                            None
                        };
                        let destination = if overwrite {
                            DestinationAdmission::overwrite(
                                &authority,
                                &parent,
                                OsStr::new("copy"),
                                16,
                                |_| Ok(()),
                            )
                            .unwrap()
                        } else {
                            DestinationAdmission::creation(&authority, &parent, OsStr::new("copy"))
                                .unwrap()
                        };
                        let names = windows_fs::directory_children(manager.root()).unwrap();
                        let error = DurableStage::create(
                            source,
                            &parent,
                            manager.root(),
                            &destination,
                            OsStr::new("unavailable-source-label"),
                            16,
                            |bytes| {
                                if bytes > 0 {
                                    Err(io("owned unverified payload stop"))
                                } else {
                                    Ok(())
                                }
                            },
                        )
                        .err()
                        .unwrap();
                        assert!(
                            error.to_string().contains("owned unverified payload stop"),
                            "{error}"
                        );
                        let mut added = windows_fs::directory_children(manager.root())
                            .unwrap()
                            .into_iter()
                            .filter(|name| {
                                !names.contains(name) && discovery::registry_id(name).is_some()
                            });
                        let central_name = added.next().unwrap();
                        assert!(added.next().is_none());
                        let header = discovery::RegisteredJournal::open(
                            &authority,
                            manager.root(),
                            &central_name,
                        )
                        .unwrap()
                        .state
                        .header
                        .unwrap();
                        let id = header.transaction.clone();
                        let payload_before =
                            later_edits(&authority, &parent, &header.candidate_name(), directory);
                        let journal_before = controls(&authority, &parent, manager.root(), &header);
                        let held = authority
                            .open_entry(
                                &parent,
                                &header.candidate_name(),
                                Some(directory),
                                FILE_GENERIC_READ,
                                0,
                            )
                            .unwrap();
                        claim_reserved(
                            &authority,
                            &parent,
                            manager.root(),
                            OsStr::new("copy"),
                            true,
                        );
                        assert!(
                            abort::RetainedCopyAbortPlan::admit(manager.path(), &id, |_| Ok(()))
                                .is_err()
                        );
                        let fresh = recapture();
                        let registry = fresh.copy_registry(&[], false).unwrap().unwrap();
                        assert!(
                            registry
                                .abort_plan_with_progress("../invalid", |_| Ok(()))
                                .is_err()
                        );
                        assert!(
                            registry
                                .abort_plan_with_progress(&id, |_| Err(io(
                                    "owned end-attempt admission cancellation"
                                )))
                                .is_err()
                        );
                        assert!(
                            registry
                                .name_release_plan_with_progress(&id, |_| Ok(()))
                                .is_err()
                        );
                        let mut plan = registry.abort_plan_with_progress(&id, |_| Ok(())).unwrap();
                        let mut stale = plan.clone();
                        assert_eq!(plan.transaction_id(), id);
                        assert_eq!(plan.destination(), windows_fs::location(&parent).unwrap());
                        assert_eq!(
                            plan.target(),
                            windows_fs::location(&parent).unwrap().join("copy")
                        );
                        assert!(
                            plan.end_attempt(|_| Err(io("owned end-attempt cancellation")))
                                .is_err()
                        );
                        assert_eq!(
                            controls(&authority, &parent, manager.root(), &header),
                            journal_before
                        );
                        let state = decode(&journal_before[0]).unwrap();
                        let reset = ResetFaults;
                        if mode != 0 {
                            FAULT.with(|fault| {
                                fault.set(Some((
                                    state.sequence,
                                    if mode == 1 {
                                        Step::CentralFlushed
                                    } else {
                                        Step::LocalFlushed
                                    },
                                    false,
                                )))
                            });
                        }
                        let result = plan.end_attempt(|_| Ok(()));
                        drop(reset);
                        if mode == 0 {
                            assert!(result.is_ok() && plan.complete());
                        } else {
                            assert!(result.is_err() && !plan.complete());
                        }
                        assert!(stale.end_attempt(|_| Ok(())).is_err());
                        let after_terminal = controls(&authority, &parent, manager.root(), &header);
                        let fresh = recapture();
                        let registry = fresh.copy_registry(&[], false).unwrap().unwrap();
                        assert!(registry.abort_plan_with_progress(&id, |_| Ok(())).is_err());
                        if mode == 1 {
                            assert_eq!(after_terminal[0], after_terminal[1]);
                            let mut release = registry
                                .name_release_plan_with_progress(&id, |_| Ok(()))
                                .unwrap();
                            assert_eq!(release.admitted_phase(), WindowsCopyPhase::Aborted);
                            assert!(
                                release
                                    .finish(|_| Err(io("owned aborted release cancellation")))
                                    .is_err()
                            );
                            assert_eq!(
                                controls(&authority, &parent, manager.root(), &header),
                                after_terminal
                            );
                            release.finish(|_| Ok(())).unwrap();
                            assert!(release.complete());
                            assert_eq!(
                                controls(&authority, &parent, manager.root(), &header),
                                after_terminal
                            );
                        } else {
                            assert!(
                                registry
                                    .name_release_plan_with_progress(&id, |_| Ok(()))
                                    .is_err()
                            );
                            if mode == 2 {
                                assert_ne!(after_terminal[0], after_terminal[1]);
                            }
                        }
                        drop(held);
                        let payload = namespace::open_source_with(
                            &authority,
                            &parent,
                            &header.candidate_name(),
                            None,
                        )
                        .unwrap();
                        assert_eq!(frozen(&authority, &payload), payload_before);
                        if let Some(original) = original {
                            let file = namespace::open_source_with(
                                &authority,
                                &parent,
                                OsStr::new("copy"),
                                Some(false),
                            )
                            .unwrap();
                            assert_eq!(frozen(&authority, &file), original);
                        } else {
                            assert!(
                                matches!(authority.probe(&parent, OsStr::new("copy")), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
                            );
                        }
                        claim_reserved(
                            &authority,
                            &parent,
                            manager.root(),
                            OsStr::new("copy"),
                            mode == 2,
                        );
                        assert_eq!(manager.binding().unwrap(), registry_binding);
                        println!(
                            "caller end attempt source_directory={directory} overwrite={overwrite} mode={mode}: caller admission, cancellation, exclusive unknown payload/original preservation, prefix-safe name release and stale refusal passed"
                        );
                    }
                }
            }
            source_pin
                .validate_existing(&source_evidence, &mut |_| Ok(()))
                .unwrap();
            let mut after = None;
            as_caller(&mut || after = Some(observe()));
            assert_eq!(after.unwrap(), before);
            caller.validate().unwrap();
        }
    }
}
