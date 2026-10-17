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
            assert!(matches!(
                statuses[0].evidence,
                WindowsCopyRecoveryEvidence::NeedsReview { .. }
            ));
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

#[test]
fn windows_native_copy_cleanup_registry_dependencies_preserve_peers_and_shared_originals() {
    for shared in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let first = root.path().join("destination");
        let second = root.path().join("second-destination");
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
        // Both original graphs exist before either retention rename; native
        // child epochs therefore remain exact in the shared-hardlink case.
        let input = source(&root.path().join("source"));
        let central = owner(&root.path().join("central"));
        let mut names = Vec::new();
        for destination in [&first, &second] {
            let parent = owner(destination);
            let admission =
                DestinationAdmission::overwrite(&parent, OsStr::new("copy"), 100, |_| Ok(()))
                    .unwrap();
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
        remove_undo_fixture_source(root.path());
        let peer_local = second.join(&names[1]).join(LOCAL);
        let peer_bytes = std::fs::read(&peer_local).unwrap();
        let peer_header = decode(&peer_bytes).unwrap().state.header.unwrap();
        let before = namespace_contents(root.path());
        let parent = owner(&first);
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
                    drop(snapshot);
                    let before = namespace_contents(root.path());
                    let plan =
                        super::super::cleanup_finalize::prepare(&parent, &central, &name, |_| {
                            Ok(())
                        })
                        .unwrap();
                    plan.finish(&parent, &central, |_| Ok(())).unwrap();
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
