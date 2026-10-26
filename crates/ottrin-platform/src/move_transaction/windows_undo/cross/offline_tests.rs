//! Three fresh ordinary-user processes surround a controller-owned VHD detach.
//! The fixture intentionally lives outside each actor's disposable TEMP; the
//! final phase checks ownership and removes it after the controller reattaches.
use super::*;
use crate::{ConflictAction, DefaultPlatform, PlatformOps};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MARKER: &str = ".ottrin-completion-offline-owner";

struct OwnedDirectory {
    path: PathBuf,
    identity: WindowsStableIdentity,
    retained: bool,
}
impl OwnedDirectory {
    fn create(path: &Path) -> Self {
        std::fs::create_dir(path).unwrap();
        Self {
            path: path.to_path_buf(),
            identity: move_recovery::stable_identity_of_entry(path).unwrap(),
            retained: false,
        }
    }
}
impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        if self.retained {
            return;
        }
        let result = move_recovery::stable_identity_of_entry(&self.path)
            .map_err(|e| e.to_string())
            .and_then(|id| {
                if id == self.identity {
                    std::fs::remove_dir_all(&self.path).map_err(|e| e.to_string())
                } else {
                    Err("Owned test directory identity changed".into())
                }
            });
        if let Err(error) = result {
            eprintln!(
                "Persistent fixture cleanup failed at {}: {error}",
                self.path.display()
            );
            assert!(
                std::thread::panicking(),
                "Owned fixture cleanup failed: {error}"
            );
        }
    }
}

