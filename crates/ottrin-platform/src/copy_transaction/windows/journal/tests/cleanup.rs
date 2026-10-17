use super::super::cleanup::{HeldCleanup, Payload};
use super::*;
use std::collections::BTreeMap;

fn terminal_fixture(
    root: &Path,
    overwrite: bool,
    undone: bool,
) -> (OsString, Option<ReadonlyCleanup>) {
    if overwrite {
        let name = original_move_fixture(root, true, false);
        original_journal(root, &name)
            .move_original(None, OriginalMove::Retain, |_| Ok(()))
            .unwrap();
        overwrite_publish(root, &name).unwrap();
        if undone {
            overwrite_undo(root, &name).unwrap();
        }
        (name, None)
    } else {
        let (name, _, cleanup) = completed_undo_fixture(root, true, false);
        if undone {
            undo_completed_copy(
                &owner(&root.join("destination")),
                &owner(&root.join("central")),
                &name,
                |_| Ok(()),
            )
            .unwrap();
        }
        (name, Some(cleanup))
    }
}

#[test]
fn windows_native_copy_cleanup_observation_uses_only_retained_payloads() {
    for overwrite in [false, true] {
        for undone in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (name, _cleanup) = terminal_fixture(root.path(), overwrite, undone);
            let public = root.path().join("destination/copy");
            // The publication/restored original is user data. Cleanup must
            // remain possible after ordinary edits or reuse at that name.
            if public.is_dir() {
                std::fs::remove_dir_all(&public).unwrap();
            } else if public.exists() {
                std::fs::remove_file(&public).unwrap();
            }
            std::fs::write(&public, b"new unrelated public data").unwrap();
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let local = root.path().join("destination").join(&name).join(LOCAL);
            let bytes = std::fs::read(&local).unwrap();
            let held = HeldCleanup::open(&parent, &central, &name, |_| Ok(())).unwrap();
            match &held.payload {
                Payload::None => assert!(!overwrite && !undone),
                Payload::Copy(_) => assert!(undone),
                Payload::Original(_) => assert!(overwrite && !undone),
            }
            held.revalidate(|_| Ok(())).unwrap();
            assert_eq!(std::fs::read(&local).unwrap(), bytes);
            assert_eq!(
                std::fs::read(&public).unwrap(),
                b"new unrelated public data"
            );
        }
    }
}

#[test]
fn windows_native_copy_cleanup_observation_refuses_unfinished_work_and_cancellation() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let input = source(&root.path().join("source"));
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let staged = stage(&input, &parent, &central).unwrap();
    let name = staged.stage.name.clone();
    drop(staged);
    let local = root.path().join("destination").join(&name).join(LOCAL);
    let bytes = std::fs::read(&local).unwrap();
    assert!(HeldCleanup::open(&parent, &central, &name, |_| Ok(())).is_err());
    assert!(
        HeldCleanup::open(&parent, &central, &name, |_| Err(PlatformError::Io(
            "owned cancellation".into()
        )))
        .is_err()
    );
    assert_eq!(std::fs::read(&local).unwrap(), bytes);
}

#[test]
fn windows_native_copy_cleanup_observation_preserves_changed_payloads_and_unexpected_stage_data() {
    for overwrite in [false, true] {
        for extra_stage_entry in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (name, _cleanup) = terminal_fixture(root.path(), overwrite, !overwrite);
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let snapshot = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).unwrap();
            let header = snapshot.state.header.as_ref().unwrap().clone();
            drop(snapshot);
            let payload = root.path().join("destination").join(if overwrite {
                header.original_name()
            } else {
                header.candidate_name()
            });
            let protected = if extra_stage_entry {
                root.path()
                    .join("destination")
                    .join(&name)
                    .join("unrecognized")
            } else {
                payload.join(if overwrite { "old" } else { "nested/file" })
            };
            std::fs::write(&protected, b"unrecorded data must survive").unwrap();
            let local = root.path().join("destination").join(&name).join(LOCAL);
            let mirror = root.path().join("central").join(header.central_name());
            let before = (
                std::fs::read(&local).unwrap(),
                std::fs::read(&mirror).unwrap(),
            );
            assert!(HeldCleanup::open(&parent, &central, &name, |_| Ok(())).is_err());
            assert_eq!(
                (
                    std::fs::read(&local).unwrap(),
                    std::fs::read(&mirror).unwrap()
                ),
                before
            );
            assert_eq!(
                std::fs::read(protected).unwrap(),
                b"unrecorded data must survive"
            );
        }
    }
}

#[test]
fn windows_native_copy_cleanup_observation_cancellation_during_validation_is_retryable() {
    let root = tempfile::tempdir().unwrap();
    let (name, _cleanup) = terminal_fixture(root.path(), true, false);
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let local = root.path().join("destination").join(&name).join(LOCAL);
    let bytes = std::fs::read(&local).unwrap();
    let mut calls = 0;
    assert!(
        HeldCleanup::open(&parent, &central, &name, |_| {
            calls += 1;
            if calls == 3 {
                Err(PlatformError::Io("owned validation cancellation".into()))
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    assert_eq!(calls, 3);
    assert_eq!(std::fs::read(&local).unwrap(), bytes);
    HeldCleanup::open(&parent, &central, &name, |_| Ok(()))
        .unwrap()
        .revalidate(|_| Ok(()))
        .unwrap();
    assert_eq!(std::fs::read(&local).unwrap(), bytes);
}

#[test]
fn windows_native_copy_cleanup_protocol_binds_terminal_evidence_and_ordered_progress() {
    use super::super::cleanup_protocol::{CleanupRecord, Owner};
    for overwrite in [false, true] {
        for undone in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (name, _cleanup) = terminal_fixture(root.path(), overwrite, undone);
            let local = root.path().join("destination").join(&name).join(LOCAL);
            let before = std::fs::read(&local).unwrap();
            let mut records = journal_records(&before);
            let mut state = decode(&before).unwrap().state;
            let manifest = state.cleanup_manifest().unwrap();
            assert_eq!(
                manifest.owner,
                if undone {
                    Owner::Copy
                } else if overwrite {
                    Owner::Original
                } else {
                    Owner::None
                }
            );
            let begin = Record::Cleanup {
                entry: CleanupRecord::Begin {
                    owner: manifest.owner,
                    count: manifest.entries.len(),
                    digest: manifest.digest,
                },
            };
            for case in 0..3 {
                let mut invalid = begin.clone();
                let Record::Cleanup {
                    entry:
                        CleanupRecord::Begin {
                            owner,
                            count,
                            digest,
                        },
                } = &mut invalid
                else {
                    unreachable!()
                };
                match case {
                    0 => {
                        *owner = if *owner == Owner::None {
                            Owner::Copy
                        } else {
                            Owner::None
                        }
                    }
                    1 => *count += 1,
                    2 => digest[0] ^= 1,
                    _ => unreachable!(),
                }
                assert!(state.check(&invalid).is_err());
            }
            let mut writer = original_journal(root.path(), &name);
            let reserved = writer.cleanup_tail_size().unwrap();
            assert!(reserved > 0);
            {
                let _budget = LogBudget::new(writer.length + reserved - 1);
                assert!(writer.reserve_cleanup_tail().is_err());
                assert_eq!(std::fs::read(&local).unwrap(), before);
            }
            writer.reserve_cleanup_tail().unwrap();
            writer.persist(begin.clone()).unwrap();
            drop(writer);
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            assert!(read_undo_snapshot(&parent, &central, &name, |_| Ok(())).is_err());
            assert!(RetainedStage::open(&parent, &central, &name, |_| Ok(())).is_err());
            let statuses = discover_at(&root.path().join("central")).unwrap();
            assert_eq!(statuses[0].phase, Some(WindowsCopyPhase::CleaningUp));
            assert_eq!(
                statuses[0].evidence,
                WindowsCopyRecoveryEvidence::VerifiedCleanup
            );
            state.apply(begin.clone()).unwrap();
            records.push(begin);
            assert!(state.check(records.last().unwrap()).is_err());
            assert!(state.check(&Record::UndoIntent).is_err());
            assert!(state.check(&Record::PublicationIntent).is_err());
            for &index in &manifest.order {
                let done = Record::Cleanup {
                    entry: CleanupRecord::PayloadRemoved,
                };
                assert!(state.check(&done).is_err());
                assert!(
                    state
                        .check(&Record::Cleanup {
                            entry: CleanupRecord::EntryIntent { index: usize::MAX }
                        })
                        .is_err()
                );
                let parent_after = manifest.entries[index].parent.map(|parent| {
                    let mut version = state.cleanup.as_ref().unwrap().expected(parent).clone();
                    version.modified += 1;
                    version.changed += 1;
                    Box::new(version)
                });
                let removed = Record::Cleanup {
                    entry: CleanupRecord::EntryRemoved {
                        index,
                        parent_after,
                        alias_after: None,
                    },
                };
                assert!(state.check(&removed).is_err());
                assert_eq!(
                    state
                        .cleanup
                        .as_ref()
                        .unwrap()
                        .expected_links(index)
                        .unwrap(),
                    1
                );
                let intent = Record::Cleanup {
                    entry: CleanupRecord::EntryIntent { index },
                };
                state.apply(intent.clone()).unwrap();
                records.push(intent.clone());
                assert!(state.check(&intent).is_err());
                if let Record::Cleanup {
                    entry:
                        CleanupRecord::EntryRemoved {
                            parent_after: Some(version),
                            ..
                        },
                } = &removed
                {
                    let mut wrong = (**version).clone();
                    wrong.created += 1;
                    assert!(
                        state
                            .check(&Record::Cleanup {
                                entry: CleanupRecord::EntryRemoved {
                                    index,
                                    parent_after: Some(Box::new(wrong)),
                                    alias_after: None
                                }
                            })
                            .is_err()
                    );
                }
                state.apply(removed.clone()).unwrap();
                records.push(removed.clone());
                assert!(state.check(&removed).is_err());
                decode(&encode_records(&records)).unwrap();
            }
            let done = Record::Cleanup {
                entry: CleanupRecord::PayloadRemoved,
            };
            let metadata_intent = Record::Cleanup {
                entry: CleanupRecord::MetadataRemovalIntent,
            };
            assert!(state.check(&metadata_intent).is_err());
            state.apply(done.clone()).unwrap();
            records.push(done.clone());
            assert!(state.check(&done).is_err());
            assert!(state.cleanup.as_ref().unwrap().payload_removed);
            assert!(state.check(&Record::UndoIntent).is_err());
            state.apply(metadata_intent.clone()).unwrap();
            records.push(metadata_intent.clone());
            assert!(state.check(&metadata_intent).is_err());
            decode(&encode_records(&records)).unwrap();
            // Codec replay above grants no filesystem deletion authority: only
            // Begin was persisted. Payload bytes remain in their native slots.
            let persisted = decode(&std::fs::read(&local).unwrap()).unwrap().state;
            assert_eq!(persisted.cleanup.as_ref().unwrap().completed, 0);
            assert!(!persisted.cleanup.as_ref().unwrap().payload_removed);
        }
    }
}

fn hardlink_fixture(root: &Path, readonly: bool) -> (OsString, ReadonlyCleanup) {
    fixture(root);
    let public = root.join("destination/copy");
    std::fs::create_dir(&public).unwrap();
    std::fs::write(public.join("old"), b"shared original data").unwrap();
    std::fs::hard_link(public.join("old"), public.join("second")).unwrap();
    let outside = root.join("outside-original");
    std::fs::hard_link(public.join("old"), &outside).unwrap();
    if readonly {
        let mut permissions = std::fs::metadata(&outside).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&outside, permissions).unwrap();
    }
    let input = source(&root.join("source"));
    let parent = owner(&root.join("destination"));
    let central = owner(&root.join("central"));
    let admission =
        DestinationAdmission::overwrite(&parent, OsStr::new("copy"), 100, |_| Ok(())).unwrap();
    let mut staged = DurableStage::create(
        &input,
        &parent,
        &central,
        &admission,
        OsStr::new("source"),
        100,
        |_| Ok(()),
    )
    .unwrap();
    drop(staged.prepare_destination_security(|| Ok(())).unwrap());
    let name = staged.stage.name.clone();
    drop((staged, input));
    remove_undo_fixture_source(root);
    let mut journal = original_journal(root, &name);
    journal.prepare_original(100, |_| Ok(())).unwrap();
    journal
        .move_original(None, OriginalMove::Retain, |_| Ok(()))
        .unwrap();
    drop(journal);
    overwrite_publish(root, &name).unwrap();
    let original = original_journal(root, &name).header.original_name();
    let cleanup = if readonly {
        ReadonlyCleanup(vec![
            outside,
            root.join("destination").join(&original).join("old"),
            root.join("destination").join(original).join("second"),
        ])
    } else {
        ReadonlyCleanup(Vec::new())
    };
    (name, cleanup)
}

#[test]
fn windows_native_copy_cleanup_protocol_tracks_retained_hardlinks_without_claiming_outside_names() {
    use super::super::cleanup_protocol::CleanupRecord;
    let root = tempfile::tempdir().unwrap();
    let (name, _cleanup) = hardlink_fixture(root.path(), false);
    let outside = root.path().join("outside-original");
    let bytes = std::fs::read(root.path().join("destination").join(&name).join(LOCAL)).unwrap();
    let mut state = decode(&bytes).unwrap().state;
    let manifest = state.cleanup_manifest().unwrap();
    assert_eq!(manifest.entries.len(), 3);
    assert!(
        manifest
            .entries
            .iter()
            .all(|node| OsString::from_wide(&node.name) != OsStr::new("outside-original"))
    );
    let alias_id = state.original.as_ref().unwrap().evidence.entries[1]
        .version
        .identity;
    state
        .apply(Record::Cleanup {
            entry: CleanupRecord::Begin {
                owner: manifest.owner,
                count: manifest.entries.len(),
                digest: manifest.digest,
            },
        })
        .unwrap();
    let mut aliases_removed = 0;
    for &index in &manifest.order {
        let node = &manifest.entries[index];
        let is_alias = node.version.identity == alias_id;
        let progress = state.cleanup.as_ref().unwrap();
        if is_alias {
            assert_eq!(progress.expected_links(index).unwrap(), 3 - aliases_removed);
        }
        let parent_after = node.parent.map(|parent| {
            let mut version = progress.expected(parent).clone();
            version.modified += 1;
            version.changed += 1;
            Box::new(version)
        });
        let alias_after = if is_alias && aliases_removed == 0 {
            let mut version = progress.expected(index).clone();
            version.changed += 1;
            Some(Box::new(version))
        } else {
            None
        };
        state
            .apply(Record::Cleanup {
                entry: CleanupRecord::EntryIntent { index },
            })
            .unwrap();
        if let Some(alias_version) = &alias_after {
            assert!(
                state
                    .check(&Record::Cleanup {
                        entry: CleanupRecord::EntryRemoved {
                            index,
                            parent_after: parent_after.clone(),
                            alias_after: None
                        }
                    })
                    .is_err()
            );
            let mut unrelated = alias_version.as_ref().clone();
            unrelated.modified += 1;
            assert!(
                state
                    .check(&Record::Cleanup {
                        entry: CleanupRecord::EntryRemoved {
                            index,
                            parent_after: parent_after.clone(),
                            alias_after: Some(Box::new(unrelated))
                        }
                    })
                    .is_err()
            );
        }
        state
            .apply(Record::Cleanup {
                entry: CleanupRecord::EntryRemoved {
                    index,
                    parent_after,
                    alias_after,
                },
            })
            .unwrap();
        if is_alias {
            aliases_removed += 1;
        }
    }
    assert_eq!(aliases_removed, 2);
    state
        .apply(Record::Cleanup {
            entry: CleanupRecord::PayloadRemoved,
        })
        .unwrap();
    assert_eq!(std::fs::read(&outside).unwrap(), b"shared original data");
    assert_eq!(
        namespace::link_count(&source(&outside)).unwrap(),
        3,
        "Progress codec alone must not unlink any name"
    );
}

#[test]
fn windows_native_copy_cleanup_reclaims_only_retained_payloads_and_replays_without_sources() {
    for overwrite in [false, true] {
        for undone in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (name, _cleanup) = terminal_fixture(root.path(), overwrite, undone);
            if root.path().join("source").exists() {
                remove_undo_fixture_source(root.path());
            }
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let local = root.path().join("destination").join(&name).join(LOCAL);
            let state = decode(&std::fs::read(&local).unwrap()).unwrap().state;
            let header = state.header.as_ref().unwrap();
            let retained = if undone {
                Some(header.candidate_name())
            } else if overwrite {
                Some(header.original_name())
            } else {
                None
            };
            let public = root.path().join("destination/copy");
            if public.is_dir() {
                std::fs::remove_dir_all(&public).unwrap();
            } else if public.exists() {
                std::fs::remove_file(&public).unwrap();
            }
            std::fs::write(&public, b"user public data remains").unwrap();
            super::super::cleanup::reclaim_payload(&parent, &central, &name, |_| Ok(())).unwrap();
            if let Some(retained) = retained {
                assert_eq!(
                    std::fs::symlink_metadata(root.path().join("destination").join(retained))
                        .unwrap_err()
                        .kind(),
                    std::io::ErrorKind::NotFound
                );
            }
            assert_eq!(std::fs::read(&public).unwrap(), b"user public data remains");
            assert!(!root.path().join("source").exists());
            let complete = std::fs::read(&local).unwrap();
            assert!(
                decode(&complete)
                    .unwrap()
                    .state
                    .cleanup
                    .as_ref()
                    .unwrap()
                    .payload_removed
            );
            super::super::cleanup::reclaim_payload(&parent, &central, &name, |_| Ok(())).unwrap();
            assert_eq!(
                std::fs::read(&local).unwrap(),
                complete,
                "Payload cleanup replay must add no records"
            );
            assert_eq!(std::fs::read(&public).unwrap(), b"user public data remains");
        }
    }
}

#[test]
fn windows_native_copy_cleanup_reclaims_readonly_candidates_without_changing_public_data() {
    for nested in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (name, candidate, _cleanup) = completed_undo_fixture(root.path(), nested, true);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
        remove_undo_fixture_source(root.path());
        super::super::cleanup::reclaim_payload(&parent, &central, &name, |_| Ok(())).unwrap();
        assert_eq!(
            std::fs::symlink_metadata(root.path().join("destination").join(candidate))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
    }
}

#[test]
fn windows_native_copy_cleanup_handoff_drift_refuses_before_recording_intent() {
    let root = tempfile::tempdir().unwrap();
    let (name, _cleanup) = terminal_fixture(root.path(), false, true);
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let local = root.path().join("destination").join(&name).join(LOCAL);
    let before = std::fs::read(&local).unwrap();
    let state = decode(&before).unwrap().state;
    let target = root
        .path()
        .join("destination")
        .join(state.header.as_ref().unwrap().candidate_name())
        .join("nested/file");
    let changed = target.clone();
    CLEANUP_HANDOFF.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            std::fs::write(changed, b"external retained change").unwrap()
        }))
    });
    assert!(super::super::cleanup::reclaim_payload(&parent, &central, &name, |_| Ok(())).is_err());
    assert_eq!(std::fs::read(&local).unwrap(), before);
    assert_eq!(std::fs::read(target).unwrap(), b"external retained change");
    assert!(CLEANUP_HANDOFF.with(|slot| slot.borrow().is_none()));
}

