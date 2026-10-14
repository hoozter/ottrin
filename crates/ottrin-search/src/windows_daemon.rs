use crate::daemon_client::CLIENT_IO_TIMEOUT;
use crate::daemon_server::serve_frame;
use crate::{IndexDaemonIdentity, SearchService, WindowsIndexPipeListener};
use ottrin_core::SearchConfig;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

/// Run a persistent per-user Windows index worker behind authenticated local IPC.
/// The listener owns the namespace until watchers and the writer lock are released.
pub fn run_windows_search_daemon(config: SearchConfig, store: PathBuf) -> Result<(), String> {
    let identity = IndexDaemonIdentity::for_current_user()?;
    let mut listener =
        WindowsIndexPipeListener::bind(identity).map_err(|error| error.to_string())?;
    let mut service = SearchService::new_with_store_path(config, store);
    service.start();
    if let Some(error) = service
        .diagnostics()
        .last_error
        .filter(|_| service.worker.is_none())
    {
        return Err(error);
    }
    let search = Arc::new(service.handle());
    let service = Arc::new(Mutex::new(service));
    let shutdown = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicUsize::new(0));
    let result = (|| {
        while !shutdown.load(Ordering::Acquire) {
            // Keep one instance free for the next connection. Avoid accepting beyond
            // the transport's instance capacity while workers are still active.
            if active.load(Ordering::Acquire) >= crate::windows_pipe::MAX_INSTANCES as usize - 2 {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            let mut stream = match listener.accept(Duration::from_millis(100)) {
                Ok(Some(stream)) => stream,
                Ok(None) => continue,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::PermissionDenied
                            | std::io::ErrorKind::Interrupted
                            | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(error.to_string()),
            };
            stream.set_io_timeout(CLIENT_IO_TIMEOUT);
            let service = Arc::clone(&service);
            let search = Arc::clone(&search);
            let shutdown = Arc::clone(&shutdown);
            let worker_active = Arc::clone(&active);
            active.fetch_add(1, Ordering::AcqRel);
            let worker = std::thread::Builder::new()
                .name("ottrin-index-client".into())
                .spawn(move || {
                    struct Active(Arc<AtomicUsize>);
                    impl Drop for Active {
                        fn drop(&mut self) {
                            self.0.fetch_sub(1, Ordering::AcqRel);
                        }
                    }
                    let _active = Active(worker_active);
                    let _ = serve_frame(&mut stream, &service, &search, &shutdown);
                });
            if let Err(error) = worker {
                active.fetch_sub(1, Ordering::AcqRel);
                return Err(format!("Cannot start index daemon request worker: {error}"));
            }
        }
        Ok(())
    })();
    shutdown.store(true, Ordering::Release);
    service.lock().expect("search service").stop();
    let deadline = Instant::now() + CLIENT_IO_TIMEOUT;
    while active.load(Ordering::Acquire) != 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    result
}
