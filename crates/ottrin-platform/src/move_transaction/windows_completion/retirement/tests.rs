use super::*;
use crate::{ConflictAction, DefaultPlatform, PathMoveUndoReceipt, PlatformOps};

struct Fixture {
    _secondary: Option<crate::tests::TestDir>,
    primary: crate::tests::TestDir,
    previous: Option<PathBuf>,
    receipt: PathMoveUndoReceipt,
    completion: Receipt,
}
impl Fixture {
    fn new(cross: bool) -> Self {
        let primary = crate::tests::TestDir::new("move-record-retirement");
        let root = windows_recovery_root().unwrap();
        let secondary = cross.then(|| {
            crate::tests::TestDir::new_at(
                &PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap()),
                "retirement-destination",
            )
        });
        let previous = replace_test_recovery_root(Some(root.clone()));
        let destination = secondary.as_ref().map_or_else(
            || primary.path().join("destination"),
            |s| s.path().join("destination"),
        );
        let source = primary.path().join("source/entry");
        std::fs::create_dir(source.parent().unwrap()).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&source, b"selected-original").unwrap();
        std::fs::write(destination.join("entry"), b"displaced-original").unwrap();
        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination, ConflictAction::Overwrite)
            .unwrap();
        let receipt = receipt.unwrap();
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
        let completion = list(&root).unwrap().pop().unwrap();
        Self {
            _secondary: secondary,
            primary,
            previous,
            receipt,
            completion,
        }
    }
    fn id(&self) -> [u8; 16] {
        self.completion.id().unwrap()
    }
    fn root(&self) -> PathBuf {
        windows_recovery_root().unwrap()
    }
    fn plan(&self) -> WindowsMoveCleanupPlan {
        WindowsMoveCleanupPlan::capture(self.id()).unwrap().unwrap()
    }
    fn payload(&self) {
        assert_eq!(
            std::fs::read(self.receipt.before_path()).unwrap(),
            b"selected-original"
        );
        assert_eq!(
            std::fs::read(self.receipt.after_path()).unwrap(),
            b"displaced-original"
        );
        DefaultPlatform
            .undo_move_with_receipt(&self.receipt)
            .unwrap();
    }
    fn done(&self) {
        self.payload();
        assert!(
            WindowsMoveCleanupPlan::capture(self.id())
                .unwrap()
                .is_none()
        );
        let plan =
            super::super::super::windows_recovery_plan::WindowsMoveRecoveryPlan::capture(self.id())
                .unwrap();
        assert!(plan.cleanup_complete());
        assert_eq!(plan.paths(), &[self.root()]);
        assert_eq!(
            plan.undo_status_for(&self.receipt),
            Some(move_recovery::WindowsMoveUndoStatus::Complete)
        );
        plan.execute().unwrap();
        assert!(
            find(&self.root(), self.id())
                .unwrap()
                .unwrap()
                .matches(&self.receipt)
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        replace_test_recovery_root(self.previous.take());
    }
}

