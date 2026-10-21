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
        "Changing spare allocation must preserve every journal byte"
    );
}

#[test]
#[ignore = "Requires a token-bound disposable NTFS volume of at most 512 MiB"]
fn windows_native_copy_disk_full_paired_journals_recover_after_space_is_freed() {
    run_case(Layout::Paired);
}
#[test]
#[ignore = "Requires a token-bound disposable NTFS volume of at most 512 MiB"]
fn windows_native_copy_disk_full_divergent_journals_preserve_data_and_refuse_retry() {
    run_case(Layout::ReservedLocal);
}
#[test]
#[ignore = "Requires a token-bound disposable NTFS volume and a separate NTFS temp volume"]
fn windows_native_copy_destination_full_with_separate_registry_recovers() {
    run_case(Layout::DestinationFull);
}
#[test]
#[ignore = "Requires a token-bound disposable NTFS volume and a separate NTFS temp volume"]
fn windows_native_copy_registry_full_with_separate_destination_preserves_and_refuses() {
    run_case(Layout::RegistryFull);
}
#[derive(Clone, Copy, Debug)]
enum Layout {
    Paired,
    ReservedLocal,
    DestinationFull,
    RegistryFull,
}
fn owned_volume() -> std::path::PathBuf {
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
    volume
}

fn run_case(layout: Layout) {
    let split = matches!(layout, Layout::DestinationFull | Layout::RegistryFull);
    let expect_divergence = matches!(layout, Layout::ReservedLocal | Layout::RegistryFull);
    let volume = owned_volume();
    let full_root = tempfile::tempdir_in(&volume).unwrap();
    let other_root = split.then(|| tempfile::tempdir().unwrap());
    let root = if matches!(layout, Layout::RegistryFull) {
        other_root.as_ref().unwrap().path()
    } else {
        full_root.path()
    };
    fixture(root);
    std::fs::remove_dir(root.join("central")).unwrap();
    let registry = if matches!(layout, Layout::DestinationFull) {
        other_root.as_ref().unwrap().path().join("central")
    } else {
        full_root.path().join("central")
    };
    drop(
        super::super::super::registry::AnchoredRoot::open_at(&registry, true)
            .unwrap()
            .unwrap(),
    );
    for index in 0..64 {
        std::fs::write(
            root.join(format!("source/nested/item-{index}")),
            format!("payload {index}"),
        )
        .unwrap();
    }
    let parent_path = root.join("destination");
    let (name, candidate) = {
        let input = source(&root.join("source"));
        let parent = owner(&parent_path);
        let central = owner(&registry);
        let staged = stage(&input, &parent, &central).unwrap();
        (
            staged.stage.name.clone(),
            staged.stage.candidate_name.clone(),
        )
    };
    remove_undo_fixture_source(root);
    let local = parent_path.join(&name).join(LOCAL);
    let header = decode(&std::fs::read(&local).unwrap())
        .unwrap()
        .state
        .header
        .unwrap();
    let mirror = registry.join(header.central_name());
    let parent_id = stable_identity_from_handle(&owner(&parent_path)).unwrap();
    let registry_id = stable_identity_from_handle(&owner(&registry)).unwrap();
    assert_eq!(
        parent_id.volume_serial_number() != registry_id.volume_serial_number(),
        split,
        "Requested native volume separation must be real"
    );
    println!(
        "disk-full layout={layout:?} parent_volume={} registry_volume={}",
        parent_id.volume_serial_number(),
        registry_id.volume_serial_number()
    );
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
            .open(full_root.path().join("owned-filler"))
            .unwrap(),
    ))));
    let retained = filler.0.clone();
    let owned_volume = volume.clone();
    APPEND_HANDOFF.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move |local, central| {
            // Keep these reservations on the actual live writer objects. Closing a
            // setup writer can discard its unused allocation before the append.
            reserve_allocation(
                local,
                if matches!(layout, Layout::ReservedLocal) {
                    64 * 1024
                } else {
                    0
                },
            );
            reserve_allocation(central, 0);
            let mut guard = retained.lock().unwrap();
            let file = guard.as_mut().unwrap();
            fill(file, &owned_volume);
        }))
    });
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
    assert!(!root.join("source").exists());
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

fn fill(file: &mut File, volume: &Path) {
    let data = vec![73u8; 1024 * 1024];
    let mut written = 0u64;
    for size in [data.len(), 4096] {
        loop {
            match file.write(&data[..size]) {
                Ok(0) => panic!("Filler write made no progress"),
                Ok(count) => {
                    written += count as u64;
                    assert!(
                        written <= 1024 * 1024 * 1024,
                        "Filler exceeded owned-volume bound"
                    );
                }
                Err(error) => {
                    assert!(
                        matches!(error.raw_os_error(), Some(code) if code == ERROR_DISK_FULL as i32 || code == ERROR_HANDLE_DISK_FULL as i32),
                        "Unexpected filler error: {error}"
                    );
                    break;
                }
            }
        }
    }
    assert!(space(volume).0 < 4096);
    println!(
        "real disk-full barrier: allocated={} free={}",
        file.metadata().unwrap().len(),
        space(volume).0
    );
}

