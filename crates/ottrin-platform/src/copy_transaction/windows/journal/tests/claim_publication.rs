//! A final claim key is published only after its first frame is durable and
//! verified. Private preparations are independent, and no-replace selects a winner.
use super::claim_processes::OwnedChild;
use super::*;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ROOT: &str = "OTTRIN_TEST_CLAIM_PUBLICATION_ROOT";
const CASE: &str = "OTTRIN_TEST_CLAIM_PUBLICATION_CASE";
const RACE: &str = "copy_transaction::windows::journal::tests::claim_publication::windows_native_copy_prepared_claim_publishers_race_without_overwrite";
const EXITS: &str = "copy_transaction::windows::journal::tests::claim_publication::windows_native_copy_prepared_claim_process_exits_leave_final_keys_free_or_complete";
const POINTS: [claims::PreparationPoint; 5] = [
    claims::PreparationPoint::Created,
    claims::PreparationPoint::Prepared,
    claims::PreparationPoint::Published,
    claims::PreparationPoint::Flushed,
    claims::PreparationPoint::Persisted,
];

fn initialized(root: &Path, sensitive: bool) {
    fixture(root);
    let parent = owner(&root.join("destination"));
    windows_fs::set_empty_directory_case_policy(&parent, sensitive).unwrap();
    let namespace = super::super::super::registry::AnchoredRoot::claims(&parent, true)
        .unwrap()
        .unwrap();
    drop(
        super::super::super::registry::AnchoredRoot::claim_bucket(namespace.root(), true, true)
            .unwrap()
            .unwrap(),
    );
    std::fs::write(root.join("destination/user-data"), b"keep public file").unwrap();
    std::fs::write(
        root.join("destination/.ottrin-copy-target-claims")
            .join(format!(".preparing-{}", "f".repeat(32))),
        b"keep foreign preparation",
    )
    .unwrap();
}
fn canonical(root: &Path) -> PathBuf {
    root.join("destination/.ottrin-copy-target-claims/short-names.ottrin-claims/copy.ottrin-claim")
}
fn preparation(root: &Path) -> PathBuf {
    root.join("destination/.ottrin-copy-target-claims")
        .join(format!(".preparing-{}", "d".repeat(32)))
}
fn preserved(root: &Path) {
    assert_eq!(
        std::fs::read(
            root.join("destination/.ottrin-copy-target-claims")
                .join(format!(".preparing-{}", "f".repeat(32)))
        )
        .unwrap(),
        b"keep foreign preparation"
    );
    assert_eq!(
        std::fs::read(root.join("destination/user-data")).unwrap(),
        b"keep public file"
    );
    assert_eq!(
        std::fs::read(root.join("source/nested/file")).unwrap(),
        b"original data"
    );
}
fn reap(child: &mut OwnedChild, code: i32) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert_eq!(status.code(), Some(code));
            return;
        }
        assert!(Instant::now() < deadline, "Publication child timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}
struct ClearFault;
impl Drop for ClearFault {
    fn drop(&mut self) {
        claims::PREPARATION_FAULT.with(|fault| fault.set(None));
    }
}

#[test]
fn windows_native_copy_prepared_claim_errors_clean_owned_preparations_or_retain_complete_keys() {
    for point in POINTS {
        let root = tempfile::tempdir().unwrap();
        initialized(root.path(), false);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let reset = ClearFault;
        claims::PREPARATION_FAULT.with(|fault| fault.set(Some((point, 0))));
        let result = claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"d".repeat(32));
        drop(reset);
        assert!(result.is_err(), "{point:?}");
        assert!(
            !preparation(root.path()).exists(),
            "Owned preparation was not cleaned at {point:?}: {:?}",
            result.err()
        );
        assert_eq!(
            canonical(root.path()).exists(),
            point == claims::PreparationPoint::Persisted
        );
        let next =
            claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"e".repeat(32)).unwrap();
        next.validate(false).unwrap();
        assert!(matches!(
            next.abort_unstarted(io("owned cancellation")),
            PlatformError::Io(_)
        ));
        preserved(root.path());
    }
}

#[test]
fn windows_native_copy_prepared_claim_process_exits_leave_final_keys_free_or_complete() {
    if let Some(root) = std::env::var_os(ROOT) {
        let root = Path::new(&root);
        let index = std::env::var(CASE).unwrap().parse::<usize>().unwrap();
        let (point, mode) = if index == POINTS.len() {
            (claims::PreparationPoint::Created, 2)
        } else {
            (POINTS[index], 1)
        };
        let parent = owner(&root.join("destination"));
        let central = owner(&root.join("central"));
        claims::PREPARATION_FAULT.with(|fault| fault.set(Some((point, mode))));
        let result = claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"d".repeat(32));
        panic!("Preparation exit was not reached: {:?}", result.err());
    }
    for index in 0..=POINTS.len() {
        let root = tempfile::tempdir().unwrap();
        initialized(root.path(), false);
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", EXITS, "--nocapture"])
                .env(ROOT, root.path())
                .env(CASE, index.to_string())
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        reap(&mut child, 86);
        let persisted = index == 4;
        assert_eq!(canonical(root.path()).exists(), persisted);
        assert!(
            !preparation(root.path()).exists(),
            "Abandoned preparation at boundary {index}"
        );
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let next =
            claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"e".repeat(32)).unwrap();
        next.validate(false).unwrap();
        assert!(matches!(
            next.abort_unstarted(io("owned cancellation")),
            PlatformError::Io(_)
        ));
        preserved(root.path());
    }
}