#[test]
fn windows_native_copy_cleanup_unlinks_retained_hardlinks_and_preserves_the_outside_alias() {
    for readonly in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (name, _cleanup) = hardlink_fixture(root.path(), readonly);
        let outside = root.path().join("outside-original");
        let before = super::super::super::version(&source(&outside)).unwrap();
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let original = original_journal(root.path(), &name).header.original_name();
        super::super::cleanup::reclaim_payload(&parent, &central, &name, |_| Ok(())).unwrap();
        assert_eq!(std::fs::read(&outside).unwrap(), b"shared original data");
        let outside_handle = source(&outside);
        assert_eq!(namespace::link_count(&outside_handle).unwrap(), 1);
        let after = super::super::super::version(&outside_handle).unwrap();
        assert_eq!(after.identity, before.identity);
        assert_eq!(
            (after.created, after.modified, after.attributes),
            (before.created, before.modified, before.attributes)
        );
        assert_eq!(
            std::fs::symlink_metadata(root.path().join("destination").join(original))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
    }
}

fn cleanup_fault_fixture(root: &Path, case: usize) -> (OsString, Option<ReadonlyCleanup>) {
    let (name, cleanup) = match case {
        0 => terminal_fixture(root, false, false),
        1 => terminal_fixture(root, false, true),
        2 | 3 => {
            let (name, _, cleanup) = completed_undo_fixture(root, case == 3, true);
            undo_completed_copy(
                &owner(&root.join("destination")),
                &owner(&root.join("central")),
                &name,
                |_| Ok(()),
            )
            .unwrap();
            (name, Some(cleanup))
        }
        4 => terminal_fixture(root, true, false),
        5 => {
            let (name, cleanup) = hardlink_fixture(root, true);
            (name, Some(cleanup))
        }
        _ => panic!("unknown owned cleanup case"),
    };
    if root.join("source").exists() {
        remove_undo_fixture_source(root);
    }
    (name, cleanup)
}

fn cleanup_faults(root: &Path, name: &OsStr) -> Vec<(u64, Step)> {
    let journal = original_journal(root, name);
    let first = journal.sequence;
    let count = journal.state.cleanup_manifest().unwrap().entries.len();
    let mut faults = vec![(first, Step::CleanupReadersReleased)];
    for sequence in first..=first + 1 + 2 * count as u64 {
        faults.extend([
            (sequence, Step::LocalFlushed),
            (sequence, Step::CentralFlushed),
        ]);
    }
    for offset in 0..count as u64 {
        for step in [
            Step::CleanupDispositionSet,
            Step::CleanupHandleClosed,
            Step::CleanupFlushed,
        ] {
            faults.push((first + 2 + offset * 2, step));
        }
    }
    faults
}

