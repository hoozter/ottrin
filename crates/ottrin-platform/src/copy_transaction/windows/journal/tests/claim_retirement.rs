//! Released reservations can be replaced while paired recovery records remain.
use super::claim_processes::OwnedChild;
use super::cleanup::namespace_contents;
use super::*;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn windows_native_copy_claim_retirement_keeps_terminal_cleanup_independent_of_new_owners() {
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
    let claim_path = root
        .path()
        .join("destination")
        .join(header.claim.test_relative_path());
    let local = root.path().join("destination").join(&name).join(LOCAL);
    let mirror = root.path().join("central").join(header.central_name());
    let journal = std::fs::read(&local).unwrap();
    let old_identity = header.claim.identities()[4];
    let next =
        claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"b".repeat(32)).unwrap();
    assert_ne!(next.binding().identities()[4], old_identity);
    assert_eq!(std::fs::read(&local).unwrap(), journal);
    assert_eq!(std::fs::read(&mirror).unwrap(), journal);
    let retained_log = std::fs::read(&claim_path).unwrap();
    let terminal = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).unwrap();
    assert!(terminal.claim.is_none());
    drop(terminal);
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    assert_eq!(std::fs::read(&claim_path).unwrap(), retained_log);
    let user = root.path().join("destination/user-data");
    std::fs::write(&user, b"preserve public sibling").unwrap();
    let cleanup =
        super::super::cleanup_finalize::prepare(&parent, &central, &name, |_| Ok(())).unwrap();
    cleanup.finish(&parent, &central, |_| Ok(())).unwrap();
    assert!(!local.exists());
    assert!(!mirror.exists());
    next.validate(false).unwrap();
    assert_eq!(std::fs::read(&claim_path).unwrap(), retained_log);
    assert!(matches!(
        next.abort_unstarted(io("owned test cancellation")),
        PlatformError::Io(_)
    ));
    let fresh =
        claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"c".repeat(32)).unwrap();
    assert_ne!(fresh.binding().identities()[4], old_identity);
    assert_ne!(std::fs::read(&claim_path).unwrap(), retained_log);
    assert!(matches!(
        fresh.abort_unstarted(io("owned test cancellation")),
        PlatformError::Io(_)
    ));
    assert_eq!(std::fs::read(&user).unwrap(), b"preserve public sibling");
}

#[test]
fn windows_native_copy_terminal_claim_discharge_requires_paired_state_and_never_adopts_foreign_controls()
 {
    let root = tempfile::tempdir().unwrap();
    let (name, _, _cleanup) = completed_undo_fixture(root.path(), false, false);
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let header = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read)
        .unwrap()
        .state
        .header
        .unwrap();
    let claim = root
        .path()
        .join("destination")
        .join(header.claim.test_relative_path());
    let displaced = claim.with_extension("held");
    let local = root.path().join("destination").join(&name).join(LOCAL);
    let mirror = root.path().join("central").join(header.central_name());
    let active = std::fs::read(&local).unwrap();
    std::fs::rename(&claim, &displaced).unwrap();
    assert!(Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).is_err());
    std::fs::write(&claim, b"foreign control bytes").unwrap();
    assert!(Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).is_err());
    assert_eq!(std::fs::read(&claim).unwrap(), b"foreign control bytes");
    std::fs::remove_file(&claim).unwrap();
    std::fs::rename(&displaced, &claim).unwrap();
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    remove_undo_fixture_source(root.path());
    let terminal = std::fs::read(&local).unwrap();
    std::fs::remove_file(&claim).unwrap();
    assert!(
        Snapshot::load(&parent, &central, &name, SnapshotAccess::Read)
            .unwrap()
            .claim
            .is_none()
    );
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    assert!(!claim.exists());
    std::fs::write(&claim, b"different owner; deliberately not a claim log").unwrap();
    let snapshot = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).unwrap();
    assert!(snapshot.claim.is_none());
    drop(snapshot);
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    assert_eq!(
        std::fs::read(&claim).unwrap(),
        b"different owner; deliberately not a claim log"
    );
    // An absent old claim never turns a lone terminal journal into authority.
    std::fs::write(&mirror, &active).unwrap();
    assert!(Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).is_err());
    assert!(undo_completed_copy(&parent, &central, &name, |_| Ok(())).is_err());
    assert_eq!(std::fs::read(&local).unwrap(), terminal);
    assert_eq!(std::fs::read(&mirror).unwrap(), active);
    assert_eq!(
        std::fs::read(&claim).unwrap(),
        b"different owner; deliberately not a claim log"
    );
}

