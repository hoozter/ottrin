use super::super::initialization as setup;
use super::*;
use std::io::Write;

struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        setup::FAULT.with(|f| f.set(None));
        setup::HANDOFF.with(|h| h.borrow_mut().take());
    }
}
fn create<'a>(
    root: &Path,
    parent: &'a File,
    central: &File,
) -> Result<DurableStage<'a>, PlatformError> {
    DurableStage::create(
        &source(&root.join("source")),
        parent,
        central,
        &DestinationAdmission::creation(parent, OsStr::new("copy")).unwrap(),
        OsStr::new("source"),
        100,
        |_| Ok(()),
    )
}
fn stages(root: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(root.join("destination"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name().unwrap().to_str().is_some_and(|n| {
                n.strip_prefix(".ottrin-copy-")
                    .is_some_and(|id| id.len() == 32)
            })
        })
        .collect()
}
#[test]
fn windows_native_copy_setup_errors_preserve_controls_and_release_unstarted_name() {
    for point in [
        setup::Point::LocalCreated,
        setup::Point::ControlsCreated,
        setup::Point::DirectoriesFlushed,
        setup::Point::HeadersFlushed,
        setup::Point::Published,
    ] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let before = std::fs::read(root.path().join("source/nested/file")).unwrap();
        let reset = Reset;
        setup::FAULT.with(|f| f.set(Some(point)));
        let error = create(root.path(), &parent, &central).err().unwrap();
        drop(reset);
        assert!(
            matches!(error, PlatformError::CleanupPending { .. }),
            "{point:?}: {error:?}"
        );
        let stages = stages(root.path());
        assert_eq!(stages.len(), 1);
        assert!(stages[0].join(LOCAL).exists());
        let names = windows_fs::directory_children(&central).unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(
            names
                .iter()
                .filter(|n| super::super::discovery::registry_id(n).is_some())
                .count(),
            usize::from(point == setup::Point::Published),
        );
        assert!(!root.path().join("destination/copy").exists());
        assert_eq!(
            std::fs::read(root.path().join("source/nested/file")).unwrap(),
            before
        );
        if point != setup::Point::Published {
            let statuses = discover_at(&root.path().join("central")).unwrap();
            assert_eq!(statuses.len(), 1);
            assert_review_only(&statuses[0]);
            let id = names[0]
                .to_str()
                .unwrap()
                .strip_prefix(".preparing-")
                .unwrap();
            assert!(restore_resume_at(&root.path().join("central"), id).is_err());
            assert!(restore_undo_at(&root.path().join("central"), id).is_err());
        }
        // A failed setup must not reserve this destination indefinitely.
        let next = create(root.path(), &parent, &central).unwrap();
        assert!(!next.journal.state.nodes.is_empty());
    }
}
#[test]
fn windows_native_copy_preheader_central_collision_preserves_foreign_record_and_retries() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let reset = Reset;
    setup::HANDOFF.with(|h| {
        *h.borrow_mut() = Some((
            setup::Point::LocalCreated,
            Box::new(|_, central, name| {
                let id = name
                    .to_str()
                    .unwrap()
                    .strip_prefix(".ottrin-copy-")
                    .unwrap();
                let mut file = windows_fs::create_private(
                    central,
                    OsStr::new(&format!("{id}.copy-journal")),
                    false,
                    FILE_SHARE_READ,
                )
                .unwrap();
                file.write_all(b"foreign recovery record").unwrap();
                file.sync_all().unwrap();
            }),
        ))
    });
    assert!(create(root.path(), &parent, &central).is_err());
    drop(reset);
    assert_eq!(stages(root.path()).len(), 1);
    let names = windows_fs::directory_children(&central).unwrap();
    assert_eq!(names.len(), 2);
    let names: Vec<_> = names
        .into_iter()
        .filter(|n| super::super::discovery::registry_id(n).is_some())
        .collect();
    assert_eq!(names.len(), 1);
    assert_eq!(
        std::fs::read(root.path().join("central").join(&names[0])).unwrap(),
        b"foreign recovery record"
    );
    drop(create(root.path(), &parent, &central).unwrap());
    assert_eq!(
        std::fs::read(root.path().join("central").join(&names[0])).unwrap(),
        b"foreign recovery record"
    );
}
#[test]
fn windows_native_copy_preheader_failure_preserves_unexpected_stage_content_and_releases_name() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let reset = Reset;
    setup::FAULT.with(|f| f.set(Some(setup::Point::LocalCreated)));
    setup::HANDOFF.with(|h| {
        *h.borrow_mut() = Some((
            setup::Point::LocalCreated,
            Box::new(|stage, _, _| {
                let mut file = windows_fs::create_private(
                    stage,
                    OsStr::new("foreign"),
                    false,
                    FILE_SHARE_READ,
                )
                .unwrap();
                file.write_all(b"must retain").unwrap();
                file.sync_all().unwrap();
            }),
        ))
    });
    assert!(matches!(
        create(root.path(), &parent, &central),
        Err(PlatformError::CleanupPending { .. })
    ));
    drop(reset);
    let stages = stages(root.path());
    assert_eq!(stages.len(), 1);
    assert_eq!(
        std::fs::read(stages[0].join("foreign")).unwrap(),
        b"must retain"
    );
    assert!(stages[0].join(LOCAL).exists());
    assert_eq!(windows_fs::directory_children(&central).unwrap().len(), 1);
    drop(create(root.path(), &parent, &central).unwrap());
}

