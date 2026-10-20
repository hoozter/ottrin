//! Cancellation of pending synchronous I/O on one operation-owned worker thread.
use crate::PlatformError;
use std::{
    marker::PhantomData,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{DuplicateHandle, ERROR_NOT_FOUND},
    System::{
        IO::CancelSynchronousIo,
        Threading::{GetCurrentProcess, GetCurrentThread, THREAD_TERMINATE},
    },
};

fn error(value: impl std::fmt::Display) -> PlatformError {
    PlatformError::Io(format!("Windows operation I/O cancellation: {value}"))
}

/// Binds cancellation to the constructing thread until `finish` (or unwind).
///
/// Use only inside a dedicated operation worker, never on a UI or shared pool
/// thread. Keep the ordinary cancellation callbacks between native calls: Windows
/// may finish an I/O normally despite a cancellation request. `finish` joins the
/// watcher before the worker can execute unrelated I/O. This guard cannot move
/// to another thread. No thread remains after the guarded operation finishes.
#[must_use = "retain this guard through the operation and report finish errors"]
pub struct NativeIoCancellation {
    finished: Option<mpsc::Sender<()>>,
    watcher: Option<JoinHandle<Result<(), PlatformError>>>,
    thread_bound: PhantomData<Rc<()>>,
}

impl NativeIoCancellation {
    pub fn start(cancelled: Arc<AtomicBool>) -> Result<Self, PlatformError> {
        let mut raw = std::ptr::null_mut();
        if unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                GetCurrentThread(),
                GetCurrentProcess(),
                &mut raw,
                THREAD_TERMINATE,
                0,
                0,
            )
        } == 0
        {
            return Err(error(std::io::Error::last_os_error()));
        }
        // DuplicateHandle gives a real owned thread handle, not a thread ID that
        // could be recycled while a delayed cancellation request is in flight.
        let worker = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        let (finished, receiver) = mpsc::channel();
        let watcher = thread::Builder::new()
            .name("ottrin-io-cancel".into())
            .spawn(move || {
                loop {
                    if cancelled.load(Ordering::Acquire)
                        && unsafe { CancelSynchronousIo(worker.as_raw_handle().cast()) } == 0
                    {
                        let cause = std::io::Error::last_os_error();
                        if cause.raw_os_error() != Some(ERROR_NOT_FOUND as i32) {
                            return Err(error(cause));
                        }
                    }
                    match receiver.recv_timeout(Duration::from_millis(20)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
            })
            .map_err(error)?;
        Ok(Self {
            finished: Some(finished),
            watcher: Some(watcher),
            thread_bound: PhantomData,
        })
    }

    /// Stop cancellation requests and join the watcher before unrelated work.
    pub fn finish(mut self) -> Result<(), PlatformError> {
        self.stop()
    }

    fn stop(&mut self) -> Result<(), PlatformError> {
        drop(self.finished.take());
        if let Some(watcher) = self.watcher.take() {
            watcher
                .join()
                .map_err(|_| error("cancellation watcher panicked"))??;
        }
        Ok(())
    }
}

impl Drop for NativeIoCancellation {
    fn drop(&mut self) {
        if let Err(cause) = self.stop() {
            tracing::error!(%cause, "Could not finish native I/O cancellation during unwind");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use windows_sys::Win32::{
        Foundation::ERROR_OPERATION_ABORTED,
        Storage::FileSystem::{ReadFile, WriteFile},
    };

    // Keep both pipe ends owned until each worker is joined, including assertion
    // failures. Closing the unused peer is the failure fallback for blocked I/O.
    fn blocked_io(write: bool, early_cancel: bool, drop_guard: bool) {
        let (reader, writer) = std::io::pipe().unwrap();
        let (next_reader, mut next_writer) = std::io::pipe().unwrap();
        let flag = Arc::new(AtomicBool::new(early_cancel));
        let worker_flag = flag.clone();
        let (ready_tx, ready) = mpsc::channel();
        let (result_tx, result) = mpsc::channel();
        let (worker_reader, worker_writer, peer_reader, peer_writer) = if write {
            (None, Some(writer), Some(reader), None)
        } else {
            (Some(reader), None, None, Some(writer))
        };
        let worker = thread::spawn(move || {
            let guard = NativeIoCancellation::start(worker_flag).unwrap();
            ready_tx.send(1).unwrap();
            let mut count = 0;
            let mut byte = [0u8];
            let ok = if write {
                let writer = worker_writer.unwrap();
                let data = vec![37u8; 2 * 1024 * 1024];
                unsafe {
                    WriteFile(
                        writer.as_raw_handle().cast(),
                        data.as_ptr(),
                        data.len() as u32,
                        &mut count,
                        std::ptr::null_mut(),
                    )
                }
            } else {
                unsafe {
                    ReadFile(
                        worker_reader.as_ref().unwrap().as_raw_handle().cast(),
                        byte.as_mut_ptr(),
                        1,
                        &mut count,
                        std::ptr::null_mut(),
                    )
                }
            };
            let code = if ok == 0 {
                std::io::Error::last_os_error().raw_os_error()
            } else {
                None
            };
            if drop_guard {
                drop(guard);
            } else {
                guard.finish().unwrap();
            }
            result_tx.send((ok, code, count, byte[0])).unwrap();
            ready_tx.send(2).unwrap();
            // The same worker must be free to do later I/O even though its old
            // operation's flag remains true. A stale watcher would cancel this.
            let ok = unsafe {
                ReadFile(
                    next_reader.as_raw_handle().cast(),
                    byte.as_mut_ptr(),
                    1,
                    &mut count,
                    std::ptr::null_mut(),
                )
            };
            let code = if ok == 0 {
                std::io::Error::last_os_error().raw_os_error()
            } else {
                None
            };
            result_tx.send((ok, code, count, byte[0])).unwrap();
        });
        let started = ready.recv_timeout(Duration::from_secs(3));
        let pending = if !early_cancel {
            Some(result.recv_timeout(Duration::from_millis(100)))
        } else {
            None
        };
        flag.store(true, Ordering::Release);
        let first = result.recv_timeout(Duration::from_secs(3));
        let next = ready.recv_timeout(Duration::from_secs(3));
        let later_pending = result.recv_timeout(Duration::from_millis(100));
        let sent = next_writer.write_all(&[91]);
        // Release all main-thread peers before joining, even if cancellation did
        // not work. No test failure may strand its private worker or pipe.
        drop((peer_reader, peer_writer, next_writer));
        let second = result.recv_timeout(Duration::from_secs(3));
        let joined = worker.join();
        assert_eq!(started.unwrap(), 1);
        if let Some(pending) = pending {
            assert!(matches!(pending, Err(mpsc::RecvTimeoutError::Timeout)));
        }
        let first = first.unwrap();
        assert_eq!(first.0, 0);
        assert_eq!(first.1, Some(ERROR_OPERATION_ABORTED as i32));
        assert_eq!(next.unwrap(), 2);
        assert!(matches!(
            later_pending,
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        sent.unwrap();
        let second = second.unwrap();
        assert_ne!(second.0, 0);
        assert_eq!((second.2, second.3), (1, 91));
        joined.unwrap();
    }

    #[test]
    fn windows_native_copy_io_cancellation_interrupts_blocked_read_and_releases_thread() {
        blocked_io(false, false, false);
    }
    #[test]
    fn windows_native_copy_io_cancellation_interrupts_blocked_write_and_releases_thread() {
        blocked_io(true, false, false);
    }
    #[test]
    fn windows_native_copy_io_cancellation_covers_early_request_and_drop() {
        for _ in 0..8 {
            blocked_io(false, true, true);
        }
    }
}
