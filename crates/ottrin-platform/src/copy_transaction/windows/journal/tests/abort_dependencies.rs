//! Ended staging freezes its recorded identity claims without adopting payloads.
use super::cleanup::namespace_contents;
use super::*;
use std::path::PathBuf;

struct Fixture {
    registry: PathBuf,
    parent: PathBuf,
    own_id: String,
    peer_root: PathBuf,
    peer: Header,
    _readonly: ReadonlyCleanup,
}
impl Fixture {
    fn new(root: &Path, overwrite_peer: bool) -> Self {
        let registry = root.join("profile/Ottrin/State/copies");
        let complete = root.join("complete");
        std::fs::create_dir(&complete).unwrap();
        let parent = complete.join("destination");
        let (name, _, readonly) =
            completed_undo_fixture_at(&complete, &parent, true, false, Some(&registry));
        undo_completed_copy(&owner(&parent), &owner(&registry), &name, |_| Ok(())).unwrap();
        let own_id = decode(&std::fs::read(parent.join(&name).join(LOCAL)).unwrap())
            .unwrap()
            .state
            .header
            .unwrap()
            .transaction;
        remove_undo_fixture_source(&complete);
        let peer_root = root.join("partial");
        std::fs::create_dir(&peer_root).unwrap();
        let destination = peer_root.join("destination");
        super::abort::partial_stage_at(&peer_root, &registry, &destination, overwrite_peer);
        let peer =
            decode(&std::fs::read(destination.join(stage_name(&destination)).join(LOCAL)).unwrap())
                .unwrap()
                .state
                .header
                .unwrap();
        std::fs::write(parent.join("item"), b"unrelated original").unwrap();
        Self {
            registry,
            parent,
            own_id,
            peer_root,
            peer,
            _readonly: readonly,
        }
    }
    fn end(&self) {
        crate::restore_retained_copy_abort_plan_at(&self.registry, &self.peer.transaction)
            .unwrap()
            .end_attempt(|_| Ok(()))
            .unwrap();
    }
    fn guard(&self, name: &str) -> Result<Vec<File>, PlatformError> {
        let parent = owner(&self.parent);
        DestinationAdmission::overwrite(&parent, OsStr::new(name), 100, |_| Ok(()))?
            .guard_overwrite_dependencies(&parent, &owner(&self.registry), |_| Ok(()))
    }
    fn observe_cleanup(&self) -> Result<RetainedCopyCleanupPlan, PlatformError> {
        RetainedCopyCleanupPlan::observe_at(
            &self.registry,
            std::slice::from_ref(&self.own_id),
            |_| Ok(()),
        )
    }
    fn payload(&self) -> PathBuf {
        self.peer_root
            .join("destination")
            .join(self.peer.candidate_name())
    }
    fn assert_partial_data(&self) {
        assert_eq!(
            std::fs::read(self.payload().join("nested/file")).unwrap(),
            b"later retained edits"
        );
        assert_eq!(
            std::fs::read(self.payload().join("nested/file:keep")).unwrap(),
            b"later named stream"
        );
        assert_eq!(
            std::fs::read(self.payload().join("unrecognized")).unwrap(),
            b"unexpected retained file"
        );
        assert_eq!(
            std::fs::read(self.peer_root.join("destination/foreign")).unwrap(),
            b"unrelated public data"
        );
        assert!(!self.peer_root.join("source").exists());
    }
}

#[test]
fn windows_native_copy_ended_attempt_allows_unrelated_overwrite_and_cleanup() {
    for overwrite_peer in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let fixture = Fixture::new(root.path(), overwrite_peer);
        let before = namespace_contents(root.path());
        assert!(
            fixture.guard("item").is_err(),
            "An active incomplete graph still blocks mutation"
        );
        assert!(fixture.observe_cleanup().is_err());
        assert_eq!(namespace_contents(root.path()), before);
        fixture.end();
        let preserved = namespace_contents(&fixture.peer_root);
        let journal = fixture.registry.join(fixture.peer.central_name());
        let control = std::fs::read(&journal).unwrap();
        let input = root.path().join("item");
        std::fs::write(&input, b"replacement data").unwrap();
        let retained_root = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(fixture.payload())
            .unwrap();
        let copied = super::super::super::dispatch::copy_at(
            &input,
            &fixture.parent,
            crate::ConflictAction::Overwrite,
            &owner(&fixture.registry),
            100,
            |_| Ok(()),
        );
        assert!(
            copied.is_ok(),
            "Ended unrelated attempt must not block Overwrite: {copied:?}"
        );
        assert_eq!(
            std::fs::read(fixture.parent.join("item")).unwrap(),
            b"replacement data"
        );
        assert_eq!(std::fs::read(&input).unwrap(), b"replacement data");
        let mut cleanup = fixture.observe_cleanup().unwrap();
        cleanup.reclaim(|_| Ok(())).unwrap();
        assert!(cleanup.complete());
        drop(retained_root);
        assert!(
            !fixture
                .registry
                .join(format!("{}.copy-journal", fixture.own_id))
                .exists()
        );
        assert_eq!(namespace_contents(&fixture.peer_root), preserved);
        assert_eq!(std::fs::read(journal).unwrap(), control);
        fixture.assert_partial_data();
    }
}