#[test]
fn windows_native_copy_setup_failure_preserves_streams_on_both_controls() {
    for point in [
        setup::Point::LocalCreated,
        setup::Point::ControlsCreated,
        setup::Point::HeadersFlushed,
        setup::Point::Published,
    ] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let reset = Reset;
        setup::FAULT.with(|f| f.set(Some(point)));
        setup::HANDOFF.with(|h| {
            *h.borrow_mut() = Some((
                point,
                Box::new(move |stage, central, name| {
                    let local = windows_fs::location(stage).unwrap().join(LOCAL);
                    std::fs::write(
                        format!("{}:foreign", local.display()),
                        b"local foreign bytes",
                    )
                    .unwrap();
                    {
                        let id = name
                            .to_str()
                            .unwrap()
                            .strip_prefix(".ottrin-copy-")
                            .unwrap();
                        let name = if point == setup::Point::Published {
                            format!("{id}.copy-journal")
                        } else {
                            format!(".preparing-{id}")
                        };
                        let path = windows_fs::location(central).unwrap().join(name);
                        std::fs::write(
                            format!("{}:foreign", path.display()),
                            b"central foreign bytes",
                        )
                        .unwrap();
                    }
                }),
            ));
        });
        assert!(matches!(
            create(root.path(), &parent, &central),
            Err(PlatformError::CleanupPending { .. })
        ));
        drop(reset);
        let stages = stages(root.path());
        assert_eq!(
            std::fs::read(format!("{}:foreign", stages[0].join(LOCAL).display())).unwrap(),
            b"local foreign bytes"
        );
        let names = windows_fs::directory_children(&central).unwrap();
        {
            assert_eq!(names.len(), 1);
            assert_eq!(
                std::fs::read(format!(
                    "{}:foreign",
                    root.path().join("central").join(&names[0]).display()
                ))
                .unwrap(),
                b"central foreign bytes"
            );
        }
        drop(create(root.path(), &parent, &central).unwrap());
        assert_eq!(
            std::fs::read(format!("{}:foreign", stages[0].join(LOCAL).display())).unwrap(),
            b"local foreign bytes"
        );
    }
}

fn assert_review_only(status: &WindowsRetainedCopyStatus) {
    assert!(status.transaction_id.is_none());
    assert!(status.phase.is_none());
    assert!(status.publication.is_none());
    assert!(status.retained.is_none());
    assert!(status.owned_location.is_none());
    assert!(status.source_label.is_none());
    assert!(status.transaction.is_some());
    assert!(matches!(
        status.evidence,
        WindowsCopyRecoveryEvidence::NeedsReview { .. }
    ));
}

