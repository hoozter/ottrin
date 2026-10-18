//! Real staged Copy owners in distinct processes, including kernel teardown.
use super::*;
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const ROOT: &str = "OTTRIN_TEST_STAGED_CLAIM_ROOT";
const ACTOR: &str = "OTTRIN_TEST_STAGED_CLAIM_ACTOR";
const TARGET: &str = "OTTRIN_TEST_STAGED_CLAIM_TARGET";
const SELF: &str = "copy_transaction::windows::journal::tests::claim_processes::windows_native_staged_copy_claims_exclude_processes_and_survive_hard_exit";

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
fn spawn(root: &Path, id: &str, target: &str) -> OwnedChild {
    OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", SELF, "--nocapture"])
            .env(ROOT, root)
            .env(ACTOR, id)
            .env(TARGET, target)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    )
}
fn ready(root: &Path, id: &str, children: &mut [OwnedChild]) -> String {
    let path = root.join(format!("{id}.ready"));
    let deadline = Instant::now() + Duration::from_secs(30);
    while !path.try_exists().unwrap() {
        assert!(
            Instant::now() < deadline,
            "staged Copy actor did not become ready"
        );
        for child in &mut *children {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "actor exited before readiness"
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    std::fs::read_to_string(path).unwrap()
}
fn actor(root: &Path) -> ! {
    let id = std::env::var(ACTOR).unwrap();
    assert!(matches!(id.as_str(), "one" | "two"));
    let target = std::env::var_os(TARGET).unwrap();
    let parent = owner(&root.join("destination"));
    let central = owner(&root.join("central"));
    let input = source(&root.join("source"));
    let admission = DestinationAdmission::creation(&parent, &target).unwrap();
    let staged = match DurableStage::create(
        &input,
        &parent,
        &central,
        &admission,
        OsStr::new("source"),
        100,
        |_| Ok(()),
    ) {
        Ok(mut staged) => {
            drop(staged.prepare_destination_security(|| Ok(())).unwrap());
            Some(staged)
        }
        Err(PlatformError::DestinationExists(_)) => None,
        Err(error) => panic!("unexpected staged admission failure: {error}"),
    };
    let result = staged
        .as_ref()
        .map_or("conflict", |stage| stage.stage.name.to_str().unwrap());
    let ready = root.join(format!("{id}.ready"));
    let temporary = root.join(format!("{id}.ready.tmp"));
    std::fs::write(&temporary, result).unwrap();
    std::fs::rename(temporary, ready).unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    while !root.join("exit").try_exists().unwrap() {
        assert!(
            Instant::now() < deadline,
            "parent did not release staged Copy actor"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // Deliberately retain the stage, journal writer and claim capability until
    // kernel process teardown; no Rust destructor supplies durability/release.
    std::process::exit(if staged.is_some() { 86 } else { 87 });
}

#[test]
fn windows_native_staged_copy_claims_exclude_processes_and_survive_hard_exit() {
    if let Some(root) = std::env::var_os(ROOT) {
        actor(Path::new(&root));
    }
    for (sensitive, second, collision) in [
        (false, "report", true),
        (true, "report", false),
        (false, "Independent", false),
    ] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        windows_fs::set_empty_directory_case_policy(&parent, sensitive).unwrap();
        let namespace = super::super::super::registry::AnchoredRoot::claims(&parent, true)
            .unwrap()
            .unwrap();
        drop(
            super::super::super::registry::AnchoredRoot::claim_bucket(namespace.root(), true, true)
                .unwrap()
                .unwrap(),
        );
        drop(namespace);
        let source_identity =
            stable_identity_from_handle(&source(&root.path().join("source"))).unwrap();
        let mut children = vec![spawn(root.path(), "one", "Report")];
        let first = ready(root.path(), "one", &mut children);
        assert_ne!(first, "conflict");
        children.push(spawn(root.path(), "two", second));
        let next = ready(root.path(), "two", &mut children);
        assert_eq!(next == "conflict", collision);
        // Independent owners must both become ready while the first remains
        // live, rather than waiting for a global operation lock to be dropped.
        for child in &mut children {
            assert!(child.0.try_wait().unwrap().is_none());
        }
        assert_eq!(
            stable_identity_from_handle(&source(&root.path().join("source"))).unwrap(),
            source_identity
        );
        assert_eq!(
            std::fs::read(root.path().join("source/nested/file")).unwrap(),
            b"original data"
        );
        std::fs::write(root.path().join("exit"), b"exit").unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut codes = Vec::new();
        for child in &mut children {
            loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    codes.push(status.code().unwrap());
                    break;
                }
                assert!(Instant::now() < deadline, "staged actor did not exit");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        codes.sort();
        assert_eq!(
            codes,
            if collision {
                vec![86, 87]
            } else {
                vec![86, 86]
            }
        );
        drop(children);
        let mut owners = vec![("Report", first)];
        if !collision {
            owners.push((second, next));
        }
        assert_eq!(
            std::fs::read_dir(root.path().join("central"))
                .unwrap()
                .count(),
            owners.len()
        );
        for (target, _) in &owners {
            assert!(
                !root
                    .path()
                    .join("destination")
                    .join(target)
                    .try_exists()
                    .unwrap()
            );
            let transaction = super::super::super::super::random_transaction_id().unwrap();
            assert!(matches!(
                claims::Lease::acquire(&parent, &central, OsStr::new(target), &transaction),
                Err(PlatformError::DestinationExists(_))
            ));
        }
        std::fs::remove_dir_all(root.path().join("source")).unwrap();
        for (target, stage) in owners {
            PermissionStage::publish(&parent, &central, OsStr::new(&stage), |_| Ok(()))
                .unwrap()
                .apply(|_| Ok(()))
                .unwrap();
            assert_eq!(
                std::fs::read(
                    root.path()
                        .join("destination")
                        .join(target)
                        .join("nested/file")
                )
                .unwrap(),
                b"original data"
            );
            undo_completed_copy(&parent, &central, OsStr::new(&stage), |_| Ok(())).unwrap();
            assert!(
                !root
                    .path()
                    .join("destination")
                    .join(target)
                    .try_exists()
                    .unwrap()
            );
            let transaction = super::super::super::super::random_transaction_id().unwrap();
            let reuse = claims::Lease::acquire(&parent, &central, OsStr::new(target), &transaction)
                .unwrap();
            let error = reuse.abort_unstarted(io("owned unstarted reuse cancellation"));
            assert!(matches!(error, PlatformError::Io(_)));
        }
    }
}
