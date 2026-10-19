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
fn windows_native_copy_preheader_errors_remove_empty_setup_and_release_name() {
    for point in [
        setup::Point::LocalCreated,
        setup::Point::ControlsCreated,
        setup::Point::DirectoriesFlushed,
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
            matches!(error, PlatformError::Io(_)),
            "{point:?}: {error:?}"
        );
        assert!(stages(root.path()).is_empty());
        assert_eq!(windows_fs::directory_children(&central).unwrap().len(), 0);
        assert!(!root.path().join("destination/copy").exists());
        assert_eq!(
            std::fs::read(root.path().join("source/nested/file")).unwrap(),
            before
        );
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
        *h.borrow_mut() = Some(Box::new(|_, central, name| {
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
        }))
    });
    assert!(create(root.path(), &parent, &central).is_err());
    drop(reset);
    assert!(stages(root.path()).is_empty());
    let names = windows_fs::directory_children(&central).unwrap();
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
fn windows_native_copy_preheader_cleanup_preserves_unexpected_stage_content_and_claim() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let reset = Reset;
    setup::FAULT.with(|f| f.set(Some(setup::Point::LocalCreated)));
    setup::HANDOFF.with(|h| {
        *h.borrow_mut() = Some(Box::new(|stage, _, _| {
            let mut file =
                windows_fs::create_private(stage, OsStr::new("foreign"), false, FILE_SHARE_READ)
                    .unwrap();
            file.write_all(b"must retain").unwrap();
            file.sync_all().unwrap();
        }))
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
    assert!(!stages[0].join(LOCAL).exists());
    assert!(windows_fs::directory_children(&central).unwrap().is_empty());
    assert!(matches!(
        create(root.path(), &parent, &central),
        Err(PlatformError::DestinationExists(_))
    ));
}