fn namespace_contents(root: &Path) -> BTreeMap<std::path::PathBuf, Option<Vec<u8>>> {
    fn visit(
        root: &Path,
        path: &Path,
        entries: &mut BTreeMap<std::path::PathBuf, Option<Vec<u8>>>,
    ) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            assert!(
                !std::fs::symlink_metadata(&path)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            if path.is_dir() {
                entries.insert(path.strip_prefix(root).unwrap().to_owned(), None);
                visit(root, &path, entries);
            } else {
                entries.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    Some(std::fs::read(&path).unwrap()),
                );
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

fn recover_cleanup_fault(root: &Path, name: &OsStr, step: Step) {
    let local = root.join("destination").join(name).join(LOCAL);
    let bytes = std::fs::read(&local).unwrap();
    let state = decode(&bytes).unwrap().state;
    let header = state.header.as_ref().unwrap();
    let mirror = root.join("central").join(header.central_name());
    let before = namespace_contents(root);
    let result = super::super::cleanup::reclaim_payload(
        &owner(&root.join("destination")),
        &owner(&root.join("central")),
        name,
        |_| Ok(()),
    );
    if step == Step::LocalFlushed {
        assert_ne!(std::fs::read(&mirror).unwrap(), bytes);
        assert!(result.is_err());
        assert_eq!(
            namespace_contents(root),
            before,
            "divergent journals must not be repaired"
        );
    } else {
        result.unwrap();
        let completed = decode(&std::fs::read(&local).unwrap()).unwrap().state;
        assert!(completed.cleanup.unwrap().payload_removed);
        assert_eq!(
            std::fs::read(&local).unwrap(),
            std::fs::read(&mirror).unwrap()
        );
        // Public names and outside aliases are never cleanup input.
        for (path, contents) in before {
            if path.starts_with("destination/copy") || path == Path::new("outside-original") {
                if let Some(contents) = contents {
                    assert_eq!(std::fs::read(root.join(path)).unwrap(), contents);
                } else {
                    assert!(root.join(path).is_dir());
                }
            }
        }
    }
    assert!(!root.join("source").exists());
}

#[test]
fn windows_native_copy_cleanup_injected_failures_resume_coherent_prefixes_only() {
    for case in 0..6 {
        let template = tempfile::tempdir().unwrap();
        let (name, _cleanup) = cleanup_fault_fixture(template.path(), case);
        for index in 0..cleanup_faults(template.path(), &name).len() {
            let root = tempfile::tempdir().unwrap();
            let (name, _cleanup) = cleanup_fault_fixture(root.path(), case);
            let (sequence, step) = cleanup_faults(root.path(), &name)[index];
            FAULT.with(|slot| slot.set(Some((sequence, step, false))));
            let result = super::super::cleanup::reclaim_payload(
                &owner(&root.path().join("destination")),
                &owner(&root.path().join("central")),
                &name,
                |_| Ok(()),
            );
            FAULT.with(|slot| slot.set(None));
            assert!(result.is_err(), "case {case}, {sequence}, {step:?}");
            recover_cleanup_fault(root.path(), &name, step);
        }
    }
}

#[test]
#[ignore = "abrupt-exit child; owned empty root supplied by parent"]
fn windows_native_copy_cleanup_exit_child() {
    let root = std::path::PathBuf::from(std::env::var_os("OTTRIN_CLEANUP_CHILD").unwrap());
    assert!(root.is_dir() && std::fs::read_dir(&root).unwrap().next().is_none());
    let case = std::env::var("OTTRIN_CLEANUP_CASE")
        .unwrap()
        .parse()
        .unwrap();
    let index: usize = std::env::var("OTTRIN_CLEANUP_FAULT")
        .unwrap()
        .parse()
        .unwrap();
    let (name, _cleanup) = cleanup_fault_fixture(&root, case);
    let (sequence, step) = cleanup_faults(&root, &name)[index];
    FAULT.with(|slot| slot.set(Some((sequence, step, true))));
    let result = super::super::cleanup::reclaim_payload(
        &owner(&root.join("destination")),
        &owner(&root.join("central")),
        &name,
        |_| Ok(()),
    );
    panic!("expected abrupt cleanup exit: {result:?}");
}

#[test]
fn windows_native_copy_cleanup_real_process_exits_resume_without_sources() {
    for case in 0..6 {
        let template = tempfile::tempdir().unwrap();
        let (name, _cleanup) = cleanup_fault_fixture(template.path(), case);
        for (index, (_, step)) in cleanup_faults(template.path(), &name)
            .into_iter()
            .enumerate()
        {
            let root = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_exit_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("OTTRIN_CLEANUP_CHILD", root.path())
                .env("OTTRIN_CLEANUP_CASE", case.to_string())
                .env("OTTRIN_CLEANUP_FAULT", index.to_string())
                .status()
                .unwrap();
            // A process exit bypasses child destructors; the parent owns cleanup.
            let _readonly = ReadonlyCleanup(
                namespace_contents(root.path())
                    .into_keys()
                    .map(|path| root.path().join(path))
                    .filter(|path| path.is_file())
                    .collect(),
            );
            assert_eq!(
                status.code(),
                Some(86),
                "case {case}, index {index}, {step:?}"
            );
            recover_cleanup_fault(
                root.path(),
                &stage_name(&root.path().join("destination")),
                step,
            );
        }
    }
}

#[test]
fn windows_native_copy_cleanup_finalizes_metadata_and_replays_its_admitted_proof() {
    for case in 0..6 {
        let root = tempfile::tempdir().unwrap();
        let (name, _readonly) = cleanup_fault_fixture(root.path(), case);
        let before = namespace_contents(root.path());
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let journal = original_journal(root.path(), &name);
        let central_name = journal.header.central_name();
        drop(journal);
        let plan =
            super::super::cleanup_finalize::prepare(&parent, &central, &name, |_| Ok(())).unwrap();
        plan.finish(&parent, &central, |_| Ok(())).unwrap();
        plan.finish(&parent, &central, |_| Ok(())).unwrap();
        assert!(!root.path().join("destination").join(&name).exists());
        assert!(!root.path().join("central").join(central_name).exists());
        assert!(root.path().join("central").is_dir());
        for (path, contents) in before {
            if path.starts_with("destination/copy") || path == Path::new("outside-original") {
                if let Some(contents) = contents {
                    assert_eq!(std::fs::read(root.path().join(path)).unwrap(), contents);
                } else {
                    assert!(root.path().join(path).is_dir());
                }
            }
        }
    }
}

#[test]
fn windows_native_copy_cleanup_metadata_refuses_unknown_content_and_aliases() {
    for case in 0..6 {
        let root = tempfile::tempdir().unwrap();
        let (name, _readonly) = cleanup_fault_fixture(root.path(), 1);
        let stage = root.path().join("destination").join(&name);
        let local = stage.join(LOCAL);
        let journal = original_journal(root.path(), &name);
        let central_path = root
            .path()
            .join("central")
            .join(journal.header.central_name());
        drop(journal);
        let unknown = match case {
            0 => stage.join("unknown-data"),
            1 => std::path::PathBuf::from(format!("{}:extra", stage.display())),
            2 => std::path::PathBuf::from(format!("{}:extra", local.display())),
            3 => std::path::PathBuf::from(format!("{}:extra", central_path.display())),
            4 | 5 => root.path().join("outside-control-alias"),
            _ => unreachable!(),
        };
        if case >= 4 {
            std::fs::hard_link(if case == 4 { &local } else { &central_path }, &unknown).unwrap();
        } else {
            std::fs::write(&unknown, b"unknown data must survive").unwrap();
        }
        let before = namespace_contents(root.path());
        let unknown_before = std::fs::read(&unknown).unwrap();
        assert!(
            super::super::cleanup_finalize::prepare(
                &owner(&root.path().join("destination")),
                &owner(&root.path().join("central")),
                &name,
                |_| Ok(()),
            )
            .is_err(),
            "unknown case {case}"
        );
        assert_eq!(namespace_contents(root.path()), before);
        assert_eq!(std::fs::read(&unknown).unwrap(), unknown_before);
    }
}

fn metadata_faults(sequence: u64) -> Vec<(u64, Step)> {
    let mut faults = Vec::new();
    for role in [
        CleanupMetadataRole::Local,
        CleanupMetadataRole::Stage,
        CleanupMetadataRole::Central,
    ] {
        for point in [
            CleanupMetadataPoint::DispositionSet,
            CleanupMetadataPoint::HandleClosed,
            CleanupMetadataPoint::Flushed,
        ] {
            faults.push((sequence, Step::CleanupMetadata { role, point }));
        }
    }
    faults
}

#[test]
#[ignore = "abrupt metadata exit; parent owns prepared fixture and admission"]
fn windows_native_copy_cleanup_metadata_exit_child() {
    let root = std::path::PathBuf::from(std::env::var_os("OTTRIN_CLEANUP_METADATA_CHILD").unwrap());
    let name = stage_name(&root.join("destination"));
    let parent = owner(&root.join("destination"));
    let central = owner(&root.join("central"));
    if let Ok(point) = std::env::var("OTTRIN_CLEANUP_METADATA_AUTHORIZE") {
        let sequence = original_journal(&root, &name).sequence;
        let step = if point == "local" {
            Step::LocalFlushed
        } else {
            Step::CentralFlushed
        };
        FAULT.with(|slot| slot.set(Some((sequence, step, true))));
        let result = super::super::cleanup_finalize::prepare(&parent, &central, &name, |_| Ok(()));
        panic!("expected metadata authorization exit: {}", result.is_err());
    }
    let plan =
        super::super::cleanup_finalize::prepare(&parent, &central, &name, |_| Ok(())).unwrap();
    let sequence = original_journal(&root, &name).sequence;
    let index: usize = std::env::var("OTTRIN_CLEANUP_METADATA_FAULT")
        .unwrap()
        .parse()
        .unwrap();
    let (sequence, step) = metadata_faults(sequence)[index];
    FAULT.with(|slot| slot.set(Some((sequence, step, true))));
    let result = plan.finish(&parent, &central, |_| Ok(()));
    panic!("expected metadata exit: {result:?}");
}

#[test]
fn windows_native_copy_cleanup_metadata_failures_and_real_exits_recover_in_order() {
    for abrupt in [false, true] {
        for case in 0..6 {
            for index in 0..9 {
                let root = tempfile::tempdir().unwrap();
                let (name, _readonly) = cleanup_fault_fixture(root.path(), case);
                let parent = owner(&root.path().join("destination"));
                let central = owner(&root.path().join("central"));
                let plan =
                    super::super::cleanup_finalize::prepare(&parent, &central, &name, |_| Ok(()))
                        .unwrap();
                let journal = original_journal(root.path(), &name);
                let sequence = journal.sequence;
                let central_name = journal.header.central_name();
                drop(journal);
                let (sequence, step) = metadata_faults(sequence)[index];
                if abrupt {
                    let status = std::process::Command::new(std::env::current_exe().unwrap())
                        .args(["--exact", "copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_metadata_exit_child", "--ignored", "--nocapture"])
                        .env("OTTRIN_CLEANUP_METADATA_CHILD", root.path())
                        .env("OTTRIN_CLEANUP_METADATA_FAULT", index.to_string())
                        .status().unwrap();
                    assert_eq!(status.code(), Some(86), "{case} {step:?}");
                } else {
                    FAULT.with(|slot| slot.set(Some((sequence, step, false))));
                    let result = plan.finish(&parent, &central, |_| Ok(()));
                    FAULT.with(|slot| slot.set(None));
                    assert!(result.is_err(), "{case} {step:?}");
                }
                if index < 6 {
                    // A new process derives its authority from the remaining
                    // central intent even when the local journal is already gone.
                    let before = namespace_contents(root.path());
                    let read_parent = windows_fs::open_directory(
                        &root.path().join("destination"),
                        FILE_GENERIC_READ,
                    )
                    .unwrap();
                    let read_central =
                        windows_fs::open_directory(&root.path().join("central"), FILE_GENERIC_READ)
                            .unwrap();
                    let resumed = super::super::cleanup_finalize::Plan::observe(
                        &read_parent,
                        &read_central,
                        &name,
                        |_| Ok(()),
                    )
                    .unwrap();
                    assert_eq!(namespace_contents(root.path()), before);
                    let statuses = discover_at(&root.path().join("central")).unwrap();
                    assert_eq!(statuses.len(), 1);
                    assert_eq!(statuses[0].phase, Some(WindowsCopyPhase::CleaningUp));
                    assert_eq!(
                        statuses[0].evidence,
                        WindowsCopyRecoveryEvidence::VerifiedCleanup,
                        "case {case}, {step:?}"
                    );
                    assert_eq!(
                        statuses[0].transaction,
                        Some(
                            windows_fs::location(&read_central)
                                .unwrap()
                                .join(&central_name)
                        )
                    );
                    assert_eq!(namespace_contents(root.path()), before);
                    drop((read_parent, read_central));
                    resumed.finish(&parent, &central, |_| Ok(())).unwrap();
                } else {
                    // Once central removal occurs, only the already admitted
                    // opaque proof can report an idempotent terminal replay.
                    plan.finish(&parent, &central, |_| Ok(())).unwrap();
                }
                assert!(!root.path().join("destination").join(&name).exists());
                assert!(!root.path().join("central").join(central_name).exists());
                assert!(root.path().join("central").is_dir());
            }
        }
    }
}

#[test]
fn windows_native_copy_cleanup_metadata_authority_requires_matching_intent_mirrors() {
    for abrupt in [false, true] {
        for case in 0..6 {
            for step in [Step::LocalFlushed, Step::CentralFlushed] {
                let root = tempfile::tempdir().unwrap();
                let (name, _readonly) = cleanup_fault_fixture(root.path(), case);
                let parent = owner(&root.path().join("destination"));
                let central = owner(&root.path().join("central"));
                super::super::cleanup::reclaim_payload(&parent, &central, &name, |_| Ok(()))
                    .unwrap();
                let sequence = original_journal(root.path(), &name).sequence;
                if abrupt {
                    let status = std::process::Command::new(std::env::current_exe().unwrap())
                        .args(["--exact", "copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_metadata_exit_child", "--ignored", "--nocapture"])
                        .env("OTTRIN_CLEANUP_METADATA_CHILD", root.path())
                        .env("OTTRIN_CLEANUP_METADATA_AUTHORIZE", if step == Step::LocalFlushed { "local" } else { "central" })
                        .status().unwrap();
                    assert_eq!(status.code(), Some(86));
                } else {
                    FAULT.with(|slot| slot.set(Some((sequence, step, false))));
                    let result =
                        super::super::cleanup_finalize::prepare(&parent, &central, &name, |_| {
                            Ok(())
                        });
                    FAULT.with(|slot| slot.set(None));
                    assert!(result.is_err());
                }
                let before = namespace_contents(root.path());
                let result =
                    super::super::cleanup_finalize::prepare(&parent, &central, &name, |_| Ok(()));
                if step == Step::LocalFlushed {
                    assert!(result.is_err());
                    assert_eq!(namespace_contents(root.path()), before);
                } else {
                    result
                        .unwrap()
                        .finish(&parent, &central, |_| Ok(()))
                        .unwrap();
                    assert!(!root.path().join("destination").join(&name).exists());
                }
            }
        }
    }
}

#[test]
fn windows_native_copy_cleanup_metadata_preserves_unknown_data_added_after_admission() {
    for case in 0..7 {
        let root = tempfile::tempdir().unwrap();
        let (name, _readonly) = cleanup_fault_fixture(root.path(), 1);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let plan =
            super::super::cleanup_finalize::prepare(&parent, &central, &name, |_| Ok(())).unwrap();
        let stage = root.path().join("destination").join(&name);
        let local = stage.join(LOCAL);
        let journal = original_journal(root.path(), &name);
        let header = journal.header.clone();
        let control = root.path().join("central").join(header.central_name());
        drop(journal);
        let unknown = match case {
            0 => stage.join("unknown"),
            1 => std::path::PathBuf::from(format!("{}:extra", stage.display())),
            2 => std::path::PathBuf::from(format!("{}:extra", local.display())),
            3 => std::path::PathBuf::from(format!("{}:extra", control.display())),
            4 => {
                std::fs::remove_file(&local).unwrap();
                local.clone()
            }
            5 => {
                std::fs::remove_file(&local).unwrap();
                stage.join("unknown")
            }
            6 => root
                .path()
                .join("destination")
                .join(header.candidate_name()),
            _ => unreachable!(),
        };
        std::fs::write(&unknown, b"new user content must survive").unwrap();
        let before = namespace_contents(root.path());
        assert!(
            super::super::cleanup_finalize::Plan::observe(&parent, &central, &name, |_| Ok(()))
                .is_err(),
            "inspection accepted unknown content in case {case}"
        );
        assert_eq!(namespace_contents(root.path()), before);
        let statuses = discover_at(&root.path().join("central")).unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].phase, Some(WindowsCopyPhase::CleaningUp));
        assert!(matches!(
            statuses[0].evidence,
            WindowsCopyRecoveryEvidence::NeedsReview { .. }
        ));
        assert_eq!(namespace_contents(root.path()), before);
        assert!(
            plan.finish(&parent, &central, |_| Ok(())).is_err(),
            "case {case}"
        );
        assert_eq!(namespace_contents(root.path()), before);
        assert_eq!(
            std::fs::read(&unknown).unwrap(),
            b"new user content must survive"
        );
    }
}

#[test]
fn windows_native_copy_cleanup_metadata_rechecks_control_before_any_unlink() {
    let root = tempfile::tempdir().unwrap();
    let (name, _readonly) = cleanup_fault_fixture(root.path(), 1);
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let plan =
        super::super::cleanup_finalize::prepare(&parent, &central, &name, |_| Ok(())).unwrap();
    let journal = original_journal(root.path(), &name);
    let control = root
        .path()
        .join("central")
        .join(journal.header.central_name());
    drop(journal);
    let _restore = super::super::super::tests::ReadOnlyAcl(vec![control.clone()]);
    let before = namespace_contents(root.path());
    let mut saw_control_data = false;
    let mut changed = false;
    let result = plan.finish(&parent, &central, |bytes| {
        if bytes > 0 {
            saw_control_data = true;
        } else if saw_control_data && !changed {
            super::super::super::tests::set_dacl(&control, "D:P(A;;FRWD;;;OW)");
            changed = true;
        }
        Ok(())
    });
    assert!(changed && result.is_err());
    assert_eq!(namespace_contents(root.path()), before);
}

#[test]
fn windows_native_copy_cleanup_closed_handle_writable_mappings_block_metadata_removal() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Memory::{
        CreateFileMappingW, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
        PAGE_READWRITE, UnmapViewOfFile,
    };
    struct Mapping {
        object: HANDLE,
        view: MEMORY_MAPPED_VIEW_ADDRESS,
    }
    impl Drop for Mapping {
        fn drop(&mut self) {
            unsafe {
                UnmapViewOfFile(self.view);
                CloseHandle(self.object);
            }
        }
    }
    for central_mapping in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (name, _readonly) = cleanup_fault_fixture(root.path(), 1);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let plan =
            super::super::cleanup_finalize::prepare(&parent, &central, &name, |_| Ok(())).unwrap();
        let journal = original_journal(root.path(), &name);
        let path = if central_mapping {
            root.path()
                .join("central")
                .join(journal.header.central_name())
        } else {
            root.path().join("destination").join(&name).join(LOCAL)
        };
        drop(journal);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&path)
            .unwrap();
        let object = unsafe {
            CreateFileMappingW(
                file.as_raw_handle().cast(),
                ptr::null(),
                PAGE_READWRITE,
                0,
                0,
                ptr::null(),
            )
        };
        assert!(!object.is_null());
        let view = unsafe { MapViewOfFile(object, FILE_MAP_WRITE, 0, 0, 0) };
        let mapping = Mapping { object, view };
        assert!(!mapping.view.Value.is_null());
        drop(file);
        let before = namespace_contents(root.path());
        assert!(plan.finish(&parent, &central, |_| Ok(())).is_err());
        assert_eq!(namespace_contents(root.path()), before);
        drop(mapping);
        plan.finish(&parent, &central, |_| Ok(())).unwrap();
    }
}

fn anchored_cleanup_fixture(root: &Path) {
    fixture(root);
    std::fs::remove_dir(root.join("central")).unwrap();
    drop(
        super::super::super::registry::ConfiguredRegistry::open_at(&root.join("central"), true)
            .unwrap()
            .unwrap(),
    );
}

fn cleanup_peer_fixture(root: &Path, shared: bool) -> [OsString; 2] {
    cleanup_peer_fixture_with_aliases(root, shared, false)
}

