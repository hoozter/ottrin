//! Faults reached by the actual caller dispatcher; no synthetic journal seeds.
use super::*;
use crate::{ConflictAction, PlatformOps, windows_privileged_transport::CallerCopyContext};
use std::{os::windows::io::AsRawHandle, sync::Arc};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_BASIC_INFO, FileBasicInfo, SetFileInformationByHandle,
};

struct Trace;
impl Trace {
    fn start() -> Self {
        BOUNDARY_TRACE.with(|slot| assert!(slot.replace(Some(Vec::new())).is_none()));
        Self
    }
    fn finish(self) -> Vec<(u64, Step, bool)> {
        BOUNDARY_TRACE.with(|slot| slot.borrow_mut().take().unwrap())
    }
}
impl Drop for Trace {
    fn drop(&mut self) {
        BOUNDARY_TRACE.with(|slot| slot.borrow_mut().take());
        FAULT.with(|slot| slot.set(None));
    }
}
fn evidence(authority: &Authority, file: &File) -> tree::ExistingTreeEvidence {
    tree::PinnedTree::open(authority, file, 8192, &mut |_| Ok(()))
        .unwrap()
        .freeze_existing(|_| Ok(()))
        .unwrap()
}
fn payload(
    authority: &Authority,
    parent: &File,
    name: &OsStr,
) -> Option<tree::ExistingTreeEvidence> {
    match authority.probe(parent, name) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("payload inspection failed: {error}"),
        Ok(_) => Some(evidence(
            authority,
            &namespace::open_source_with(authority, parent, name, None).unwrap(),
        )),
    }
}