#[test]
fn windows_native_move_cleanup_retires_only_owned_journals_and_empty_directories() {
    let f = Fixture::new(false);
    let plan = f.plan();
    assert!(plan.paths().len() >= 3);
    let report = plan.execute().unwrap();
    assert_eq!(report.journals_removed, 2);
    assert_eq!(report.directories_removed, 1);
    assert_eq!(report.directories_kept, 0);
    for owner in &f.completion.body.controls {
        assert!(!f.root().join(central_name(owner)).exists());
        assert!(!owner.directory.decode().unwrap().exists());
    }
    assert_eq!(plan.execute().unwrap(), report);
    f.done();
}
#[test]
fn windows_native_move_cleanup_preserves_unknown_files_streams_and_child_permissions() {
    let f = Fixture::new(false);
    let owner = &f.completion.body.controls[0];
    let dir = owner.directory.decode().unwrap();
    let unknown = dir.join("unknown-user-file");
    std::fs::write(&unknown, b"keep-this-file").unwrap();
    std::fs::write(
        format!("{}:private-stream", dir.display()),
        b"keep-directory-stream",
    )
    .unwrap();
    let child = std::fs::File::open(&unknown).unwrap();
    // An inherited child ACL is deliberately outside the admitted private policy.
    let before = security_bytes(&child);
    let id = move_recovery::stable_identity_of_entry(&unknown).unwrap();
    let report = f.plan().execute().unwrap();
    assert_eq!(report.journals_removed, 2);
    assert_eq!(report.directories_kept, 1);
    assert_eq!(std::fs::read(&unknown).unwrap(), b"keep-this-file");
    assert_eq!(
        move_recovery::stable_identity_of_entry(&unknown).unwrap(),
        id
    );
    assert_eq!(security_bytes(&child), before);
    assert_eq!(
        std::fs::read(format!("{}:private-stream", dir.display())).unwrap(),
        b"keep-directory-stream"
    );
    assert_eq!(
        Policy::observe(
            &windows_fs::open_directory(&dir, FILE_GENERIC_READ).unwrap(),
            true
        )
        .unwrap()
        .1,
        State::Full
    );
    f.done();
}
fn security_bytes(file: &File) -> Vec<u8> {
    use windows_sys::Win32::Security::{
        Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
        DACL_SECURITY_INFORMATION,
    };
    let (mut dacl, mut raw) = (ptr::null_mut(), ptr::null_mut());
    assert_eq!(
        unsafe {
            GetSecurityInfo(
                file.as_raw_handle().cast(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut raw,
            )
        },
        0
    );
    assert!(!dacl.is_null());
    let bytes =
        unsafe { std::slice::from_raw_parts(dacl.cast(), (*dacl).AclSize as usize) }.to_vec();
    unsafe { windows_sys::Win32::Foundation::LocalFree(raw) };
    bytes
}
#[test]
fn windows_native_move_cleanup_refuses_replaced_modified_and_extended_journals_without_mutation() {
    for mode in 0..3 {
        let f = Fixture::new(false);
        let plan = f.plan();
        let owner = &f.completion.body.controls[0];
        let central = f.root().join(central_name(owner));
        let local = owner
            .directory
            .decode()
            .unwrap()
            .join(WINDOWS_LOCAL_JOURNAL_NAME);
        let before = std::fs::read(&local).unwrap();
        match mode {
            0 => {
                let bytes = std::fs::read(&central).unwrap();
                std::fs::rename(&central, central.with_extension("held")).unwrap();
                std::fs::write(&central, bytes).unwrap();
            }
            1 => std::fs::write(&central, b"later-unrecognized-data").unwrap(),
            _ => std::fs::write(format!("{}:extra", central.display()), b"unknown-stream").unwrap(),
        }
        let bytes = std::fs::read(&central).unwrap();
        assert!(plan.execute().is_err());
        assert_eq!(std::fs::read(&central).unwrap(), bytes);
        assert_eq!(std::fs::read(local).unwrap(), before);
        assert!(
            !f.root()
                .join(record_name(f.id(), RecordKind::RetirementIntent))
                .exists()
        );
        f.payload();
        if mode == 2 {
            assert_eq!(
                std::fs::read(format!("{}:extra", central.display())).unwrap(),
                b"unknown-stream"
            );
        }
    }
}
struct ResetHook;
impl Drop for ResetHook {
    fn drop(&mut self) {
        WINDOWS_MOVE_HOOK.with(|h| *h.borrow_mut() = None);
    }
}
#[test]
fn windows_native_move_cleanup_freeze_blocks_late_stream_and_directory_writes() {
    let f = Fixture::new(false);
    let dir = f.completion.body.controls[0].directory.decode().unwrap();
    let local = dir.join(WINDOWS_LOCAL_JOURNAL_NAME);
    let _reset = ResetHook;
    WINDOWS_MOVE_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |boundary, _| {
            if boundary == WindowsMoveBoundary::RetirementAccessFrozen {
                assert_eq!(
                    std::fs::write(format!("{}:raced", local.display()), b"new-stream")
                        .unwrap_err()
                        .kind(),
                    std::io::ErrorKind::PermissionDenied
                );
                assert_eq!(
                    std::fs::write(dir.join("raced-file"), b"new-file")
                        .unwrap_err()
                        .kind(),
                    std::io::ErrorKind::PermissionDenied
                );
            }
        }))
    });
    f.plan().execute().unwrap();
    f.done();
}
#[test]
fn windows_native_move_cleanup_damaged_or_replayed_marker_does_not_poison_undo() {
    for replay in [false, true] {
        let f = Fixture::new(false);
        f.plan().execute().unwrap();
        let marker = f
            .root()
            .join(record_name(f.id(), RecordKind::RetirementComplete));
        if replay {
            let bytes = std::fs::read(&marker).unwrap();
            std::fs::rename(&marker, marker.with_extension("held")).unwrap();
            std::fs::write(&marker, bytes).unwrap();
        } else {
            std::fs::write(&marker, b"damaged-cleanup-marker").unwrap();
        }
        assert!(WindowsMoveCleanupPlan::capture(f.id()).is_err());
        let plan = crate::WindowsMoveRecoveryPlan::capture(f.id()).unwrap();
        assert!(plan.cleanup_error().is_some());
        assert_eq!(
            plan.undo_status_for(&f.receipt),
            Some(move_recovery::WindowsMoveUndoStatus::Complete)
        );
        plan.execute().unwrap();
        f.payload();
    }
}
#[test]
fn windows_native_move_cleanup_preserves_writable_mapped_journals_until_unmapped() {
    use std::os::windows::fs::OpenOptionsExt;
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
    for central in [false, true] {
        let f = Fixture::new(false);
        let plan = f.plan();
        let owner = &f.completion.body.controls[0];
        let path = if central {
            f.root().join(central_name(owner))
        } else {
            owner
                .directory
                .decode()
                .unwrap()
                .join(WINDOWS_LOCAL_JOURNAL_NAME)
        };
        let before = std::fs::read(&path).unwrap();
        let file = std::fs::OpenOptions::new()
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
        let mapping = Mapping {
            object,
            view: unsafe { MapViewOfFile(object, FILE_MAP_WRITE, 0, 0, 0) },
        };
        assert!(!mapping.view.Value.is_null());
        drop(file);
        assert!(
            plan.execute().is_err(),
            "mapped journal must remain recoverable"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(
            !f.root()
                .join(record_name(f.id(), RecordKind::RetirementComplete))
                .exists()
        );
        f.payload();
        drop(mapping);
        // The first attempt may already have removed earlier owned records.
        // Fresh admission must resume from the original immutable ownership proof.
        f.plan().execute().unwrap();
        f.done();
    }
}
#[test]
fn windows_native_move_cleanup_follows_renamed_parents_and_keeps_a_renamed_private_folder() {
    let f = Fixture::new(false);
    let owner = &f.completion.body.controls[0];
    let old = owner.directory.decode().unwrap();
    let new_parent = f.primary.path().join("renamed-destination");
    std::fs::rename(old.parent().unwrap(), &new_parent).unwrap();
    let private = new_parent.join("user-renamed-private-folder");
    std::fs::rename(new_parent.join(old.file_name().unwrap()), &private).unwrap();
    let plan = f.plan();
    let report = plan.execute().unwrap();
    assert_eq!(report.directories_kept, 1);
    assert!(private.is_dir());
    assert!(!private.join(WINDOWS_LOCAL_JOURNAL_NAME).exists());
    assert_eq!(
        std::fs::read(new_parent.join("entry")).unwrap(),
        b"displaced-original"
    );
    assert!(WindowsMoveCleanupPlan::capture(f.id()).unwrap().is_none());
    assert_eq!(
        Policy::observe(
            &windows_fs::open_directory(&private, FILE_GENERIC_READ).unwrap(),
            true
        )
        .unwrap()
        .1,
        State::Full
    );
}
#[test]
fn windows_native_move_cleanup_handles_deleted_private_directory_without_adopting_recreated_names()
{
    let f = Fixture::new(false);
    let owner = &f.completion.body.controls[0];
    let dir = owner.directory.decode().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join(WINDOWS_LOCAL_JOURNAL_NAME), b"foreign-new-data").unwrap();
    let report = f.plan().execute().unwrap();
    assert_eq!(report.journals_removed, 1);
    assert_eq!(
        std::fs::read(dir.join(WINDOWS_LOCAL_JOURNAL_NAME)).unwrap(),
        b"foreign-new-data"
    );
    f.done();
}
#[test]
#[ignore = "requires the owned second native Windows volume"]
fn windows_native_cross_move_cleanup_retires_both_owner_pairs_and_keeps_inverse_history() {
    let f = Fixture::new(true);
    let plan = f.plan();
    assert!(
        plan.paths()
            .iter()
            .any(|p| p.to_string_lossy().contains("D:"))
    );
    let report = plan.execute().unwrap();
    assert_eq!(report.journals_removed, 4);
    assert_eq!(report.directories_removed, 2);
    f.done();
    for owner in &f.completion.body.controls {
        assert!(
            WindowsMoveCleanupPlan::capture(owner.transaction)
                .unwrap()
                .is_none()
        );
    }
}
#[test]
fn windows_native_move_cleanup_recovers_after_process_exit() {
    if let Ok(encoded) = std::env::var("OTTRIN_CLEANUP_CHILD") {
        let id: [u8; 16] = serde_json::from_str(&encoded).unwrap();
        WindowsMoveCleanupPlan::capture(id)
            .unwrap()
            .unwrap()
            .execute()
            .unwrap();
        panic!("missed cleanup interruption");
    }
    for boundary in [
        "RetirementIntentPrepared",
        "RetirementIntentPublished",
        "RetirementIntentFlushed",
        "RetirementAccessFrozen",
        "RetirementLocalRemoved",
        "RetirementDirectoryRemoved",
        "RetirementCentralRemoved",
        "RetirementMarkerPrepared",
        "RetirementMarkerPublished",
        "RetirementMarkerFlushed",
    ] {
        let f = Fixture::new(false);
        let status=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","move_transaction::windows_completion::retirement::tests::windows_native_move_cleanup_recovers_after_process_exit","--nocapture"])
            .env("OTTRIN_CLEANUP_CHILD",serde_json::to_string(&f.id()).unwrap())
            .env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT",f.root()).env("OTTRIN_TEST_WINDOWS_MOVE_EXIT_BOUNDARY",boundary).status().unwrap();
        assert_eq!(status.code(), Some(87), "{boundary}");
        if let Some(plan) = WindowsMoveCleanupPlan::capture(f.id()).unwrap() {
            plan.execute().unwrap();
        }
        assert!(
            !std::fs::read_dir(f.root()).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".preparing-")),
            "{boundary}"
        );
        f.done();
    }
}