fn cleanup_peer_fixture_with_aliases(
    root: &Path,
    shared: bool,
    extra_aliases: bool,
) -> [OsString; 2] {
    anchored_cleanup_fixture(root);
    let first = root.join("destination");
    let second = root.join("second-destination");
    std::fs::create_dir(&second).unwrap();
    for destination in [&first, &second] {
        std::fs::create_dir(destination.join("copy")).unwrap();
    }
    std::fs::write(first.join("copy/old"), b"old original data").unwrap();
    if shared {
        std::fs::hard_link(first.join("copy/old"), second.join("copy/old")).unwrap();
    } else {
        std::fs::write(second.join("copy/old"), b"old original data").unwrap();
    }
    if extra_aliases {
        assert!(shared);
        std::fs::hard_link(first.join("copy/old"), second.join("copy/other")).unwrap();
        std::fs::hard_link(first.join("copy/old"), root.join("outside")).unwrap();
    }
    // Both original graphs exist before either retention rename; native
    // child epochs therefore remain exact in the shared-hardlink case.
    let input = source(&root.join("source"));
    let central = owner(&root.join("central"));
    let mut names = Vec::new();
    for destination in [&first, &second] {
        let parent = owner(destination);
        let admission =
            DestinationAdmission::overwrite(&parent, OsStr::new("copy"), 100, |_| Ok(())).unwrap();
        let mut staged = DurableStage::create(
            &input,
            &parent,
            &central,
            &admission,
            OsStr::new("source"),
            100,
            |_| Ok(()),
        )
        .unwrap();
        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
        let name = staged.stage.name.clone();
        drop(staged);
        let snapshot = Snapshot::load(&parent, &central, &name, SnapshotAccess::Write).unwrap();
        let mut journal = Journal::from_snapshot(&parent, &central, snapshot).unwrap();
        journal.prepare_original(100, |_| Ok(())).unwrap();
        journal
            .move_original(None, OriginalMove::Retain, |_| Ok(()))
            .unwrap();
        drop(journal);
        PermissionStage::publish(&parent, &central, &name, |_| Ok(()))
            .unwrap()
            .apply(|_| Ok(()))
            .unwrap();
        names.push(name);
    }
    drop(input);
    remove_undo_fixture_source(root);
    names.try_into().unwrap()
}

#[test]
fn windows_native_copy_cleanup_registry_dependencies_preserve_peers_and_shared_originals() {
    for (shared, drift) in [(false, 0), (false, 1), (false, 2), (false, 3), (true, 0)] {
        let root = tempfile::tempdir().unwrap();
        let names = cleanup_peer_fixture(root.path(), shared);
        let first = root.path().join("destination");
        let second = root.path().join("second-destination");
        let central = owner(&root.path().join("central"));
        let peer_local = second.join(&names[1]).join(LOCAL);
        let peer_bytes = std::fs::read(&peer_local).unwrap();
        let peer_header = decode(&peer_bytes).unwrap().state.header.unwrap();
        if shared {
            assert!(
                super::super::cleanup::Admission::observe(
                    &owner(&first),
                    &central,
                    &names[0],
                    |_| Ok(())
                )
                .is_err()
            );
            // A peer starting cleanup gives up publication claims, not its
            // actual remaining retained hard-link names.
            let other_parent = owner(&second);
            let snapshot =
                Snapshot::load(&other_parent, &central, &names[1], SnapshotAccess::Write).unwrap();
            let mut writer = Journal::from_snapshot(&other_parent, &central, snapshot).unwrap();
            let manifest = writer.state.cleanup_manifest().unwrap();
            writer.reserve_cleanup_tail().unwrap();
            writer
                .persist(Record::Cleanup {
                    entry: super::super::cleanup_protocol::CleanupRecord::Begin {
                        owner: manifest.owner,
                        count: manifest.entries.len(),
                        digest: manifest.digest,
                    },
                })
                .unwrap();
        }
        let before = namespace_contents(root.path());
        let parent = owner(&first);
        if drift != 0 {
            let admission =
                super::super::cleanup::Admission::observe(&parent, &central, &names[0], |_| Ok(()))
                    .unwrap();
            let peer_control = root.path().join("central").join(peer_header.central_name());
            if drift == 1 {
                std::fs::write(&peer_control, &peer_bytes).unwrap();
            }
            if drift <= 2 {
                owner(&peer_control)
                    .set_times(std::fs::FileTimes::new().set_modified(
                        std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
                    ))
                    .unwrap();
            } else {
                std::fs::hard_link(&peer_control, root.path().join("outside-peer-alias")).unwrap();
            }
            let changed = namespace_contents(root.path());
            assert!(
                admission.reclaim(&parent, &central, |_| Ok(())).is_err(),
                "adopted peer drift {drift}"
            );
            assert_eq!(namespace_contents(root.path()), changed);
            assert_eq!(std::fs::read(&peer_local).unwrap(), peer_bytes);
            continue;
        }
        let result =
            super::super::cleanup_finalize::prepare(&parent, &central, &names[0], |_| Ok(()));
        if shared {
            let error = match result {
                Err(error) => error,
                Ok(_) => panic!("shared retained object was admitted"),
            };
            assert!(
                error.to_string().contains("dependent transaction"),
                "{error}"
            );
            assert_eq!(namespace_contents(root.path()), before);
        } else {
            result
                .unwrap()
                .finish(&parent, &central, |_| Ok(()))
                .unwrap();
            assert_eq!(std::fs::read(&peer_local).unwrap(), peer_bytes);
            assert_eq!(
                std::fs::read(second.join(peer_header.original_name()).join("old")).unwrap(),
                b"old original data"
            );
        }
    }
}

