use crate::daemon_protocol::{
    DaemonRequest, DaemonResponse, PROTOCOL_VERSION, RequestFrame, ResponseFrame, read_frame,
    write_frame,
};
use crate::{SearchIndexDiagnostics, SearchQueryResult};
use ottrin_core::{SearchConfig, SearchQuery};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
type DaemonStream = UnixStream;
#[cfg(target_os = "windows")]
type DaemonStream = crate::WindowsIndexPipeStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
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
        self.transact_stream(stream, request)
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