#[test]
fn windows_native_copy_unpublished_native_names_are_review_only_and_preserved() {
    use std::os::windows::ffi::OsStringExt;
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let central_path = root.path().join("central");
    let mut invalid: Vec<_> = ".PREPARING-".encode_utf16().collect();
    invalid.push(0xd800);
    let names = [
        OsString::from(".preparing-"),
        OsString::from(".preparing-not-an-id"),
        OsString::from(".preparing-0123456789abcdef0123456789abcdef"),
        OsString::from_wide(&invalid),
    ];
    for (index, name) in names.iter().enumerate() {
        std::fs::write(central_path.join(name), [index as u8; 3]).unwrap();
    }
    // A directory with a matching name must not be followed or treated as a journal.
    let directory = central_path.join(".preparing-directory");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("foreign"), b"preserve nested data").unwrap();
    for _ in 0..2 {
        let statuses = discover_at(&central_path).unwrap();
        assert_eq!(statuses.len(), names.len() + 1);
        for status in &statuses {
            assert_review_only(status);
        }
        for (index, name) in names.iter().enumerate() {
            assert_eq!(
                std::fs::read(central_path.join(name)).unwrap(),
                [index as u8; 3]
            );
            assert!(
                statuses
                    .iter()
                    .any(|s| s.transaction.as_ref().unwrap().file_name() == Some(name.as_os_str()))
            );
        }
        assert_eq!(
            std::fs::read(directory.join("foreign")).unwrap(),
            b"preserve nested data"
        );
    }
}

#[test]
fn windows_native_copy_unpublished_marker_before_stage_allows_orphan_reclaim() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let id = super::super::super::super::random_transaction_id().unwrap();
    let claim = claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &id).unwrap();
    let prepared = setup::prepare(&central, &id).unwrap();
    prepared.sync_all().unwrap();
    central.sync_all().unwrap();
    drop(prepared);
    drop(claim);
    assert!(stages(root.path()).is_empty());
    let statuses = discover_at(&root.path().join("central")).unwrap();
    assert_eq!(statuses.len(), 1);
    assert_review_only(&statuses[0]);
    let path = root
        .path()
        .join("central")
        .join(setup::preparation_name(&id));
    assert!(std::fs::read(&path).unwrap().is_empty());
    // With no local stage or canonical transaction, an abandoned name can be reclaimed.
    drop(create(root.path(), &parent, &central).unwrap());
    assert!(std::fs::read(path).unwrap().is_empty());
}

#[test]
fn windows_native_copy_unpublished_setup_hard_kill_stays_visible_without_actions() {
    use super::claim_processes::OwnedChild;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    const ROOT: &str = "OTTRIN_TEST_UNPUBLISHED_SETUP_ROOT";
    const POINT: &str = "OTTRIN_TEST_UNPUBLISHED_SETUP_POINT";
    const SELF: &str = "copy_transaction::windows::journal::tests::initialization::windows_native_copy_unpublished_setup_hard_kill_stays_visible_without_actions";
    if let Some(root) = std::env::var_os(ROOT) {
        let root = Path::new(&root);
        let point = match std::env::var(POINT).unwrap().as_str() {
            "local" => setup::Point::LocalCreated,
            "paired" => setup::Point::HeadersFlushed,
            _ => panic!("invalid setup barrier"),
        };
        let ready = root.join("ready");
        setup::HANDOFF.with(|h| {
            *h.borrow_mut() = Some((
                point,
                Box::new(move |_, _, _| {
                    std::fs::write(&ready, b"ready").unwrap();
                    let deadline = Instant::now() + Duration::from_secs(60);
                    while Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    panic!("parent did not kill setup actor");
                }),
            ))
        });
        let parent = owner(&root.join("destination"));
        let central = owner(&root.join("central"));
        let _stage = create(root, &parent, &central).unwrap();
        panic!("setup barrier returned");
    }
    for point in ["local", "paired"] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", SELF, "--nocapture"])
                .env(ROOT, root.path())
                .env(POINT, point)
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        while !root.path().join("ready").exists() {
            assert!(Instant::now() < deadline, "setup actor not ready");
            assert!(child.0.try_wait().unwrap().is_none(), "setup actor exited");
            std::thread::sleep(Duration::from_millis(10));
        }
        let registry = root.path().join("central");
        let live = discover_at(&registry).unwrap();
        assert_eq!(live.len(), 1);
        assert_review_only(&live[0]);
        let path = live[0].transaction.as_ref().unwrap();
        let before = std::fs::read(path).unwrap();
        assert_eq!(before.is_empty(), point == "local");
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let statuses = discover_at(&registry).unwrap();
        assert_eq!(statuses.len(), 1);
        assert_review_only(&statuses[0]);
        assert_eq!(std::fs::read(path).unwrap(), before);
        let id = path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .strip_prefix(".preparing-")
            .unwrap();
        assert!(restore_resume_at(&registry, id).is_err());
        assert!(restore_undo_at(&registry, id).is_err());
        assert!(!root.path().join("destination/copy").exists());
        assert_eq!(
            std::fs::read(root.path().join("source/nested/file")).unwrap(),
            b"original data"
        );
    }
}