#[test]
#[ignore = "Requires a token-bound disposable NTFS volume and a separate NTFS temp volume"]
fn windows_native_copy_payload_disk_full_preserves_source_and_unpublished_stage() {
    let volume = owned_volume();
    let full_root = tempfile::tempdir_in(&volume).unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let original = vec![37u8; super::super::super::BUFFER_BYTES * 8 + 17];
    let source_path = root.path().join("source/nested/file");
    std::fs::write(&source_path, &original).unwrap();
    let named = vec![83u8; super::super::super::BUFFER_BYTES * 4 + 19];
    let named_path = root.path().join("source/nested/file:payload");
    std::fs::write(&named_path, &named).unwrap();
    let destination = full_root.path().join("destination");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("foreign"), b"unrelated public data").unwrap();
    let registry = root.path().join("central");
    std::fs::remove_dir(&registry).unwrap();
    drop(
        super::super::super::registry::AnchoredRoot::open_at(&registry, true)
            .unwrap()
            .unwrap(),
    );
    let mut filler = Filler(Arc::new(Mutex::new(Some(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(FILE_FLAG_WRITE_THROUGH)
            .open(full_root.path().join("owned-filler"))
            .unwrap(),
    ))));
    let mut barrier = None;
    let outcome = {
        let src = source(&root.path().join("source"));
        let parent = owner(&destination);
        let central = owner(&registry);
        let parent_id = stable_identity_from_handle(&parent).unwrap();
        let central_id = stable_identity_from_handle(&central).unwrap();
        assert_ne!(
            parent_id.volume_serial_number(),
            central_id.volume_serial_number()
        );
        println!(
            "payload-full parent_volume={} registry_volume={}",
            parent_id.volume_serial_number(),
            central_id.volume_serial_number()
        );
        let admission = DestinationAdmission::creation(&parent, OsStr::new("copy")).unwrap();
        DurableStage::create(
            &src,
            &parent,
            &central,
            &admission,
            OsStr::new("source"),
            100,
            |bytes| {
                if bytes > 0 && barrier.is_none() {
                    assert!(bytes < original.len() as u64);
                    barrier = Some(bytes);
                    fill(filler.0.lock().unwrap().as_mut().unwrap(), &volume);
                }
                Ok(())
            },
        )
        .map(|_| ())
    };
    filler.release().unwrap();
    let error = outcome.unwrap_err();
    assert!(matches!(error, PlatformError::CleanupPending { .. }));
    let message = error.to_string();
    assert!(
        message.contains("(os error 112)") || message.contains("(os error 39)"),
        "{message}"
    );
    assert!(barrier.is_some());
    assert_eq!(std::fs::read(&source_path).unwrap(), original);
    assert!(!destination.join("copy").exists());
    assert_eq!(
        std::fs::read(destination.join("foreign")).unwrap(),
        b"unrelated public data"
    );
    let name = stage_name(&destination);
    let local = destination.join(&name).join(LOCAL);
    let before = std::fs::read(&local).unwrap();
    let state = decode(&before).unwrap().state;
    let header = state.header.as_ref().unwrap();
    let mirror = registry.join(header.central_name());
    assert_eq!(std::fs::read(&mirror).unwrap(), before);
    assert!(state.nodes.iter().any(|node| node.staged.is_none()));
    let candidate = destination.join(header.candidate_name());
    let partial = std::fs::read(candidate.join("nested/file")).unwrap();
    assert_eq!(partial, original);
    let read_stream = || match std::fs::read(candidate.join("nested/file:payload")) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("Could not inspect retained stream: {error}"),
    };
    let partial_stream = read_stream();
    assert!(
        partial_stream
            .as_ref()
            .is_none_or(|bytes| bytes.len() <= named.len())
    );
    assert_ne!(partial_stream.as_deref(), Some(named.as_slice()));
    assert_eq!(std::fs::read(&named_path).unwrap(), named);
    assert!(
        discovery::RetainedCopyResumePlan::admit(&registry, &header.transaction, |_| Ok(()))
            .is_err()
    );
    assert!(
        PermissionStage::publish(&owner(&destination), &owner(&registry), &name, |_| Ok(()))
            .is_err()
    );
    assert_eq!(std::fs::read(&local).unwrap(), before);
    assert_eq!(std::fs::read(&mirror).unwrap(), before);
    assert_eq!(
        std::fs::read(candidate.join("nested/file")).unwrap(),
        partial
    );
    assert_eq!(read_stream(), partial_stream);
    assert_eq!(std::fs::read(&source_path).unwrap(), original);
    assert_eq!(std::fs::read(&named_path).unwrap(), named);
    assert!(!destination.join("copy").exists());
    println!(
        "payload-full result: error={message}; barrier={}; primary={}; source={}; named_retained={}; named_source={}; journals={}; publication refused with bytes preserved",
        barrier.unwrap(),
        partial.len(),
        original.len(),
        partial_stream.as_ref().map_or(0, Vec::len),
        named.len(),
        before.len()
    );
}