#[test]
fn windows_native_copy_prepared_claim_publishers_race_without_overwrite() {
    if let Some(root) = std::env::var_os(ROOT) {
        let root = PathBuf::from(root);
        let id = std::env::var(CASE).unwrap();
        let gate = root.clone();
        let actor = id.clone();
        claims::PREPARATION_HANDOFF.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                std::fs::write(gate.join(format!("ready-{actor}")), b"prepared").unwrap();
                let deadline = Instant::now() + Duration::from_secs(30);
                while !gate.join("publish").exists() {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(5));
                }
            }))
        });
        let parent = owner(&root.join("destination"));
        let central = owner(&root.join("central"));
        let target = if id == "1" { "copy" } else { "COPY" };
        let lease =
            match claims::Lease::acquire(&parent, &central, OsStr::new(target), &id.repeat(32)) {
                Ok(lease) => Some(lease),
                Err(PlatformError::DestinationExists(_)) => None,
                Err(error) => panic!("Unexpected publication race error: {error}"),
            };
        std::fs::write(
            root.join(format!("result-{id}.tmp")),
            if lease.is_some() {
                b"owner".as_slice()
            } else {
                b"conflict".as_slice()
            },
        )
        .unwrap();
        std::fs::rename(
            root.join(format!("result-{id}.tmp")),
            root.join(format!("result-{id}")),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !root.join("exit").exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        std::process::exit(if lease.is_some() { 86 } else { 87 });
    }
    for sensitive in [false, true] {
        let root = tempfile::tempdir().unwrap();
        initialized(root.path(), sensitive);
        let mut children = Vec::new();
        for id in ["1", "2"] {
            children.push(OwnedChild(
                Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", RACE, "--nocapture"])
                    .env(ROOT, root.path())
                    .env(CASE, id)
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap(),
            ));
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        for prefix in ["ready", "result"] {
            while !(root.path().join(format!("{prefix}-1")).exists()
                && root.path().join(format!("{prefix}-2")).exists())
            {
                assert!(Instant::now() < deadline, "Publication barrier timed out");
                for child in &mut children {
                    assert!(
                        child.0.try_wait().unwrap().is_none(),
                        "Publication actor exited early"
                    );
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            if prefix == "ready" {
                std::fs::write(root.path().join("publish"), b"go").unwrap();
            }
        }
        let results: [String; 2] = ["1", "2"]
            .map(|id| std::fs::read_to_string(root.path().join(format!("result-{id}"))).unwrap());
        assert_eq!(
            results
                .iter()
                .filter(|result| result.as_str() == "owner")
                .count(),
            if sensitive { 2 } else { 1 }
        );
        let names = std::fs::read_dir(root.path().join("destination/.ottrin-copy-target-claims"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert!(
            !names
                .iter()
                .any(|name| name.to_string_lossy().starts_with(".preparing-")
                    && *name != OsString::from(format!(".preparing-{}", "f".repeat(32)))),
            "Losing preparation leaked"
        );
        std::fs::write(root.path().join("exit"), b"exit").unwrap();
        for (index, child) in children.iter_mut().enumerate() {
            reap(child, if results[index] == "owner" { 86 } else { 87 });
        }
        preserved(root.path());
    }
}

#[test]
fn windows_native_copy_temporary_claim_hard_kill_removes_only_its_preparation() {
    const SELF: &str = "copy_transaction::windows::journal::tests::claim_publication::windows_native_copy_temporary_claim_hard_kill_removes_only_its_preparation";
    if let Some(path) = std::env::var_os(ROOT) {
        let root = PathBuf::from(path);
        let gate = root.clone();
        claims::PREPARATION_HANDOFF.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                std::fs::write(gate.join("ready"), b"prepared").unwrap();
                let deadline = Instant::now() + Duration::from_secs(90);
                while Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                panic!("Parent did not kill the owned preparation writer");
            }))
        });
        let parent = owner(&root.join("destination"));
        let central = owner(&root.join("central"));
        let result = claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"d".repeat(32));
        panic!("Preparation gate returned: {:?}", result.err());
    }
    let root = tempfile::tempdir().unwrap();
    initialized(root.path(), false);
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", SELF, "--nocapture"])
            .env(ROOT, root.path())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !root.path().join("ready").exists() {
        assert!(Instant::now() < deadline && child.0.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(preparation(root.path()).exists());
    assert!(!canonical(root.path()).exists());
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());
    assert!(!preparation(root.path()).exists());
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let next =
        claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"e".repeat(32)).unwrap();
    next.validate(false).unwrap();
    assert!(matches!(
        next.abort_unstarted(io("owned cancellation")),
        PlatformError::Io(_)
    ));
    preserved(root.path());
}

#[test]
fn windows_native_copy_temporary_claim_collision_preserves_foreign_preparation() {
    let root = tempfile::tempdir().unwrap();
    initialized(root.path(), false);
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    assert!(
        claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"f".repeat(32)).is_err()
    );
    assert!(!canonical(root.path()).exists());
    preserved(root.path());
}