const RETIRE_ROOT: &str = "OTTRIN_TEST_CLAIM_RETIREMENT_ROOT";
const RETIRE_SELF: &str = "copy_transaction::windows::journal::tests::claim_retirement::windows_native_copy_claim_retirement_errors_and_exits_preserve_recovery";
const PUBLISH_SELF: &str = "copy_transaction::windows::journal::tests::claim_retirement::windows_native_copy_claim_retirement_yields_to_a_new_process_owner";
const RETIRE_POINTS: [claims::RetirementPoint; 4] = [
    claims::RetirementPoint::BeforeUnlink,
    claims::RetirementPoint::Marked,
    claims::RetirementPoint::Unlinked,
    claims::RetirementPoint::Flushed,
];

struct RetirementReset;
impl Drop for RetirementReset {
    fn drop(&mut self) {
        claims::RETIREMENT_FAULT.with(|fault| fault.set(None));
        claims::RETIREMENT_HANDOFF.with(|hook| hook.borrow_mut().take());
    }
}
fn retirement_fixture(root: &Path) -> (OsString, Header) {
    let (name, _, _cleanup) = completed_undo_fixture(root, true, false);
    let parent = owner(&root.join("destination"));
    let central = owner(&root.join("central"));
    let header = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read)
        .unwrap()
        .state
        .header
        .unwrap();
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    remove_undo_fixture_source(root);
    std::fs::write(
        root.join("destination")
            .join(header.candidate_name())
            .join("nested/file"),
        b"retained user edits",
    )
    .unwrap();
    std::fs::write(root.join("destination/user-data"), b"unrelated public file").unwrap();
    std::fs::write(
        root.join("destination/.ottrin-copy-target-claims")
            .join(format!(".preparing-{}", "f".repeat(32))),
        b"foreign preparation",
    )
    .unwrap();
    (name, header)
}
fn retirement_bytes(root: &Path, header: &Header) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let mut bytes = namespace_contents(root);
    bytes.retain(|name, _| name.starts_with("destination") || name.starts_with("central"));
    bytes.remove(&PathBuf::from("destination").join(header.claim.test_relative_path()));
    bytes
}

#[test]
fn windows_native_copy_claim_retirement_errors_and_exits_preserve_recovery() {
    if let Some(root) = std::env::var_os(RETIRE_ROOT) {
        let root = Path::new(&root);
        let point = RETIRE_POINTS[std::env::var("OTTRIN_TEST_RETIRE_POINT")
            .unwrap()
            .parse::<usize>()
            .unwrap()];
        claims::RETIREMENT_FAULT.with(|fault| fault.set(Some((point, true))));
        let parent = owner(&root.join("destination"));
        let central = owner(&root.join("central"));
        let result = claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"b".repeat(32));
        panic!("Retirement exit boundary not reached: {:?}", result.err());
    }
    for (index, point) in RETIRE_POINTS.into_iter().enumerate() {
        for exit in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (name, header) = retirement_fixture(root.path());
            let ledger = root
                .path()
                .join("destination")
                .join(header.claim.test_relative_path());
            let old = std::fs::read(&ledger).unwrap();
            let before = retirement_bytes(root.path(), &header);
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            if exit {
                let mut child = OwnedChild(
                    Command::new(std::env::current_exe().unwrap())
                        .args(["--exact", RETIRE_SELF, "--nocapture"])
                        .env(RETIRE_ROOT, root.path())
                        .env("OTTRIN_TEST_RETIRE_POINT", index.to_string())
                        .stdout(Stdio::null())
                        .spawn()
                        .unwrap(),
                );
                let deadline = Instant::now() + Duration::from_secs(30);
                loop {
                    if let Some(status) = child.0.try_wait().unwrap() {
                        assert_eq!(status.code(), Some(86));
                        break;
                    }
                    assert!(Instant::now() < deadline, "Retirement child timed out");
                    std::thread::sleep(Duration::from_millis(5));
                }
            } else {
                let reset = RetirementReset;
                claims::RETIREMENT_FAULT.with(|fault| fault.set(Some((point, false))));
                let result =
                    claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"b".repeat(32));
                assert!(
                    matches!(result, Err(PlatformError::Io(message)) if message.contains("injected Copy claim retirement failure"))
                );
                drop(reset);
            }
            assert_eq!(
                ledger.try_exists().unwrap(),
                point == claims::RetirementPoint::BeforeUnlink
            );
            if point == claims::RetirementPoint::BeforeUnlink {
                assert_eq!(std::fs::read(&ledger).unwrap(), old);
            }
            assert!(
                retirement_bytes(root.path(), &header) == before,
                "Retained namespace changed at {point:?}"
            );
            let snapshot = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).unwrap();
            assert!(snapshot.state.undo_complete());
            drop(snapshot);
            let next =
                claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"c".repeat(32))
                    .unwrap();
            assert_ne!(next.binding().identities()[4], header.claim.identities()[4]);
            next.validate(false).unwrap();
            assert!(
                retirement_bytes(root.path(), &header) == before,
                "Retained namespace changed at {point:?}"
            );
            assert!(matches!(
                next.abort_unstarted(io("owned cancellation")),
                PlatformError::Io(_)
            ));
        }
    }
}