#[test]
fn windows_native_copy_cleanup_unverified_registry_peer_refuses_before_any_write() {
    let root = tempfile::tempdir().unwrap();
    let (name, _readonly) = cleanup_fault_fixture(root.path(), 1);
    let central = owner(&root.path().join("central"));
    let mut unknown = windows_fs::create_private(
        &central,
        OsStr::new("00000000000000000000000000000000.copy-journal"),
        false,
        FILE_SHARE_READ,
    )
    .unwrap();
    unknown.write_all(b"unknown recovery data").unwrap();
    unknown.sync_all().unwrap();
    drop(unknown);
    let before = namespace_contents(root.path());
    assert!(
        super::super::cleanup_finalize::prepare(
            &owner(&root.path().join("destination")),
            &central,
            &name,
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(namespace_contents(root.path()), before);
}

#[test]
fn windows_native_copy_cleanup_configured_registry_writer_supports_registry_destinations() {
    use super::super::super::registry::ConfiguredRegistry;
    for fault_role in [
        None,
        Some(CleanupMetadataRole::Local),
        Some(CleanupMetadataRole::Stage),
        Some(CleanupMetadataRole::Central),
    ] {
        for same_root in [false, true] {
            for directory in [false, true] {
                for overwrite in [false, true] {
                    for undone in [false, true] {
                        let root = tempfile::tempdir().unwrap();
                        fixture(root.path());
                        std::fs::remove_dir(root.path().join("central")).unwrap();
                        let path = root.path().join("profile/Ottrin/State/copies");
                        assert!(ConfiguredRegistry::open_at(&path, false).unwrap().is_none());
                        assert!(!root.path().join("profile").exists());
                        drop(ConfiguredRegistry::open_at(&path, true).unwrap().unwrap());
                        let registry = ConfiguredRegistry::open_at(&path, false).unwrap().unwrap();
                        let central = registry.writable_root().unwrap();
                        central.sync_all().unwrap();
                        // The manager pins its root without sharing DELETE. The
                        // same-root parent needs only read/write directory rights.
                        let parent = if same_root {
                            central.try_clone().unwrap()
                        } else {
                            owner(&root.path().join("destination"))
                        };
                        let destination = windows_fs::location(&parent).unwrap();
                        if overwrite {
                            if directory {
                                std::fs::create_dir(destination.join("copy")).unwrap();
                                std::fs::write(destination.join("copy/old"), b"original user data")
                                    .unwrap();
                            } else {
                                std::fs::write(destination.join("copy"), b"original user data")
                                    .unwrap();
                            }
                        }
                        let input = source(&root.path().join(if directory {
                            "source"
                        } else {
                            "source/nested/file"
                        }));
                        let admission = if overwrite {
                            DestinationAdmission::overwrite(
                                &parent,
                                OsStr::new("copy"),
                                100,
                                |_| Ok(()),
                            )
                            .unwrap()
                        } else {
                            DestinationAdmission::creation(&parent, OsStr::new("copy")).unwrap()
                        };
                        let mut staged = DurableStage::create(
                            &input,
                            &parent,
                            &central,
                            &admission,
                            OsStr::new("source"),
                            100,
                            |_| Ok(()),
                        )
                        .unwrap();
                        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
                        let name = staged.stage.name.clone();
                        drop((staged, input));
                        if overwrite {
                            let snapshot =
                                Snapshot::load(&parent, &central, &name, SnapshotAccess::Write)
                                    .unwrap();
                            let mut journal =
                                Journal::from_snapshot(&parent, &central, snapshot).unwrap();
                            journal.prepare_original(100, |_| Ok(())).unwrap();
                            journal
                                .move_original(None, OriginalMove::Retain, |_| Ok(()))
                                .unwrap();
                        }
                        PermissionStage::publish(&parent, &central, &name, |_| Ok(()))
                            .unwrap()
                            .apply(|_| Ok(()))
                            .unwrap();
                        if undone {
                            undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
                        }
                        remove_undo_fixture_source(root.path());
                        let snapshot =
                            Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).unwrap();
                        let header = snapshot.state.header.as_ref().unwrap().clone();
                        let metadata_sequence = snapshot.sequence
                            + 2 * snapshot.state.cleanup_manifest().unwrap().entries.len() as u64
                            + 3;
                        drop(snapshot);
                        let before = namespace_contents(root.path());
                        let mut plan =
                            RetainedCopyCleanupPlan::observe_at(&path, &header.transaction, |_| {
                                Ok(())
                            })
                            .unwrap();
                        assert_eq!(namespace_contents(root.path()), before);
                        assert_eq!(plan.transaction_id(), header.transaction);
                        assert_eq!(
                            plan.admitted_phase(),
                            if undone {
                                WindowsCopyPhase::Undone
                            } else {
                                WindowsCopyPhase::Completed
                            }
                        );
                        assert!(!plan.complete());
                        if let Some(role) = fault_role {
                            FAULT.with(|slot| {
                                slot.set(Some((
                                    metadata_sequence,
                                    Step::CleanupMetadata {
                                        role,
                                        point: CleanupMetadataPoint::Flushed,
                                    },
                                    false,
                                )))
                            });
                        }
                        let result = plan.reclaim(|_| Ok(()));
                        FAULT.with(|slot| slot.set(None));
                        if let Some(role) = fault_role {
                            assert!(
                                result.is_err(),
                                "{same_root} {directory} {overwrite} {undone} {role:?}"
                            );
                            assert!(!plan.complete());
                            let before_retry = namespace_contents(root.path());
                            let fresh = RetainedCopyCleanupPlan::observe_at(
                                &path,
                                &header.transaction,
                                |_| Ok(()),
                            );
                            if role == CleanupMetadataRole::Central {
                                assert!(
                                    fresh.is_err(),
                                    "absent IDs must not manufacture cleanup authority"
                                );
                            } else {
                                assert!(fresh.is_ok(), "{fresh:?}");
                            }
                            assert_eq!(namespace_contents(root.path()), before_retry);
                            drop(fresh);
                            // The same admitted action keeps its metadata proof,
                            // including after final central unlink.
                            plan.reclaim(|_| Ok(())).unwrap();
                        } else {
                            result.unwrap();
                        }
                        assert!(plan.complete());
                        plan.reclaim(|_| Ok(())).unwrap();
                        registry.validate().unwrap();
                        assert!(path.is_dir());
                        assert!(!path.join(header.central_name()).exists());
                        assert!(!destination.join(&name).exists());
                        let marker = path.parent().unwrap().join(".ottrin-copy-recovery-root");
                        let marker_key = marker.strip_prefix(root.path()).unwrap();
                        assert_eq!(
                            Some(&Some(std::fs::read(&marker).unwrap())),
                            before.get(marker_key)
                        );
                        if !undone || overwrite {
                            let public = destination.join("copy");
                            let contents = if directory {
                                public.join(if undone { "old" } else { "nested/file" })
                            } else {
                                public
                            };
                            assert_eq!(
                                std::fs::read(contents).unwrap(),
                                if undone {
                                    b"original user data".as_slice()
                                } else {
                                    b"original data".as_slice()
                                }
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn windows_native_copy_cleanup_admission_is_readonly_handle_free_and_keeps_public_reuse() {
    use super::super::cleanup::Admission;
    for case in 0..6 {
        let root = tempfile::tempdir().unwrap();
        let (name, _readonly) = cleanup_fault_fixture(root.path(), case);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let before = namespace_contents(root.path());
        let admission = Admission::observe(&parent, &central, &name, |_| Ok(())).unwrap();
        assert_eq!(admission.header().stage_name(), name);
        assert_eq!(namespace_contents(root.path()), before);
        // Confirmation retains no readers: the user's public name can be
        // changed/reused, and is still outside the admitted cleanup set.
        let public = root.path().join("destination/copy");
        if public.is_dir() {
            std::fs::remove_dir_all(&public).unwrap();
        } else if public.exists() {
            std::fs::remove_file(&public).unwrap();
        }
        std::fs::write(&public, b"user data after admission").unwrap();
        admission.reclaim(&parent, &central, |_| Ok(())).unwrap();
        assert_eq!(
            std::fs::read(&public).unwrap(),
            b"user data after admission"
        );
    }
}

#[test]
fn windows_native_copy_cleanup_admission_refuses_payload_metadata_catalog_and_progress_drift() {
    use super::super::cleanup::Admission;
    for case in 0..5 {
        let root = tempfile::tempdir().unwrap();
        let (name, _readonly) = cleanup_fault_fixture(root.path(), 1);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let admission = Admission::observe(&parent, &central, &name, |_| Ok(())).unwrap();
        let stage = root.path().join("destination").join(&name);
        let local = stage.join(LOCAL);
        match case {
            0 => {
                let file = root
                    .path()
                    .join("destination")
                    .join(admission.header().candidate_name())
                    .join("nested/file");
                std::fs::write(file, b"external retained edit").unwrap();
            }
            1 => {
                let before = super::super::super::version(&source(&local)).unwrap();
                let bytes = std::fs::read(&local).unwrap();
                std::fs::write(&local, bytes).unwrap();
                assert_ne!(
                    super::super::super::version(&source(&local)).unwrap(),
                    before
                );
            }
            2 => {
                std::fs::write(stage.join("unknown"), b"unrecorded data").unwrap();
            }
            3 => {
                std::fs::write(
                    root.path()
                        .join("central/00000000000000000000000000000000.copy-journal"),
                    b"new unknown peer",
                )
                .unwrap();
            }
            4 => {
                super::super::cleanup::reclaim_payload(&parent, &central, &name, |_| Ok(()))
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let before = namespace_contents(root.path());
        assert!(
            admission.reclaim(&parent, &central, |_| Ok(())).is_err(),
            "case {case}"
        );
        assert_eq!(namespace_contents(root.path()), before);
    }
}

#[test]
fn windows_native_copy_cleanup_pending_absence_admission_never_adopts_a_later_parent_epoch() {
    use super::super::cleanup::Admission;
    let root = tempfile::tempdir().unwrap();
    let (name, _readonly) = cleanup_fault_fixture(root.path(), 1);
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let journal = original_journal(root.path(), &name);
    let sequence = journal.sequence;
    let header = journal.header.clone();
    drop(journal);
    FAULT.with(|slot| slot.set(Some((sequence + 2, Step::CleanupFlushed, false))));
    let result = super::super::cleanup::reclaim_payload(&parent, &central, &name, |_| Ok(()));
    FAULT.with(|slot| slot.set(None));
    assert!(result.is_err());
    let admission = Admission::observe(&parent, &central, &name, |_| Ok(())).unwrap();
    let nested = root
        .path()
        .join("destination")
        .join(header.candidate_name())
        .join("nested");
    assert!(!nested.join("file").exists());
    let file = owner(&nested);
    let before = super::super::super::version(&file).unwrap();
    let modified = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    file.set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let changed = super::super::super::version(&file).unwrap();
    assert_ne!(changed, before);
    drop(file);
    let before = namespace_contents(root.path());
    let error = admission
        .reclaim(&parent, &central, |_| Ok(()))
        .unwrap_err();
    assert!(
        error.to_string().contains("retained epoch changed"),
        "{error}"
    );
    assert_eq!(namespace_contents(root.path()), before);
    assert_eq!(
        super::super::super::version(&owner(&nested)).unwrap(),
        changed
    );
}

#[test]
fn windows_native_copy_cleanup_metadata_inspection_never_authorizes_cleanup() {
    for case in 0..6 {
        let root = tempfile::tempdir().unwrap();
        let (name, _readonly) = cleanup_fault_fixture(root.path(), case);
        let parent =
            windows_fs::open_directory(&root.path().join("destination"), FILE_GENERIC_READ)
                .unwrap();
        let central =
            windows_fs::open_directory(&root.path().join("central"), FILE_GENERIC_READ).unwrap();
        let before = namespace_contents(root.path());
        assert!(
            super::super::cleanup_finalize::Plan::observe(&parent, &central, &name, |_| Ok(()))
                .is_err()
        );
        assert_eq!(namespace_contents(root.path()), before);
    }
}

#[test]
fn windows_native_copy_cleanup_payload_completion_binds_metadata_authorization() {
    for change in 0..8 {
        let root = tempfile::tempdir().unwrap();
        let (name, _readonly) = cleanup_fault_fixture(root.path(), 1);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let admission =
            super::super::cleanup::Admission::observe(&parent, &central, &name, |_| Ok(()))
                .unwrap();
        let header = admission.header().clone();
        let completion = admission.reclaim(&parent, &central, |_| Ok(())).unwrap();
        let stage = root.path().join("destination").join(&name);
        let local = stage.join(LOCAL);
        let control = root.path().join("central").join(header.central_name());
        match change {
            0 | 7 => {}
            1 => {
                let bytes = std::fs::read(&local).unwrap();
                std::fs::write(&local, bytes).unwrap();
            }
            2 | 3 => {
                let file = owner(if change == 2 { &control } else { &stage });
                file.set_times(std::fs::FileTimes::new().set_modified(
                    std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
                ))
                .unwrap();
            }
            4 => {
                let mut unknown = windows_fs::create_private(
                    &central,
                    OsStr::new("00000000000000000000000000000000.copy-journal"),
                    false,
                    FILE_SHARE_READ,
                )
                .unwrap();
                unknown.write_all(b"unknown recovery claims").unwrap();
                unknown.sync_all().unwrap();
            }
            5 => std::fs::write(stage.join("unknown"), b"new user data").unwrap(),
            6 => std::fs::write(
                root.path()
                    .join("destination")
                    .join(header.candidate_name()),
                b"new user data",
            )
            .unwrap(),
            _ => unreachable!(),
        }
        let before = namespace_contents(root.path());
        let result = completion.begin(&parent, &central, |_| {
            if change == 7 {
                Err(io("owned metadata authorization cancellation"))
            } else {
                Ok(())
            }
        });
        if change == 0 {
            result
                .unwrap()
                .finish(&parent, &central, |_| Ok(()))
                .unwrap();
        } else {
            assert!(result.is_err(), "completion adopted change {change}");
            assert_eq!(namespace_contents(root.path()), before);
            if change == 7 {
                completion
                    .begin(&parent, &central, |_| Ok(()))
                    .unwrap()
                    .finish(&parent, &central, |_| Ok(()))
                    .unwrap();
            }
        }
    }
}

#[test]
fn windows_native_copy_cleanup_unrecognized_registry_names_stay_visible_and_block_mutation() {
    for case in 0..5 {
        let root = tempfile::tempdir().unwrap();
        let (name, _readonly) = cleanup_fault_fixture(root.path(), 1);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let control = original_journal(root.path(), &name);
        let old_name = control.header.central_name();
        drop(control);
        let unrecognized = match case {
            0 => OsString::from(format!("{}.COPY-JOURNAL", "0".repeat(32))),
            1 => OsString::from(format!("{}.copy-journal", "A".repeat(32))),
            2 => OsString::from("broken.copy-journal"),
            3 => {
                let mut units = vec![0xd800];
                units.extend(".CoPy-JoUrNaL".encode_utf16());
                OsString::from_wide(&units)
            }
            4 => OsString::from(old_name.to_str().unwrap().to_ascii_uppercase()),
            _ => unreachable!(),
        };
        if case == 4 {
            std::fs::rename(
                root.path().join("central").join(&old_name),
                root.path().join("central").join(&unrecognized),
            )
            .unwrap();
        } else {
            let mut file =
                windows_fs::create_private(&central, &unrecognized, false, FILE_SHARE_READ)
                    .unwrap();
            file.write_all(b"unknown recovery claims must remain")
                .unwrap();
            file.sync_all().unwrap();
        }
        // Legitimate ordinary data in a same-root registry is still unrelated.
        std::fs::write(root.path().join("central/unrelated.txt"), b"user data").unwrap();
        let before = namespace_contents(root.path());
        let statuses = discover_at(&root.path().join("central")).unwrap();
        assert_eq!(statuses.len(), if case == 4 { 1 } else { 2 });
        let unknown = statuses
            .iter()
            .find(|entry| entry.transaction_id.is_none())
            .unwrap();
        assert_eq!(unknown.phase, None);
        assert!(matches!(
            unknown.evidence,
            WindowsCopyRecoveryEvidence::NeedsReview { .. }
        ));
        assert_eq!(
            unknown.transaction,
            Some(windows_fs::location(&central).unwrap().join(&unrecognized))
        );
        assert!(
            unknown.publication.is_none()
                && unknown.retained.is_none()
                && unknown.owned_location.is_none()
        );
        let result =
            super::super::cleanup::Admission::observe(&parent, &central, &name, |_| Ok(()));
        assert!(result.is_err(), "ignored unrecognized registry name {case}");
        if case != 4 {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("unrecognized registry record")
            );
        }
        assert_eq!(namespace_contents(root.path()), before);
    }
}

fn assert_only_journals_changed(
    before: &BTreeMap<std::path::PathBuf, Option<Vec<u8>>>,
    after: &BTreeMap<std::path::PathBuf, Option<Vec<u8>>>,
    headers: &[Header],
) {
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    let journals: BTreeSet<_> = headers
        .iter()
        .flat_map(|header| {
            [
                std::path::PathBuf::from(
                    header
                        .parent_location
                        .decode()
                        .unwrap()
                        .file_name()
                        .unwrap(),
                )
                .join(header.stage_name())
                .join(LOCAL),
                std::path::PathBuf::from("central").join(header.central_name()),
            ]
        })
        .collect();
    for (path, bytes) in before {
        if !journals.contains(path) {
            assert_eq!(after.get(path), Some(bytes), "{}", path.display());
        }
    }
}

fn cleanup_chain_fixture(root: &Path, directory: bool, initial_overwrite: bool) -> [OsString; 2] {
    cleanup_chain_at(root, directory, initial_overwrite, false)
}

fn cleanup_chain_at(
    root: &Path,
    directory: bool,
    initial_overwrite: bool,
    registry_destination: bool,
) -> [OsString; 2] {
    anchored_cleanup_fixture(root);
    let destination = root.join(if registry_destination {
        "central"
    } else {
        "destination"
    });
    let parent = owner(&destination);
    let central = owner(&root.join("central"));
    if initial_overwrite {
        if directory {
            std::fs::create_dir(destination.join("copy")).unwrap();
            std::fs::write(destination.join("copy/old"), b"before first").unwrap();
        } else {
            std::fs::write(destination.join("copy"), b"before first").unwrap();
        }
    }
    let second = root.join("second-source");
    if directory {
        std::fs::create_dir(&second).unwrap();
        std::fs::write(second.join("next"), b"second publication").unwrap();
    } else {
        std::fs::write(&second, b"second publication").unwrap();
    }
    let mut names = Vec::new();
    for index in 0..2 {
        let input_path = if index == 0 {
            root.join(if directory {
                "source"
            } else {
                "source/nested/file"
            })
        } else {
            second.clone()
        };
        let input = source(&input_path);
        let overwrite = initial_overwrite || index == 1;
        let admission = if overwrite {
            DestinationAdmission::overwrite(&parent, OsStr::new("copy"), 100, |_| Ok(())).unwrap()
        } else {
            DestinationAdmission::creation(&parent, OsStr::new("copy")).unwrap()
        };
        let mut staged = DurableStage::create(
            &input,
            &parent,
            &central,
            &admission,
            OsStr::new("source"),
            100,
            |_| Ok(()),
        )
        .unwrap();
        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
        let name = staged.stage.name.clone();
        drop((staged, input));
        if overwrite {
            let snapshot = Snapshot::load(&parent, &central, &name, SnapshotAccess::Write).unwrap();
            let mut journal = Journal::from_snapshot(&parent, &central, snapshot).unwrap();
            journal.prepare_original(100, |_| Ok(())).unwrap();
            journal
                .move_original(None, OriginalMove::Retain, |_| Ok(()))
                .unwrap();
        }
        PermissionStage::publish(&parent, &central, &name, |_| Ok(()))
            .unwrap()
            .apply(|_| Ok(()))
            .unwrap();
        names.push(name);
    }
    remove_undo_fixture_source(root);
    if directory {
        std::fs::remove_dir_all(second).unwrap();
    } else {
        std::fs::remove_file(second).unwrap();
    }
    names.try_into().unwrap()
}

#[test]
fn windows_native_copy_cleanup_start_is_bound_idempotent_and_never_reclaims_payload() {
    for case in 0..6 {
        let root = tempfile::tempdir().unwrap();
        let (name, _readonly) = cleanup_fault_fixture(root.path(), case);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let admission =
            super::super::cleanup::Admission::observe(&parent, &central, &name, |_| Ok(()))
                .unwrap();
        let header = admission.header().clone();
        let before = namespace_contents(root.path());
        assert!(
            admission
                .start(&parent, &central, |_| Err(io("owned start cancellation")))
                .is_err()
        );
        assert_eq!(namespace_contents(root.path()), before);
        let started = admission.start(&parent, &central, |_| Ok(())).unwrap();
        let after = namespace_contents(root.path());
        assert_only_journals_changed(&before, &after, std::slice::from_ref(&header));
        let snapshot = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).unwrap();
        let progress = snapshot.state.cleanup.as_ref().unwrap();
        assert_eq!(progress.completed, 0);
        assert!(
            !progress.pending && !progress.payload_removed && !progress.metadata_removal_intent
        );
        assert!(snapshot.state.check(&Record::UndoIntent).is_err());
        drop(snapshot);
        assert!(admission.reclaim(&parent, &central, |_| Ok(())).is_err());
        assert_eq!(namespace_contents(root.path()), after);
        let repeated = started.start(&parent, &central, |_| Ok(())).unwrap();
        assert_eq!(namespace_contents(root.path()), after);
        repeated
            .reclaim(&parent, &central, |_| Ok(()))
            .unwrap()
            .begin(&parent, &central, |_| Ok(()))
            .unwrap()
            .finish(&parent, &central, |_| Ok(()))
            .unwrap();
    }
}

#[test]
fn windows_native_copy_cleanup_start_barrier_releases_only_relinquished_publication_claims() {
    for directory in [false, true] {
        for initial_overwrite in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let names = cleanup_chain_fixture(root.path(), directory, initial_overwrite);
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let headers: Vec<_> = names
                .iter()
                .map(|name| original_journal(root.path(), name).header.clone())
                .collect();
            let before = namespace_contents(root.path());
            assert!(
                super::super::cleanup::Admission::observe(&parent, &central, &names[1], |_| Ok(()))
                    .is_err()
            );
            assert_eq!(namespace_contents(root.path()), before);
            let first =
                super::super::cleanup::Admission::observe(&parent, &central, &names[0], |_| Ok(()))
                    .unwrap()
                    .start(&parent, &central, |_| Ok(()))
                    .unwrap();
            let second =
                super::super::cleanup::Admission::observe(&parent, &central, &names[1], |_| Ok(()))
                    .unwrap()
                    .start(&parent, &central, |_| Ok(()))
                    .unwrap();
            let barrier = namespace_contents(root.path());
            assert_only_journals_changed(&before, &barrier, &headers);
            for name in &names {
                let snapshot =
                    Snapshot::load(&parent, &central, name, SnapshotAccess::Read).unwrap();
                let progress = snapshot.state.cleanup.as_ref().unwrap();
                assert_eq!(progress.completed, 0);
                assert!(!progress.pending && !progress.payload_removed);
                assert!(snapshot.state.check(&Record::UndoIntent).is_err());
            }
            // A plain old member token must not silently adopt another member's
            // start. The eventual group executor needs a known-own catalog handoff.
            assert!(first.reclaim(&parent, &central, |_| Ok(())).is_err());
            assert_eq!(namespace_contents(root.path()), barrier);
            second
                .reclaim(&parent, &central, |_| Ok(()))
                .unwrap()
                .begin(&parent, &central, |_| Ok(()))
                .unwrap()
                .finish(&parent, &central, |_| Ok(()))
                .unwrap();
            if initial_overwrite {
                let original = root
                    .path()
                    .join("destination")
                    .join(headers[0].original_name());
                assert_eq!(
                    std::fs::read(if directory {
                        original.join("old")
                    } else {
                        original
                    })
                    .unwrap(),
                    b"before first"
                );
            }
            // Explicit recovery admission of the already-started first member,
            // not a coordinated-batch implementation or a blind refresh in it.
            super::super::cleanup::Admission::observe(&parent, &central, &names[0], |_| Ok(()))
                .unwrap()
                .reclaim(&parent, &central, |_| Ok(()))
                .unwrap()
                .begin(&parent, &central, |_| Ok(()))
                .unwrap()
                .finish(&parent, &central, |_| Ok(()))
                .unwrap();
            assert_eq!(
                std::fs::read(root.path().join(if directory {
                    "destination/copy/next"
                } else {
                    "destination/copy"
                }))
                .unwrap(),
                b"second publication"
            );
        }
    }
}

#[test]
fn windows_native_copy_cleanup_unpaired_start_never_releases_a_peer_claim() {
    for step in [Step::LocalFlushed, Step::CentralFlushed] {
        let root = tempfile::tempdir().unwrap();
        let names = cleanup_chain_fixture(root.path(), false, false);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let header = original_journal(root.path(), &names[0]).header.clone();
        let admission =
            super::super::cleanup::Admission::observe(&parent, &central, &names[0], |_| Ok(()))
                .unwrap();
        let sequence = original_journal(root.path(), &names[0]).sequence;
        let before = namespace_contents(root.path());
        FAULT.with(|slot| slot.set(Some((sequence, step, false))));
        let result = admission.start(&parent, &central, |_| Ok(()));
        FAULT.with(|slot| slot.set(None));
        assert!(matches!(result, Err(PlatformError::CleanupPending { .. })));
        let after = namespace_contents(root.path());
        assert_only_journals_changed(&before, &after, std::slice::from_ref(&header));
        let peer =
            super::super::cleanup::Admission::observe(&parent, &central, &names[1], |_| Ok(()));
        assert_eq!(peer.is_ok(), step == Step::CentralFlushed);
        assert_eq!(namespace_contents(root.path()), after);
    }
}

#[test]
fn windows_native_copy_cleanup_group_observation_admits_chains_and_retained_aliases_without_writes()
{
    use super::super::cleanup_group::{Member, Observation};
    for directory in [false, true] {
        for overwrite in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let names = cleanup_chain_fixture(root.path(), directory, overwrite);
            let ids = names
                .iter()
                .map(|name| {
                    original_journal(root.path(), name)
                        .header
                        .transaction
                        .clone()
                })
                .collect::<Vec<_>>();
            let before = namespace_contents(root.path());
            let group =
                Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(())).unwrap();
            assert_eq!(namespace_contents(root.path()), before);
            assert_eq!(
                group.registry,
                windows_fs::location(&owner(&root.path().join("central"))).unwrap()
            );
            assert_eq!(group.members.len(), 2);
            assert!(group.references.contains(&(0, 1)));
            assert!(group.shared.is_empty());
            for (index, member) in group.members.iter().enumerate() {
                assert_eq!(member.header().transaction, ids[index]);
                assert!(matches!(member, Member::Payload(_)));
            }
            // The selection is not a destructive exclusion list. An ordinary
            // member still refuses the unresolved peer's dependency.
            let Member::Payload(blocked) = &group.members[1] else {
                unreachable!()
            };
            assert!(
                blocked
                    .start(
                        &owner(&root.path().join("destination")),
                        &owner(&root.path().join("central")),
                        |_| Ok(())
                    )
                    .is_err()
            );
            assert_eq!(namespace_contents(root.path()), before);
            let central = owner(&root.path().join("central"));
            let record = super::super::discovery::RegisteredJournal::open(
                &central,
                &OsString::from(format!("{}.copy-journal", ids[0])),
            )
            .unwrap();
            assert!(group.selection.admits(&record).unwrap());
        }
    }
    for shared in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let names = cleanup_peer_fixture(root.path(), shared);
        let ids = [
            original_journal(root.path(), &names[0])
                .header
                .transaction
                .clone(),
            {
                let snapshot = Snapshot::load(
                    &owner(&root.path().join("second-destination")),
                    &owner(&root.path().join("central")),
                    &names[1],
                    SnapshotAccess::Read,
                )
                .unwrap();
                snapshot.state.header.as_ref().unwrap().transaction.clone()
            },
        ];
        let before = namespace_contents(root.path());
        let group =
            Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(())).unwrap();
        assert_eq!(namespace_contents(root.path()), before);
        assert_eq!(group.shared.len(), usize::from(shared));
        if shared {
            let owners = group.shared.values().next().unwrap();
            assert_eq!(owners.len(), 2);
            assert!(group.references.contains(&(0, 1)) && group.references.contains(&(1, 0)));
        }
    }
}

#[test]
fn windows_native_copy_cleanup_group_observation_preserves_unselected_claims_changes_and_cancellation()
 {
    use super::super::cleanup_group::Observation;
    for case in 0..6 {
        let root = tempfile::tempdir().unwrap();
        let names = cleanup_chain_fixture(root.path(), false, false);
        let headers = names
            .iter()
            .map(|name| original_journal(root.path(), name).header.clone())
            .collect::<Vec<_>>();
        let mut ids = headers
            .iter()
            .map(|header| header.transaction.clone())
            .collect::<Vec<_>>();
        match case {
            0 => {
                ids.remove(0);
            } // the selected original still belongs to an unselected publication
            1 => ids.push(ids[0].clone()),
            2 => {
                ids[1] = "0".repeat(32);
            }
            3 => std::fs::write(
                root.path()
                    .join("destination")
                    .join(headers[1].original_name()),
                b"changed retained bytes",
            )
            .unwrap(),
            4 => std::fs::write(
                root.path().join("central/bad.COPY-JOURNAL"),
                b"unknown record",
            )
            .unwrap(),
            5 => {}
            _ => unreachable!(),
        }
        let before = namespace_contents(root.path());
        assert!(
            Observation::observe_at(&root.path().join("central"), &ids, |_| {
                if case == 5 {
                    Err(io("owned group observation cancellation"))
                } else {
                    Ok(())
                }
            })
            .is_err(),
            "adopted case {case}"
        );
        assert_eq!(namespace_contents(root.path()), before);
    }
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing/profile/copies");
    assert!(Observation::observe_at(&missing, &["0".repeat(32)], |_| Ok(())).is_err());
    assert!(!root.path().join("missing").exists());
}

#[test]
fn windows_native_copy_cleanup_group_observation_handles_partial_metadata_and_freezes_peer_epochs()
{
    use super::super::cleanup_group::{Member, Observation};
    for role in [CleanupMetadataRole::Local, CleanupMetadataRole::Stage] {
        let root = tempfile::tempdir().unwrap();
        let names = cleanup_chain_fixture(root.path(), false, true);
        let parent = owner(&root.path().join("destination"));
        let manager = super::super::super::registry::ConfiguredRegistry::open_at(
            &root.path().join("central"),
            false,
        )
        .unwrap()
        .unwrap();
        let central = manager.writable_root().unwrap();
        let headers = names
            .iter()
            .map(|name| {
                Snapshot::load(&parent, &central, name, SnapshotAccess::Read)
                    .unwrap()
                    .state
                    .header
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let ids = headers
            .iter()
            .map(|header| header.transaction.clone())
            .collect::<Vec<_>>();
        let plan =
            super::super::cleanup_finalize::prepare(&parent, &central, &names[0], |_| Ok(()))
                .unwrap();
        let sequence = Snapshot::load(&parent, &central, &names[0], SnapshotAccess::Read)
            .unwrap()
            .sequence;
        FAULT.with(|slot| {
            slot.set(Some((
                sequence,
                Step::CleanupMetadata {
                    role,
                    point: CleanupMetadataPoint::Flushed,
                },
                false,
            )))
        });
        let result = plan.finish(&parent, &central, |_| Ok(()));
        FAULT.with(|slot| slot.set(None));
        assert!(result.is_err());
        let before = namespace_contents(root.path());
        let group =
            Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(())).unwrap();
        assert!(matches!(group.members[0], Member::Metadata(_)));
        assert!(matches!(group.members[1], Member::Payload(_)));
        assert_eq!(namespace_contents(root.path()), before);
        let control_path = root.path().join("central").join(headers[1].central_name());
        owner(&control_path)
            .set_times(std::fs::FileTimes::new().set_modified(
                std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
            ))
            .unwrap();
        let record =
            super::super::discovery::RegisteredJournal::open(&central, &headers[1].central_name())
                .unwrap();
        assert!(group.selection.admits(&record).is_err());
    }
}

#[test]
fn windows_native_copy_cleanup_group_start_hands_off_only_its_own_control_changes() {
    use super::super::cleanup_group::{Member, Observation};
    for directory in [false, true] {
        for overwrite in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let names = cleanup_chain_fixture(root.path(), directory, overwrite);
            let headers = names
                .iter()
                .map(|name| original_journal(root.path(), name).header.clone())
                .collect::<Vec<_>>();
            let ids = headers
                .iter()
                .map(|header| header.transaction.clone())
                .collect::<Vec<_>>();
            let mut group =
                Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(())).unwrap();
            let mut stale = group.clone();
            let before = namespace_contents(root.path());
            group.start_all(|_| Ok(())).unwrap();
            let after = namespace_contents(root.path());
            assert_only_journals_changed(&before, &after, &headers);
            for name in &names {
                let journal = original_journal(root.path(), name);
                let progress = journal.state.cleanup.as_ref().unwrap();
                assert_eq!(progress.completed, 0);
                assert!(!progress.payload_removed && !progress.metadata_removal_intent);
            }
            assert!(stale.start_all(|_| Ok(())).is_err());
            assert_eq!(namespace_contents(root.path()), after);
            group.start_all(|_| Ok(())).unwrap();
            assert_eq!(namespace_contents(root.path()), after);
            // The later member is now individually admitted by the group
            // handoff; the old publication reference was durably relinquished.
            let Member::Payload(admission) = &group.members[1] else {
                unreachable!()
            };
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            admission
                .validate_in_group(&parent, &central, &group.selection, |_| Ok(()))
                .unwrap();
            admission.reclaim(&parent, &central, |_| Ok(())).unwrap();
            assert_eq!(
                std::fs::read(root.path().join(if directory {
                    "destination/copy/next"
                } else {
                    "destination/copy"
                }))
                .unwrap(),
                b"second publication"
            );
        }
    }
    for shared in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let names = cleanup_peer_fixture(root.path(), shared);
        let mut headers = Vec::new();
        for (directory, name) in ["destination", "second-destination"].iter().zip(&names) {
            let snapshot = Snapshot::load(
                &owner(&root.path().join(directory)),
                &owner(&root.path().join("central")),
                name,
                SnapshotAccess::Read,
            )
            .unwrap();
            headers.push(snapshot.state.header.unwrap());
        }
        let ids = headers
            .iter()
            .map(|header| header.transaction.clone())
            .collect::<Vec<_>>();
        let mut group =
            Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(())).unwrap();
        let before = namespace_contents(root.path());
        group.start_all(|_| Ok(())).unwrap();
        let after = namespace_contents(root.path());
        assert_only_journals_changed(&before, &after, &headers);
        group.start_all(|_| Ok(())).unwrap();
        assert_eq!(namespace_contents(root.path()), after);
        if shared {
            let Member::Payload(admission) = &group.members[0] else {
                unreachable!()
            };
            assert!(
                admission
                    .reclaim(
                        &owner(&root.path().join("destination")),
                        &owner(&root.path().join("central")),
                        |_| Ok(())
                    )
                    .is_err()
            );
            assert_eq!(namespace_contents(root.path()), after);
        }
    }
}

#[test]
fn windows_native_copy_cleanup_group_start_revalidates_every_member_before_first_begin() {
    use super::super::cleanup_group::Observation;
    for change in 0..4 {
        let root = tempfile::tempdir().unwrap();
        let names = cleanup_chain_fixture(root.path(), false, true);
        let headers = names
            .iter()
            .map(|name| original_journal(root.path(), name).header.clone())
            .collect::<Vec<_>>();
        let ids = headers
            .iter()
            .map(|header| header.transaction.clone())
            .collect::<Vec<_>>();
        let mut group =
            Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(())).unwrap();
        match change {
            0 => std::fs::write(
                root.path()
                    .join("destination")
                    .join(headers[1].original_name()),
                b"changed second retained file",
            )
            .unwrap(),
            1 => std::fs::write(root.path().join("central/unrelated.txt"), b"new entry").unwrap(),
            2 => {
                owner(&root.path().join("central").join(headers[1].central_name()))
                    .set_times(std::fs::FileTimes::new().set_modified(
                        std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
                    ))
                    .unwrap();
            }
            3 => {}
            _ => unreachable!(),
        }
        let before = namespace_contents(root.path());
        assert!(
            group
                .start_all(|_| if change == 3 {
                    Err(io("owned cancellation"))
                } else {
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(namespace_contents(root.path()), before);
        for name in &names {
            assert!(original_journal(root.path(), name).state.cleanup.is_none());
        }
    }
}

#[test]
fn windows_native_copy_cleanup_group_start_uncertain_member_never_refreshes_by_id() {
    use super::super::cleanup_group::Observation;
    for step in [Step::LocalFlushed, Step::CentralFlushed] {
        let root = tempfile::tempdir().unwrap();
        let names = cleanup_chain_fixture(root.path(), false, true);
        let headers = names
            .iter()
            .map(|name| original_journal(root.path(), name).header.clone())
            .collect::<Vec<_>>();
        let ids = headers
            .iter()
            .map(|header| header.transaction.clone())
            .collect::<Vec<_>>();
        let mut group =
            Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(())).unwrap();
        let before = namespace_contents(root.path());
        let first_control = root.path().join("central").join(headers[0].central_name());
        let first_bytes = std::fs::read(&first_control).unwrap();
        let second_sequence = original_journal(root.path(), &names[1]).sequence;
        let mut armed = false;
        let result = group.start_all(|_| {
            if !armed && std::fs::read(&first_control).unwrap() != first_bytes {
                FAULT.with(|slot| slot.set(Some((second_sequence, step, false))));
                armed = true;
            }
            Ok(())
        });
        FAULT.with(|slot| slot.set(None));
        assert!(armed);
        assert!(matches!(result, Err(PlatformError::CleanupPending { .. })));
        let after = namespace_contents(root.path());
        assert_only_journals_changed(&before, &after, &headers);
        assert!(
            original_journal(root.path(), &names[0])
                .state
                .cleanup
                .is_some()
        );
        assert!(group.start_all(|_| Ok(())).is_err());
        assert_eq!(namespace_contents(root.path()), after);
        if step == Step::CentralFlushed {
            let mut recovered =
                Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(())).unwrap();
            recovered.start_all(|_| Ok(())).unwrap();
            assert_eq!(namespace_contents(root.path()), after);
        }
    }
}

#[test]
fn windows_native_copy_cleanup_group_start_checks_confirmed_partial_metadata_proof() {
    use super::super::cleanup_group::Observation;
    for role in [CleanupMetadataRole::Local, CleanupMetadataRole::Stage] {
        for drift in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let names = cleanup_peer_fixture(root.path(), false);
            let manager = super::super::super::registry::ConfiguredRegistry::open_at(
                &root.path().join("central"),
                false,
            )
            .unwrap()
            .unwrap();
            let central = manager.writable_root().unwrap();
            let parent = owner(&root.path().join("destination"));
            let headers = ["destination", "second-destination"]
                .iter()
                .zip(&names)
                .map(|(directory, name)| {
                    Snapshot::load(
                        &owner(&root.path().join(directory)),
                        &central,
                        name,
                        SnapshotAccess::Read,
                    )
                    .unwrap()
                    .state
                    .header
                    .unwrap()
                })
                .collect::<Vec<_>>();
            let ids = headers
                .iter()
                .map(|header| header.transaction.clone())
                .collect::<Vec<_>>();
            let plan =
                super::super::cleanup_finalize::prepare(&parent, &central, &names[0], |_| Ok(()))
                    .unwrap();
            let sequence = Snapshot::load(&parent, &central, &names[0], SnapshotAccess::Read)
                .unwrap()
                .sequence;
            FAULT.with(|slot| {
                slot.set(Some((
                    sequence,
                    Step::CleanupMetadata {
                        role,
                        point: CleanupMetadataPoint::Flushed,
                    },
                    false,
                )))
            });
            let result = plan.finish(&parent, &central, |_| Ok(()));
            FAULT.with(|slot| slot.set(None));
            assert!(result.is_err());
            let mut group =
                Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(())).unwrap();
            if drift {
                let old = super::super::super::security::owner_group_dacl_digest(&parent).unwrap();
                super::super::super::tests::set_dacl(
                    &root.path().join("destination"),
                    "D:P(A;;FA;;;OW)",
                );
                assert_ne!(
                    super::super::super::security::owner_group_dacl_digest(&parent).unwrap(),
                    old
                );
            }
            let before = namespace_contents(root.path());
            let result = group.start_all(|_| Ok(()));
            if drift {
                assert!(result.is_err());
                assert_eq!(namespace_contents(root.path()), before);
                let snapshot = Snapshot::load(
                    &owner(&root.path().join("second-destination")),
                    &central,
                    &names[1],
                    SnapshotAccess::Read,
                )
                .unwrap();
                assert!(snapshot.state.cleanup.is_none());
            } else {
                result.unwrap();
                assert_only_journals_changed(&before, &namespace_contents(root.path()), &headers);
                group.start_all(|_| Ok(())).unwrap();
            }
        }
    }
}

