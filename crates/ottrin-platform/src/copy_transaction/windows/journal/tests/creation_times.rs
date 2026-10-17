use super::*;
use crate::copy_transaction::windows::version;
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_BASIC_INFO, FileBasicInfo, SetFileInformationByHandle,
};

const COPIED: i64 = 125_911_584_000_000_000;
const ORIGINAL: i64 = 129_067_776_000_000_000;
const GHOST: i64 = 120_000_000_000_000_000;

fn stamp(path: &Path, created: i64, changed: i64) {
    let file = owner(path);
    let info = FILE_BASIC_INFO {
        CreationTime: created,
        ChangeTime: changed,
        ..Default::default()
    };
    assert_ne!(
        unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle().cast(),
                FileBasicInfo,
                ptr::from_ref(&info).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        },
        0
    );
}

fn prime(path: &Path) {
    assert_eq!(
        std::fs::symlink_metadata(path).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    std::fs::write(path, b"owned tunnel-cache fixture").unwrap();
    stamp(path, GHOST, 0);
    std::fs::remove_file(path).unwrap();
}

fn prepared(root: &Path, overwrite: bool, readonly: bool) -> (OsString, ReadonlyCleanup) {
    fixture(root);
    let input = root.join("source/nested/file");
    stamp(&input, COPIED, 0);
    if readonly {
        let mut permissions = std::fs::metadata(&input).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&input, permissions).unwrap();
    }
    let target = root.join("destination/copy");
    std::fs::write(&target, b"original").unwrap();
    stamp(&target, ORIGINAL, 0);
    if !overwrite {
        std::fs::remove_file(&target).unwrap();
    }
    let parent = owner(&root.join("destination"));
    let central = owner(&root.join("central"));
    let admission = if overwrite {
        DestinationAdmission::overwrite(&parent, OsStr::new("copy"), 100, |_| Ok(())).unwrap()
    } else {
        DestinationAdmission::creation(&parent, OsStr::new("copy")).unwrap()
    };
    let input = source(&input);
    let mut staged = DurableStage::create(
        &input,
        &parent,
        &central,
        &admission,
        OsStr::new("file"),
        100,
        |_| Ok(()),
    )
    .unwrap();
    drop(staged.prepare_destination_security(|| Ok(())).unwrap());
    let name = staged.stage.name.clone();
    let candidate = staged.stage.candidate_name.clone();
    drop((staged, input));
    if overwrite {
        let mut journal = original_journal(root, &name);
        journal.prepare_original(100, |_| Ok(())).unwrap();
        prime(
            &root
                .join("destination")
                .join(journal.header.original_name()),
        );
    }
    remove_undo_fixture_source(root);
    let cleanup = ReadonlyCleanup(vec![
        root.join("destination/copy"),
        root.join("destination").join(candidate),
    ]);
    (name, cleanup)
}

fn forward(root: &Path, name: &OsStr, overwrite: bool) -> Result<(), PlatformError> {
    if overwrite {
        original_journal(root, name).move_original(None, OriginalMove::Retain, |_| Ok(()))?;
    }
    PermissionStage::publish(
        &owner(&root.join("destination")),
        &owner(&root.join("central")),
        name,
        |_| Ok(()),
    )?
    .apply(|_| Ok(()))
}

fn inverse(root: &Path, name: &OsStr) -> Result<(), PlatformError> {
    undo_completed_copy(
        &owner(&root.join("destination")),
        &owner(&root.join("central")),
        name,
        |_| Ok(()),
    )
}

fn journal_path(root: &Path, name: &OsStr) -> std::path::PathBuf {
    root.join("destination").join(name).join(LOCAL)
}

