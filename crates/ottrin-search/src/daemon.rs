use crate::daemon_protocol::{
    DaemonRequest, DaemonResponse, PROTOCOL_VERSION, RequestFrame, ResponseFrame, read_frame,
    write_frame,
};
use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::{SearchHandle, SearchIndexDiagnostics, SearchQueryResult, SearchService};
use ottrin_core::{SearchConfig, SearchQuery};

const MAX_ACTIVE_CLIENTS: usize = 64;
const CLIENT_IO_TIMEOUT: Duration = Duration::from_secs(5);
const OPERATION_IO_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);
static NEXT_OPERATION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct SearchDaemonClient {
    socket_path: PathBuf,
}

impl SearchDaemonClient {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    pub fn connect_default() -> Result<Self, String> {
        Ok(Self::new(default_daemon_socket_path()?))
    }

    pub fn ping(&self) -> Result<(), String> {
        match self.transact(DaemonRequest::Ping)? {
            DaemonResponse::Pong => Ok(()),
            response => Err(unexpected_response(response)),
        }
    }

    pub fn server_pid(&self) -> Result<u32, String> {
        let stream = UnixStream::connect(&self.socket_path).map_err(|error| error.to_string())?;
        let credentials = peer_credentials(&stream)?;
        if credentials.uid != current_uid() {
            return Err("index daemon belongs to another user".to_string());
        }
        u32::try_from(credentials.pid)
            .map_err(|_| "index daemon reported an invalid process id".to_string())
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
        let mut stream = UnixStream::connect(&self.socket_path).map_err(|error| {
            format!(
                "could not connect to Ottrin index daemon at {}: {error}",
                self.socket_path.display()
            )
        })?;
        let read_timeout = if matches!(
            request,
            DaemonRequest::Reconcile { .. } | DaemonRequest::Rebuild { .. }
        ) {
            OPERATION_IO_TIMEOUT
        } else {
            CLIENT_IO_TIMEOUT
        };
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

pub fn default_daemon_socket_path() -> Result<PathBuf, String> {
    let socket_path = daemon_socket_path_for_unix_user(
        // SAFETY: geteuid has no preconditions and no side effects.
        unsafe { libc::geteuid() },
        std::env::var_os("XDG_RUNTIME_DIR")
            .as_deref()
            .map(Path::new),
    )?;
    let runtime = if current_uid() == 0 {
        socket_path.parent()
    } else {
        socket_path.parent().and_then(Path::parent)
    }
    .expect("daemon socket path has a runtime parent");
    let metadata = fs::metadata(runtime).map_err(|error| {
        format!(
            "could not inspect runtime directory {}: {error}",
            runtime.display()
        )
    })?;
    if metadata.uid() != current_uid() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "runtime directory {} must be owned by the current user with mode 0700",
            runtime.display()
        ));
    }
    Ok(socket_path)
}

fn daemon_socket_path_for_unix_user(
    effective_uid: u32,
    xdg_runtime_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    if effective_uid == 0 {
        return Ok(PathBuf::from("/run/ottrin-administrator/index-v1.sock"));
    }
    xdg_runtime_dir
        .map(|runtime| runtime.join("ottrin/index-v1.sock"))
        .ok_or_else(|| {
            "XDG_RUNTIME_DIR is unavailable; cannot authenticate a user daemon".to_string()
        })
}

fn accept_ready_client(listener: &UnixListener) -> std::io::Result<Option<UnixStream>> {
    let mut descriptor = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // Wait for socket readiness instead of waking every 10 ms. Incoming
    // requests wake immediately; the timeout bounds shutdown from a worker.
    // SAFETY: descriptor is initialized and live for the call, and its fd is
    // owned by the listener, which remains alive throughout this function.
    let ready = unsafe { libc::poll(&mut descriptor, 1, 100) };
    if ready < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if ready == 0 {
        return Ok(None);
    }
    if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        return Err(std::io::Error::other(format!(
            "index daemon listener readiness failed: {}",
            descriptor.revents
        )));
    }
    listener.accept().map(|(stream, _)| Some(stream))
}

