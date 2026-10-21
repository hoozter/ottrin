//! Real NTFS allocation failure on an explicitly owned small secondary volume.
use super::*;
use std::{
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    sync::{Arc, Mutex},
};
use windows_sys::Win32::{
    Foundation::{ERROR_DISK_FULL, ERROR_HANDLE_DISK_FULL},
    Storage::FileSystem::{
        FILE_ALLOCATION_INFO, FILE_FLAG_WRITE_THROUGH, FILE_STANDARD_INFO, FileAllocationInfo,
        FileStandardInfo, GetDiskFreeSpaceExW, GetFileInformationByHandleEx,
        SetFileInformationByHandle,
    },
};

fn space(path: &Path) -> (u64, u64) {
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let (mut available, mut capacity) = (0, 0);
    assert_ne!(
        unsafe {
            GetDiskFreeSpaceExW(
                path.as_ptr(),
                &mut available,
                &mut capacity,
                ptr::null_mut(),
            )
        },
        0,
        "{}",
        std::io::Error::last_os_error()
    );
    (available, capacity)
}

struct Filler(Arc<Mutex<Option<File>>>);
impl Filler {
    fn release(&mut self) -> std::io::Result<()> {
        // Recover a poisoned mutex too: an assertion in the filling hook must
        // still release its allocation before TempDir removes fixture entries.
        let file = self
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(file) = file {
            file.set_len(0)?;
        }
        Ok(())
    }
}
impl Drop for Filler {
    fn drop(&mut self) {
        if let Err(error) = self.release() {
            eprintln!("Could not release owned filler allocation: {error}");
        }
    }
}

fn reserve_allocation(file: &File, spare: i64) {
    let before = read(file).unwrap();
    let info = FILE_ALLOCATION_INFO {
        AllocationSize: (before.len() as i64 + 4095) / 4096 * 4096 + spare,
    };
    assert_ne!(
        unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle().cast(),
                FileAllocationInfo,
                ptr::from_ref(&info).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        },
        0,
        "{}",
        std::io::Error::last_os_error()
    );
    file.sync_all().unwrap();
    let mut actual = FILE_STANDARD_INFO::default();
    assert_ne!(
        unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle().cast(),
                FileStandardInfo,
                ptr::from_mut(&mut actual).cast(),
                std::mem::size_of_val(&actual) as u32,
            )
        },
        0
    );
    assert!(actual.AllocationSize >= info.AllocationSize);
    println!(
        "held journal allocation: eof={} allocation={}",
        before.len(),
        actual.AllocationSize
    );
    assert_eq!(
        read(file).unwrap(),
        before,
        "Trimming spare allocation must preserve every journal byte"
    );
}