#[test]
fn windows_native_copy_cleanup_group_payload_handoff_preserves_remaining_members_and_completion() {
    use super::super::cleanup_group::{Member, Observation};
    for registry_destination in [false, true] {
        for directory in [false, true] {
            for overwrite in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let names =
                    cleanup_chain_at(root.path(), directory, overwrite, registry_destination);
                let destination = root.path().join(if registry_destination {
                    "central"
                } else {
                    "destination"
                });
                let parent = owner(&destination);
                let central = owner(&root.path().join("central"));
                let headers = names
                    .iter()
                    .map(|name| {
                        Snapshot::load(&parent, &central, name, SnapshotAccess::Read)
                            .unwrap()
                            .state
                            .header
                            .unwrap()
                    })
                    .collect::<Vec<_>>();
                let ids = headers
                    .iter()
                    .map(|header| header.transaction.clone())
                    .collect::<Vec<_>>();
                drop((parent, central));
                let mut group =
                    Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(()))
                        .unwrap();
                group.start_all(|_| Ok(())).unwrap();
                let mut stale = group.clone();
                for index in [1, 0] {
                    group.reclaim_payload(index, |_| Ok(())).unwrap();
                    assert!(matches!(group.members[index], Member::Completion(_)));
                    assert!(!destination.join(headers[index].original_name()).exists());
                    assert!(destination.join(headers[index].stage_name()).exists());
                    assert!(
                        root.path()
                            .join("central")
                            .join(headers[index].central_name())
                            .exists()
                    );
                    let after = namespace_contents(root.path());
                    group.reclaim_payload(index, |_| Ok(())).unwrap();
                    assert_eq!(namespace_contents(root.path()), after);
                }
                let after = namespace_contents(root.path());
                assert!(stale.reclaim_payload(0, |_| Ok(())).is_err());
                assert_eq!(namespace_contents(root.path()), after);
                assert_eq!(
                    std::fs::read(destination.join(if directory { "copy/next" } else { "copy" }))
                        .unwrap(),
                    b"second publication"
                );
                let parent = owner(&destination);
                let central = owner(&root.path().join("central"));
                for member in &group.members {
                    let Member::Completion(completion) = member else {
                        unreachable!()
                    };
                    // Both completion proofs survived the other member's exact
                    // control/root deltas. Finish one only after checking both.
                    completion
                        .validate_in_group(&parent, &central, &group.selection, |_| Ok(()))
                        .unwrap();
                }
                let Member::Completion(completion) = &group.members[0] else {
                    unreachable!()
                };
                completion
                    .begin(&parent, &central, |_| Ok(()))
                    .unwrap()
                    .finish(&parent, &central, |_| Ok(()))
                    .unwrap();
                assert!(
                    !root
                        .path()
                        .join("central")
                        .join(headers[0].central_name())
                        .exists()
                );
                assert_eq!(
                    std::fs::read(destination.join(if directory { "copy/next" } else { "copy" }))
                        .unwrap(),
                    b"second publication"
                );
            }
        }
    }
}