fn observed_versions(root: &Path, header: &Header) -> Vec<Option<(EntryVersion, Vec<u8>)>> {
    [
        OsString::from_wide(&header.target),
        header.candidate_name(),
        header.original_name(),
    ]
    .into_iter()
    .map(|name| {
        let path = root.join("destination").join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => Some((
                version(&source(&path)).unwrap(),
                std::fs::read(path).unwrap(),
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("Owned observation failed: {error}"),
        }
    })
    .collect()
}

fn trace(overwrite: bool, readonly: bool) -> Vec<Record> {
    let root = tempfile::tempdir().unwrap();
    let (name, _cleanup) = prepared(root.path(), overwrite, readonly);
    forward(root.path(), &name, overwrite).unwrap();
    let state = decode(&std::fs::read(journal_path(root.path(), &name)).unwrap())
        .unwrap()
        .state;
    prime(
        &root
            .path()
            .join("destination")
            .join(state.header.as_ref().unwrap().candidate_name()),
    );
    inverse(root.path(), &name).unwrap();
    journal_records(&std::fs::read(journal_path(root.path(), &name)).unwrap())
}

fn reference(overwrite: bool, readonly: bool) -> Vec<(u64, Step, bool)> {
    let records = trace(overwrite, readonly);
    let mut result = Vec::new();
    let mut kinds = Vec::new();
    for (index, record) in records.iter().enumerate() {
        match record {
            Record::RenameCreationIntent { kind, .. } => {
                kinds.push(*kind);
                let undo = matches!(kind, RenameKind::Withdraw | RenameKind::RestoreOriginal);
                result.extend([
                    (index as u64, Step::LocalFlushed, undo),
                    (index as u64, Step::CentralFlushed, undo),
                ]);
            }
            Record::RenameCreationApplied { kind, .. } => {
                let undo = matches!(kind, RenameKind::Withdraw | RenameKind::RestoreOriginal);
                result.extend([
                    (index as u64, Step::CreationRestored, undo),
                    (index as u64, Step::CreationFlushed, undo),
                    (index as u64, Step::LocalFlushed, undo),
                    (index as u64, Step::CentralFlushed, undo),
                ]);
            }
            _ => {}
        }
    }
    for kind in [RenameKind::Publish, RenameKind::Withdraw] {
        assert!(kinds.contains(&kind), "{kind:?}");
    }
    if overwrite {
        for kind in [RenameKind::RetainOriginal, RenameKind::RestoreOriginal] {
            assert!(kinds.contains(&kind), "{kind:?}");
        }
    }
    assert_eq!(kinds.len(), if overwrite { 4 } else { 2 });
    result
}

fn resume(root: &Path, name: &OsStr, undo: bool) {
    let registry = root.join("central");
    let state = decode(&std::fs::read(journal_path(root, name)).unwrap())
        .unwrap()
        .state;
    let id = &state.header.as_ref().unwrap().transaction;
    if undo {
        crate::restore_retained_copy_undo_plan_at(&registry, id)
            .unwrap()
            .undo_next()
            .unwrap();
    } else {
        crate::restore_retained_copy_resume_plan_at(&registry, id)
            .unwrap()
            .resume(|_| Ok(()))
            .unwrap();
    }
}

fn assert_outcome(root: &Path, name: &OsStr, overwrite: bool, undo: bool) {
    let state = decode(&std::fs::read(journal_path(root, name)).unwrap())
        .unwrap()
        .state;
    let header = state.header.as_ref().unwrap();
    if undo {
        assert!(state.undo_complete());
        assert_eq!(
            std::fs::read(root.join("destination").join(header.candidate_name())).unwrap(),
            b"original data"
        );
        if overwrite {
            assert_eq!(
                std::fs::read(root.join("destination/copy")).unwrap(),
                b"original"
            );
            assert_eq!(
                version(&source(&root.join("destination/copy")))
                    .unwrap()
                    .created,
                ORIGINAL
            );
        } else {
            assert!(!root.join("destination/copy").exists());
        }
    } else {
        assert!(state.completion_verified);
        assert_eq!(
            std::fs::read(root.join("destination/copy")).unwrap(),
            b"original data"
        );
        assert_eq!(
            version(&source(&root.join("destination/copy")))
                .unwrap()
                .created,
            COPIED
        );
        if overwrite {
            assert_eq!(
                version(&source(
                    &root.join("destination").join(header.original_name())
                ))
                .unwrap()
                .created,
                ORIGINAL
            );
        }
    }
    assert!(!root.join("source").exists());
}

struct ResetFault;
impl Drop for ResetFault {
    fn drop(&mut self) {
        FAULT.with(|slot| slot.set(None));
    }
}

#[test]
#[ignore = "owned subprocess for creation-time interruption tests"]
fn windows_native_copy_creation_exit_child() {
    let root = std::path::PathBuf::from(
        std::env::var_os("OTTRIN_CREATION_CHILD").expect("Owned root required"),
    );
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    let overwrite = std::env::var("OTTRIN_CREATION_OVERWRITE").unwrap() == "1";
    let readonly = std::env::var("OTTRIN_CREATION_READONLY").unwrap() == "1";
    let undo = std::env::var("OTTRIN_CREATION_UNDO").unwrap() == "1";
    let sequence = std::env::var("OTTRIN_CREATION_SEQUENCE")
        .unwrap()
        .parse()
        .unwrap();
    let step = match std::env::var("OTTRIN_CREATION_STEP").unwrap().as_str() {
        "local" => Step::LocalFlushed,
        "central" => Step::CentralFlushed,
        "restored" => Step::CreationRestored,
        "flushed" => Step::CreationFlushed,
        _ => panic!("Unknown owned step"),
    };
    let (name, _cleanup) = prepared(&root, overwrite, readonly);
    if undo {
        forward(&root, &name, overwrite).unwrap();
        let state = decode(&std::fs::read(journal_path(&root, &name)).unwrap())
            .unwrap()
            .state;
        prime(
            &root
                .join("destination")
                .join(state.header.as_ref().unwrap().candidate_name()),
        );
    }
    FAULT.with(|slot| slot.set(Some((sequence, step, true))));
    let _reset = ResetFault;
    if undo {
        inverse(&root, &name).unwrap();
    } else {
        forward(&root, &name, overwrite).unwrap();
    }
    panic!("Owned exit boundary not reached");
}

#[test]
fn windows_native_copy_creation_fault_and_exit_prefixes_recover_or_preserve_divergence() {
    for overwrite in [false, true] {
        for readonly in [false, true] {
            for (sequence, step, undo) in reference(overwrite, readonly) {
                for exit in [false, true] {
                    let root = tempfile::tempdir().unwrap();
                    let (name, cleanup) = if exit {
                        let label = match step {
                            Step::LocalFlushed => "local",
                            Step::CentralFlushed => "central",
                            Step::CreationRestored => "restored",
                            Step::CreationFlushed => "flushed",
                            _ => unreachable!(),
                        };
                        let status=std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact","copy_transaction::windows::journal::tests::creation_times::windows_native_copy_creation_exit_child","--ignored","--nocapture"])
                    .env("OTTRIN_CREATION_CHILD",root.path()).env("OTTRIN_CREATION_SEQUENCE",sequence.to_string()).env("OTTRIN_CREATION_STEP",label)
                    .env("OTTRIN_CREATION_OVERWRITE",if overwrite{"1"}else{"0"}).env("OTTRIN_CREATION_READONLY",if readonly{"1"}else{"0"})
                    .env("OTTRIN_CREATION_UNDO",if undo{"1"}else{"0"}).status().unwrap();
                        assert_eq!(
                            status.code(),
                            Some(86),
                            "{overwrite} {readonly} {sequence} {step:?} {undo}"
                        );
                        let name = stage_name(&root.path().join("destination"));
                        let state =
                            decode(&std::fs::read(journal_path(root.path(), &name)).unwrap())
                                .unwrap()
                                .state;
                        let cleanup = ReadonlyCleanup(vec![
                            root.path().join("destination/copy"),
                            root.path()
                                .join("destination")
                                .join(state.header.as_ref().unwrap().candidate_name()),
                        ]);
                        (name, cleanup)
                    } else {
                        let (name, cleanup) = prepared(root.path(), overwrite, readonly);
                        if undo {
                            forward(root.path(), &name, overwrite).unwrap();
                            let state =
                                decode(&std::fs::read(journal_path(root.path(), &name)).unwrap())
                                    .unwrap()
                                    .state;
                            prime(
                                &root
                                    .path()
                                    .join("destination")
                                    .join(state.header.as_ref().unwrap().candidate_name()),
                            );
                        }
                        FAULT.with(|slot| slot.set(Some((sequence, step, false))));
                        let reset = ResetFault;
                        let result = if undo {
                            inverse(root.path(), &name)
                        } else {
                            forward(root.path(), &name, overwrite)
                        };
                        drop(reset);
                        assert!(result.is_err(), "{sequence} {step:?}");
                        (name, cleanup)
                    };
                    let path = journal_path(root.path(), &name);
                    let before = std::fs::read(&path).unwrap();
                    if step == Step::LocalFlushed {
                        let registry = root.path().join("central");
                        let state = decode(&before).unwrap().state;
                        let id = &state.header.as_ref().unwrap().transaction;
                        let observed =
                            observed_versions(root.path(), state.header.as_ref().unwrap());
                        if undo {
                            assert!(
                                crate::restore_retained_copy_undo_plan_at(&registry, id).is_err()
                            );
                        } else {
                            assert!(
                                crate::restore_retained_copy_resume_plan_at(&registry, id).is_err()
                            );
                        }
                        assert_eq!(std::fs::read(&path).unwrap(), before);
                        assert_eq!(
                            observed_versions(root.path(), state.header.as_ref().unwrap()),
                            observed,
                            "Divergent mirrors must preserve data and metadata"
                        );
                    } else {
                        resume(root.path(), &name, undo);
                        assert_outcome(root.path(), &name, overwrite, undo);
                        let complete = std::fs::read(&path).unwrap();
                        if undo {
                            inverse(root.path(), &name).unwrap();
                        } else {
                            PermissionStage::publish(
                                &owner(&root.path().join("destination")),
                                &owner(&root.path().join("central")),
                                &name,
                                |_| Ok(()),
                            )
                            .unwrap()
                            .apply(|_| Ok(()))
                            .unwrap();
                        }
                        assert_eq!(
                            std::fs::read(&path).unwrap(),
                            complete,
                            "Completed replay must add no records"
                        );
                    }
                    drop(cleanup);
                }
            }
        }
    }
}

#[test]
fn windows_native_copy_creation_codec_rejects_unowned_or_incomplete_repairs() {
    let records = trace(true, false);
    let reject = |prefix: &[Record], record: Record| {
        let mut invalid = prefix.to_vec();
        invalid.push(record);
        assert!(decode(&encode_records(&invalid)).is_err());
    };
    let mut checked = 0;
    for (index, record) in records.iter().enumerate() {
        let Record::RenameCreationIntent { kind, before } = record else {
            continue;
        };
        checked += 1;
        let prefix = &records[..index];
        let state = decode(&encode_records(prefix)).unwrap().state;
        let expected = state.rename_expected(*kind).unwrap();
        for case in 0..3 {
            let mut invalid = before.clone();
            match case {
                0 => invalid.identity = state.header.as_ref().unwrap().parent,
                1 => invalid.modified += 1,
                2 => invalid.created = expected.created,
                _ => unreachable!(),
            }
            reject(
                prefix,
                Record::RenameCreationIntent {
                    kind: *kind,
                    before: invalid,
                },
            );
        }
        let Record::RenameCreationApplied {
            kind: applied_kind,
            after,
        } = &records[index + 1]
        else {
            panic!("Non-readonly reference must pair creation records directly")
        };
        assert_eq!(kind, applied_kind);
        reject(prefix, records[index + 1].clone());
        let pending = &records[..=index];
        reject(pending, record.clone());
        for case in 0..3 {
            let mut invalid = after.clone();
            match case {
                0 => invalid.identity = state.header.as_ref().unwrap().parent,
                1 => invalid.modified += 1,
                2 => invalid.created = before.created,
                _ => unreachable!(),
            }
            reject(
                pending,
                Record::RenameCreationApplied {
                    kind: *kind,
                    after: invalid,
                },
            );
        }
        let wrong_kind = if *kind == RenameKind::Publish {
            RenameKind::Withdraw
        } else {
            RenameKind::Publish
        };
        reject(
            pending,
            Record::RenameCreationApplied {
                kind: wrong_kind,
                after: after.clone(),
            },
        );
        let acknowledgement = match kind {
            RenameKind::Publish => Record::Published,
            RenameKind::Withdraw => Record::Undone {
                root_version: after.clone(),
            },
            RenameKind::RetainOriginal => Record::OriginalRetained {
                root_version: after.clone(),
            },
            RenameKind::RestoreOriginal => Record::OriginalRestored {
                root_version: after.clone(),
            },
        };
        reject(pending, acknowledgement);
        let state = decode(&encode_records(pending)).unwrap().state;
        let mut drift = (**before).clone();
        drift.created = GHOST + 1;
        assert_ne!(drift.created, expected.created);
        assert!(state.validate_creation_epoch(*kind, &drift).is_err());
        assert!(!state.admitted_rename_matches(*kind, before, &drift));
        let applied = &records[..=index + 1];
        reject(applied, records[index + 1].clone());
        let state = decode(&encode_records(applied)).unwrap().state;
        let mut drift = (**after).clone();
        drift.changed += 1;
        assert!(state.validate_creation_epoch(*kind, &drift).is_err());
        assert!(!state.admitted_rename_matches(*kind, after, &drift));
    }
    assert_eq!(checked, 4);
}

#[test]
fn windows_native_copy_creation_admission_refuses_external_timestamp_drift_without_mutation() {
    let reference = trace(true, false);
    let boundaries: Vec<_> = reference
        .iter()
        .enumerate()
        .filter_map(|(index, record)| {
            if let Record::RenameCreationIntent { kind, .. } = record {
                Some((index as u64, *kind))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(boundaries.len(), 4);
    for (sequence, kind) in boundaries {
        for recorded in [false, true] {
            for creation in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let (name, _cleanup) = prepared(root.path(), true, false);
                let undo = matches!(kind, RenameKind::Withdraw | RenameKind::RestoreOriginal);
                if undo {
                    forward(root.path(), &name, true).unwrap();
                    let journal = original_journal(root.path(), &name);
                    prime(
                        &root
                            .path()
                            .join("destination")
                            .join(journal.header.candidate_name()),
                    );
                }
                let step = if recorded {
                    Step::CentralFlushed
                } else {
                    match kind {
                        RenameKind::Publish => Step::PublicationRenamed,
                        RenameKind::Withdraw => Step::UndoRenamed,
                        RenameKind::RetainOriginal | RenameKind::RestoreOriginal => {
                            Step::OriginalRenamed
                        }
                    }
                };
                FAULT.with(|slot| slot.set(Some((sequence, step, false))));
                let reset = ResetFault;
                let result = if undo {
                    inverse(root.path(), &name)
                } else {
                    forward(root.path(), &name, true)
                };
                drop(reset);
                assert!(result.is_err(), "{kind:?} {recorded}");
                let local = journal_path(root.path(), &name);
                let bytes = std::fs::read(&local).unwrap();
                let state = decode(&bytes).unwrap().state;
                let header = state.header.as_ref().unwrap();
                assert_eq!(state.rename_creation.is_some(), recorded);
                let registry = root.path().join("central");
                let central = registry.join(header.central_name());
                let mirror = std::fs::read(&central).unwrap();
                let mut forward_plan = if !undo {
                    Some(
                        crate::restore_retained_copy_resume_plan_at(&registry, &header.transaction)
                            .unwrap(),
                    )
                } else {
                    None
                };
                let mut undo_plan = if undo {
                    Some(
                        crate::restore_retained_copy_undo_plan_at(&registry, &header.transaction)
                            .unwrap(),
                    )
                } else {
                    None
                };
                let target = root
                    .path()
                    .join("destination")
                    .join(kind.destination(header));
                let before = version(&source(&target)).unwrap();
                stamp(
                    &target,
                    before.created + i64::from(creation),
                    before.changed + i64::from(!creation),
                );
                let changed = version(&source(&target)).unwrap();
                assert_ne!(changed, before);
                let observed = observed_versions(root.path(), header);
                let result = if let Some(plan) = &mut forward_plan {
                    plan.resume(|_| Ok(()))
                } else {
                    undo_plan.as_mut().unwrap().undo_next()
                };
                assert!(
                    result.is_err(),
                    "{kind:?} recorded={recorded} creation={creation}"
                );
                assert_eq!(std::fs::read(&local).unwrap(), bytes);
                assert_eq!(std::fs::read(&central).unwrap(), mirror);
                assert_eq!(
                    observed_versions(root.path(), header),
                    observed,
                    "External timestamp drift must never grant a repair write"
                );
            }
        }
    }
}
