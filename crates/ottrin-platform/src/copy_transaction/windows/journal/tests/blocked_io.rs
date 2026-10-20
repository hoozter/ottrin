use super::*;
use std::{
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{ERROR_IO_PENDING, ERROR_NOT_FOUND, ERROR_OPERATION_ABORTED, WAIT_OBJECT_0},
    Storage::FileSystem::FILE_FLAG_OVERLAPPED,
    System::{
        IO::{CancelIoEx, DeviceIoControl, GetOverlappedResult, OVERLAPPED},
        Ioctl::FSCTL_REQUEST_OPLOCK_LEVEL_1,
        Threading::{CreateEventW, WaitForSingleObject},
    },
};

// The heap-owned OVERLAPPED and event live until the request has completed.
// Closing the file implicitly acknowledges the break, including failure teardown.
struct Oplock {
    file: Option<File>,
    event: OwnedHandle,
    overlapped: Box<OVERLAPPED>,
    pending: bool,
}
impl Oplock {
    fn request(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OVERLAPPED | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if event.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let event = unsafe { OwnedHandle::from_raw_handle(event.cast()) };
        let mut result = Self {
            file: Some(file),
            overlapped: Box::new(OVERLAPPED {
                hEvent: event.as_raw_handle().cast(),
                ..Default::default()
            }),
            event,
            pending: false,
        };
        let mut bytes = 0;
        let ok = unsafe {
            DeviceIoControl(
                result.file.as_ref().unwrap().as_raw_handle().cast(),
                FSCTL_REQUEST_OPLOCK_LEVEL_1,
                ptr::null(),
                0,
                ptr::null_mut(),
                0,
                &mut bytes,
                result.overlapped.as_mut(),
            )
        };
        let error = std::io::Error::last_os_error();
        if ok != 0 {
            return Err(std::io::Error::other(
                "Oplock request completed without a pending lease",
            ));
        }
        if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
            return Err(error);
        }
        result.pending = true;
        Ok(result)
    }
    fn wait_break(&self) -> u32 {
        unsafe { WaitForSingleObject(self.event.as_raw_handle().cast(), 3000) }
    }
    fn close(&mut self) -> std::io::Result<()> {
        let mut result = Ok(());
        if let Some(file) = &self.file
            && self.pending
        {
            if unsafe { CancelIoEx(file.as_raw_handle().cast(), self.overlapped.as_ref()) } == 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(ERROR_NOT_FOUND as i32) {
                    result = Err(error);
                }
            }
            let mut bytes = 0;
            if unsafe {
                GetOverlappedResult(
                    file.as_raw_handle().cast(),
                    self.overlapped.as_mut(),
                    &mut bytes,
                    1,
                )
            } == 0
            {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(ERROR_OPERATION_ABORTED as i32) {
                    result = Err(error);
                }
            }
            self.pending = false;
        }
        drop(self.file.take());
        result
    }
}
impl Drop for Oplock {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            eprintln!("Owned oplock teardown failed: {error}");
        }
    }
}

