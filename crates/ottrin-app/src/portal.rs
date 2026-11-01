//! On-demand Linux FileChooser backend. No file-manager/index window is resident.
use crate::portal_options::{self, Method, Options};
use futures_lite::{StreamExt, future};
use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom, Write},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use zbus::{Connection, message::Header, zvariant::OwnedObjectPath};
const BUS: &str = "org.freedesktop.impl.portal.desktop.ottrin";
const FRONTEND: &str = "org.freedesktop.portal.Desktop";
const OBJECT: &str = "/org/freedesktop/portal/desktop";
const MAX_RESPONSE: u64 = 16 * 1024 * 1024;
struct RequestState {
    owner: String,
    cancelled: AtomicBool,
    finished: AtomicBool,
    watchdog: AtomicBool,
    stdin: Mutex<Option<ChildStdin>>,
    cancellation: async_channel::Sender<()>,
    cancelled_receiver: async_channel::Receiver<()>,
    pidfd: Mutex<Option<Arc<OwnedFd>>>,
}
impl RequestState {
    fn new(owner: String) -> Self {
        let (cancellation, cancelled_receiver) = async_channel::bounded(1);
        Self {
            cancellation,
            cancelled_receiver,
            owner,
            cancelled: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            watchdog: AtomicBool::new(false),
            stdin: Mutex::new(None),
            pidfd: Mutex::new(None),
        }
    }
    fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::AcqRel) {
            let _ = self.cancellation.try_send(()); // One coalesced cancellation notification.
        }
        self.stdin.lock().expect("portal lease").take();
        if self.finished.load(Ordering::Acquire) {
            return;
        }
        let pidfd = self.pidfd.lock().expect("portal process identity").clone();
        if let Some(pidfd) = pidfd
            && !self.watchdog.swap(true, Ordering::AcqRel)
        {
            let fallback = pidfd.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("portal-close".into())
                .spawn(move || {
                    let deadline = Instant::now() + Duration::from_secs(3);
                    loop {
                        let mut poll = libc::pollfd {
                            fd: pidfd.as_raw_fd(),
                            events: libc::POLLIN,
                            revents: 0,
                        };
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        // SAFETY: poll references one live owned pidfd, retained throughout this wait.
                        let result = unsafe {
                            libc::poll(
                                &mut poll,
                                1,
                                remaining.as_millis().min(i32::MAX as u128) as i32,
                            )
                        };
                        if result > 0 && poll.revents & libc::POLLIN != 0 {
                            return;
                        }
                        if result < 0
                            && std::io::Error::last_os_error().kind()
                                == std::io::ErrorKind::Interrupted
                        {
                            continue;
                        }
                        break;
                    }
                    force_stop(&pidfd);
                })
            {
                eprintln!("ottrin portal: cannot supervise chooser cancellation: {error}");
                force_stop(&fallback);
            }
        }
    }
    fn complete(&self) {
        self.finished.store(true, Ordering::Release);
        self.stdin.lock().expect("portal lease").take();
    }
}
fn force_stop(pidfd: &OwnedFd) {
    // SAFETY: a pidfd identifies only the owned chooser, even after PID reuse.
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd.as_raw_fd(),
            libc::SIGKILL,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    if result < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            eprintln!("ottrin portal: cannot stop unresponsive chooser: {error}");
        }
    }
}
struct ChildOwner {
    child: Child,
    state: Arc<RequestState>,
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        if !self.state.finished.load(Ordering::Acquire) {
            self.state.cancel();
            if let Err(error) = self.child.wait() {
                eprintln!("ottrin portal: cannot reap owned chooser: {error}");
            }
            self.state.complete();
        }
    }
}
fn chooser(
    executable: &PathBuf,
    request: &ottrin_core::picker::PickerRequest,
    state: Arc<RequestState>,
) -> Result<ottrin_core::picker::PickerResponse, String> {
    if state.cancelled.load(Ordering::Acquire) {
        return Ok(request.cancelled_response());
    }
    let mut bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    if bytes.len() > crate::MAX_PICKER_REQUEST_BYTES {
        return Err("Portal request exceeds the chooser's128KiB limit".into());
    }
    bytes.push(b'\n');
    let mut output = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut child = Command::new(executable)
        .args(["picker", "--cancel-on-disconnect"])
        .stdin(Stdio::piped())
        .stdout(output.try_clone().map_err(|e| e.to_string())?)
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Could not launch Ottrin chooser: {e}"))?;
    // Open before any waiter can reap this child. Unsupported pidfd kernels are
    // refused rather than introducing a reusable numeric-PID kill path.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id(), 0) };
    if fd < 0 {
        let original = std::io::Error::last_os_error();
        if let Err(error) = child.kill() {
            eprintln!("ottrin portal: failed-start chooser stop: {error}");
        }
        child
            .wait()
            .map_err(|e| format!("Could not reap failed-start chooser: {e}"))?;
        return Err(format!(
            "Could not retain chooser process identity: {original}"
        ));
    }
    // SAFETY: successful pidfd_open returned a new descriptor owned by this scope.
    *state.pidfd.lock().expect("portal process identity") =
        Some(Arc::new(unsafe { OwnedFd::from_raw_fd(fd as i32) }));
    let mut owned = ChildOwner {
        child,
        state: state.clone(),
    };
    if state.cancelled.load(Ordering::Acquire) {
        state.cancel();
    }
    let mut stdin = owned
        .child
        .stdin
        .take()
        .ok_or("Chooser input pipe unavailable")?;
    if let Err(error) = stdin.write_all(&bytes) {
        if state.cancelled.load(Ordering::Acquire) {
            return Ok(request.cancelled_response());
        }
        return Err(format!("Could not send chooser request: {error}"));
    }
    *state.stdin.lock().expect("portal lease") = Some(stdin);
    if state.cancelled.load(Ordering::Acquire) {
        state.cancel();
    }
    let status = owned
        .child
        .wait()
        .map_err(|e| format!("Could not wait for chooser: {e}"))?;
    state.complete();
    if state.cancelled.load(Ordering::Acquire) {
        return Ok(request.cancelled_response());
    }
    if !status.success() {
        return Err(format!("Ottrin chooser failed ({status})"));
    }
    if output.metadata().map_err(|e| e.to_string())?.len() > MAX_RESPONSE {
        return Err("Chooser response exceeds16MiB".into());
    }
    output.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    output
        .take(MAX_RESPONSE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let response: ottrin_core::picker::PickerResponse =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid chooser response: {e}"))?;
    response.validate_for(request)?;
    Ok(response)
}
struct Shared {
    executable: PathBuf,
    active: Mutex<HashMap<OwnedObjectPath, Arc<RequestState>>>,
    completed: async_channel::Sender<OwnedObjectPath>,
}
struct Registration {
    shared: Arc<Shared>,
    path: OwnedObjectPath,
    state: Arc<RequestState>,
}
impl Drop for Registration {
    fn drop(&mut self) {
        if !self.state.finished.load(Ordering::Acquire) {
            self.state.cancel();
        }
        if let Err(error) = self.shared.completed.try_send(self.path.clone()) {
            eprintln!("ottrin portal: request cleanup channel closed: {error}");
        }
    }
}
struct PortalRequest {
    state: Arc<RequestState>,
}
#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl PortalRequest {
    fn close(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        if header.sender().map(|s| s.as_str()) != Some(&self.state.owner) {
            return Err(zbus::fdo::Error::AccessDenied(
                "Only this portal request's frontend can close it".into(),
            ));
        }
        self.state.cancel();
        Ok(())
    }
}
struct Portal {
    shared: Arc<Shared>,
}
async fn authorize(header: &Header<'_>, connection: &Connection) -> zbus::fdo::Result<String> {
    let owner = zbus::fdo::DBusProxy::new(connection)
        .await?
        .get_name_owner(
            FRONTEND
                .try_into()
                .map_err(|e: zbus::names::Error| zbus::fdo::Error::Failed(e.to_string()))?,
        )
        .await?;
    if header.sender().map(|s| s.as_str()) != Some(owner.as_str()) {
        return Err(zbus::fdo::Error::AccessDenied(
            "Only the desktop portal frontend may request a chooser".into(),
        ));
    }
    Ok(owner.to_string())
}
impl Portal {
    async fn choose(
        &self,
        owner: String,
        handle: OwnedObjectPath,
        request: Result<portal_options::Request, String>,
        connection: &Connection,
    ) -> zbus::fdo::Result<(u32, Options)> {
        if !handle
            .as_str()
            .starts_with("/org/freedesktop/portal/desktop/request/")
        {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Invalid portal request path".into(),
            ));
        }
        let request = match request {
            Ok(request) => request,
            Err(error) => {
                eprintln!("ottrin portal: {error}");
                return Ok((2, HashMap::new()));
            }
        };
        let state = Arc::new(RequestState::new(owner));
        {
            let mut active = self.shared.active.lock().expect("portal requests");
            if active.contains_key(&handle) {
                return Err(zbus::fdo::Error::InvalidArgs(
                    "Portal request already active".into(),
                ));
            }
            active.insert(handle.clone(), state.clone());
        }
        match connection
            .object_server()
            .at(
                handle.clone(),
                PortalRequest {
                    state: state.clone(),
                },
            )
            .await
        {
            Ok(true) => {}
            result => {
                self.shared
                    .active
                    .lock()
                    .expect("portal requests")
                    .remove(&handle);
                return Err(zbus::fdo::Error::Failed(format!(
                    "Could not register portal request: {result:?}"
                )));
            }
        }
        let registration = Registration {
            shared: self.shared.clone(),
            path: handle,
            state: state.clone(),
        };
        let (sender, receiver) = async_channel::bounded(1);
        let executable = self.shared.executable.clone();
        let worker_state = state.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("portal-chooser".into())
            .spawn(move || {
                let _registration = registration;
                let result = chooser(&executable, &request.picker, worker_state.clone())
                    .and_then(|response| portal_options::response(&request, response));
                worker_state.complete();
                if sender.send_blocking(result).is_err()
                    && !worker_state.cancelled.load(Ordering::Acquire)
                {
                    eprintln!("ottrin portal: chooser request receiver closed");
                }
            })
        {
            return Err(zbus::fdo::Error::Failed(format!(
                "Could not start chooser worker: {error}"
            )));
        }
        let result = future::race(receiver.recv(), async {
            let _ = state.cancelled_receiver.recv().await;
            Ok(Ok((1, HashMap::new())))
        })
        .await;
        match result {
            Ok(Ok(reply)) => Ok(reply),
            result => {
                eprintln!("ottrin portal: chooser interaction failed: {result:?}");
                Ok((2, HashMap::new()))
            }
        }
    }
}
#[zbus::interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl Portal {
    #[zbus(property)]
    fn version(&self) -> u32 {
        4
    }
    #[zbus(out_args("response", "results"))]
    #[expect(
        clippy::too_many_arguments,
        reason = "Fixed desktop portal D-Bus signature plus transport context"
    )]
    async fn open_file(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        options: Options,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<(u32, Options)> {
        let _ = app_id; // The frontend identity, not this presentation field, grants access.
        let owner = authorize(&header, connection).await?;
        self.choose(
            owner,
            handle,
            portal_options::request(Method::Open, parent_window, title, options),
            connection,
        )
        .await
    }
    #[zbus(out_args("response", "results"))]
    #[expect(
        clippy::too_many_arguments,
        reason = "Fixed desktop portal D-Bus signature plus transport context"
    )]
    async fn save_file(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        options: Options,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<(u32, Options)> {
        let _ = app_id; // The frontend identity, not this presentation field, grants access.
        let owner = authorize(&header, connection).await?;
        self.choose(
            owner,
            handle,
            portal_options::request(Method::Save, parent_window, title, options),
            connection,
        )
        .await
    }
    #[zbus(out_args("response", "results"))]
    #[expect(
        clippy::too_many_arguments,
        reason = "Fixed desktop portal D-Bus signature plus transport context"
    )]
    async fn save_files(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        options: Options,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<(u32, Options)> {
        let _ = app_id; // The frontend identity, not this presentation field, grants access.
        let owner = authorize(&header, connection).await?;
        self.choose(
            owner,
            handle,
            portal_options::request(Method::SaveFiles, parent_window, title, options),
            connection,
        )
        .await
    }
}
pub(super) fn run() -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 {
        return Err("Run the chooser portal in the normal user session".into());
    }
    // SAFETY: fixed NUL-terminated process label, with no borrowed lifetime beyond prctl.
    if unsafe { libc::prctl(libc::PR_SET_NAME, c"ottrin-portal".as_ptr(), 0, 0, 0) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    async_io::block_on(async {
        let (completed, requests) = async_channel::unbounded();
        let shared = Arc::new(Shared {
            executable: std::env::current_exe().map_err(|e| e.to_string())?,
            active: Mutex::new(HashMap::new()),
            completed,
        });
        let connection = zbus::connection::Builder::session()
            .map_err(|e| e.to_string())?
            .serve_at(
                OBJECT,
                Portal {
                    shared: shared.clone(),
                },
            )
            .map_err(|e| e.to_string())?
            .build()
            .await
            .map_err(|e| e.to_string())?;
        let proxy = zbus::fdo::DBusProxy::new(&connection)
            .await
            .map_err(|e| e.to_string())?;
        let mut changes = proxy
            .receive_name_owner_changed_with_args(&[(0, FRONTEND)])
            .await
            .map_err(|e| e.to_string())?;
        connection
            .request_name_with_flags(BUS, zbus::fdo::RequestNameFlags::DoNotQueue.into())
            .await
            .map_err(|e| e.to_string())?;
        enum Event {
            Completed(Option<OwnedObjectPath>),
            Owner(Option<Option<String>>),
        }
        loop {
            let event = future::race(
                async { Event::Completed(requests.recv().await.ok()) },
                async {
                    Event::Owner(changes.next().await.map(|signal| {
                        signal
                            .args()
                            .ok()
                            .and_then(|args| args.old_owner().as_ref().map(ToString::to_string))
                    }))
                },
            )
            .await;
            match event {
                Event::Completed(Some(path)) => {
                    if let Err(error) = connection
                        .object_server()
                        .remove::<PortalRequest, _>(path.clone())
                        .await
                    {
                        eprintln!("ottrin portal: cannot remove completed request: {error}");
                    }
                    shared.active.lock().expect("portal requests").remove(&path);
                }
                Event::Owner(Some(Some(owner))) => {
                    for state in shared.active.lock().expect("portal requests").values() {
                        if state.owner == owner {
                            state.cancel();
                        }
                    }
                }
                Event::Owner(Some(None)) => {}
                Event::Owner(None) | Event::Completed(None) => {
                    let states = shared
                        .active
                        .lock()
                        .expect("portal requests")
                        .values()
                        .cloned()
                        .collect::<Vec<_>>();
                    for state in &states {
                        state.cancel();
                    }
                    let deadline = Instant::now() + Duration::from_secs(4);
                    while states.iter().any(|state| {
                        state
                            .pidfd
                            .lock()
                            .expect("portal process identity")
                            .is_some()
                            && !state.finished.load(Ordering::Acquire)
                    }) && Instant::now() < deadline
                    {
                        async_io::Timer::after(Duration::from_millis(20)).await;
                    }
                    return Err("Desktop portal connection closed".into());
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct OwnedTest {
        state: Arc<RequestState>,
        worker:
            Option<std::thread::JoinHandle<Result<ottrin_core::picker::PickerResponse, String>>>,
    }
    impl OwnedTest {
        fn join(&mut self) -> Result<ottrin_core::picker::PickerResponse, String> {
            self.worker.take().unwrap().join().unwrap()
        }
    }
    impl Drop for OwnedTest {
        fn drop(&mut self) {
            if let Some(worker) = self.worker.take() {
                self.state.cancel();
                if let Some(fd) = self.state.pidfd.lock().unwrap().as_ref() {
                    force_stop(fd);
                }
                assert!(worker.join().is_ok());
            }
        }
    }
    fn start(dir: &std::path::Path, body: &str) -> OwnedTest {
        let executable = dir.join("chooser");
        std::fs::write(
            &executable,
            format!(
                "#!/usr/bin/python3\nimport sys,json,time,pathlib\nsys.stdin.readline()\n{body}\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let state = Arc::new(RequestState::new(":owned-test".into()));
        let child_state = state.clone();
        let worker =
            std::thread::spawn(move || chooser(&executable, &Default::default(), child_state));
        let owner = OwnedTest {
            state,
            worker: Some(worker),
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while owner.state.stdin.lock().unwrap().is_none() {
            assert!(
                !owner.worker.as_ref().unwrap().is_finished(),
                "Chooser exited before publishing its lease"
            );
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        owner
    }
    #[test]
    fn portal_close_releases_the_input_lease_and_reaps_a_graceful_chooser() {
        let dir = tempfile::tempdir().unwrap();
        let body = "sys.stdin.read()\npathlib.Path(__file__).with_name('graceful').write_text('closed')\nprint(json.dumps(dict(accepted=False,paths=[],current_filter=None,choices={})))";
        let mut owner = start(dir.path(), body);
        owner.state.cancel();
        owner.state.cancel();
        assert!(!owner.join().unwrap().accepted);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("graceful")).unwrap(),
            "closed"
        );
        assert!(owner.state.finished.load(Ordering::Acquire));
    }
    #[test]
    fn unresponsive_chooser_is_stopped_through_its_pinned_pidfd() {
        let dir = tempfile::tempdir().unwrap();
        let mut owner = start(dir.path(), "while True: time.sleep(30)");
        let started = Instant::now();
        owner.state.cancel();
        assert!(!owner.join().unwrap().accepted);
        assert!(started.elapsed() >= Duration::from_secs(2));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(owner.state.finished.load(Ordering::Acquire));
    }
    #[test]
    fn completed_request_cleanup_does_not_cancel_an_accepted_reply() {
        let state = Arc::new(RequestState::new(":owned-test".into()));
        state.complete();
        let (completed, receiver) = async_channel::unbounded();
        let shared = Arc::new(Shared {
            executable: PathBuf::new(),
            active: Mutex::new(HashMap::new()),
            completed,
        });
        let path: OwnedObjectPath = "/org/freedesktop/portal/desktop/request/owned/test"
            .try_into()
            .unwrap();
        drop(Registration {
            shared,
            path: path.clone(),
            state: state.clone(),
        });
        assert_eq!(receiver.try_recv().unwrap(), path);
        assert!(!state.cancelled.load(Ordering::Acquire));
        assert!(state.cancelled_receiver.try_recv().is_err());
    }
}