pub fn run_search_daemon(
    config: SearchConfig,
    store_path: PathBuf,
    socket_path: PathBuf,
) -> Result<(), String> {
    let listener = bind_socket(&socket_path)?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;

    let mut service = SearchService::new_with_store_path(config, store_path);
    service.start();
    if let Some(error) = service
        .diagnostics()
        .last_error
        .filter(|_| service.worker.is_none())
    {
        let _ = fs::remove_file(&socket_path);
        return Err(error);
    }

    let service = Arc::new(Mutex::new(service));
    let search = Arc::new(service.lock().expect("search service").handle());
    let shutdown = Arc::new(AtomicBool::new(false));
    let active_clients = Arc::new(AtomicUsize::new(0));

    while !shutdown.load(Ordering::Acquire) {
        match accept_ready_client(&listener) {
            Ok(Some(mut stream)) => {
                if active_clients.fetch_add(1, Ordering::AcqRel) >= MAX_ACTIVE_CLIENTS {
                    active_clients.fetch_sub(1, Ordering::AcqRel);
                    let _ = write_frame(
                        &mut stream,
                        &ResponseFrame {
                            version: PROTOCOL_VERSION,
                            response: DaemonResponse::Error {
                                code: "server_busy".to_string(),
                                message: format!(
                                    "Ottrin index daemon has reached its {MAX_ACTIVE_CLIENTS}-request limit"
                                ),
                            },
                        },
                    );
                    continue;
                }
                let service = Arc::clone(&service);
                let search = Arc::clone(&search);
                let shutdown = Arc::clone(&shutdown);
                let worker_active_clients = Arc::clone(&active_clients);
                let mut rejected_client = stream.try_clone().ok();
                let worker = thread::Builder::new()
                    .name("ottrin-index-client".to_string())
                    .spawn(move || {
                        let _active = ActiveClient(worker_active_clients);
                        let _ = serve_client(stream, &service, &search, &shutdown);
                    });
                if let Err(error) = worker {
                    active_clients.fetch_sub(1, Ordering::AcqRel);
                    if let Some(ref mut client) = rejected_client {
                        let _ = write_frame(
                            client,
                            &ResponseFrame {
                                version: PROTOCOL_VERSION,
                                response: DaemonResponse::Error {
                                    code: "server_unavailable".to_string(),
                                    message: format!(
                                        "Ottrin index daemon could not start a request worker: {error}"
                                    ),
                                },
                            },
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(error) => {
                shutdown.store(true, Ordering::Release);
                service.lock().expect("search service").stop();
                let _ = fs::remove_file(&socket_path);
                return Err(error.to_string());
            }
        }
    }

    service.lock().expect("search service").stop();
    let deadline = Instant::now() + CLIENT_IO_TIMEOUT;
    while active_clients.load(Ordering::Acquire) != 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let _ = fs::remove_file(&socket_path);
    Ok(())
}

struct ActiveClient(Arc<AtomicUsize>);

impl Drop for ActiveClient {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn serve_client(
    mut stream: UnixStream,
    service: &Arc<Mutex<SearchService>>,
    search: &SearchHandle,
    shutdown: &AtomicBool,
) -> Result<(), String> {
    verify_peer(&stream)?;
    stream
        .set_read_timeout(Some(CLIENT_IO_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(CLIENT_IO_TIMEOUT)))
        .map_err(|error| error.to_string())?;
    let frame: RequestFrame = read_frame(&mut stream)?;
    let response = if frame.version != PROTOCOL_VERSION {
        DaemonResponse::Error {
            code: "protocol_mismatch".to_string(),
            message: format!(
                "client protocol {}, server protocol {PROTOCOL_VERSION}",
                frame.version
            ),
        }
    } else {
        dispatch_request(frame.request, service, search, shutdown)
    };
    write_frame(
        &mut stream,
        &ResponseFrame {
            version: PROTOCOL_VERSION,
            response,
        },
    )
}

fn dispatch_request(
    request: DaemonRequest,
    service: &Arc<Mutex<SearchService>>,
    search: &SearchHandle,
    shutdown: &AtomicBool,
) -> DaemonResponse {
    let result = match request {
        DaemonRequest::Ping => return DaemonResponse::Pong,
        DaemonRequest::Status => {
            return DaemonResponse::Status {
                diagnostics: Box::new(search.diagnostics()),
            };
        }
        DaemonRequest::Query { query } => {
            return DaemonResponse::Query {
                result: search.query_with_diagnostics(query),
            };
        }
        DaemonRequest::UpdateConfig { config } => {
            service
                .lock()
                .expect("search service")
                .update_config(config);
            Ok(())
        }
        DaemonRequest::Reconcile { operation_id } => {
            return complete_operation(
                operation_id,
                service
                    .lock()
                    .expect("search service")
                    .begin_index_reconcile(),
            );
        }
        DaemonRequest::Rebuild { operation_id } => {
            return complete_operation(
                operation_id,
                service
                    .lock()
                    .expect("search service")
                    .begin_rebuild_index(),
            );
        }
        DaemonRequest::ClearStorage => {
            return match service.lock().expect("search service").clear_storage() {
                Ok(bytes) => DaemonResponse::StorageCleared { bytes },
                Err(message) => DaemonResponse::Error {
                    code: "operation_failed".to_string(),
                    message,
                },
            };
        }
        DaemonRequest::Shutdown => {
            shutdown.store(true, Ordering::Release);
            Ok(())
        }
        DaemonRequest::PrepareUpgrade => {
            shutdown.store(true, Ordering::Release);
            Ok(())
        }
    };
    match result {
        Ok(()) => DaemonResponse::Complete,
        Err(message) => DaemonResponse::Error {
            code: "operation_failed".to_string(),
            message,
        },
    }
}

fn complete_operation(
    operation_id: u64,
    started: Result<std::sync::mpsc::Receiver<Result<(), String>>, String>,
) -> DaemonResponse {
    let result = started.and_then(|completion| {
        completion
            .recv()
            .map_err(|_| "Search worker stopped before operation completed".to_string())?
    });
    match result {
        Ok(()) => DaemonResponse::OperationComplete { operation_id },
        Err(message) => DaemonResponse::Error {
            code: "operation_failed".to_string(),
            message,
        },
    }
}

fn bind_socket(path: &Path) -> Result<UnixListener, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "index daemon socket has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let parent_metadata = fs::metadata(parent).map_err(|error| error.to_string())?;
    if parent_metadata.uid() != current_uid() || parent_metadata.permissions().mode() & 0o077 != 0 {
        return Err("index daemon socket directory is not private to the current user".to_string());
    }
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_socket() || metadata.uid() != current_uid() {
            return Err(format!(
                "refusing to replace unsafe daemon socket path {}",
                path.display()
            ));
        }
        if UnixStream::connect(path).is_ok() {
            return Err("Ottrin index daemon is already running".to_string());
        }
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    let listener = UnixListener::bind(path).map_err(|error| error.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    Ok(listener)
}

fn peer_credentials(stream: &UnixStream) -> Result<libc::ucred, String> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `credentials` and `length` are valid writable pointers for SO_PEERCRED.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(credentials)
}

fn verify_peer(stream: &UnixStream) -> Result<(), String> {
    let credentials = peer_credentials(stream)?;
    if credentials.uid != current_uid() {
        return Err("index daemon rejected a client from another user".to_string());
    }
    Ok(())
}

fn current_uid() -> u32 {
    // SAFETY: getuid has no preconditions and no side effects.
    unsafe { libc::getuid() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ottrin_core::{SearchIndexStatus, SearchScope, SearchSort};

    #[test]
    fn administrator_socket_path_ignores_preserved_user_runtime() {
        assert_eq!(
            daemon_socket_path_for_unix_user(0, Some(Path::new("/run/user/1000"))),
            Ok(PathBuf::from("/run/ottrin-administrator/index-v1.sock"))
        );
    }
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn client_reports_the_connected_daemon_process() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ottrin-daemon-peer-{}-{unique}",
            std::process::id()
        ));
        let files = root.join("files");
        let store = root.join("search.db");
        let socket = root.join("runtime/index-v1.sock");
        fs::create_dir_all(&files).expect("create fixture root");
        let config = SearchConfig {
            include_roots: vec![files],
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        };
        let daemon_socket = socket.clone();
        let daemon = thread::spawn(move || run_search_daemon(config, store, daemon_socket));
        wait_for_socket(&socket);
        let client = SearchDaemonClient::new(socket);

        assert_eq!(client.server_pid(), Ok(std::process::id()));

        client.shutdown().expect("shutdown daemon");
        daemon
            .join()
            .expect("join daemon")
            .expect("daemon clean exit");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn daemon_rejects_a_protocol_mismatch_with_the_server_version() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let store = std::env::temp_dir().join(format!(
            "ottrin-daemon-protocol-{}-{unique}.db",
            std::process::id()
        ));
        let service = Arc::new(Mutex::new(SearchService::new_with_store_path(
            SearchConfig::default(),
            store,
        )));
        let search = service.lock().expect("search service").handle();
        let shutdown = AtomicBool::new(false);
        let (mut client, server) = UnixStream::pair().expect("socket pair");

        thread::scope(|scope| {
            let handler = scope.spawn(|| serve_client(server, &service, &search, &shutdown));
            write_frame(
                &mut client,
                &RequestFrame {
                    version: PROTOCOL_VERSION + 1,
                    request: DaemonRequest::Ping,
                },
            )
            .expect("write mismatched request");
            let response: ResponseFrame = read_frame(&mut client).expect("read mismatch response");
            assert_eq!(response.version, PROTOCOL_VERSION);
            assert!(matches!(
                response.response,
                DaemonResponse::Error { ref code, .. } if code == "protocol_mismatch"
            ));
            handler
                .join()
                .expect("join handler")
                .expect("serve mismatch");
        });
        assert!(!shutdown.load(Ordering::Acquire));
    }

    fn query(name: &str) -> SearchQuery {
        SearchQuery {
            text: name.to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: false,
        }
    }

    fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if predicate() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(predicate(), "condition was not met before timeout");
    }

    fn wait_for_socket(path: &Path) {
        wait_until(Duration::from_secs(3), || path.exists());
    }

    #[test]
    fn daemon_survives_client_exit_watches_changes_and_restarts_from_its_snapshot() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ottrin-daemon-integration-{}-{unique}",
            std::process::id()
        ));
        let files = root.join("files");
        let store = root.join("search.db");
        let socket = root.join("runtime").join("index-v1.sock");
        fs::create_dir_all(&files).expect("create fixture root");
        fs::write(files.join("alpha.txt"), "alpha").expect("write alpha");
        let config = SearchConfig {
            include_roots: vec![files.clone()],
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        };

        let first_config = config.clone();
        let first_store = store.clone();
        let first_socket = socket.clone();
        let first =
            thread::spawn(move || run_search_daemon(first_config, first_store, first_socket));
        wait_for_socket(&socket);
        let client_a = SearchDaemonClient::new(socket.clone());
        wait_until(Duration::from_secs(5), || {
            client_a
                .diagnostics()
                .is_ok_and(|status| status.status == SearchIndexStatus::Ready)
        });
        assert_eq!(
            client_a
                .query(query("alpha.txt"))
                .expect("query alpha")
                .response
                .total,
            1
        );
        drop(client_a);

        fs::write(files.join("beta.txt"), "beta").expect("write beta");
        let client_b = SearchDaemonClient::new(socket.clone());
        wait_until(Duration::from_secs(5), || {
            client_b
                .query(query("beta.txt"))
                .is_ok_and(|result| result.response.total == 1)
        });
        client_b
            .prepare_upgrade()
            .expect("prepare first daemon for upgrade");
        first
            .join()
            .expect("join first daemon")
            .expect("first daemon clean exit");

        let second_socket = socket.clone();
        let second = thread::spawn(move || run_search_daemon(config, store, second_socket));
        wait_for_socket(&socket);
        let restarted = SearchDaemonClient::new(socket.clone());
        wait_until(Duration::from_secs(5), || {
            restarted
                .query(query("beta.txt"))
                .is_ok_and(|result| result.response.total == 1)
        });
        restarted.shutdown().expect("shutdown restarted daemon");
        second
            .join()
            .expect("join restarted daemon")
            .expect("restarted daemon clean exit");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn daemon_returns_a_protocol_error_when_the_request_capacity_is_full() {
        const BLOCKED_CLIENTS: usize = MAX_ACTIVE_CLIENTS;
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ottrin-daemon-request-capacity-{}-{unique}",
            std::process::id()
        ));
        let files = root.join("files");
        let store = root.join("search.db");
        let socket = root.join("runtime/index-v1.sock");
        fs::create_dir_all(&files).expect("create fixture root");
        fs::write(files.join("needle.txt"), []).expect("write fixture file");
        let config = SearchConfig {
            include_roots: vec![files],
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            content_indexing_enabled: false,
            ..SearchConfig::default()
        };
        let daemon_socket = socket.clone();
        let daemon = thread::spawn(move || run_search_daemon(config, store, daemon_socket));
        wait_for_socket(&socket);
        let mut blocked = Vec::with_capacity(BLOCKED_CLIENTS);
        for _ in 0..BLOCKED_CLIENTS {
            blocked.push(UnixStream::connect(&socket).expect("open blocked client"));
        }
        thread::sleep(Duration::from_millis(100));
        let queued_socket = socket.clone();
        let queued = thread::spawn(move || SearchDaemonClient::new(queued_socket).ping());
        let error = queued
            .join()
            .expect("join queued client")
            .expect_err("full daemon must return a protocol error");
        assert!(error.contains("server_busy"), "unexpected error: {error}");
        assert!(!error.contains("os error 11"), "unexpected error: {error}");
        drop(blocked);
        wait_until(Duration::from_secs(1), || {
            SearchDaemonClient::new(socket.clone()).shutdown().is_ok()
        });
        daemon
            .join()
            .expect("join daemon")
            .expect("daemon clean exit");
        let _ = fs::remove_dir_all(root);
    }
}