fn prime(parent: &File, name: &OsStr, directory: bool) {
    let seed = windows_fs::create_private(
        parent,
        name,
        directory,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )
    .unwrap();
    let basic = FILE_BASIC_INFO {
        CreationTime: 120_000_000_000_000_000,
        ..Default::default()
    };
    assert_ne!(
        unsafe {
            SetFileInformationByHandle(
                seed.as_raw_handle().cast(),
                FileBasicInfo,
                ptr::from_ref(&basic).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        },
        0
    );
    let path = windows_fs::location(&seed).unwrap();
    drop(seed);
    if directory {
        std::fs::remove_dir(path).unwrap();
    } else {
        std::fs::remove_file(path).unwrap();
    }
}
fn assert_copy(actual: &tree::ExistingTreeEvidence, source: &tree::ExistingTreeEvidence) {
    assert_eq!(actual.entries.len(), source.entries.len());
    for (actual, source) in actual.entries.iter().zip(&source.entries) {
        assert_eq!(
            (
                actual.parent,
                &actual.name,
                &actual.streams,
                actual.compression
            ),
            (
                source.parent,
                &source.name,
                &source.streams,
                source.compression
            )
        );
        assert_ne!(actual.version.identity, source.version.identity);
        assert_eq!(actual.version.created, source.version.created);
        assert_eq!(actual.version.modified, source.version.modified);
        assert_eq!(actual.version.attributes, source.version.attributes);
    }
}

pub(in super::super) fn exercise_caller_dispatch_prefixes(
    caller: &Arc<CallerCopyContext>,
    protected_parent: &File,
    source: &File,
    preferences: &File,
    index: &File,
    mut as_caller: impl FnMut(&mut dyn FnMut()),
) {
    let authority = Authority::Caller(Arc::clone(caller));
    let source_path = windows_fs::location(source).unwrap();
    let name = source_path.file_name().unwrap();
    let pin = tree::PinnedTree::open(&authority, source, 16, &mut |_| Ok(())).unwrap();
    let original = pin.freeze_existing(|_| Ok(())).unwrap();
    let directory = pin.root().directory;
    let registry = caller.copy_registry(&[], false).unwrap().unwrap();
    let binding = registry.binding().unwrap();
    let observe_user = || {
        [preferences, index].map(|file| {
            (
                read(file).unwrap(),
                stable_identity_from_handle(file).unwrap(),
                security::owner_group_dacl_digest(file).unwrap(),
            )
        })
    };
    let mut before_user = None;
    as_caller(&mut || before_user = Some(observe_user()));
    let reference = windows_fs::create_private(
        protected_parent,
        OsStr::new(&format!("caller-prefix-reference-{directory}")),
        true,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )
    .unwrap();
    prime(&reference, name, directory);
    let trace = Trace::start();
    let receipt = caller
        .copy_entry_with_progress(
            &source_path,
            &windows_fs::location(&reference).unwrap(),
            ConflictAction::Overwrite,
            16,
            |_| Ok(()),
        )
        .unwrap()
        .unwrap();
    let trace = trace.finish();
    let mut points = Vec::new();
    for (sequence, step, faulted) in trace {
        assert!(!faulted);
        if !points.contains(&(sequence, step)) {
            points.push((sequence, step));
        }
    }
    assert!(points.len() >= 10);
    for step in [
        Step::LocalFlushed,
        Step::CentralFlushed,
        Step::PublicationRenamed,
        Step::PermissionsChanged,
    ] {
        assert!(
            points.iter().any(|(_, actual)| *actual == step),
            "missing reference boundary {step:?}"
        );
    }
    // NTFS file-name tunneling needs a creation-time repair for the file
    // reference. Directory publication does not necessarily take that path.
    // Both flows still verify the final creation time in assert_copy below.
    if !directory {
        assert!(
            points
                .iter()
                .any(|(_, step)| *step == Step::CreationRestored),
            "missing file creation-time repair boundary"
        );
    }
    assert_copy(&payload(&authority, &reference, name).unwrap(), &original);
    crate::DefaultPlatform
        .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
        .unwrap();
    let mut cases = Vec::new();
    for (case, &(sequence, step)) in points.iter().enumerate() {
        let parent = windows_fs::create_private(
            protected_parent,
            OsStr::new(&format!("caller-prefix-{directory}-{case}")),
            true,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
        .unwrap();
        prime(&parent, name, directory);
        let before = windows_fs::directory_children(registry.root()).unwrap();
        let trace = Trace::start();
        FAULT.with(|slot| slot.set(Some((sequence, step, false))));
        let result = caller.copy_entry_with_progress(
            &source_path,
            &windows_fs::location(&parent).unwrap(),
            ConflictAction::Overwrite,
            16,
            |_| Ok(()),
        );
        let observed = trace.finish();
        assert!(
            observed.contains(&(sequence, step, true)),
            "fault did not fire {directory} {sequence} {step:?}"
        );
        assert!(
            result.is_err(),
            "fault returned success {directory} {sequence} {step:?}"
        );
        let new = windows_fs::directory_children(registry.root())
            .unwrap()
            .into_iter()
            .filter(|name| !before.contains(name))
            .collect::<Vec<_>>();
        let ids = new
            .iter()
            .filter_map(|name| {
                name.to_str()
                    .and_then(|name| name.strip_suffix(".copy-journal"))
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert!(ids.len() <= 1);
        cases.push((parent, ids, sequence, step));
        pin.validate_existing(&original, &mut |_| Ok(())).unwrap();
    }
    // Discovery must not reconcile or repair either journal peer or its payload.
    let before = cases
        .iter()
        .map(|(parent, _, _, _)| evidence(&authority, parent))
        .collect::<Vec<_>>();
    let catalog_before = evidence(&authority, registry.root());
    let fresh = caller.copy_registry(&[], false).unwrap().unwrap();
    let rows = fresh.retained_with_progress(|_| Ok(())).unwrap();
    assert_eq!(evidence(&authority, registry.root()), catalog_before);
    for ((parent, _, _, _), expected) in cases.iter().zip(&before) {
        assert_eq!(evidence(&authority, parent), *expected);
    }
    let (mut refused, mut ended, mut resumed, mut undone, mut unregistered) = (0, 0, 0, 0, 0);
    for (parent, ids, sequence, step) in &cases {
        let context = format!("directory={directory} sequence={sequence} step={step:?}");
        let Some(id) = ids.first() else {
            assert!(
                payload(&authority, parent, name).is_none(),
                "unregistered publication: {context}"
            );
            unregistered += 1;
            continue;
        };
        let row = rows
            .iter()
            .find(|row| row.transaction_id.as_ref() == Some(id))
            .unwrap();
        match &row.evidence {
            WindowsCopyRecoveryEvidence::NeedsReview { .. } => {
                let parent_before = evidence(&authority, parent);
                let central = OsString::from(format!("{id}.copy-journal"));
                let central_before = read(
                    &namespace::open_source_with(
                        &authority,
                        registry.root(),
                        &central,
                        Some(false),
                    )
                    .unwrap(),
                )
                .unwrap();
                assert!(
                    fresh.resume_plan_with_progress(id, |_| Ok(())).is_err(),
                    "{context}"
                );
                assert!(
                    fresh.undo_plan_with_progress(id, |_| Ok(())).is_err(),
                    "{context}"
                );
                assert!(
                    fresh.abort_plan_with_progress(id, |_| Ok(())).is_err(),
                    "{context}"
                );
                assert!(
                    fresh
                        .name_release_plan_with_progress(id, |_| Ok(()))
                        .is_err(),
                    "{context}"
                );
                assert_eq!(evidence(&authority, parent), parent_before);
                assert_eq!(
                    read(
                        &namespace::open_source_with(
                            &authority,
                            registry.root(),
                            &central,
                            Some(false)
                        )
                        .unwrap()
                    )
                    .unwrap(),
                    central_before
                );
                refused += 1;
            }
            WindowsCopyRecoveryEvidence::UnverifiedStage => {
                if row.phase != Some(WindowsCopyPhase::Aborted) {
                    let candidate = row.retained.as_ref().unwrap().file_name().unwrap();
                    let before = payload(&authority, parent, candidate);
                    let mut plan = fresh.abort_plan_with_progress(id, |_| Ok(())).unwrap();
                    plan.end_attempt(|_| Ok(())).unwrap();
                    assert!(plan.complete());
                    assert_eq!(payload(&authority, parent, candidate), before);
                }
                assert!(payload(&authority, parent, name).is_none(), "{context}");
                ended += 1;
            }
            WindowsCopyRecoveryEvidence::VerifiedTree => {
                if row.phase != Some(WindowsCopyPhase::Completed) {
                    let mut plan = fresh
                        .resume_plan_with_progress(id, |_| Ok(()))
                        .unwrap_or_else(|error| panic!("{context}: {error}"));
                    plan.resume(|_| Ok(())).unwrap();
                    assert!(plan.complete());
                    resumed += 1;
                }
                assert_copy(&payload(&authority, parent, name).unwrap(), &original);
                let mut plan = fresh.undo_plan_with_progress(id, |_| Ok(())).unwrap();
                plan.undo_next_with_progress(|_| Ok(())).unwrap();
                assert_eq!(plan.remaining(), 0);
                assert!(payload(&authority, parent, name).is_none(), "{context}");
                undone += 1;
            }
            WindowsCopyRecoveryEvidence::PendingNameRelease => {
                let mut plan = fresh
                    .name_release_plan_with_progress(id, |_| Ok(()))
                    .unwrap();
                plan.finish(|_| Ok(())).unwrap();
                assert!(plan.complete());
                ended += 1;
            }
            other => panic!("Unexpected creation-prefix state {context}: {other:?}"),
        }
        pin.validate_existing(&original, &mut |_| Ok(())).unwrap();
    }
    assert!(refused > 0 && resumed > 0 && undone > 0);
    assert_eq!(refused + ended + undone + unregistered, points.len());
    assert_eq!(registry.binding().unwrap(), binding);
    let mut after_user = None;
    as_caller(&mut || after_user = Some(observe_user()));
    assert_eq!(after_user, before_user);
    caller.validate().unwrap();
    println!(
        "caller public dispatcher journal-prefix matrix directory={directory}: boundaries={} refused={refused} ended={ended} resumed={resumed} undone={undone} unregistered={unregistered}; actual public creation, readonly discovery, source and user data preserved; no process-restart/UAC acceptance",
        points.len()
    );
}
