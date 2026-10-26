use crate::daemon_protocol::{
    DaemonRequest, DaemonResponse, PROTOCOL_VERSION, RequestFrame, ResponseFrame, read_frame,
    write_frame,
};
use crate::daemon_transport::{DaemonStream, QueryFinished};
use crate::{SearchIndexDiagnostics, SearchQueryResult};
use ottrin_core::{SearchConfig, SearchQuery};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
#[cfg(unix)]
use std::{thread, time::Instant};
pub(super) const CLIENT_IO_TIMEOUT: Duration = Duration::from_secs(5);
const OPERATION_IO_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);
static NEXT_OPERATION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct SearchDaemonClient {
    #[cfg(unix)]
    socket_path: PathBuf,
    #[cfg(target_os = "windows")]
    identity: crate::IndexDaemonIdentity,
}

impl SearchDaemonClient {
    #[cfg(unix)]
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    pub fn connect_default() -> Result<Self, String> {
        #[cfg(unix)]
        {
            Ok(Self::new(crate::default_daemon_socket_path()?))
        }
        #[cfg(target_os = "windows")]
        {
            Ok(Self {
                identity: crate::IndexDaemonIdentity::for_current_user()?,
            })
        }
    }

    pub fn ping(&self) -> Result<(), String> {
        match self.transact(DaemonRequest::Ping)? {
            DaemonResponse::Pong => Ok(()),
            response => Err(unexpected_response(response)),
        }
    }

    pub fn server_pid(&self) -> Result<u32, String> {
        #[cfg(unix)]
        {
            let stream =
                UnixStream::connect(&self.socket_path).map_err(|error| error.to_string())?;
            let credentials = crate::daemon::peer_credentials(&stream)?;
            if credentials.uid != crate::daemon::current_uid() {
                return Err("index daemon belongs to another user".to_string());
            }
            u32::try_from(credentials.pid)
                .map_err(|_| "index daemon reported an invalid process id".to_string())
        }
        #[cfg(target_os = "windows")]
        {
            Ok(
                crate::WindowsIndexPipeStream::connect(&self.identity, CLIENT_IO_TIMEOUT)
                    .map_err(|e| e.to_string())?
                    .peer_pid(),
            )
        }
    }

    /// Package removal must also release session daemons, without stopping a
    /// portable/AppImage daemon sharing the same user's endpoint.
    #[cfg(target_os = "linux")]
    pub fn stop_native_package_daemon() -> Result<bool, String> {
        let Some(socket) = crate::daemon::existing_daemon_socket_path()? else {
            return Ok(false);
        };
        Self::new(socket).stop_if_executable(std::path::Path::new("/usr/bin/ottrin-indexd"))
    }