#[test]
fn windows_native_copy_cleanup_group_payload_preserves_shared_and_foreign_changes() {
    use super::super::cleanup_group::Observation;
    for change in 0..4 {
        let root = tempfile::tempdir().unwrap();
        let names = cleanup_peer_fixture(root.path(), change == 0);
        let headers = ["destination", "second-destination"]
            .iter()
            .zip(&names)
            .map(|(directory, name)| {
                Snapshot::load(
                    &owner(&root.path().join(directory)),
                    &owner(&root.path().join("central")),
                    name,
                    SnapshotAccess::Read,
                )
                .unwrap()
                .state
                .header
                .unwrap()
            })
            .collect::<Vec<_>>();
        let ids = headers
            .iter()
            .map(|header| header.transaction.clone())
            .collect::<Vec<_>>();
        let mut group =
            Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(())).unwrap();
        group.start_all(|_| Ok(())).unwrap();
        match change {
            0 | 3 => {}
            1 => std::fs::write(
                root.path()
                    .join("second-destination")
                    .join(headers[1].original_name())
                    .join("old"),
                b"foreign changed retained data",
            )
            .unwrap(),
            2 => std::fs::write(
                root.path().join("central/foreign.txt"),
                b"unrelated new occupant",
            )
            .unwrap(),
            _ => unreachable!(),
        }
        let before = namespace_contents(root.path());
        assert!(
            group
                .reclaim_payload(0, |_| if change == 3 {
                    Err(io("owned cancellation"))
                } else {
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(namespace_contents(root.path()), before);
    }
}

#[test]
fn windows_native_copy_cleanup_group_payload_uncertainty_requires_fresh_group_admission() {
    use super::super::cleanup_group::{Member, Observation};
    for step in [Step::CleanupDispositionSet, Step::CleanupFlushed] {
        let root = tempfile::tempdir().unwrap();
        let names = cleanup_chain_fixture(root.path(), false, true);
        let headers = names
            .iter()
            .map(|name| original_journal(root.path(), name).header.clone())
            .collect::<Vec<_>>();
        let ids = headers
            .iter()
            .map(|header| header.transaction.clone())
            .collect::<Vec<_>>();
        let mut group =
            Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(())).unwrap();
        group.reclaim_payload(1, |_| Ok(())).unwrap();
        assert!(matches!(group.members[1], Member::Completion(_)));
        let peer_bytes =
            std::fs::read(root.path().join("central").join(headers[1].central_name())).unwrap();
        let sequence = original_journal(root.path(), &names[0]).sequence + 1;
        FAULT.with(|slot| slot.set(Some((sequence, step, false))));
        let result = group.reclaim_payload(0, |_| Ok(()));
        FAULT.with(|slot| slot.set(None));
        assert!(matches!(result, Err(PlatformError::CleanupPending { .. })));
        assert!(matches!(group.members[1], Member::Completion(_)));
        assert_eq!(
            std::fs::read(root.path().join("central").join(headers[1].central_name())).unwrap(),
            peer_bytes
        );
        let after = namespace_contents(root.path());
        assert!(group.reclaim_payload(0, |_| Ok(())).is_err());
        assert_eq!(namespace_contents(root.path()), after);
        let mut recovered =
            Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(())).unwrap();
        for index in [0, 1] {
            recovered.reclaim_payload(index, |_| Ok(())).unwrap();
        }
        assert!(
            recovered
                .members
                .iter()
                .all(|member| matches!(member, Member::Completion(_)))
        );
        assert_eq!(
            std::fs::read(root.path().join("destination/copy")).unwrap(),
            b"second publication"
        );
    }
}

fn observed_chain_group(
    root: &Path,
    registry_destination: bool,
    directory: bool,
    overwrite: bool,
) -> (super::super::cleanup_group::Observation, Vec<Header>) {
    let names = cleanup_chain_at(root, directory, overwrite, registry_destination);
    let headers = {
        let parent = owner(&root.join(if registry_destination {
            "central"
        } else {
            "destination"
        }));
        let central = owner(&root.join("central"));
        names
            .iter()
            .map(|name| {
                Snapshot::load(&parent, &central, name, SnapshotAccess::Read)
                    .unwrap()
                    .state
                    .header
                    .unwrap()
            })
            .collect::<Vec<_>>()
    };
    let ids = headers
        .iter()
        .map(|header| header.transaction.clone())
        .collect::<Vec<_>>();
    (
        super::super::cleanup_group::Observation::observe_at(&root.join("central"), &ids, |_| {
            Ok(())
        })
        .unwrap(),
        headers,
    )
}

#[test]
fn windows_native_copy_cleanup_group_metadata_finishes_members_with_bound_catalog_handoffs() {
    use super::super::cleanup_group::{Member, Observation};
    for registry_destination in [false, true] {
        for directory in [false, true] {
            for overwrite in [false, true] {
                for order in [[0, 1], [1, 0]] {
                    let root = tempfile::tempdir().unwrap();
                    let (mut group, headers) = observed_chain_group(
                        root.path(),
                        registry_destination,
                        directory,
                        overwrite,
                    );
                    for index in [1, 0] {
                        group.reclaim_payload(index, |_| Ok(())).unwrap();
                    }
                    for index in order {
                        group.finish_metadata(index, |_| Ok(())).unwrap();
                        assert!(matches!(group.members[index], Member::Complete(_)));
                        let destination = root.path().join(if registry_destination {
                            "central"
                        } else {
                            "destination"
                        });
                        assert!(!destination.join(headers[index].stage_name()).exists());
                        assert!(
                            !root
                                .path()
                                .join("central")
                                .join(headers[index].central_name())
                                .exists()
                        );
                        let after = namespace_contents(root.path());
                        group.finish_metadata(index, |_| Ok(())).unwrap();
                        assert_eq!(namespace_contents(root.path()), after);
                    }
                    assert!(
                        group
                            .members
                            .iter()
                            .all(|member| matches!(member, Member::Complete(_)))
                    );
                    let after = namespace_contents(root.path());
                    group.start_all(|_| Ok(())).unwrap();
                    for index in 0..2 {
                        group.reclaim_payload(index, |_| Ok(())).unwrap();
                    }
                    assert_eq!(namespace_contents(root.path()), after);
                    let ids = headers
                        .iter()
                        .map(|header| header.transaction.clone())
                        .collect::<Vec<_>>();
                    assert!(
                        Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(()))
                            .is_err()
                    );
                    assert_eq!(
                        std::fs::read(
                            root.path()
                                .join(if registry_destination {
                                    "central"
                                } else {
                                    "destination"
                                })
                                .join(if directory { "copy/next" } else { "copy" })
                        )
                        .unwrap(),
                        b"second publication"
                    );
                }
            }
        }
    }
}