enum Recovery {
    Forward(discovery::RetainedCopyResumePlan),
    Cleanup(Box<RetainedCopyCleanupPlan>),
}
impl Recovery {
    fn run(
        &mut self,
        progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        match self {
            Self::Forward(plan) => plan.resume(progress),
            Self::Cleanup(plan) => plan.reclaim(progress),
        }
    }
    fn complete(&self) -> bool {
        match self {
            Self::Forward(plan) => plan.complete(),
            Self::Cleanup(plan) => plan.complete(),
        }
    }
}
#[test]
fn windows_native_copy_oplock_cancellation_keeps_recovery_bytes_and_retries_source_free() {
    blocked_cases(false);
}
#[test]
fn windows_native_copy_oplock_cancellation_keeps_cleanup_bytes_and_retries_source_free() {
    blocked_cases(true);
}
fn blocked_cases(cleanup: bool) {
    for blocked in ["central", "local", "payload"] {
        let root = tempfile::tempdir().unwrap();
        let registry = root.path().join("central");
        let destination = root.path().join("destination");
        let (name, candidate, _cleanup) =
            staged_resume_fixture_at(root.path(), &destination, true, false, Some(&registry));
        remove_undo_fixture_source(root.path());
        let local = destination.join(&name).join(LOCAL);
        let header = decode(&std::fs::read(&local).unwrap())
            .unwrap()
            .state
            .header
            .unwrap();
        let central = registry.join(header.central_name());
        let payload = destination.join(&candidate).join("nested/file");
        std::fs::write(destination.join("foreign"), b"keep unrelated data").unwrap();
        let mut plan = if cleanup {
            let parent = owner(&destination);
            let central = owner(&registry);
            let mut publication =
                PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
            publication.apply(|_| Ok(())).unwrap();
            drop(publication);
            undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
            drop((parent, central));
            Recovery::Cleanup(Box::new(
                RetainedCopyCleanupPlan::observe_at(
                    &registry,
                    std::slice::from_ref(&header.transaction),
                    |_| Ok(()),
                )
                .unwrap(),
            ))
        } else {
            Recovery::Forward(
                discovery::RetainedCopyResumePlan::admit(
                    &registry,
                    &header.transaction,
                    |_| Ok(()),
                )
                .unwrap(),
            )
        };
        let before_local = std::fs::read(&local).unwrap();
        let before_central = std::fs::read(&central).unwrap();
        let before_payload = std::fs::read(&payload).unwrap();
        let mut lock = Oplock::request(match blocked {
            "central" => &central,
            "local" => &local,
            _ => &payload,
        })
        .unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancelled.clone();
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let guard = crate::NativeIoCancellation::start(worker_cancel.clone()).unwrap();
            let result = plan.run(|_| {
                if worker_cancel.load(Ordering::Acquire) {
                    Err(io("stopped between native calls"))
                } else {
                    Ok(())
                }
            });
            let finish = guard.finish();
            tx.send((plan, result, finish)).unwrap();
        });
        let broken = lock.wait_break();
        let pending = rx.recv_timeout(Duration::from_millis(100));
        let start = Instant::now();
        cancelled.store(true, Ordering::Release);
        let stopped = rx.recv_timeout(Duration::from_secs(3));
        let elapsed = start.elapsed();
        // Unblock the worker even if a cancellation assertion will fail. The
        // overlapped request is completed before its buffers can be freed.
        let close = lock.close();
        let joined = worker.join();
        close.unwrap();
        joined.unwrap();
        assert_eq!(broken, WAIT_OBJECT_0, "{blocked}: no real oplock break");
        assert!(
            matches!(pending, Err(mpsc::RecvTimeoutError::Timeout)),
            "{blocked}: operation was not blocked"
        );
        let (mut plan, result, finish) = stopped.unwrap();
        finish.unwrap();
        assert!(!plan.complete());
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("995"),
            "{blocked}: expected native operation-aborted, got {error}"
        );
        println!(
            "oplock cancellation: cleanup={cleanup} target={blocked} elapsed_ms={}",
            elapsed.as_millis()
        );
        assert_eq!(std::fs::read(&local).unwrap(), before_local);
        assert_eq!(std::fs::read(&central).unwrap(), before_central);
        assert_eq!(std::fs::read(&payload).unwrap(), before_payload);
        assert!(!destination.join("copy").exists());
        let plan = std::thread::spawn(move || {
            let guard =
                crate::NativeIoCancellation::start(Arc::new(AtomicBool::new(false))).unwrap();
            let result = plan.run(|_| Ok(()));
            guard.finish().unwrap();
            result.unwrap();
            plan
        })
        .join()
        .unwrap();
        assert!(plan.complete());
        if cleanup {
            assert!(!payload.exists());
            assert!(!local.exists());
            assert!(discover_at(&registry).unwrap().is_empty());
        } else {
            assert_eq!(
                std::fs::read(destination.join("copy/nested/file")).unwrap(),
                before_payload
            );
        }
        assert_eq!(
            std::fs::read(destination.join("foreign")).unwrap(),
            b"keep unrelated data"
        );
        assert!(!root.path().join("source").exists());
    }
}