#[derive(Serialize, Deserialize)]
enum Saved {
    Same(Box<super::super::super::WindowsOverwriteRecord>),
    Cross {
        origin: Box<Origin>,
        initial: Box<Record>,
    },
}
impl Saved {
    fn capture(receipt: &PathMoveUndoReceipt) -> Self {
        match receipt.windows_undo.as_ref().unwrap() {
            WindowsPathUndo::Move(WindowsMoveUndoGuard::Replace(record)) => {
                Self::Same(record.clone())
            }
            WindowsPathUndo::Move(WindowsMoveUndoGuard::CrossVolume(guard)) => Self::Cross {
                origin: Box::new(guard.origin.clone()),
                initial: Box::new(guard.inverse.get().unwrap().clone()),
            },
            _ => panic!("expected a durable inverse"),
        }
    }
    fn receipt(&self, bind_inverse: bool) -> PathMoveUndoReceipt {
        match self {
            Self::Same(record) => PathMoveUndoReceipt {
                windows_undo: Some(WindowsPathUndo::Move(WindowsMoveUndoGuard::Replace(
                    record.clone(),
                ))),
                before_path: record.source_path().unwrap(),
                after_path: record.target_path().unwrap(),
                identity: PathMoveIdentity {
                    windows: record.source_identity,
                },
                displaced_identity: Some(PathMoveIdentity {
                    windows: record.retained_identity,
                }),
                retained_displaced_path: Some(record.retained_path().unwrap()),
                requirement: PathMoveUndoRequirement::None,
            },
            Self::Cross { origin, initial } => {
                let inverse = OnceLock::new();
                if bind_inverse {
                    inverse.set(initial.as_ref().clone()).unwrap();
                }
                PathMoveUndoReceipt {
                    windows_undo: Some(WindowsPathUndo::Move(WindowsMoveUndoGuard::CrossVolume(
                        Box::new(Guard {
                            origin: origin.as_ref().clone(),
                            inverse: Arc::new(inverse),
                        }),
                    ))),
                    before_path: path(&origin.original).unwrap(),
                    after_path: path(&origin.publication).unwrap(),
                    identity: PathMoveIdentity {
                        windows: origin.publication_identity,
                    },
                    displaced_identity: origin
                        .retained
                        .as_ref()
                        .map(|b| PathMoveIdentity { windows: b.entry }),
                    retained_displaced_path: origin
                        .retained
                        .as_ref()
                        .map(|b| b.retained_path().unwrap()),
                    requirement: PathMoveUndoRequirement::None,
                }
            }
        }
    }
    fn owners(&self) -> Vec<[u8; 16]> {
        match self {
            Self::Same(record) => vec![record.transaction_id().unwrap()],
            Self::Cross { origin, initial } => {
                let mut ids = vec![initial.transaction_id().unwrap()];
                if let Some(controller) = origin.completion_controller().unwrap() {
                    ids.push(controller);
                }
                ids
            }
        }
    }
    fn unknown_private_files(&self) -> Vec<PathBuf> {
        let directories = match self {
            Self::Same(record) => vec![record.transaction_path().unwrap()],
            Self::Cross { origin, initial } => vec![
                initial.transaction_path().unwrap(),
                origin
                    .retained
                    .as_ref()
                    .unwrap()
                    .retained_path()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .to_path_buf(),
            ],
        };
        directories
            .into_iter()
            .map(|p| p.join("unknown-user-file"))
            .collect()
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    path: move_recovery::NativePath,
    identity: WindowsStableIdentity,
    bytes: Option<Vec<u8>>,
    link: Option<move_recovery::NativePath>,
}
fn snapshot(root: &Path) -> Vec<Entry> {
    fn visit(path: &Path, entries: &mut Vec<Entry>) {
        let identity = move_recovery::stable_identity_of_entry(path).unwrap();
        entries.push(Entry {
            path: move_recovery::NativePath::encode(path),
            identity,
            bytes: identity
                .is_regular_file_without_reparse()
                .then(|| std::fs::read(path).unwrap()),
            link: identity
                .is_symbolic_link()
                .then(|| move_recovery::NativePath::encode(&std::fs::read_link(path).unwrap())),
        });
        if identity.is_directory_without_reparse() {
            let mut children = std::fs::read_dir(path)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect::<Vec<_>>();
            children.sort();
            for child in children {
                visit(&child, entries);
            }
        }
    }
    let mut entries = Vec::new();
    visit(root, &mut entries);
    entries
}

#[derive(Serialize, Deserialize)]
struct Fixture {
    retired: bool,
    primary: WindowsStableIdentity,
    secondary: move_recovery::NativePath,
    secondary_identity: WindowsStableIdentity,
    token: [u8; 16],
    saved: Vec<Saved>,
    primary_payload: Vec<Entry>,
    secondary_payload: Vec<Entry>,
    registry: Vec<Entry>,
}
fn root() -> PathBuf {
    assert_eq!(std::env::var("USERNAME").unwrap(), "OttrinCopyQA");
    let root = PathBuf::from(
        std::env::var_os("OTTRIN_TEST_WINDOWS_MOVE_OFFLINE_ROOT")
            .expect("owned three-stage controller required"),
    );
    assert!(root.is_absolute());
    root
}
struct RecoveryRoot(Option<PathBuf>);
impl RecoveryRoot {
    fn install(root: &Path) -> Self {
        Self(super::super::super::replace_test_recovery_root(Some(
            root.join("state"),
        )))
    }
}
impl Drop for RecoveryRoot {
    fn drop(&mut self) {
        super::super::super::replace_test_recovery_root(self.0.take());
    }
}
fn load(root: &Path) -> Fixture {
    let fixture: Fixture =
        serde_json::from_slice(&std::fs::read(root.join("fixture.json")).unwrap()).unwrap();
    assert_eq!(
        move_recovery::stable_identity_of_entry(root).unwrap(),
        fixture.primary
    );
    assert_eq!(std::fs::read(root.join(MARKER)).unwrap(), fixture.token);
    fixture
}
fn create(path: &Path, kind: u8, bytes: &[u8]) {
    match kind {
        0 => std::fs::write(path, bytes).unwrap(),
        1 => {
            std::fs::create_dir(path).unwrap();
            std::fs::write(path.join("child"), bytes).unwrap();
        }
        _ => std::os::windows::fs::symlink_file(std::str::from_utf8(bytes).unwrap(), path).unwrap(),
    }
}

#[test]
#[ignore = "owned persistent fixture; run prepare/detach/verify/attach/cleanup through the native controller"]
fn windows_native_move_completion_offline_prepare() {
    prepare(false);
}
#[test]
#[ignore = "owned persistent fixture; requires cleanup before detach and fresh verification"]
fn windows_native_move_completion_offline_prepare_retired() {
    prepare(true);
}
fn prepare(retired: bool) {
    let root = root();
    let mut primary_owner = OwnedDirectory::create(&root);
    let _recovery = RecoveryRoot::install(&root);
    let volume = PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
    let secondary = volume.join("completion-offline-payload");
    let mut secondary_owner = OwnedDirectory::create(&secondary);
    let mut token = [0; 16];
    getrandom::fill(&mut token).unwrap();
    std::fs::write(root.join(MARKER), token).unwrap();
    std::fs::write(secondary.join(MARKER), token).unwrap();
    std::fs::create_dir(root.join("payload")).unwrap();
    let mut saved = Vec::new();
    for cross in [true, false] {
        for kind in 0..3 {
            let label = format!("{}-{kind}", if cross { "cross" } else { "same" });
            let source_parent = if cross {
                root.join("payload").join(&label)
            } else {
                secondary.join(format!("source-{label}"))
            };
            let destination = secondary.join(format!("destination-{label}"));
            std::fs::create_dir(&source_parent).unwrap();
            std::fs::create_dir(&destination).unwrap();
            let source = source_parent.join("entry");
            create(&source, kind, b"selected-original");
            create(&destination.join("entry"), kind, b"displaced-original");
            let (_, receipt) = DefaultPlatform
                .execute_move_with_undo_receipt(&source, &destination, ConflictAction::Overwrite)
                .unwrap();
            let receipt = receipt.unwrap();
            // Keep both publication and retained edits, including named streams.
            if kind == 0 {
                std::fs::write(receipt.after_path(), b"edited-publication").unwrap();
                if !cross {
                    std::fs::write(
                        format!("{}:later-stream", receipt.after_path().display()),
                        b"publication-stream",
                    )
                    .unwrap();
                }
                std::fs::write(
                    format!(
                        "{}:later-stream",
                        receipt.retained_displaced_path.as_ref().unwrap().display()
                    ),
                    b"retained-stream",
                )
                .unwrap();
            }
            if kind == 1 {
                std::fs::write(receipt.after_path().join("added"), b"added-after-Move").unwrap();
            }
            DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
            let saved_receipt = Saved::capture(&receipt);
            for file in saved_receipt.unknown_private_files() {
                std::fs::write(file, b"unknown-private-data-must-survive").unwrap();
            }
            saved.push(saved_receipt);
        }
    }
    if retired {
        for receipt in &saved {
            let id = receipt.owners()[0];
            let plan = crate::WindowsMoveRecoveryPlan::capture_cleanup(id)
                .unwrap()
                .unwrap();
            let report = plan.execute_cleanup().unwrap();
            let expected = if matches!(receipt, Saved::Same(_)) {
                1
            } else {
                2
            };
            assert_eq!(report.journals_removed, expected * 2);
            assert_eq!(report.directories_kept, expected);
            assert_eq!(report.directories_removed, 0);
        }
    }
    let fixture = Fixture {
        retired,
        primary: move_recovery::stable_identity_of_entry(&root).unwrap(),
        secondary: move_recovery::NativePath::encode(&secondary),
        secondary_identity: move_recovery::stable_identity_of_entry(&secondary).unwrap(),
        token,
        saved,
        primary_payload: snapshot(&root.join("payload")),
        secondary_payload: snapshot(&secondary),
        registry: snapshot(&root.join("state")),
    };
    assert_eq!(
        super::super::super::discover_retained_windows_move_transactions()
            .unwrap()
            .len(),
        6
    );
    std::fs::write(
        root.join("fixture.json"),
        serde_json::to_vec(&fixture).unwrap(),
    )
    .unwrap();
    // The controller owns the persistent fixture only after its manifest exists.
    primary_owner.retained = true;
    secondary_owner.retained = true;
}

#[test]
#[ignore = "requires successful prepare and actual controller-owned VHD detach"]
fn windows_native_move_completion_offline_verify() {
    let root = root();
    let fixture = load(&root);
    let _recovery = RecoveryRoot::install(&root);
    let secondary = path(&fixture.secondary).unwrap();
    assert!(
        std::fs::symlink_metadata(
            secondary
                .ancestors()
                .find(|p| p.parent().is_none())
                .unwrap()
        )
        .is_err(),
        "secondary drive must actually be unavailable"
    );
    let found = super::super::super::discover_retained_windows_move_transactions().unwrap();
    assert_eq!(found.len(), 6);
    assert!(
        found
            .iter()
            .all(|r| r.undo_status == move_recovery::WindowsMoveUndoStatus::Complete)
    );
    for saved in &fixture.saved {
        for bind_inverse in [true, false] {
            let receipt = saved.receipt(bind_inverse);
            for id in saved.owners() {
                let plan = crate::WindowsMoveRecoveryPlan::capture(id).unwrap();
                assert_eq!(plan.cleanup_complete(), fixture.retired);
                if fixture.retired {
                    assert!(
                        crate::WindowsMoveRecoveryPlan::capture_cleanup(id)
                            .unwrap()
                            .is_none()
                    );
                }
                assert_eq!(plan.paths(), &[root.join("state")]);
                assert_eq!(
                    plan.undo_status_for(&receipt),
                    Some(move_recovery::WindowsMoveUndoStatus::Complete)
                );
                plan.execute().unwrap();
            }
            let native_undo = crate::WindowsMoveUndoPlan::capture(&receipt, &[]).unwrap();
            assert_eq!(
                native_undo.paths(),
                &[std::fs::canonicalize(root.join("state")).unwrap()]
            );
            native_undo.execute().unwrap();
            DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
            DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
        }
    }
    let source = root.join("unrelated-source");
    let destination = root.join("unrelated-destination");
    std::fs::write(&source, b"unrelated-new-Move").unwrap();
    std::fs::create_dir(&destination).unwrap();
    let (_, receipt) = DefaultPlatform
        .execute_move_with_undo_receipt(&source, &destination, ConflictAction::Skip)
        .unwrap();
    assert_eq!(
        std::fs::read(destination.join("unrelated-source")).unwrap(),
        b"unrelated-new-Move"
    );
    DefaultPlatform
        .undo_move_with_receipt(&receipt.unwrap())
        .unwrap();
    std::fs::remove_file(source).unwrap();
    std::fs::remove_dir(destination).unwrap();
    assert_eq!(snapshot(&root.join("state")), fixture.registry);
    assert_eq!(snapshot(&root.join("payload")), fixture.primary_payload);
    std::fs::write(root.join("verified"), fixture.token).unwrap();
}

#[test]
#[ignore = "requires controller reattachment; verifies and tears down the persistent owned fixture"]
fn windows_native_move_completion_offline_cleanup() {
    let root = root();
    let fixture = load(&root);
    let _recovery = RecoveryRoot::install(&root);
    let secondary = path(&fixture.secondary).unwrap();
    assert_eq!(
        move_recovery::stable_identity_of_entry(&secondary).unwrap(),
        fixture.secondary_identity
    );
    assert_eq!(
        std::fs::read(secondary.join(MARKER)).unwrap(),
        fixture.token
    );
    let verification = std::panic::catch_unwind(|| {
        assert_eq!(std::fs::read(root.join("verified")).unwrap(), fixture.token);
        assert_eq!(snapshot(&root.join("state")), fixture.registry);
        assert_eq!(snapshot(&root.join("payload")), fixture.primary_payload);
        assert_eq!(snapshot(&secondary), fixture.secondary_payload);
        for saved in &fixture.saved {
            for file in saved.unknown_private_files() {
                assert_eq!(
                    std::fs::read(file).unwrap(),
                    b"unknown-private-data-must-survive"
                );
            }
            let receipt = saved.receipt(true);
            if receipt.before_path().is_file()
                && std::fs::symlink_metadata(receipt.before_path())
                    .unwrap()
                    .file_type()
                    .is_file()
            {
                assert_eq!(
                    std::fs::read(receipt.before_path()).unwrap(),
                    b"edited-publication"
                );
                assert_eq!(
                    std::fs::read(format!("{}:later-stream", receipt.after_path().display()))
                        .unwrap(),
                    b"retained-stream"
                );
                if matches!(saved, Saved::Same(_)) {
                    assert_eq!(
                        std::fs::read(format!("{}:later-stream", receipt.before_path().display()))
                            .unwrap(),
                        b"publication-stream"
                    );
                }
            }
        }
    });
    std::fs::remove_dir_all(secondary).unwrap();
    std::fs::remove_dir_all(root).unwrap();
    if let Err(error) = verification {
        std::panic::resume_unwind(error);
    }
}

#[test]
#[ignore = "owned native controller checks persistent-fixture teardown on both failure paths"]
fn windows_native_move_completion_offline_failure_teardown() {
    let root = root();
    let secondary = PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap())
        .join("completion-offline-payload");
    assert!(!root.exists() && !secondary.exists());
    let failed_preparation = std::panic::catch_unwind(|| {
        let _primary = OwnedDirectory::create(&root);
        let _secondary = OwnedDirectory::create(&secondary);
        std::fs::write(root.join("owned-data"), b"partial-primary").unwrap();
        std::fs::write(secondary.join("owned-data"), b"partial-secondary").unwrap();
        panic!("expected preparation failure");
    })
    .unwrap_err();
    assert_eq!(
        failed_preparation.downcast_ref::<&str>(),
        Some(&"expected preparation failure")
    );
    assert!(
        !root.exists() && !secondary.exists(),
        "uncommitted preparation leaked"
    );
    windows_native_move_completion_offline_prepare();
    assert!(root.join("fixture.json").is_file());
    assert!(!root.join("verified").exists());
    // Missing verification must still fail the check after removing only the
    // identity-admitted owned fixture; teardown cannot turn failure into success.
    assert!(std::panic::catch_unwind(windows_native_move_completion_offline_cleanup).is_err());
    assert!(
        !root.exists() && !secondary.exists(),
        "failed verification leaked its persistent fixture"
    );
}
