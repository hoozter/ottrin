//! The UI's single ordered RPC worker. Browsing never waits for index I/O.
use crate::ResultRepaint;
use ottrin_core::{SearchConfig, SearchIndexStatus, SearchQuery};
use ottrin_search::{SearchDaemonClient, SearchIndexDiagnostics, SearchQueryResult};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

trait IndexRpc: Send + Sync {
    fn configure(&self, config: SearchConfig) -> Result<(), String>;
    fn diagnostics(&self) -> Result<SearchIndexDiagnostics, String>;
    fn endpoint_present(&self) -> Result<bool, String>;
    fn query(
        &self,
        query: SearchQuery,
        cancelled: &AtomicBool,
    ) -> Result<SearchQueryResult, String>;
    fn rebuild(&self) -> Result<(), String>;
    fn clear_storage(&self) -> Result<u64, String>;
}
impl IndexRpc for SearchDaemonClient {
    fn configure(&self, config: SearchConfig) -> Result<(), String> {
        self.update_config(config)
    }
    fn diagnostics(&self) -> Result<SearchIndexDiagnostics, String> {
        SearchDaemonClient::diagnostics(self)
    }
    fn endpoint_present(&self) -> Result<bool, String> {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            SearchDaemonClient::endpoint_present(self)
        }
        #[cfg(all(unix, not(target_os = "linux")))]
        {
            ottrin_search::default_daemon_socket_path()?
                .try_exists()
                .map_err(|e| e.to_string())
        }
    }
    fn query(
        &self,
        query: SearchQuery,
        cancelled: &AtomicBool,
    ) -> Result<SearchQueryResult, String> {
        self.query_cancellable(query, cancelled)
    }
    fn rebuild(&self) -> Result<(), String> {
        SearchDaemonClient::rebuild(self)
    }
    fn clear_storage(&self) -> Result<u64, String> {
        let bytes = SearchDaemonClient::clear_storage(self)?;
        self.shutdown()?;
        Ok(bytes)
    }
}
/// One coherent background observation; rendering never probes a socket or pipe.
#[derive(Clone, PartialEq)]
pub(crate) struct Observation {
    pub(crate) diagnostics: SearchIndexDiagnostics,
    pub(crate) stopped: bool,
}
impl From<SearchIndexDiagnostics> for Observation {
    fn from(diagnostics: SearchIndexDiagnostics) -> Self {
        Self {
            diagnostics,
            stopped: false,
        }
    }
}
struct Shared {
    snapshot: Mutex<Observation>,
    refresh_queued: AtomicBool,
    stopped: AtomicBool,
    maintenance_active: AtomicBool,
    active_query: Mutex<Option<Arc<AtomicBool>>>,
    repaint: ResultRepaint,
}
impl Shared {
    fn publish(&self, observation: Observation) {
        let mut snapshot = self.snapshot.lock().expect("index snapshot");
        if *snapshot != observation {
            *snapshot = observation;
            drop(snapshot);
            self.repaint.wake();
        }
    }
}
enum Command {
    Configure(SearchConfig),
    Refresh,
    Query {
        query: SearchQuery,
        cancelled: Arc<AtomicBool>,
        reply: mpsc::Sender<Result<SearchQueryResult, String>>,
    },
    Rebuild,
    MaintenanceDone(Option<String>),
    Clear(mpsc::Sender<Result<u64, String>>),
    Stop,
}
#[derive(Clone)]
pub(crate) struct Handle {
    sender: mpsc::Sender<Command>,
    shared: Arc<Shared>,
}
pub(crate) struct RemoteIndex {
    handle: Handle,
}
impl Handle {
    fn send(&self, command: Command) -> Result<(), String> {
        if self.shared.stopped.load(Ordering::Acquire) {
            return Err("Index connection has closed".into());
        }
        self.sender
            .send(command)
            .map_err(|_| "Index connection worker is unavailable".into())
    }
    pub(crate) fn query(
        &self,
        query: SearchQuery,
        cancelled: &AtomicBool,
    ) -> Result<SearchQueryResult, String> {
        if cancelled.load(Ordering::Acquire) {
            return Err("Search cancelled".into());
        }
        let (reply, result) = mpsc::channel();
        let active = Arc::new(AtomicBool::new(false));
        self.send(Command::Query {
            query,
            cancelled: active.clone(),
            reply,
        })?;
        loop {
            if cancelled.load(Ordering::Acquire) || self.shared.stopped.load(Ordering::Acquire) {
                active.store(true, Ordering::Release);
                return Err("Search cancelled".into());
            }
            match result.recv_timeout(Duration::from_millis(20)) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Index connection worker stopped".into());
                }
            }
        }
    }
}
impl RemoteIndex {
    #[cfg(not(test))]
    pub(crate) fn new(config: SearchConfig, repaint: ResultRepaint) -> Self {
        Self::with_factory(config, repaint, || {
            SearchDaemonClient::connect_default()
                .map(|client| Arc::new(client) as Arc<dyn IndexRpc>)
        })
    }
    fn with_factory(
        config: SearchConfig,
        repaint: ResultRepaint,
        mut connect: impl FnMut() -> Result<Arc<dyn IndexRpc>, String> + Send + 'static,
    ) -> Self {
        let mut diagnostics =
            SearchIndexDiagnostics::unavailable("Connecting to the index service");
        diagnostics.status = SearchIndexStatus::Loading;
        let shared = Arc::new(Shared {
            snapshot: Mutex::new(diagnostics.into()),
            refresh_queued: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            maintenance_active: AtomicBool::new(false),
            active_query: Mutex::new(None),
            repaint,
        });
        let (sender, commands) = mpsc::channel();
        let state = shared.clone();
        let completed = sender.clone();
        let worker = std::thread::Builder::new()
            .name("ottrin-index-ui".into())
            .spawn(move || {
                let mut client: Option<Arc<dyn IndexRpc>> = None;
                let mut config = config;
                let mut needs_configuration = true;
                let mut poll_at = Instant::now();
                loop {
                    let command = match commands
                        .recv_timeout(poll_at.saturating_duration_since(Instant::now()))
                    {
                        Ok(command) => command,
                        Err(mpsc::RecvTimeoutError::Timeout) => Command::Refresh,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    if state.stopped.load(Ordering::Acquire) || matches!(command, Command::Stop) {
                        break;
                    }
                    let is_query = matches!(&command, Command::Query { .. });
                    let is_refresh = matches!(&command, Command::Refresh);
                    if let Command::Configure(new) = &command {
                        config = new.clone();
                        needs_configuration = true;
                    }
                    let query_cancelled = match &command {
                        Command::Query { cancelled, .. } => Some(cancelled.clone()),
                        _ => None,
                    };
                    if let Some(cancelled) = &query_cancelled {
                        *state.active_query.lock().expect("active index query") =
                            Some(cancelled.clone());
                    }
                    let ready = (|| {
                        if client.is_none() {
                            client = Some(connect()?);
                            needs_configuration = true;
                        }
                        let client = client.as_ref().expect("connected index client");
                        if needs_configuration {
                            client.configure(config.clone())?;
                            needs_configuration = false;
                        }
                        Ok::<_, String>(())
                    })();
                    let operation = match command {
                        Command::Query {
                            query,
                            cancelled,
                            reply,
                        } => {
                            let result = ready.and_then(|()| {
                                if cancelled.load(Ordering::Acquire) {
                                    Err("Search cancelled".into())
                                } else {
                                    client
                                        .as_ref()
                                        .expect("connected index client")
                                        .query(query, &cancelled)
                                }
                            });
                            let failure = result
                                .as_ref()
                                .err()
                                .filter(|_| !cancelled.load(Ordering::Acquire))
                                .cloned();
                            let _ = reply.send(result);
                            *state.active_query.lock().expect("active index query") = None;
                            failure.map_or(Ok(None), Err)
                        }
                        Command::Clear(reply) => match ready {
                            Ok(()) => start_maintenance(
                                client.as_ref().expect("connected index client").clone(),
                                Some(reply),
                                state.clone(),
                                completed.clone(),
                            )
                            .map(|()| None),
                            Err(error) => {
                                let _ = reply.send(Err(error.clone()));
                                Err(error)
                            }
                        },
                        Command::Rebuild => ready
                            .and_then(|()| {
                                start_maintenance(
                                    client.as_ref().expect("connected index client").clone(),
                                    None,
                                    state.clone(),
                                    completed.clone(),
                                )
                            })
                            .map(|()| None),
                        Command::MaintenanceDone(error) => error.map_or(Ok(None), Err),
                        Command::Refresh | Command::Configure(_) => ready
                            .and_then(|()| {
                                client
                                    .as_ref()
                                    .expect("connected index client")
                                    .diagnostics()
                            })
                            .map(Some),
                        Command::Stop => break,
                    };
                    if is_refresh {
                        state.refresh_queued.store(false, Ordering::Release);
                    }
                    match operation {
                        Ok(Some(diagnostics)) => {
                            poll_at = Instant::now()
                                + match diagnostics.status {
                                    SearchIndexStatus::Ready
                                        if !state.maintenance_active.load(Ordering::Acquire) =>
                                    {
                                        Duration::from_secs(2)
                                    }
                                    SearchIndexStatus::Ready => Duration::from_millis(250),
                                    SearchIndexStatus::Loading | SearchIndexStatus::Indexing => {
                                        Duration::from_millis(250)
                                    }
                                    SearchIndexStatus::Unavailable => Duration::from_secs(2),
                                };
                            state.publish(diagnostics.into());
                        }
                        Ok(None) => {
                            if !is_query {
                                poll_at = Instant::now();
                            }
                        }
                        Err(error) => {
                            let endpoint = client.as_ref().map(|client| client.endpoint_present());
                            let mut observation =
                                Observation::from(SearchIndexDiagnostics::unavailable(error));
                            match endpoint {
                                Some(Ok(false)) => {
                                    observation.stopped = true;
                                    observation.diagnostics.detail = Some(
                                        "Indexing is stopped. Click to manage the index service."
                                            .into(),
                                    );
                                    observation.diagnostics.last_error = None;
                                }
                                Some(Err(probe_error)) => {
                                    observation
                                        .diagnostics
                                        .last_error
                                        .as_mut()
                                        .expect("failed RPC")
                                        .push_str(&format!("; {probe_error}"));
                                }
                                Some(Ok(true)) | None => {}
                            }
                            client = None;
                            needs_configuration = true;
                            state.publish(observation);
                            poll_at = Instant::now() + Duration::from_secs(2);
                        }
                    }
                }
            });
        if let Err(error) = worker {
            *shared.snapshot.lock().expect("index snapshot") = SearchIndexDiagnostics::unavailable(
                format!("Cannot start index connection worker: {error}"),
            )
            .into();
        }
        Self {
            handle: Handle { sender, shared },
        }
    }
    pub(crate) fn handle(&self) -> Handle {
        self.handle.clone()
    }
    #[cfg(test)]
    pub(crate) fn diagnostics(&self) -> SearchIndexDiagnostics {
        self.observation().diagnostics
    }
    pub(crate) fn observation(&self) -> Observation {
        self.handle
            .shared
            .snapshot
            .lock()
            .expect("index snapshot")
            .clone()
    }
    pub(crate) fn refresh(&self) {
        if !self
            .handle
            .shared
            .refresh_queued
            .swap(true, Ordering::AcqRel)
            && self.handle.send(Command::Refresh).is_err()
        {
            self.handle
                .shared
                .refresh_queued
                .store(false, Ordering::Release);
        }
    }
    pub(crate) fn update_config(&self, config: SearchConfig) {
        if let Err(error) = self.handle.send(Command::Configure(config)) {
            self.record_error(error);
        }
    }
    pub(crate) fn rebuild(&self) {
        if let Err(error) = self.handle.send(Command::Rebuild) {
            self.record_error(error);
        }
    }
    pub(crate) fn clear_storage(&self) -> Result<u64, String> {
        let (reply, result) = mpsc::channel();
        self.handle.send(Command::Clear(reply))?;
        result
            .recv()
            .map_err(|_| "Index connection worker stopped".to_string())?
    }
    fn record_error(&self, error: String) {
        self.handle
            .shared
            .publish(SearchIndexDiagnostics::unavailable(error).into());
    }
}
struct MaintenanceCompletion {
    shared: Arc<Shared>,
    completed: mpsc::Sender<Command>,
    finished: bool,
}
impl Drop for MaintenanceCompletion {
    fn drop(&mut self) {
        if !self.finished {
            self.shared
                .maintenance_active
                .store(false, Ordering::Release);
            let _ = self.completed.send(Command::MaintenanceDone(Some(
                "Index maintenance worker stopped unexpectedly".into(),
            )));
        }
    }
}
fn start_maintenance(
    client: Arc<dyn IndexRpc>,
    reply: Option<mpsc::Sender<Result<u64, String>>>,
    shared: Arc<Shared>,
    completed: mpsc::Sender<Command>,
) -> Result<(), String> {
    if shared
        .maintenance_active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        let error = "Index maintenance is already running. Wait for it to finish.".to_string();
        if let Some(reply) = reply {
            let _ = reply.send(Err(error.clone()));
        }
        return Err(error);
    }
    let failed_reply = reply.clone();
    let task_state = shared.clone();
    let started = std::thread::Builder::new()
        .name("ottrin-index-maintenance".into())
        .spawn(move || {
            let mut guard = MaintenanceCompletion {
                shared: task_state,
                completed,
                finished: false,
            };
            let failure = if let Some(reply) = reply {
                let result = client.clear_storage();
                let failure = result.as_ref().err().cloned();
                let _ = reply.send(result);
                failure
            } else {
                client.rebuild().err()
            };
            guard.finished = true;
            guard
                .shared
                .maintenance_active
                .store(false, Ordering::Release);
            let _ = guard.completed.send(Command::MaintenanceDone(failure));
        });
    if let Err(error) = started {
        shared.maintenance_active.store(false, Ordering::Release);
        let error = format!("Cannot start index maintenance worker: {error}");
        if let Some(reply) = failed_reply {
            let _ = reply.send(Err(error.clone()));
        }
        return Err(error);
    }
    Ok(())
}