#[test]
fn windows_native_copy_claim_retirement_yields_to_a_new_process_owner() {
    if let Some(root) = std::env::var_os(RETIRE_ROOT) {
        let root = Path::new(&root);
        let parent = owner(&root.join("destination"));
        let central = owner(&root.join("central"));
        let lease =
            match claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"c".repeat(32)) {
                Ok(lease) => Some(lease),
                Err(PlatformError::Io(message))
                    if message.ends_with("(os error 32)")
                        || message.ends_with("(os error 303)")
                        || (std::env::var("OTTRIN_TEST_RETIRE_HANDOFF").as_deref()
                            == Ok("Marked")
                            && message.ends_with("(os error 5)")) =>
                {
                    None
                }
                Err(error) => panic!("Unexpected publisher failure: {error:?}"),
            };
        let temporary = root.join("publisher-ready.tmp");
        std::fs::write(
            &temporary,
            serde_json::to_vec(&lease.as_ref().map(claims::Lease::binding)).unwrap(),
        )
        .unwrap();
        std::fs::rename(temporary, root.join("publisher-ready")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(90);
        while !root.join("publisher-exit").try_exists().unwrap() {
            assert!(
                Instant::now() < deadline,
                "Parent did not finish publisher check"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        std::process::exit(86);
    }
    for point in RETIRE_POINTS {
        let root = tempfile::tempdir().unwrap();
        let (name, header) = retirement_fixture(root.path());
        let before = retirement_bytes(root.path(), &header);
        let slot = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = slot.clone();
        let gate = root.path().to_path_buf();
        let reset = RetirementReset;
        claims::RETIREMENT_HANDOFF.with(|hook| {
            *hook.borrow_mut() = Some((
                point,
                Box::new(move || {
                    let mut child = OwnedChild(
                        Command::new(std::env::current_exe().unwrap())
                            .args(["--exact", PUBLISH_SELF, "--nocapture"])
                            .env(RETIRE_ROOT, &gate)
                            .env("OTTRIN_TEST_RETIRE_HANDOFF", format!("{point:?}"))
                            .stdout(Stdio::null())
                            .spawn()
                            .unwrap(),
                    );
                    let deadline = Instant::now() + Duration::from_secs(30);
                    while !gate.join("publisher-ready").try_exists().unwrap() {
                        assert!(Instant::now() < deadline && child.0.try_wait().unwrap().is_none());
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    *output.borrow_mut() = Some(child);
                }),
            ))
        });
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let result = claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"b".repeat(32));
        drop(reset);
        let binding: Option<claims::Binding> =
            serde_json::from_slice(&std::fs::read(root.path().join("publisher-ready")).unwrap())
                .unwrap();
        if point == claims::RetirementPoint::BeforeUnlink {
            assert!(binding.is_none());
        }
        if matches!(
            point,
            claims::RetirementPoint::Unlinked | claims::RetirementPoint::Flushed
        ) {
            assert!(binding.is_some());
        }
        if let Some(binding) = binding {
            assert!(matches!(result, Err(PlatformError::DestinationExists(_))));
            let ledger = root
                .path()
                .join("destination")
                .join(binding.test_relative_path());
            let bytes = std::fs::read(&ledger).unwrap();
            let observed = claims::Lease::open(&parent, &binding, false, false).unwrap();
            observed.validate(false).unwrap();
            drop(observed);
            assert!(matches!(
                claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &"d".repeat(32)),
                Err(PlatformError::DestinationExists(_))
            ));
            assert_eq!(std::fs::read(&ledger).unwrap(), bytes);
        } else {
            let lease = result.unwrap();
            lease.validate(false).unwrap();
            assert!(matches!(
                lease.abort_unstarted(io("owned cancellation")),
                PlatformError::Io(_)
            ));
        }
        assert!(
            Snapshot::load(&parent, &central, &name, SnapshotAccess::Read)
                .unwrap()
                .claim
                .is_none()
        );
        assert!(
            retirement_bytes(root.path(), &header) == before,
            "Retained namespace changed at {point:?}"
        );
        std::fs::write(root.path().join("publisher-exit"), b"exit").unwrap();
        let mut child = slot.borrow_mut().take().unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert_eq!(status.code(), Some(86));
                break;
            }
            assert!(Instant::now() < deadline, "Publisher exit timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
