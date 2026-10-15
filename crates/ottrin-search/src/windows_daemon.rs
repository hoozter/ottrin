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

/// Start or replace this user's daemon using the executable owned by this package.
/// The GUI and command-line writers use this path rather than opening another writer.
pub fn ensure_windows_search_daemon(
    executable: &std::path::Path,
) -> Result<crate::SearchDaemonClient, String> {
    let client = crate::SearchDaemonClient::connect_default()?;
    if client.ping().is_ok() {
        if same_executable_content(&client.server_executable()?, executable)
            .map_err(|error| format!("Cannot compare index daemon executables: {error}"))?
        {
            return Ok(client);
        }
        client.prepare_upgrade_and_wait()?;
    }
    let child = spawn_daemon_without_inherited_handles(executable)
        .map_err(|error| format!("Cannot start the index daemon: {error}"))?;
    wait_for_daemon(executable, Some(&child))
}

/// A persistent daemon must not retain any caller-owned pipe or file handles.
/// Command::spawn on stable Rust inherits every inheritable Windows handle even
/// with null stdio. That keeps captured CLI output open after the CLI exits.
fn spawn_daemon_without_inherited_handles(
    executable: &std::path::Path,
) -> std::io::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
    };
    let mut application: Vec<u16> = executable.as_os_str().encode_wide().collect();
    if application.contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Index daemon executable contains a null character",
        ));
    }
    let mut command = vec![u16::from(b'"')];
    command.extend_from_slice(&application);
    command.extend("\" --index-daemon".encode_utf16());
    command.push(0);
    application.push(0);
    // SAFETY: these Win32 structures permit zero initialization; cb is set below.
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: buffers/structures remain live, command is writable and terminated.
    // FALSE inheritance excludes all caller handles. Null environment/directory
    // preserve the caller's profile and working directory without copying handles.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NO_WINDOW,
            std::ptr::null(),
            std::ptr::null(),
            &startup,
            &mut process,
        )
    };
    if created == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: successful CreateProcess transfers two distinct owned handles.
    let thread = unsafe { OwnedHandle::from_raw_handle(process.hThread) };
    drop(thread);
    Ok(unsafe { OwnedHandle::from_raw_handle(process.hProcess) })
}

pub(crate) fn wait_for_windows_search_daemon(
    executable: &std::path::Path,
) -> Result<crate::SearchDaemonClient, String> {
    wait_for_daemon(executable, None)
}

fn wait_for_daemon(
    executable: &std::path::Path,
    child: Option<&std::os::windows::io::OwnedHandle>,
) -> Result<crate::SearchDaemonClient, String> {
    let client = crate::SearchDaemonClient::connect_default()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if client.ping().is_ok() {
            // Another window may have won the exclusive pipe bind.
            if same_executable_content(&client.server_executable()?, executable)
                .map_err(|error| format!("Cannot compare index daemon executables: {error}"))?
            {
                return Ok(client);
            }
            return Err("Another Ottrin installation started a different index daemon".to_string());
        }
        if let Some(child) = child {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Foundation::{WAIT_FAILED, WAIT_OBJECT_0};
            use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
            // SAFETY: the owned process handle stays live; zero timeout never blocks.
            let state = unsafe { WaitForSingleObject(child.as_raw_handle(), 0) };
            if state == WAIT_FAILED {
                return Err(std::io::Error::last_os_error().to_string());
            }
            if state == WAIT_OBJECT_0 {
                let mut code = 0;
                // SAFETY: handle remains live and the output pointer is writable.
                if unsafe { GetExitCodeProcess(child.as_raw_handle(), &mut code) } == 0 {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                return Err(format!(
                    "Index daemon exited before becoming ready ({code})"
                ));
            }
        }
        if Instant::now() >= deadline {
            return Err("Ottrin index daemon did not become ready within 30 seconds".to_string());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn same_executable_content(
    left: &std::path::Path,
    right: &std::path::Path,
) -> std::io::Result<bool> {
    use std::io::Read;
    let mut left = std::fs::File::open(left)?;
    let mut right = std::fs::File::open(right)?;
    let length = left.metadata()?.len();
    if length != right.metadata()?.len() {
        return Ok(false);
    }
    let mut remaining = length;
    let mut a = [0u8; 65536];
    let mut b = [0u8; 65536];
    while remaining != 0 {
        let size = remaining.min(a.len() as u64) as usize;
        left.read_exact(&mut a[..size])?;
        right.read_exact(&mut b[..size])?;
        if a[..size] != b[..size] {
            return Ok(false);
        }
        remaining -= size as u64;
    }
    Ok(true)
}