#[test]
fn windows_native_copy_cleanup_group_metadata_retains_authority_through_uncertain_removal() {
    use super::super::cleanup_group::{Member, Observation};
    for registry_destination in [false, true] {
        for peer_finished in [false, true] {
            for role in [
                CleanupMetadataRole::Local,
                CleanupMetadataRole::Stage,
                CleanupMetadataRole::Central,
            ] {
                for point in [
                    CleanupMetadataPoint::DispositionSet,
                    CleanupMetadataPoint::HandleClosed,
                    CleanupMetadataPoint::Flushed,
                ] {
                    let root = tempfile::tempdir().unwrap();
                    let (mut group, headers) =
                        observed_chain_group(root.path(), registry_destination, false, true);
                    for index in [1, 0] {
                        group.reclaim_payload(index, |_| Ok(())).unwrap();
                    }
                    if peer_finished {
                        group.finish_metadata(1, |_| Ok(())).unwrap();
                    }
                    let destination = root.path().join(if registry_destination {
                        "central"
                    } else {
                        "destination"
                    });
                    let sequence = Snapshot::load(
                        &owner(&destination),
                        &owner(&root.path().join("central")),
                        &headers[0].stage_name(),
                        SnapshotAccess::Read,
                    )
                    .unwrap()
                    .sequence
                        + 1;
                    FAULT.with(|slot| {
                        slot.set(Some((
                            sequence,
                            Step::CleanupMetadata { role, point },
                            false,
                        )))
                    });
                    let result = group.finish_metadata(0, |_| Ok(()));
                    FAULT.with(|slot| slot.set(None));
                    assert!(
                        matches!(result, Err(PlatformError::CleanupPending { .. })),
                        "unexpected result for {registry_destination}/{peer_finished}/{role:?}/{point:?}: {result:?}"
                    );
                    assert!(matches!(group.members[0], Member::Metadata(_)));
                    let after = namespace_contents(root.path());
                    assert!(group.start_all(|_| Ok(())).is_err());
                    assert!(group.finish_metadata(1, |_| Ok(())).is_err());
                    assert_eq!(namespace_contents(root.path()), after);
                    if !root
                        .path()
                        .join("central")
                        .join(headers[0].central_name())
                        .exists()
                    {
                        let ids = headers
                            .iter()
                            .map(|header| header.transaction.clone())
                            .collect::<Vec<_>>();
                        assert!(
                            Observation::observe_at(&root.path().join("central"), &ids, |_| Ok(()))
                                .is_err()
                        );
                    }
                    group.finish_metadata(0, |_| Ok(())).unwrap();
                    if !peer_finished {
                        group.finish_metadata(1, |_| Ok(())).unwrap();
                    }
                    assert!(
                        group
                            .members
                            .iter()
                            .all(|member| matches!(member, Member::Complete(_)))
                    );
                    assert_eq!(
                        std::fs::read(destination.join("copy")).unwrap(),
                        b"second publication"
                    );
                }
            }
        }
    }
}

#[test]
fn windows_native_copy_cleanup_group_metadata_preserves_foreign_changes_while_pending() {
    for change in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let (mut group, headers) = observed_chain_group(root.path(), true, false, true);
        let before = namespace_contents(root.path());
        assert!(group.finish_metadata(0, |_| Ok(())).is_err());
        assert_eq!(namespace_contents(root.path()), before);
        for index in [1, 0] {
            group.reclaim_payload(index, |_| Ok(())).unwrap();
        }
        let sequence = Snapshot::load(
            &owner(&root.path().join("central")),
            &owner(&root.path().join("central")),
            &headers[0].stage_name(),
            SnapshotAccess::Read,
        )
        .unwrap()
        .sequence
            + 1;
        FAULT.with(|slot| {
            slot.set(Some((
                sequence,
                Step::CleanupMetadata {
                    role: CleanupMetadataRole::Local,
                    point: CleanupMetadataPoint::Flushed,
                },
                false,
            )))
        });
        let result = group.finish_metadata(0, |_| Ok(()));
        FAULT.with(|slot| slot.set(None));
        assert!(matches!(result, Err(PlatformError::CleanupPending { .. })));
        match change {
            0 => std::fs::write(
                root.path()
                    .join("central")
                    .join(headers[0].stage_name())
                    .join("unknown"),
                b"preserve unknown metadata-stage data",
            )
            .unwrap(),
            1 => {
                owner(&root.path().join("central").join(headers[1].central_name()))
                    .set_times(std::fs::FileTimes::new().set_modified(
                        std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
                    ))
                    .unwrap();
            }
            2 => std::fs::write(
                root.path().join("central/foreign.txt"),
                b"preserve unrelated data",
            )
            .unwrap(),
            _ => unreachable!(),
        }
        let before = namespace_contents(root.path());
        assert!(group.finish_metadata(0, |_| Ok(())).is_err());
        assert!(group.finish_metadata(1, |_| Ok(())).is_err());
        assert_eq!(namespace_contents(root.path()), before);
        assert_eq!(
            std::fs::read(root.path().join("central/copy")).unwrap(),
            b"second publication"
        );
    }
}

fn peer_unlink_codec_fixture(
    root: &Path,
) -> (
    Journal,
    super::super::cleanup_protocol::PeerUnlink,
    std::path::PathBuf,
) {
    let names = cleanup_peer_fixture_with_aliases(root, true, true);
    let headers = ["destination", "second-destination"]
        .iter()
        .zip(&names)
        .map(|(directory, name)| {
            Snapshot::load(
                &owner(&root.join(directory)),
                &owner(&root.join("central")),
                name,
                SnapshotAccess::Read,
            )
            .unwrap()
            .state
            .header
            .unwrap()
        })
        .collect::<Vec<_>>();
    let ids = headers
        .iter()
        .map(|header| header.transaction.clone())
        .collect::<Vec<_>>();
    super::super::cleanup_group::Observation::observe_at(&root.join("central"), &ids, |_| Ok(()))
        .unwrap()
        .start_all(|_| Ok(()))
        .unwrap();
    let mut peer = original_journal(root, &names[0]);
    let progress = peer.state.cleanup.as_ref().unwrap();
    let peer_index = progress.manifest.order[progress.completed];
    let parent = progress
        .expected(progress.manifest.entries[peer_index].parent.unwrap())
        .identity;
    let name = progress.manifest.entries[peer_index].name.clone();
    peer.persist(Record::Cleanup {
        entry: super::super::cleanup_protocol::CleanupRecord::EntryIntent { index: peer_index },
    })
    .unwrap();
    let bytes = read(&peer.central).unwrap();
    let sequence = peer.sequence;
    drop(peer);
    let parent_b = owner(&root.join("second-destination"));
    let central = owner(&root.join("central"));
    let snapshot = Snapshot::load(&parent_b, &central, &names[1], SnapshotAccess::Write).unwrap();
    let journal = Journal::from_snapshot(&parent_b, &central, snapshot).unwrap();
    let index = journal
        .state
        .cleanup
        .as_ref()
        .unwrap()
        .manifest
        .entries
        .iter()
        .position(|entry| !entry.version.directory && entry.links > 1)
        .unwrap();
    let proof = super::super::cleanup_protocol::PeerUnlink {
        index,
        transaction: headers[0].transaction.clone(),
        control: headers[0].central_file,
        parent,
        name,
        peer_index,
        sequence,
        intent_digest: Sha256::digest(bytes).into(),
    };
    (
        journal,
        proof,
        root.join("destination")
            .join(headers[0].original_name())
            .join("old"),
    )
}

#[test]
fn windows_native_copy_cleanup_peer_unlink_codec_replays_one_external_alias_acknowledgement() {
    use super::super::cleanup_protocol::CleanupRecord;
    let root = tempfile::tempdir().unwrap();
    let (mut journal, proof, removed) = peer_unlink_codec_fixture(root.path());
    let key = proof.key().unwrap();
    journal.prepare_peer_unlink(proof.clone()).unwrap();
    let intent_bytes = read(&journal.local).unwrap();
    journal.prepare_peer_unlink(proof.clone()).unwrap();
    assert_eq!(read(&journal.local).unwrap(), intent_bytes);
    let own_index = journal.state.cleanup.as_ref().unwrap().manifest.order[0];
    assert!(
        journal
            .persist(Record::Cleanup {
                entry: CleanupRecord::EntryIntent { index: own_index }
            })
            .is_err()
    );
    let before = journal
        .state
        .cleanup
        .as_ref()
        .unwrap()
        .expected(proof.index)
        .clone();
    for field in 0..4 {
        let mut after = before.clone();
        let mut candidate_key = key;
        match field {
            0 => candidate_key[0] ^= 1,
            1 => after.length += 1,
            2 => after.modified += 1,
            3 => after.attributes ^= 2,
            _ => unreachable!(),
        }
        assert!(
            journal
                .persist(Record::Cleanup {
                    entry: CleanupRecord::PeerUnlinkRemoved {
                        key: candidate_key,
                        after: Box::new(after)
                    }
                })
                .is_err()
        );
        assert_eq!(read(&journal.local).unwrap(), intent_bytes);
    }
    // Test-only native unlink represents the peer's pending operation. Runtime
    // peer-proof validation/removal is intentionally not implemented by this codec.
    std::fs::remove_file(&removed).unwrap();
    owner(removed.parent().unwrap()).sync_all().unwrap();
    let retained = root
        .path()
        .join("second-destination")
        .join(journal.header.original_name())
        .join("old");
    let file = owner(&retained);
    let after = super::super::super::version(&file).unwrap();
    assert_eq!(
        super::super::super::namespace::link_count(&file).unwrap(),
        3
    );
    journal
        .persist(Record::Cleanup {
            entry: CleanupRecord::PeerUnlinkRemoved {
                key,
                after: Box::new(after.clone()),
            },
        })
        .unwrap();
    let progress = journal.state.cleanup.as_ref().unwrap();
    assert!(progress.peer_unlink.is_none());
    for &alias in progress.manifest.same_object(proof.index) {
        assert_eq!(progress.expected_links(alias).unwrap(), 3);
        assert_eq!(progress.expected(alias), &after);
    }
    drop(file);
    super::super::cleanup_tree::Remaining::open(
        &owner(&root.path().join("second-destination")),
        &journal.state,
        progress,
        |_| Ok(()),
    )
    .unwrap();
    let bytes = read(&journal.local).unwrap();
    let parent = owner(&root.path().join("second-destination"));
    let central = owner(&root.path().join("central"));
    let name = journal.header.stage_name();
    drop(journal);
    let snapshot = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).unwrap();
    assert_eq!(snapshot.bytes, bytes);
    assert_eq!(
        snapshot
            .state
            .cleanup
            .as_ref()
            .unwrap()
            .expected_links(proof.index)
            .unwrap(),
        3
    );
    let mut duplicate = proof.clone();
    duplicate.sequence += 1;
    duplicate.intent_digest = [42; 32];
    duplicate.index = own_index;
    assert_eq!(duplicate.key().unwrap(), key);
    assert!(
        snapshot
            .state
            .check(&Record::Cleanup {
                entry: CleanupRecord::PeerUnlinkIntent {
                    proof: Box::new(duplicate)
                }
            })
            .is_err()
    );
    assert_eq!(
        std::fs::read(root.path().join("outside")).unwrap(),
        b"old original data"
    );
    assert_eq!(std::fs::read(retained).unwrap(), b"old original data");
}

#[test]
fn windows_native_copy_cleanup_peer_unlink_codec_rejects_unbound_and_out_of_order_records() {
    use super::super::cleanup_protocol::CleanupRecord;
    let root = tempfile::tempdir().unwrap();
    let (mut journal, proof, _) = peer_unlink_codec_fixture(root.path());
    let before = read(&journal.local).unwrap();
    let version = journal
        .state
        .cleanup
        .as_ref()
        .unwrap()
        .expected(proof.index)
        .clone();
    assert!(
        journal
            .persist(Record::Cleanup {
                entry: CleanupRecord::PeerUnlinkRemoved {
                    key: proof.key().unwrap(),
                    after: Box::new(version)
                }
            })
            .is_err()
    );
    for change in 0..10 {
        let mut candidate = proof.clone();
        match change {
            0 => candidate.transaction = journal.header.transaction.clone(),
            1 => candidate.transaction = "A".repeat(32),
            2 => candidate.control = journal.header.local_file,
            3 => candidate.parent = journal.header.central_file,
            4 => candidate.name = "../escape".encode_utf16().collect(),
            5 => candidate.index = usize::MAX,
            6 => candidate.sequence = 0,
            7 => candidate.intent_digest = [0; 32],
            8 => {
                let progress = journal.state.cleanup.as_ref().unwrap();
                let entry = &progress.manifest.entries[candidate.index];
                candidate.parent = progress.expected(entry.parent.unwrap()).identity;
                candidate.name = entry.name.clone();
            }
            9 => candidate.index = 0,
            _ => unreachable!(),
        }
        assert!(
            journal.prepare_peer_unlink(candidate).is_err(),
            "accepted change {change}"
        );
        assert_eq!(read(&journal.local).unwrap(), before);
    }
    let mut header = journal.header.clone();
    header.schema = 12;
    assert!(!header.valid());
    let mut old_magic = before.clone();
    old_magic[..8].copy_from_slice(b"OTCPWJ12");
    assert!(decode(&old_magic).is_err());
}

#[test]
fn windows_native_copy_cleanup_peer_unlink_codec_reserves_acknowledgement_before_intent() {
    use super::super::cleanup_protocol::CleanupRecord;
    let root = tempfile::tempdir().unwrap();
    let (mut journal, proof, _) = peer_unlink_codec_fixture(root.path());
    let before = read(&journal.local).unwrap();
    let intent = Record::Cleanup {
        entry: CleanupRecord::PeerUnlinkIntent {
            proof: Box::new(proof.clone()),
        },
    };
    let mut after = journal
        .state
        .cleanup
        .as_ref()
        .unwrap()
        .expected(proof.index)
        .clone();
    after.changed = i64::MIN;
    let ack = Record::Cleanup {
        entry: CleanupRecord::PeerUnlinkRemoved {
            key: [255; 32],
            after: Box::new(after),
        },
    };
    let required = journal.length
        + journal.cleanup_tail_size().unwrap()
        + 2 * FRAME_HEADER as u64
        + record_payload(&intent).unwrap().len() as u64
        + record_payload(&ack).unwrap().len() as u64;
    let previous = LOG_BUDGET.with(|limit| limit.replace(required - 1));
    let refused = journal.prepare_peer_unlink(proof.clone());
    LOG_BUDGET.with(|limit| limit.set(previous));
    assert!(refused.is_err());
    assert_eq!(read(&journal.local).unwrap(), before);
    let previous = LOG_BUDGET.with(|limit| limit.replace(required));
    let accepted = journal.prepare_peer_unlink(proof);
    let tail = journal.reserve_cleanup_tail();
    LOG_BUDGET.with(|limit| limit.set(previous));
    accepted.unwrap();
    tail.unwrap();
    assert_eq!(
        journal.length + journal.cleanup_tail_size().unwrap(),
        required
    );
}