#[test]
fn windows_native_copy_ended_attempt_still_protects_recorded_created_aliases() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(root.path(), true);
    fixture.end();
    let alias = fixture.parent.join("alias");
    std::fs::hard_link(fixture.payload().join("nested/file"), &alias).unwrap();
    let before = namespace_contents(root.path());
    let refusal = fixture.guard("alias").err().unwrap().to_string();
    assert!(refusal.contains("retained Copy transaction"), "{refusal}");
    assert_eq!(namespace_contents(root.path()), before);
    // Construct an already completed overwrite without dispatch admission. This
    // is a recovery fixture: its retained original genuinely aliases the older
    // partial file, so explicit cleanup must also keep that dependency intact.
    let input = root.path().join("replacement");
    std::fs::write(&input, b"new publication").unwrap();
    let id = {
        let parent = owner(&fixture.parent);
        let central = owner(&fixture.registry);
        let admission =
            DestinationAdmission::overwrite(&parent, OsStr::new("alias"), 100, |_| Ok(())).unwrap();
        let receipt = DurableStage::execute_with_receipt(
            &source(&input),
            &parent,
            &central,
            &admission,
            OsStr::new("replacement"),
            100,
            |_| Ok(()),
        )
        .unwrap();
        receipt.windows_copy.as_ref().unwrap().transaction_ids()[0].to_owned()
    };
    let before = namespace_contents(root.path());
    let refusal = RetainedCopyCleanupPlan::observe_at(&fixture.registry, &[id], |_| Ok(()))
        .err()
        .unwrap()
        .to_string();
    assert!(refusal.contains("dependent transaction"), "{refusal}");
    assert_eq!(namespace_contents(root.path()), before);
    fixture.assert_partial_data();
}

#[test]
fn windows_native_copy_ended_dependency_requires_immutable_matching_journal_pair() {
    for changed_local in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let fixture = Fixture::new(root.path(), false);
        let peer_parent = fixture.peer_root.join("destination");
        let local = peer_parent.join(fixture.peer.stage_name()).join(LOCAL);
        let before_end = std::fs::read(&local).unwrap();
        fixture.end();
        let mut stale_cleanup = changed_local.then(|| fixture.observe_cleanup().unwrap());
        if changed_local {
            // A valid but nonterminal local prefix cannot agree with the
            // terminal central control; checking that control alone is unsafe.
            std::fs::write(&local, &before_end).unwrap();
        }
        let held = if changed_local {
            None
        } else {
            Some(
                Snapshot::load(
                    &owner(&peer_parent),
                    &owner(&fixture.registry),
                    &fixture.peer.stage_name(),
                    SnapshotAccess::Write,
                )
                .unwrap(),
            )
        };
        let before = namespace_contents(root.path());
        assert!(fixture.guard("item").is_err());
        assert!(fixture.observe_cleanup().is_err());
        if let Some(plan) = &mut stale_cleanup {
            assert!(
                plan.reclaim(|_| Ok(())).is_err(),
                "Writer handoff must recheck the peer's local mirror"
            );
        }
        assert_eq!(namespace_contents(root.path()), before);
        drop(held);
        if !changed_local {
            let writable_local = || {
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .share_mode(FILE_SHARE_READ)
                    .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                    .open(&local)
            };
            let leases = fixture.guard("item").unwrap();
            assert!(
                writable_local().is_err(),
                "Overwrite must lease the local peer mirror too"
            );
            drop(leases);
            assert!(writable_local().is_ok());
            {
                let parent = owner(&fixture.parent);
                let central = owner(&fixture.registry);
                let own_name = OsString::from(format!(".ottrin-copy-{}", fixture.own_id));
                let snapshot =
                    Snapshot::load(&parent, &central, &own_name, SnapshotAccess::Read).unwrap();
                let manifest = snapshot.state.cleanup_manifest().unwrap();
                let dependencies = super::super::cleanup_dependencies::Dependencies::open_selected(
                    &central,
                    &snapshot.state,
                    &manifest,
                    None,
                    |_| Ok(()),
                )
                .unwrap();
                assert!(
                    writable_local().is_err(),
                    "Cleanup must lease the local peer mirror too"
                );
                drop(dependencies);
                assert!(writable_local().is_ok());
            }
            assert!(fixture.observe_cleanup().is_ok());
        }
        fixture.assert_partial_data();
    }
}
