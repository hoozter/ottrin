//! Exact interrupted releases have one writer, with independent paired-terminal
//! or no-records authority. No I/O failure authorizes an in-memory retry.
use super::claim_processes::OwnedChild;
use super::*;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ROOT: &str = "OTTRIN_TEST_CLAIM_REPAIR_ROOT";
const SELF: &str = "copy_transaction::windows::journal::tests::claim_repair::windows_native_copy_release_repair_errors_and_process_exits_preserve_payload";
const POINTS: [claims::ReleasePoint; 6] = [
    claims::ReleasePoint::BeforeTruncate,
    claims::ReleasePoint::Truncated,
    claims::ReleasePoint::TruncateFlushed,
    claims::ReleasePoint::BeforeWrite,
    claims::ReleasePoint::Written,
    claims::ReleasePoint::Flushed,
];

struct Fixture {
    stage: OsString,
    ledger: PathBuf,
    prefix: Vec<u8>,
    release: Vec<u8>,
    local: PathBuf,
    mirror: PathBuf,
    journal: Vec<u8>,
    retained: PathBuf,
}
fn registry(root: &Path) -> PathBuf {
    root.join("profile/Ottrin/State/copies")
}
fn prepared(root: &Path, orphan: bool) -> Fixture {
    let central_path = registry(root);
    let (stage, _, _cleanup) = completed_undo_fixture_at(
        root,
        &root.join("destination"),
        true,
        false,
        Some(&central_path),
    );
    let parent = owner(&root.join("destination"));
    let central = owner(&central_path);
    let header = Snapshot::load(&parent, &central, &stage, SnapshotAccess::Read)
        .unwrap()
        .state
        .header
        .unwrap();
    undo_completed_copy(&parent, &central, &stage, |_| Ok(())).unwrap();
    let ledger = root
        .join("destination")
        .join(header.claim.test_relative_path());
    let (binding, prefix) = if orphan {
        let lease =
            claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"d".repeat(32)).unwrap();
        let binding = lease.binding().clone();
        drop(lease);
        (binding, std::fs::read(&ledger).unwrap())
    } else {
        let full = std::fs::read(&ledger).unwrap();
        let end = FRAME_HEADER + u32::from_le_bytes(full[8..12].try_into().unwrap()) as usize;
        (header.claim.clone(), full[..end].to_vec())
    };
    let release = binding.test_release_frame(&prefix);
    let local = root.join("destination").join(&stage).join(LOCAL);
    let mirror = central_path.join(header.central_name());
    let journal = std::fs::read(&local).unwrap();
    assert_eq!(std::fs::read(&mirror).unwrap(), journal);
    let retained = root
        .join("destination")
        .join(header.candidate_name())
        .join("nested/file");
    std::fs::write(&retained, b"retained user edits").unwrap();
    std::fs::write(root.join("destination/user-data"), b"keep public file").unwrap();
    remove_undo_fixture_source(root);
    Fixture {
        stage,
        ledger,
        prefix,
        release,
        local,
        mirror,
        journal,
        retained,
    }
}
fn replace(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}
fn preserved(root: &Path, fixture: &Fixture) {
    assert_eq!(std::fs::read(&fixture.local).unwrap(), fixture.journal);
    assert_eq!(std::fs::read(&fixture.mirror).unwrap(), fixture.journal);
    assert_eq!(
        std::fs::read(&fixture.retained).unwrap(),
        b"retained user edits"
    );
    assert_eq!(
        std::fs::read(root.join("destination/user-data")).unwrap(),
        b"keep public file"
    );
    assert!(!root.join("source").exists());
}
fn perform(root: &Path, orphan: bool) -> Result<(), PlatformError> {
    let central_path = registry(root);
    if orphan {
        let parent = owner(&root.join("destination"));
        let central = owner(&central_path);
        let lease = claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"e".repeat(32))?;
        match lease.abort_unstarted(io("owned admission cancellation")) {
            PlatformError::Io(_) => Ok(()),
            error => Err(error),
        }
    } else {
        let central = owner(&central_path);
        let name = windows_fs::directory_children_limited(&central, 1)
            .map_err(io)?
            .remove(0);
        let control = discovery::RegisteredJournal::open(&central, &name)?;
        let transaction = control.state.header.as_ref().unwrap().transaction.clone();
        drop((control, central));
        let mut plan =
            crate::restore_retained_copy_name_release_plan_at(&central_path, &transaction)?;
        plan.finish(|_| Ok(()))
    }
}
struct ClearFault;
impl Drop for ClearFault {
    fn drop(&mut self) {
        claims::RELEASE_FAULT.with(|fault| fault.set(None));
    }
}

#[test]
fn windows_native_copy_orphan_release_recovers_every_exact_prefix_without_rewriting_history() {
    let root = tempfile::tempdir().unwrap();
    let fixture = prepared(root.path(), true);
    let expected = [fixture.prefix.as_slice(), fixture.release.as_slice()].concat();
    for cut in 0..=fixture.release.len() {
        replace(
            &fixture.ledger,
            &[fixture.prefix.as_slice(), &fixture.release[..cut]].concat(),
        );
        perform(root.path(), true).unwrap();
        assert!(
            std::fs::read(&fixture.ledger)
                .unwrap()
                .starts_with(&expected),
            "prefix {cut}"
        );
        preserved(root.path(), &fixture);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&registry(root.path()));
        assert!(
            Snapshot::load(&parent, &central, &fixture.stage, SnapshotAccess::Read)
                .unwrap()
                .state
                .undo_complete()
        );
    }
}