#[test]
#[ignore = "Requires a token-bound disposable NTFS volume of at most 512 MiB"]
fn windows_native_copy_disk_full_paired_journals_recover_after_space_is_freed() {
    run_case(false);
}
#[test]
#[ignore = "Requires a token-bound disposable NTFS volume of at most 512 MiB"]
fn windows_native_copy_disk_full_divergent_journals_preserve_data_and_refuse_retry() {
    run_case(true);
}
fn run_case(expect_divergence: bool) {
    let volume = std::path::PathBuf::from(
        std::env::var_os("OTTRIN_TEST_WINDOWS_COPY_IO_VOLUME").expect("Owned volume required"),
    );
    let token = std::env::var("OTTRIN_TEST_WINDOWS_COPY_IO_TOKEN").expect("Owner token required");
    assert_eq!(
        std::fs::read_to_string(volume.join(".ottrin-owned-secondary-volume"))
            .unwrap()
            .trim(),
        token
    );
    let (_, capacity) = space(&volume);
    assert!(
        (128 * 1024 * 1024..=512 * 1024 * 1024).contains(&capacity),
        "Refuse to fill any other volume"
    );
    let root = tempfile::tempdir_in(&volume).unwrap();
    fixture(root.path());
    let registry = root.path().join("central");
    std::fs::remove_dir(&registry).unwrap();
    drop(
        super::super::super::registry::AnchoredRoot::open_at(&registry, true)
            .unwrap()
            .unwrap(),
    );
    for index in 0..64 {
        std::fs::write(
            root.path().join(format!("source/nested/item-{index}")),
            format!("payload {index}"),
        )
        .unwrap();
    }
    let parent_path = root.path().join("destination");
    let (name, candidate) = {
        let input = source(&root.path().join("source"));
        let parent = owner(&parent_path);
        let central = owner(&registry);
        let staged = stage(&input, &parent, &central).unwrap();
        (
            staged.stage.name.clone(),
            staged.stage.candidate_name.clone(),
        )
    };
    remove_undo_fixture_source(root.path());
    let local = parent_path.join(&name).join(LOCAL);
    let header = decode(&std::fs::read(&local).unwrap())
        .unwrap()
        .state
        .header
        .unwrap();
    let mirror = registry.join(header.central_name());
    let before = std::fs::read(&local).unwrap();
    assert_eq!(std::fs::read(&mirror).unwrap(), before);
    let payload = parent_path.join(&candidate);
    let payload_before = super::cleanup::namespace_contents(&payload);
    std::fs::write(parent_path.join("foreign"), b"unrelated public data").unwrap();
    let mut plan =
        discovery::RetainedCopyResumePlan::admit(&registry, &header.transaction, |_| Ok(()))
            .unwrap();
    let mut filler = Filler(Arc::new(Mutex::new(Some(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(FILE_FLAG_WRITE_THROUGH)
            .open(root.path().join("owned-filler"))
            .unwrap(),
    ))));
    let retained = filler.0.clone();
    let owned_volume = volume.clone();
    APPEND_HANDOFF.with(|slot| *slot.borrow_mut() = Some(Box::new(move |local, central| {
        // Keep these reservations on the actual live writer objects. Closing a
        // setup writer can discard its unused allocation before the append.
        reserve_allocation(local, if expect_divergence { 64 * 1024 } else { 0 });
        reserve_allocation(central, 0);
        let mut guard = retained.lock().unwrap();let file = guard.as_mut().unwrap();
        let data = vec![73u8; 1024 * 1024];
        let mut written = 0u64;
        for size in [data.len(), 4096] {
            loop {
                match file.write(&data[..size]) {
                    Ok(0) => panic!("Filler write made no progress"),
                    Ok(count) => { written += count as u64;assert!(written <= 1024 * 1024 * 1024, "Filler exceeded owned-volume bound"); },
                    Err(error) => { assert!(matches!(error.raw_os_error(), Some(code) if code == ERROR_DISK_FULL as i32 || code == ERROR_HANDLE_DISK_FULL as i32), "Unexpected filler error: {error}");break; }
                }
            }
        }
        assert!(space(&owned_volume).0 < 4096);
        println!("real disk-full barrier: allocated={} free={}", file.metadata().unwrap().len(), space(&owned_volume).0);
    })));
    let outcome = plan.resume(|_| Ok(()));
    let local_after = std::fs::read(&local).unwrap();
    let central_after = std::fs::read(&mirror).unwrap();
    filler.release().unwrap(); // Restore space before assertions and recovery.
    let error = outcome.unwrap_err().to_string();
    assert!(
        error.contains("(os error 112)") || error.contains("(os error 39)"),
        "Expected native disk-full write error, got {error}"
    );
    assert!(!plan.complete());
    assert!(!parent_path.join("copy").exists());
    assert_eq!(super::cleanup::namespace_contents(&payload), payload_before);
    assert_eq!(
        std::fs::read(parent_path.join("foreign")).unwrap(),
        b"unrelated public data"
    );
    assert!(!root.path().join("source").exists());
    let paired = local_after == central_after && decode(&local_after).is_ok();
    assert_eq!(
        paired, !expect_divergence,
        "The allocation fixture must exercise its requested mirror state"
    );
    println!(
        "disk-full journal result: error={error}; before={}; local={}; central={}; paired={paired}",
        before.len(),
        local_after.len(),
        central_after.len()
    );
    let fresh =
        discovery::RetainedCopyResumePlan::admit(&registry, &header.transaction, |_| Ok(()));
    if paired {
        let mut retry = fresh.unwrap();
        retry.resume(|_| Ok(())).unwrap();
        assert!(retry.complete());
        assert_eq!(
            super::cleanup::namespace_contents(&parent_path.join("copy")),
            payload_before
        );
    } else {
        assert!(
            fresh.is_err(),
            "Divergent or incomplete mirrors grant no write authority"
        );
        assert!(
            plan.resume(|_| Ok(())).is_err(),
            "The retained plan must refuse the uncertain append too"
        );
        assert_eq!(std::fs::read(&local).unwrap(), local_after);
        assert_eq!(std::fs::read(&mirror).unwrap(), central_after);
        assert_eq!(super::cleanup::namespace_contents(&payload), payload_before);
    }
}
