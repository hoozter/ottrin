//! Kernel probes shared by the ordinary positive controls and sandbox actor.
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_NO_MORE_ITEMS, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Diagnostics::ProcessSnapshotting::{
    HPSS, HPSSWALK, PSS_CAPTURE_HANDLES, PSS_HANDLE_ENTRY, PSS_WALK_HANDLES, PssCaptureSnapshot,
    PssFreeSnapshot, PssWalkMarkerCreate, PssWalkMarkerFree, PssWalkSnapshot,
};
use windows_sys::Win32::System::Pipes::{
    CreateNamedPipeW, PIPE_READMODE_MESSAGE, PIPE_TYPE_MESSAGE, PIPE_UNLIMITED_INSTANCES,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

/// Inspect our own valid handle table without triggering strict invalid-handle
/// exceptions. PSS_CAPTURE_HANDLES does not clone the process or capture memory.
pub fn current_process_has_handle(expected: HANDLE) -> bool {
    struct Snapshot {
        capture: HPSS,
        walk: HPSSWALK,
    }
    impl Drop for Snapshot {
        fn drop(&mut self) {
            // SAFETY: both handles are uniquely owned PSS objects from this call.
            unsafe {
                if !self.walk.is_null() {
                    assert_eq!(PssWalkMarkerFree(self.walk), 0);
                }
                if !self.capture.is_null() {
                    assert_eq!(PssFreeSnapshot(GetCurrentProcess(), self.capture), 0);
                }
            }
        }
    }
    let mut snapshot = Snapshot {
        capture: std::ptr::null_mut(),
        walk: std::ptr::null_mut(),
    };
    // SAFETY: current process and writable PSS outputs; no foreign handles queried.
    unsafe {
        assert_eq!(
            PssCaptureSnapshot(
                GetCurrentProcess(),
                PSS_CAPTURE_HANDLES,
                0,
                &mut snapshot.capture
            ),
            0
        );
        assert_eq!(PssWalkMarkerCreate(std::ptr::null(), &mut snapshot.walk), 0);
    }
    loop {
        let mut entry = PSS_HANDLE_ENTRY::default();
        // SAFETY: live snapshot/marker and the correctly sized output structure.
        let result = unsafe {
            PssWalkSnapshot(
                snapshot.capture,
                PSS_WALK_HANDLES,
                snapshot.walk,
                (&mut entry as *mut PSS_HANDLE_ENTRY).cast(),
                std::mem::size_of::<PSS_HANDLE_ENTRY>() as u32,
            )
        };
        if result == ERROR_NO_MORE_ITEMS {
            return false;
        }
        assert_eq!(result, 0, "Current-process handle snapshot failed");
        if entry.Handle == expected {
            return true;
        }
    }
}

pub fn pipe_creation_error(namespace: &str) -> u32 {
    let name: Vec<u16> = format!(
        r"\\.\pipe\{namespace}Ottrin-Office-fixture-{}",
        std::process::id()
    )
    .encode_utf16()
    .chain(Some(0))
    .collect();
    // SAFETY: the unique pipe name is live and NUL-terminated; no client connects.
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE,
            PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            std::ptr::null(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        unsafe { GetLastError() }
    } else {
        unsafe {
            CloseHandle(handle);
        }
        0
    }
}