#[test]
fn windows_native_copy_orphan_release_refuses_records_unknown_tails_and_streams_unchanged() {
    for case in ["stage", "journal", "unknown-tail", "stream"] {
        let root = tempfile::tempdir().unwrap();
        let fixture = prepared(root.path(), true);
        let mut bytes = [fixture.prefix.as_slice(), &fixture.release[..12]].concat();
        match case {
            "stage" => {
                let stage = root
                    .path()
                    .join("destination")
                    .join(format!(".ottrin-copy-{}", "d".repeat(32)));
                std::fs::create_dir(&stage).unwrap();
                std::fs::write(stage.join("unknown"), b"keep staging data").unwrap();
            }
            "journal" => std::fs::write(
                registry(root.path()).join(format!("{}.copy-journal", "d".repeat(32))),
                b"keep unknown record",
            )
            .unwrap(),
            "unknown-tail" => *bytes.last_mut().unwrap() ^= 0x40,
            "stream" => (),
            _ => unreachable!(),
        }
        replace(&fixture.ledger, &bytes);
        let stream = PathBuf::from(format!("{}:foreign", fixture.ledger.display()));
        if case == "stream" {
            std::fs::write(&stream, b"keep named content").unwrap();
        }
        let before = super::cleanup::namespace_contents(root.path());
        assert!(perform(root.path(), true).is_err(), "{case}");
        assert_eq!(
            super::cleanup::namespace_contents(root.path()),
            before,
            "{case}"
        );
        assert_eq!(std::fs::read(&fixture.ledger).unwrap(), bytes);
        if case == "stream" {
            assert_eq!(std::fs::read(stream).unwrap(), b"keep named content");
        }
        preserved(root.path(), &fixture);
    }
}

#[test]
fn windows_native_copy_release_repair_errors_and_process_exits_preserve_payload() {
    if let Some(root) = std::env::var_os(ROOT) {
        let point = POINTS[std::env::var("OTTRIN_TEST_CLAIM_REPAIR_POINT")
            .unwrap()
            .parse::<usize>()
            .unwrap()];
        let orphan = std::env::var("OTTRIN_TEST_CLAIM_REPAIR_ORPHAN").unwrap() == "true";
        claims::RELEASE_FAULT.with(|fault| fault.set(Some((point, true))));
        let result = perform(Path::new(&root), orphan);
        panic!("Release exit boundary was not reached: {result:?}");
    }
    for orphan in [false, true] {
        for (index, point) in POINTS.into_iter().enumerate() {
            for exit in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let fixture = prepared(root.path(), orphan);
                let partial = [fixture.prefix.as_slice(), &fixture.release[..12]].concat();
                replace(&fixture.ledger, &partial);
                if exit {
                    let mut child = OwnedChild(
                        Command::new(std::env::current_exe().unwrap())
                            .args(["--exact", SELF, "--nocapture"])
                            .env(ROOT, root.path())
                            .env("OTTRIN_TEST_CLAIM_REPAIR_POINT", index.to_string())
                            .env("OTTRIN_TEST_CLAIM_REPAIR_ORPHAN", orphan.to_string())
                            .stdout(Stdio::null())
                            .spawn()
                            .unwrap(),
                    );
                    let deadline = Instant::now() + Duration::from_secs(30);
                    loop {
                        if let Some(status) = child.0.try_wait().unwrap() {
                            assert_eq!(status.code(), Some(86), "{orphan} {point:?}");
                            break;
                        }
                        assert!(Instant::now() < deadline, "Release exit actor timed out");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                } else {
                    let reset = ClearFault;
                    claims::RELEASE_FAULT.with(|fault| fault.set(Some((point, false))));
                    assert!(perform(root.path(), orphan).is_err(), "{orphan} {point:?}");
                    drop(reset);
                }
                let expected = match point {
                    claims::ReleasePoint::BeforeTruncate => partial,
                    claims::ReleasePoint::Truncated
                    | claims::ReleasePoint::TruncateFlushed
                    | claims::ReleasePoint::BeforeWrite => fixture.prefix.clone(),
                    claims::ReleasePoint::Written | claims::ReleasePoint::Flushed => {
                        [fixture.prefix.as_slice(), fixture.release.as_slice()].concat()
                    }
                };
                assert_eq!(
                    std::fs::read(&fixture.ledger).unwrap(),
                    expected,
                    "{orphan} {point:?}"
                );
                preserved(root.path(), &fixture);
                let complete = [fixture.prefix.as_slice(), fixture.release.as_slice()].concat();
                if orphan {
                    perform(root.path(), true).unwrap();
                    assert!(
                        std::fs::read(&fixture.ledger)
                            .unwrap()
                            .starts_with(&complete)
                    );
                } else {
                    let transaction = fixture
                        .stage
                        .to_str()
                        .unwrap()
                        .strip_prefix(".ottrin-copy-")
                        .unwrap();
                    let plan = crate::restore_retained_copy_name_release_plan_at(
                        &registry(root.path()),
                        transaction,
                    );
                    if matches!(
                        point,
                        claims::ReleasePoint::Written | claims::ReleasePoint::Flushed
                    ) {
                        assert!(
                            plan.is_err(),
                            "A fully released generation needs no second action"
                        );
                    } else {
                        plan.unwrap().finish(|_| Ok(())).unwrap();
                    }
                    assert_eq!(std::fs::read(&fixture.ledger).unwrap(), complete);
                }
                preserved(root.path(), &fixture);
            }
        }
    }
}