    #[cfg(target_os = "linux")]
    pub(super) fn stop_if_executable(&self, expected: &std::path::Path) -> Result<bool, String> {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

        let stream = match UnixStream::connect(&self.socket_path) {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) =>
            {
                return Ok(false);
            }
            Err(error) => return Err(format!("Cannot inspect native index daemon: {error}")),
        };
        let peer = crate::daemon::peer_credentials(&stream)?;
        if peer.uid != crate::daemon::current_uid() || peer.pid <= 0 {
            return Err("Native package daemon peer is not owned by this user".into());
        }
        // Keep both the authenticated connection and a process reference. The
        // shutdown request uses this connection, never a re-opened endpoint.
        // SAFETY: pidfd_open has no pointer arguments; peer.pid is positive.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, peer.pid, 0) };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(false);
            }
            return Err(format!("Cannot retain native daemon process: {error}"));
        }
        // SAFETY: a successful pidfd_open returned an owned descriptor.
        let process = unsafe { OwnedFd::from_raw_fd(fd as i32) };
        let executable = match std::fs::read_link(format!("/proc/{}/exe", peer.pid)) {
            Ok(executable) => executable,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(format!("Cannot identify index executable: {error}")),
        };
        let mut deleted = expected.as_os_str().to_os_string();
        deleted.push(" (deleted)");
        if executable != expected && executable.as_os_str() != deleted {
            return Ok(false);
        }
        // A container or private mount namespace can give an independent
        // executable the same absolute name. Compare the installed entry in
        // both filesystem views, not the running inode: a legitimate native
        // daemon may still hold the preceding package's deleted executable.
        use std::os::unix::fs::MetadataExt;
        let relative = expected
            .strip_prefix("/")
            .map_err(|_| "Native package executable path must be absolute")?;
        let installed = match std::fs::metadata(expected) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(format!("Cannot inspect native package executable: {error}")),
        };
        let peer_installed = match std::fs::metadata(
            std::path::PathBuf::from(format!("/proc/{}/root", peer.pid)).join(relative),
        ) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(format!("Cannot inspect daemon filesystem: {error}")),
        };
        if !installed.is_file()
            || !peer_installed.is_file()
            || installed.dev() != peer_installed.dev()
            || installed.ino() != peer_installed.ino()
        {
            return Ok(false);
        }
        match self.transact_stream(stream, DaemonRequest::PrepareUpgrade)? {
            DaemonResponse::Complete => (),
            response => return Err(unexpected_response(response)),
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("Native index daemon did not exit before package removal".into());
            }
            let mut descriptor = libc::pollfd {
                fd: process.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: descriptor is live and contains a retained process fd.
            let ready =
                unsafe { libc::poll(&mut descriptor, 1, remaining.as_millis().min(30_000) as i32) };
            if ready > 0 && descriptor.revents & libc::POLLIN != 0 {
                return Ok(true);
            }
            if ready < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            }
            if ready != 0 {
                return Err("Cannot observe native daemon process exit".into());
            }
        }
    }

    pub fn diagnostics(&self) -> Result<SearchIndexDiagnostics, String> {
        match self.transact(DaemonRequest::Status)? {
            DaemonResponse::Status { diagnostics } => Ok(*diagnostics),
            response => Err(unexpected_response(response)),
        }
    }

    pub fn query(&self, query: SearchQuery) -> Result<SearchQueryResult, String> {
        match self.transact(DaemonRequest::Query { query })? {
            DaemonResponse::Query { result } => Ok(result),
            response => Err(unexpected_response(response)),
        }
    }

    /// Interrupt the authenticated connection when the UI abandons this query.
    /// The server observes disconnect and cancels its reader/provider before exit.
    pub fn query_cancellable(
        &self,
        query: SearchQuery,
        cancelled: &AtomicBool,
    ) -> Result<SearchQueryResult, String> {
        if cancelled.load(Ordering::Acquire) {
            return Err("Search cancelled".into());
        }
        let stream = self.connect_stream()?;
        let monitor = stream.try_clone().map_err(|error| error.to_string())?;
        let done = AtomicBool::new(false);
        let response = std::thread::scope(|scope| {
            let watcher = std::thread::Builder::new()
                .name("ottrin-query-cancel".into())
                .spawn_scoped(scope, || {
                    while !done.load(Ordering::Acquire) {
                        if cancelled.load(Ordering::Acquire) {
                            crate::daemon_transport::interrupt(&monitor).map_err(|error| {
                                format!("Cannot interrupt search connection: {error}")
                            })?;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Ok::<_, String>(())
                })
                .map_err(|error| format!("Cannot start query monitor: {error}"))?;
            let finished = QueryFinished(&done);
            let response = self.transact_stream(stream, DaemonRequest::Query { query });
            drop(finished);
            watcher
                .join()
                .map_err(|_| "Search cancellation monitor failed".to_string())??;
            if cancelled.load(Ordering::Acquire) {
                return Err("Search cancelled".into());
            }
            response
        })?;
        match response {
            DaemonResponse::Query { result } => Ok(result),
            response => Err(unexpected_response(response)),
        }
    }

    pub fn update_config(&self, config: SearchConfig) -> Result<(), String> {
        self.expect_complete(DaemonRequest::UpdateConfig { config })
    }

    pub fn reconcile(&self) -> Result<(), String> {
        self.run_operation(false)
    }

    pub fn rebuild(&self) -> Result<(), String> {
        self.run_operation(true)
    }

    pub fn clear_storage(&self) -> Result<u64, String> {
        match self.transact(DaemonRequest::ClearStorage)? {
            DaemonResponse::StorageCleared { bytes } => Ok(bytes),
            response => Err(unexpected_response(response)),
        }
    }

    pub fn shutdown(&self) -> Result<(), String> {
        self.expect_complete(DaemonRequest::Shutdown)
    }

    pub fn prepare_upgrade(&self) -> Result<(), String> {
        self.expect_complete(DaemonRequest::PrepareUpgrade)
    }

    /// Wait for persisted index work and filesystem watches to be released before
    /// replacing the daemon. Large watched trees can exceed the request timeout.
    #[cfg(unix)]
    pub fn prepare_upgrade_and_wait(&self) -> Result<(), String> {
        self.prepare_upgrade()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.socket_path.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if self.socket_path.exists() {
            return Err(
                "Ottrin index daemon did not finish preparing for upgrade within 30 seconds"
                    .to_string(),
            );
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn server_process(&self) -> Result<crate::windows_identity::IndexPeerProcess, String> {
        let stream = crate::WindowsIndexPipeStream::connect(&self.identity, CLIENT_IO_TIMEOUT)
            .map_err(|error| error.to_string())?;
        // Keep the authenticated pipe alive until its process object is captured.
        self.identity.open_process(stream.peer_pid())
    }

    /// A nonblocking endpoint-presence check for stopped-state presentation.
    /// Presence alone does not authenticate a daemon; commands still verify its peer.
    #[cfg(target_os = "windows")]
    pub fn endpoint_present(&self) -> Result<bool, String> {
        use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SEM_TIMEOUT};
        use windows_sys::Win32::System::Pipes::{NMPWAIT_NOWAIT, WaitNamedPipeW};
        let name: Vec<u16> = self
            .identity
            .pipe_name()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: name is a live NUL-terminated string; NOWAIT never waits for a busy pipe.
        if unsafe { WaitNamedPipeW(name.as_ptr(), NMPWAIT_NOWAIT) } != 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(code) if code == ERROR_FILE_NOT_FOUND as i32 => Ok(false),
            Some(code) if code == ERROR_SEM_TIMEOUT as i32 => Ok(true),
            _ => Err(format!("Cannot inspect index daemon endpoint: {error}")),
        }
    }

    #[cfg(target_os = "windows")]
    pub fn server_executable(&self) -> Result<PathBuf, String> {
        self.server_process()?.executable()
    }

    /// Wait for the authenticated server process to exit before replacing its files.
    #[cfg(target_os = "windows")]
    pub fn prepare_upgrade_and_wait(&self) -> Result<(), String> {
        let stream = crate::WindowsIndexPipeStream::connect(&self.identity, CLIENT_IO_TIMEOUT)
            .map_err(|error| error.to_string())?;
        let process = self.identity.open_process(stream.peer_pid())?;
        // Send preparation on the same authenticated connection whose process we captured.
        match self.transact_stream(stream, DaemonRequest::PrepareUpgrade)? {
            DaemonResponse::Complete => process.wait_for_exit(Duration::from_secs(30)),
            response => Err(unexpected_response(response)),
        }
    }

    fn run_operation(&self, rebuild: bool) -> Result<(), String> {
        let operation_id = NEXT_OPERATION_ID.fetch_add(1, Ordering::Relaxed);
        let request = if rebuild {
            DaemonRequest::Rebuild { operation_id }
        } else {
            DaemonRequest::Reconcile { operation_id }
        };
        match self.transact(request)? {
            DaemonResponse::OperationComplete {
                operation_id: completed,
            } if completed == operation_id => Ok(()),
            DaemonResponse::OperationComplete {
                operation_id: completed,
            } => Err(format!(
                "index daemon completed operation {completed}, expected {operation_id}"
            )),
            response => Err(unexpected_response(response)),
        }
    }

    fn expect_complete(&self, request: DaemonRequest) -> Result<(), String> {
        match self.transact(request)? {
            DaemonResponse::Complete => Ok(()),
            response => Err(unexpected_response(response)),
        }
    }

    fn transact(&self, request: DaemonRequest) -> Result<DaemonResponse, String> {
        self.transact_stream(self.connect_stream()?, request)
    }

    fn connect_stream(&self) -> Result<DaemonStream, String> {
        #[cfg(unix)]
        let stream = UnixStream::connect(&self.socket_path).map_err(|error| {
            format!(
                "could not connect to Ottrin index daemon at {}: {error}",
                self.socket_path.display()
            )
        })?;
        #[cfg(target_os = "windows")]
        let stream = crate::WindowsIndexPipeStream::connect(&self.identity, CLIENT_IO_TIMEOUT)
            .map_err(|error| error.to_string())?;
        Ok(stream)
    }

    fn transact_stream(
        &self,
        mut stream: DaemonStream,
        request: DaemonRequest,
    ) -> Result<DaemonResponse, String> {
        let read_timeout = if matches!(
            request,
            DaemonRequest::Reconcile { .. } | DaemonRequest::Rebuild { .. }
        ) {
            OPERATION_IO_TIMEOUT
        } else if matches!(request, DaemonRequest::Query { .. }) {
            Duration::from_secs(60)
        } else {
            CLIENT_IO_TIMEOUT
        };
        #[cfg(unix)]
        stream
            .set_read_timeout(Some(read_timeout))
            .and_then(|()| stream.set_write_timeout(Some(CLIENT_IO_TIMEOUT)))
            .map_err(|error| error.to_string())?;
        let write_error = write_frame(
            &mut stream,
            &RequestFrame {
                version: PROTOCOL_VERSION,
                request,
            },
        )
        .err();
        #[cfg(target_os = "windows")]
        stream.set_io_timeout(read_timeout);
        // A full listener can send a rejection immediately after accepting and
        // close before our request finishes writing. Read its bounded error
        // frame, but never accept a successful operation after a failed write.
        let response: ResponseFrame = read_frame(&mut stream)
            .map_err(|read_error| write_error.as_ref().cloned().unwrap_or(read_error))?;
        if let Some(error) = write_error
            && !matches!(response.response, DaemonResponse::Error { .. })
        {
            return Err(error);
        }
        if response.version != PROTOCOL_VERSION {
            return Err(format!(
                "index daemon protocol mismatch: client {PROTOCOL_VERSION}, server {}",
                response.version
            ));
        }
        match response.response {
            DaemonResponse::Error { code, message } => Err(format!("{code}: {message}")),
            response => Ok(response),
        }
    }
}

fn unexpected_response(response: DaemonResponse) -> String {
    format!("unexpected index daemon response: {response:?}")
}

#[cfg(test)]
mod query_lifetime_tests {
    use super::*;
    use ottrin_core::{SearchIndexStatus, SearchResponse, SearchScope, SearchSort};
    use std::time::Instant;

    #[test]
    fn abandoned_remote_query_reaps_provider_on_cancel_and_disconnect() {
        let root = crate::test_temp_dir().join("remote-query-cancel");
        std::fs::create_dir(&root).unwrap();
        for mode in ["cancel", "disconnect"] {
            let marker = root.join(mode);
            #[cfg(unix)]
            let listener =
                std::os::unix::net::UnixListener::bind(root.join(format!("{mode}.sock"))).unwrap();
            #[cfg(unix)]
            listener.set_nonblocking(true).unwrap();
            #[cfg(unix)]
            let client = SearchDaemonClient::new(root.join(format!("{mode}.sock")));
            #[cfg(target_os = "windows")]
            let identity = crate::IndexDaemonIdentity::isolated_for_test();
            #[cfg(target_os = "windows")]
            let mut listener = crate::WindowsIndexPipeListener::bind(identity.clone()).unwrap();
            #[cfg(target_os = "windows")]
            let client = SearchDaemonClient { identity };
            let cancelled = AtomicBool::new(false);
            let shutdown = AtomicBool::new(false);
            let query = SearchQuery {
                text: "needle".into(),
                scope: SearchScope::Global,
                root_path: None,
                include_hidden_system: false,
                sort: SearchSort::Name,
                limit: 1,
                offset: 0,
                content_search: true,
            };
            let started = Instant::now();
            let deadline = started + Duration::from_secs(10);
            std::thread::scope(|scope| {
                let server = scope.spawn(|| {
                    #[cfg(unix)]
                    let mut stream = {
                        let deadline = Instant::now() + Duration::from_secs(5);
                        loop {
                            match listener.accept() {
                                Ok((stream, _)) => break stream,
                                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                    assert!(Instant::now() < deadline);
                                    std::thread::sleep(Duration::from_millis(5));
                                }
                                Err(error) => panic!("accept query: {error}"),
                            }
                        }
                    };
                    #[cfg(target_os = "windows")]
                    let mut stream = listener.accept(Duration::from_secs(5)).unwrap().unwrap();
                    crate::daemon_transport::monitor_timeout(&mut stream, Duration::from_secs(5))
                        .unwrap();
                    let frame: RequestFrame = read_frame(&mut stream).unwrap();
                    assert!(matches!(frame.request, DaemonRequest::Query { .. }));
                    crate::daemon_server::with_connected_query(&stream, &shutdown, |cancelled| {
                        let mut command = crate::internal_command(std::env::current_exe().unwrap());
                        command
                            .args([
                                "--exact",
                                "tests::content_provider_wait_fixture",
                                "--nocapture",
                            ])
                            .env("OTTRIN_TEST_CONTENT_WAIT", &marker);
                        let error =
                            crate::run_content_provider(command, Some(cancelled)).unwrap_err();
                        assert_eq!(error, "Search cancelled");
                        crate::SearchQueryResult {
                            response: SearchResponse {
                                items: Vec::new(),
                                total: 0,
                                status: SearchIndexStatus::Unavailable,
                                error: Some(error),
                            },
                            warning: None,
                        }
                    })
                    .unwrap()
                });
                if mode == "cancel" {
                    scope.spawn(|| {
                        while !marker.exists() && Instant::now() < deadline {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        cancelled.store(true, Ordering::Release);
                    });
                    assert_eq!(
                        client.query_cancellable(query, &cancelled).unwrap_err(),
                        "Search cancelled"
                    );
                } else {
                    let mut stream = client.connect_stream().unwrap();
                    write_frame(
                        &mut stream,
                        &RequestFrame {
                            version: PROTOCOL_VERSION,
                            request: DaemonRequest::Query { query },
                        },
                    )
                    .unwrap();
                    while !marker.exists() && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    drop(stream);
                }
                assert!(marker.exists(), "provider never drained both pipes");
                assert_eq!(
                    server.join().unwrap().response.error.as_deref(),
                    Some("Search cancelled")
                );
            });
            assert!(
                started.elapsed() < Duration::from_secs(12),
                "remote query retained its provider"
            );
        }
    }
}
