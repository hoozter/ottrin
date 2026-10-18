//! A complete pre-journal reservation can be released only after its writer dies
//! and neither canonical recovery record exists. Ambiguous content stays intact.
use super::claim_processes::OwnedChild;
use super::cleanup::namespace_contents;
use super::*;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ROOT: &str = "OTTRIN_TEST_ORPHAN_CLAIM_ROOT";
const SELF: &str = "copy_transaction::windows::journal::tests::claim_orphans::windows_native_copy_orphan_claim_recovers_after_kernel_owner_exit";

#[test]
fn windows_native_copy_orphan_claim_recovers_after_kernel_owner_exit() {
    if let Some(root) = std::env::var_os(ROOT) {
        let root = Path::new(&root);
        let parent = owner(&root.join("destination"));
        let central = owner(&root.join("central"));
        let lease =
            claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"d".repeat(32)).unwrap();
        std::fs::write(
            root.join("ready"),
            serde_json::to_vec(lease.binding()).unwrap(),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(90);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(lease);
        panic!("Parent did not kill the owned orphan actor");
    }
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    std::fs::write(
        root.path().join("destination/user-data"),
        b"keep public file",
    )
    .unwrap();
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", SELF, "--nocapture"])
            .env(ROOT, root.path())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let ready = root.path().join("ready");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready.try_exists().unwrap() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "Orphan actor exited early"
        );
        assert!(
            Instant::now() < deadline,
            "Orphan actor did not become ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // The actor may still be completing its small readiness write.
    let binding: claims::Binding = loop {
        if let Ok(binding) = serde_json::from_slice(&std::fs::read(&ready).unwrap()) {
            break binding;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    let before = namespace_contents(root.path());
    assert!(matches!(
        claims::Lease::acquire(&parent, &central, OsStr::new("COPY"), &"e".repeat(32)),
        Err(PlatformError::DestinationExists(_))
    ));
    assert_eq!(namespace_contents(root.path()), before);
    // TerminateProcess supplies the release of the native writer, without any
    // Rust destructor or abort_unstarted call in the owner.
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());
    assert_eq!(namespace_contents(root.path()), before);
    let next =
        claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"e".repeat(32)).unwrap();
    next.validate(false).unwrap();
    assert_ne!(next.binding().identities()[4], binding.identities()[4]);
    assert!(matches!(
        next.abort_unstarted(io("owned cancellation")),
        PlatformError::Io(_)
    ));
    assert_eq!(
        std::fs::read(root.path().join("destination/user-data")).unwrap(),
        b"keep public file"
    );
    assert_eq!(
        std::fs::read(root.path().join("source/nested/file")).unwrap(),
        b"original data"
    );
    assert!(
        windows_fs::directory_children_limited(&central, 1)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn windows_native_copy_orphan_claim_release_keeps_older_recovery_history() {
    let root = tempfile::tempdir().unwrap();
    let (name, _, _cleanup) = completed_undo_fixture(root.path(), true, false);
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let header = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read)
        .unwrap()
        .state
        .header
        .unwrap();
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    let local = root.path().join("destination").join(&name).join(LOCAL);
    let mirror = root.path().join("central").join(header.central_name());
    let journal = std::fs::read(&local).unwrap();
    let orphan =
        claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"d".repeat(32)).unwrap();
    assert_ne!(
        orphan.binding().identities()[4],
        header.claim.identities()[4]
    );
    let orphan_identity = orphan.binding().identities()[4];
    drop(orphan);
    let ledger = root
        .path()
        .join("destination")
        .join(header.claim.test_relative_path());
    let before = std::fs::read(&ledger).unwrap();
    let next =
        claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"e".repeat(32)).unwrap();
    assert_ne!(next.binding().identities()[4], orphan_identity);
    assert_ne!(std::fs::read(&ledger).unwrap(), before);
    assert!(matches!(
        next.abort_unstarted(io("owned cancellation")),
        PlatformError::Io(_)
    ));
    assert_eq!(std::fs::read(&local).unwrap(), journal);
    assert_eq!(std::fs::read(&mirror).unwrap(), journal);
    assert!(
        Snapshot::load(&parent, &central, &name, SnapshotAccess::Read)
            .unwrap()
            .state
            .undo_complete()
    );
    assert_eq!(
        std::fs::read(
            root.path()
                .join("destination")
                .join(header.candidate_name())
                .join("nested/file")
        )
        .unwrap(),
        b"original data"
    );
}

#[test]
fn windows_native_copy_orphan_claim_preserves_ambiguous_or_partial_construction() {
    for case in ["stage", "journal", "stream", "partial", "missing-registry"] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let transaction = "d".repeat(32);
        let lease =
            claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &transaction).unwrap();
        let binding = lease.binding().clone();
        drop(lease);
        let ledger = root
            .path()
            .join("destination")
            .join(binding.test_relative_path());
        let stream = PathBuf::from(format!("{}:foreign", ledger.display()));
        match case {
            "stage" => {
                let stage = root
                    .path()
                    .join("destination")
                    .join(format!(".ottrin-copy-{transaction}"));
                std::fs::create_dir(&stage).unwrap();
                std::fs::write(stage.join("unknown"), b"keep staged content").unwrap();
            }
            "journal" => std::fs::write(
                root.path()
                    .join("central")
                    .join(format!("{transaction}.copy-journal")),
                b"keep unknown journal",
            )
            .unwrap(),
            "stream" => std::fs::write(&stream, b"keep named stream").unwrap(),
            "partial" => {
                let mut bytes = std::fs::read(&ledger).unwrap();
                bytes.pop();
                std::fs::write(&ledger, bytes).unwrap();
            }
            "missing-registry" => (),
            _ => unreachable!(),
        }
        drop(central);
        let registry = if case == "missing-registry" {
            std::fs::remove_dir(root.path().join("central")).unwrap();
            let other = root.path().join("other-registry");
            std::fs::create_dir(&other).unwrap();
            owner(&other)
        } else {
            owner(&root.path().join("central"))
        };
        let before = namespace_contents(root.path());
        assert!(
            claims::Lease::acquire(&parent, &registry, OsStr::new("copy"), &"e".repeat(32))
                .is_err(),
            "{case}"
        );
        assert_eq!(namespace_contents(root.path()), before, "{case}");
        if case == "stream" {
            assert_eq!(std::fs::read(&stream).unwrap(), b"keep named stream");
        }
    }
}