impl Drop for RemoteIndex {
    fn drop(&mut self) {
        self.handle.shared.stopped.store(true, Ordering::Release);
        if let Some(cancelled) = self
            .handle
            .shared
            .active_query
            .lock()
            .expect("active index query")
            .as_ref()
        {
            cancelled.store(true, Ordering::Release);
        }
        let _ = self.handle.sender.send(Command::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ottrin_core::{SearchResponse, SearchScope, SearchSort};
    use std::sync::atomic::AtomicUsize;
    #[derive(Default)]
    struct Trace {
        events: Mutex<Vec<String>>,
        diagnostics: AtomicUsize,
        indexed_items: AtomicUsize,
        endpoint: Mutex<Option<Result<bool, String>>>,
        endpoint_probes: AtomicUsize,
        fail_configure: AtomicBool,
        hold_query: AtomicBool,
        fail_rebuild: AtomicBool,
        hold_maintenance: AtomicBool,
        maintenance_started: Mutex<Option<mpsc::Sender<()>>>,
        query_started: Mutex<Option<mpsc::Sender<()>>>,
        dropped: Mutex<Option<mpsc::Sender<()>>>,
    }
    struct Fake {
        trace: Arc<Trace>,
        configure_gate: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
    }
    impl Fake {
        fn new(trace: Arc<Trace>) -> Self {
            Self {
                trace,
                configure_gate: Mutex::new(None),
            }
        }
    }
    impl Drop for Fake {
        fn drop(&mut self) {
            if let Some(tx) = self.trace.dropped.lock().unwrap().take() {
                let _ = tx.send(());
            }
        }
    }
    impl IndexRpc for Fake {
        fn configure(&self, config: SearchConfig) -> Result<(), String> {
            if let Some((started, release)) = self.configure_gate.lock().unwrap().take() {
                started.send(()).unwrap();
                release.recv_timeout(Duration::from_secs(3)).unwrap();
            }
            self.trace
                .events
                .lock()
                .unwrap()
                .push(format!("configure:{:?}", config.include_roots));
            if self.trace.fail_configure.load(Ordering::Acquire) {
                return Err("configure refused".into());
            }
            Ok(())
        }
        fn endpoint_present(&self) -> Result<bool, String> {
            self.trace.endpoint_probes.fetch_add(1, Ordering::AcqRel);
            self.trace
                .endpoint
                .lock()
                .unwrap()
                .clone()
                .unwrap_or(Ok(true))
        }
        fn diagnostics(&self) -> Result<SearchIndexDiagnostics, String> {
            self.trace.diagnostics.fetch_add(1, Ordering::AcqRel);
            let mut diagnostics = SearchIndexDiagnostics::unavailable("");
            diagnostics.status = SearchIndexStatus::Ready;
            diagnostics.last_error = None;
            diagnostics.indexed_items = self.trace.indexed_items.load(Ordering::Acquire);
            Ok(diagnostics)
        }
        fn query(
            &self,
            _query: SearchQuery,
            cancelled: &AtomicBool,
        ) -> Result<SearchQueryResult, String> {
            self.trace.events.lock().unwrap().push("query".into());
            if let Some(tx) = self.trace.query_started.lock().unwrap().take() {
                tx.send(()).unwrap();
            }
            let until = Instant::now() + Duration::from_secs(3);
            while self.trace.hold_query.load(Ordering::Acquire)
                && !cancelled.load(Ordering::Acquire)
            {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(1));
            }
            if cancelled.load(Ordering::Acquire) {
                return Err("Search cancelled".into());
            }
            Ok(SearchQueryResult {
                response: SearchResponse {
                    items: vec![],
                    total: 0,
                    status: SearchIndexStatus::Ready,
                    error: None,
                },
                warning: None,
            })
        }
        fn rebuild(&self) -> Result<(), String> {
            self.trace.events.lock().unwrap().push("rebuild".into());
            if let Some(tx) = self.trace.maintenance_started.lock().unwrap().take() {
                tx.send(()).unwrap();
            }
            let until = Instant::now() + Duration::from_secs(3);
            while self.trace.hold_maintenance.load(Ordering::Acquire) {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(1));
            }
            if self.trace.fail_rebuild.swap(false, Ordering::AcqRel) {
                Err("rebuild refused".into())
            } else {
                Ok(())
            }
        }
        fn clear_storage(&self) -> Result<u64, String> {
            self.trace.events.lock().unwrap().push("clear".into());
            Ok(7)
        }
    }
    fn query() -> SearchQuery {
        SearchQuery {
            text: "test".into(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 10,
            offset: 0,
            content_search: false,
        }
    }
    fn wait_for(mut predicate: impl FnMut() -> bool) {
        let until = Instant::now() + Duration::from_secs(2);
        while !predicate() {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn stalled_configuration_cannot_block_ui_snapshot_reads_or_new_settings() {
        let trace = Arc::new(Trace::default());
        let (started, entered) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let mut first = Some((started, gate));
        let state = trace.clone();
        let remote = RemoteIndex::with_factory(
            SearchConfig::default(),
            ResultRepaint::default(),
            move || {
                Ok(Arc::new(Fake {
                    trace: state.clone(),
                    configure_gate: Mutex::new(first.take()),
                }))
            },
        );
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let started = Instant::now();
        for _ in 0..100 {
            assert_eq!(remote.diagnostics().status, SearchIndexStatus::Loading);
        }
        remote.update_config(SearchConfig::default());
        remote.refresh();
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "UI reads must not wait for a stalled RPC"
        );
        release.send(()).unwrap();
        wait_for(|| remote.diagnostics().status == SearchIndexStatus::Ready);
    }
    #[test]
    fn queries_observe_configuration_order_and_control_errors_are_not_hidden() {
        let trace = Arc::new(Trace::default());
        let state = trace.clone();
        let remote = RemoteIndex::with_factory(
            SearchConfig::default(),
            ResultRepaint::default(),
            move || Ok(Arc::new(Fake::new(state.clone()))),
        );
        let config = SearchConfig {
            include_roots: vec!["selected-root".into()],
            ..Default::default()
        };
        remote.update_config(config);
        remote
            .handle()
            .query(query(), &AtomicBool::new(false))
            .unwrap();
        let events = trace.events.lock().unwrap().clone();
        let position = events.iter().position(|x| x == "query").unwrap();
        assert!(
            events[..position]
                .iter()
                .any(|x| x.contains("selected-root"))
        );
        trace.fail_rebuild.store(true, Ordering::Release);
        remote.rebuild();
        wait_for(|| remote.diagnostics().last_error.as_deref() == Some("rebuild refused"));
        assert_eq!(remote.clear_storage().unwrap(), 7);
        let events = trace.events.lock().unwrap();
        assert!(events.iter().any(|x| x == "rebuild"));
        assert!(events.iter().any(|x| x == "clear"));
    }
    #[test]
    fn closing_the_ui_owner_cancels_active_queries_even_when_a_handle_is_retained() {
        let trace = Arc::new(Trace::default());
        trace.hold_query.store(true, Ordering::Release);
        let (started, entered) = mpsc::channel();
        *trace.query_started.lock().unwrap() = Some(started);
        let (drop_tx, dropped) = mpsc::channel();
        *trace.dropped.lock().unwrap() = Some(drop_tx);
        let state = trace.clone();
        let remote = RemoteIndex::with_factory(
            SearchConfig::default(),
            ResultRepaint::default(),
            move || Ok(Arc::new(Fake::new(state.clone()))),
        );
        let handle = remote.handle();
        let request = std::thread::spawn(move || handle.query(query(), &AtomicBool::new(false)));
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(remote);
        assert!(request.join().unwrap().unwrap_err().contains("cancelled"));
        dropped.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    #[test]
    fn missing_endpoint_recovers_with_the_latest_configuration_and_no_local_scanner() {
        let trace = Arc::new(Trace::default());
        let state = trace.clone();
        let mut missing = true;
        let remote = RemoteIndex::with_factory(
            SearchConfig::default(),
            ResultRepaint::default(),
            move || {
                if missing {
                    missing = false;
                    Err("index not ready".into())
                } else {
                    Ok(Arc::new(Fake::new(state.clone())))
                }
            },
        );
        wait_for(|| remote.diagnostics().status == SearchIndexStatus::Unavailable);
        assert!(
            remote
                .diagnostics()
                .last_error
                .unwrap()
                .contains("not ready")
        );
        let config = SearchConfig {
            include_roots: vec!["latest-root".into()],
            ..Default::default()
        };
        remote.update_config(config);
        remote
            .handle()
            .query(query(), &AtomicBool::new(false))
            .unwrap();
        assert!(trace.events.lock().unwrap()[0].contains("latest-root"));
    }
    #[test]
    fn stopped_endpoint_is_neutral_but_live_and_probe_failures_remain_errors() {
        let trace = Arc::new(Trace::default());
        trace.fail_configure.store(true, Ordering::Release);
        *trace.endpoint.lock().unwrap() = Some(Ok(false));
        let state = trace.clone();
        let remote = RemoteIndex::with_factory(
            SearchConfig::default(),
            ResultRepaint::default(),
            move || Ok(Arc::new(Fake::new(state.clone()))),
        );
        wait_for(|| remote.observation().stopped);
        let stopped = remote.observation();
        assert_eq!(stopped.diagnostics.status, SearchIndexStatus::Unavailable);
        assert_eq!(stopped.diagnostics.last_error, None);
        assert!(stopped.diagnostics.detail.unwrap().contains("stopped"));
        let probes = trace.endpoint_probes.load(Ordering::Acquire);
        for _ in 0..100 {
            assert!(remote.observation().stopped);
        }
        assert_eq!(trace.endpoint_probes.load(Ordering::Acquire), probes);

        *trace.endpoint.lock().unwrap() = Some(Err("endpoint ownership denied".into()));
        remote.refresh();
        wait_for(|| remote.diagnostics().last_error.is_some());
        let failed = remote.observation();
        assert!(!failed.stopped);
        let error = failed.diagnostics.last_error.unwrap();
        assert!(error.contains("configure refused") && error.contains("ownership denied"));

        *trace.endpoint.lock().unwrap() = Some(Ok(true));
        remote.refresh();
        wait_for(|| remote.diagnostics().last_error.as_deref() == Some("configure refused"));
        assert!(!remote.observation().stopped);

        trace.fail_configure.store(false, Ordering::Release);
        remote.refresh();
        wait_for(|| remote.diagnostics().status == SearchIndexStatus::Ready);
        assert!(!remote.observation().stopped);
        assert_eq!(remote.diagnostics().last_error, None);
    }

    #[test]
    fn endpoint_probe_cannot_block_gui_snapshot_reads() {
        let trace = Arc::new(Trace::default());
        trace.fail_configure.store(true, Ordering::Release);
        let mut gate = trace.endpoint.lock().unwrap();
        *gate = Some(Ok(false));
        let state = trace.clone();
        let remote = RemoteIndex::with_factory(
            SearchConfig::default(),
            ResultRepaint::default(),
            move || Ok(Arc::new(Fake::new(state.clone()))),
        );
        wait_for(|| trace.endpoint_probes.load(Ordering::Acquire) == 1);
        // The worker is blocked on the endpoint gate, while the GUI remains able
        // to read its last complete observation without touching that gate.
        let started = Instant::now();
        for _ in 0..1000 {
            assert!(!remote.observation().stopped);
        }
        assert!(started.elapsed() < Duration::from_millis(100));
        drop(gate);
        wait_for(|| remote.observation().stopped);
    }

    #[test]
    fn ready_snapshot_reads_do_not_enqueue_rpc() {
        let trace = Arc::new(Trace::default());
        let state = trace.clone();
        let remote = RemoteIndex::with_factory(
            SearchConfig::default(),
            ResultRepaint::default(),
            move || Ok(Arc::new(Fake::new(state.clone()))),
        );
        wait_for(|| remote.diagnostics().status == SearchIndexStatus::Ready);
        let before = trace.diagnostics.load(Ordering::Acquire);
        std::thread::sleep(Duration::from_millis(100));
        for _ in 0..100 {
            remote.diagnostics();
        }
        assert_eq!(trace.diagnostics.load(Ordering::Acquire), before);
    }
    #[test]
    fn ready_changes_reach_a_sleeping_gui_without_repainting_unchanged_polls() {
        let trace = Arc::new(Trace::default());
        let state = trace.clone();
        let repaint = ResultRepaint::default();
        let remote =
            RemoteIndex::with_factory(SearchConfig::default(), repaint.clone(), move || {
                Ok(Arc::new(Fake::new(state.clone())))
            });
        wait_for(|| remote.diagnostics().status == SearchIndexStatus::Ready);
        let ctx = egui::Context::default();
        let (wake, wakes) = mpsc::channel();
        ctx.set_request_repaint_callback(move |info| {
            let _ = wake.send(info.delay);
        });
        repaint.bind(&ctx);
        // No UI snapshot reads or explicit refresh commands drive these polls.
        let deadline = Instant::now() + Duration::from_secs(6);
        while trace.diagnostics.load(Ordering::Acquire) < 3 {
            assert!(
                Instant::now() < deadline,
                "Ready worker stopped observing the daemon"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            wakes.try_recv().is_err(),
            "Unchanged status repainted the GUI"
        );
        trace.indexed_items.store(1, Ordering::Release);
        assert_eq!(
            wakes.recv_timeout(Duration::from_secs(4)).unwrap(),
            Duration::ZERO
        );
        assert_eq!(
            remote.diagnostics().indexed_items,
            1,
            "Snapshot must precede wake"
        );
        let (dropped, gone) = mpsc::channel();
        *trace.dropped.lock().unwrap() = Some(dropped);
        drop(remote);
        gone.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    #[test]
    fn long_maintenance_does_not_hold_the_status_and_query_worker() {
        let trace = Arc::new(Trace::default());
        trace.hold_maintenance.store(true, Ordering::Release);
        let (started, entered) = mpsc::channel();
        *trace.maintenance_started.lock().unwrap() = Some(started);
        let state = trace.clone();
        let remote = RemoteIndex::with_factory(
            SearchConfig::default(),
            ResultRepaint::default(),
            move || Ok(Arc::new(Fake::new(state.clone()))),
        );
        remote.rebuild();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        remote
            .handle()
            .query(query(), &AtomicBool::new(false))
            .unwrap();
        remote.diagnostics();
        trace.hold_maintenance.store(false, Ordering::Release);
        wait_for(|| {
            !remote
                .handle
                .shared
                .maintenance_active
                .load(Ordering::Acquire)
        });
    }
}
