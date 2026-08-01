use crate::db::SearchStore;
use crate::mutation::{ActivationFence, GenerationSnapshot, IndexMutation};
use crate::plan::ScanPlan;
use crate::scanner::{ScanOutcome, ScanProgress};
use crate::watch::WatchRegistry;
use crate::{IndexState, current_unix_secs};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use ottrin_core::{SearchConfig, SearchIndexStatus};
use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const EVENT_DEBOUNCE: Duration = Duration::from_millis(500);
const WATCH_RETRY_INITIAL: Duration = Duration::from_secs(1);
const WATCH_RETRY_MAXIMUM: Duration = Duration::from_secs(60);
const INCREMENTAL_REBUILD_THRESHOLD: usize = 500;

pub(crate) trait StoreBackend: Send {
    fn load(
        &mut self,
        plan: &ScanPlan,
        cancelled: &AtomicBool,
    ) -> Result<Option<GenerationSnapshot>, String>;
    fn commit_guarded(
        &mut self,
        plan: &ScanPlan,
        mutation: IndexMutation,
        fence: &ActivationFence,
        expected_epoch: u64,
        publish: &mut dyn FnMut(&GenerationSnapshot),
    ) -> Result<(), StoreBackendError>;
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StoreBackendError {
    ActivationCancelled,
    Other(String),
}

impl StoreBackend for SearchStore {
    fn load(
        &mut self,
        plan: &ScanPlan,
        cancelled: &AtomicBool,
    ) -> Result<Option<GenerationSnapshot>, String> {
        self.load_cancellable(plan, cancelled)
            .map_err(|error| error.to_string())
    }

    fn commit_guarded(
        &mut self,
        plan: &ScanPlan,
        mutation: IndexMutation,
        fence: &ActivationFence,
        expected_epoch: u64,
        publish: &mut dyn FnMut(&GenerationSnapshot),
    ) -> Result<(), StoreBackendError> {
        self.commit_guarded_then(plan, mutation, fence, expected_epoch, publish)
            .map(|_| ())
            .map_err(|error| match error {
                crate::db::SearchDbError::ActivationCancelled => {
                    StoreBackendError::ActivationCancelled
                }
                other => StoreBackendError::Other(other.to_string()),
            })
    }
}

pub(crate) type ScanLauncher = Arc<
    dyn Fn(Arc<ScanPlan>, Arc<AtomicBool>, u64, mpsc::Sender<Command>) -> Option<JoinHandle<()>>
        + Send
        + Sync
        + 'static,
>;

type WatchSetupHook = Arc<dyn Fn() + Send + Sync + 'static>;

pub(crate) enum Command {
    UpdateConfig(SearchConfig),
    Rebuild(Option<mpsc::Sender<Result<(), String>>>),
    FilesystemEvent(Vec<PathBuf>),
    WatchBackendError {
        paths: Vec<PathBuf>,
        error: String,
    },
    ScanProgress {
        revision: u64,
        progress: ScanProgress,
    },
    ScanFinished {
        revision: u64,
        result: Result<ScanOutcome, String>,
    },
    #[cfg(test)]
    Barrier(mpsc::Sender<()>),
    Stop,
}

pub(crate) struct WorkerHandle {
    pub(crate) commands: mpsc::Sender<Command>,
    fence: Arc<ActivationFence>,
    desired_plan_identity: Arc<Mutex<Option<String>>>,
    shutdown: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl WorkerHandle {
    pub(crate) fn update_config(&self, config: SearchConfig) {
        let next_identity = ScanPlan::new(&config)
            .ok()
            .map(|plan| plan.identity().to_string());
        let mut desired = self
            .desired_plan_identity
            .lock()
            .expect("desired plan identity");
        if *desired != next_identity {
            self.fence.invalidate();
            *desired = next_identity;
        }
        drop(desired);
        let _ = self.commands.send(Command::UpdateConfig(config));
    }

    pub(crate) fn request_rebuild(&self) {
        self.invalidate_and_send(Command::Rebuild(None));
    }

    pub(crate) fn rebuild_blocking(&self) -> Result<(), String> {
        let (done, completed) = mpsc::channel();
        self.fence.invalidate();
        self.commands
            .send(Command::Rebuild(Some(done)))
            .map_err(|_| "Search worker stopped before rebuild was requested".to_string())?;
        completed
            .recv()
            .map_err(|_| "Search worker stopped before rebuild completed".to_string())?
    }

    #[cfg(test)]
    pub(crate) fn filesystem_event(&self) {
        let _ = self
            .commands
            .send(Command::FilesystemEvent(vec![PathBuf::from("/root/event")]));
    }

    pub(crate) fn stop(mut self) {
        self.shutdown.store(true, Ordering::Release);
        self.invalidate_and_send(Command::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }

    fn invalidate_and_send(&self, command: Command) {
        self.fence.invalidate();
        let _ = self.commands.send(command);
    }
}

struct ActiveScan {
    revision: u64,
    expected_epoch: u64,
    covered_event_revision: u64,
    cancelled: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    completion: Option<mpsc::Sender<Result<(), String>>>,
}

struct WorkerLoop {
    config: SearchConfig,
    index: Arc<RwLock<IndexState>>,
    store: Box<dyn StoreBackend>,
    launcher: ScanLauncher,
    commands: mpsc::Sender<Command>,
    fence: Arc<ActivationFence>,
    desired_plan_identity: Arc<Mutex<Option<String>>>,
    shutdown: Arc<AtomicBool>,
    current_plan: Option<Arc<ScanPlan>>,
    active_scan: Option<ActiveScan>,
    revision: u64,
    has_snapshot: bool,
    last_full_rebuild_at: Option<Instant>,
    last_updatedb_at: Option<Instant>,
    locate_db_initialized: bool,
    filesystem_rebuild_deadline: Option<Instant>,
    filesystem_event_revision: u64,
    pending_paths: BTreeSet<PathBuf>,
    watch_registry: Option<WatchRegistry>,
    watch_clock_started: Instant,
    before_watch_setup: Option<WatchSetupHook>,
}

struct WatchSetupOutcome {
    watcher: Option<RecommendedWatcher>,
    watched_roots: Vec<PathBuf>,
    registry: WatchRegistry,
    unavailable_roots: Vec<PathBuf>,
}

pub(crate) fn spawn_with_runtime(
    config: SearchConfig,
    index: Arc<RwLock<IndexState>>,
    store: SearchStore,
    launcher: ScanLauncher,
    watch_filesystem: bool,
) -> WorkerHandle {
    spawn_inner(
        config,
        index,
        Box::new(store),
        launcher,
        watch_filesystem,
        None,
    )
}

pub(crate) fn real_launcher() -> ScanLauncher {
    Arc::new(|plan, cancelled, revision, commands| {
        Some(std::thread::spawn(move || {
            let progress_commands = commands.clone();
            let result = crate::scanner::scan(&plan, &cancelled, move |progress| {
                let _ = progress_commands.send(Command::ScanProgress { revision, progress });
            });
            let _ = commands.send(Command::ScanFinished { revision, result });
        }))
    })
}

#[cfg(test)]
fn spawn_with(
    config: SearchConfig,
    index: Arc<RwLock<IndexState>>,
    store: Box<dyn StoreBackend>,
    launcher: ScanLauncher,
) -> WorkerHandle {
    spawn_inner(config, index, store, launcher, false, None)
}

#[cfg(test)]
fn spawn_with_watch_setup_hook(
    config: SearchConfig,
    index: Arc<RwLock<IndexState>>,
    store: Box<dyn StoreBackend>,
    launcher: ScanLauncher,
    before_watch_setup: WatchSetupHook,
) -> WorkerHandle {
    spawn_inner(
        config,
        index,
        store,
        launcher,
        false,
        Some(before_watch_setup),
    )
}

fn spawn_inner(
    config: SearchConfig,
    index: Arc<RwLock<IndexState>>,
    store: Box<dyn StoreBackend>,
    launcher: ScanLauncher,
    watch_filesystem: bool,
    before_watch_setup: Option<WatchSetupHook>,
) -> WorkerHandle {
    let desired_plan_identity = Arc::new(Mutex::new(
        ScanPlan::new(&config)
            .ok()
            .map(|plan| plan.identity().to_string()),
    ));
    let loop_desired_plan_identity = Arc::clone(&desired_plan_identity);
    let shutdown = Arc::new(AtomicBool::new(false));
    let loop_shutdown = Arc::clone(&shutdown);
    let (commands, receiver) = mpsc::channel();
    let loop_commands = commands.clone();
    let fence = Arc::new(ActivationFence::new());
    let loop_fence = Arc::clone(&fence);
    let join = std::thread::spawn(move || {
        let mut worker = WorkerLoop {
            config,
            index,
            store,
            launcher,
            commands: loop_commands,
            fence: loop_fence,
            desired_plan_identity: loop_desired_plan_identity,
            shutdown: loop_shutdown,
            current_plan: None,
            active_scan: None,
            revision: 0,
            has_snapshot: false,
            last_full_rebuild_at: None,
            last_updatedb_at: None,
            locate_db_initialized: false,
            filesystem_rebuild_deadline: None,
            filesystem_event_revision: 0,
            pending_paths: BTreeSet::new(),
            watch_registry: None,
            watch_clock_started: Instant::now(),
            before_watch_setup,
        };
        worker.run(receiver, watch_filesystem);
    });
    WorkerHandle {
        commands,
        fence,
        desired_plan_identity,
        shutdown,
        join: Some(join),
    }
}

impl WorkerLoop {
    fn run(&mut self, receiver: mpsc::Receiver<Command>, watch_filesystem: bool) {
        let mut watcher = watch_filesystem.then(|| self.make_watcher()).flatten();
        let mut watched_roots = Vec::new();
        let mut pending_commands = VecDeque::new();
        if !self.install_plan(
            &mut watcher,
            &mut watched_roots,
            &receiver,
            &mut pending_commands,
        ) {
            return;
        }

        loop {
            let command = pending_commands
                .pop_front()
                .map(Ok)
                .unwrap_or_else(|| receiver.recv_timeout(Duration::from_millis(100)));
            match command {
                Ok(Command::UpdateConfig(config)) => {
                    let identity_changed = ScanPlan::new(&config)
                        .ok()
                        .map(|plan| plan.identity().to_string())
                        != self
                            .current_plan
                            .as_ref()
                            .map(|plan| plan.identity().to_string());
                    self.config = config;
                    if identity_changed
                        && !self.install_plan(
                            &mut watcher,
                            &mut watched_roots,
                            &receiver,
                            &mut pending_commands,
                        )
                    {
                        return;
                    }
                }
                Ok(Command::Rebuild(completion)) => self.start_scan(completion),
                Ok(Command::FilesystemEvent(paths)) => {
                    self.filesystem_event_revision = self.filesystem_event_revision.wrapping_add(1);
                    self.pending_paths.extend(paths);
                    self.filesystem_rebuild_deadline = Some(Instant::now() + EVENT_DEBOUNCE);
                }
                Ok(Command::WatchBackendError { paths, error }) => {
                    if let Some(registry) = self.watch_registry.as_mut() {
                        registry.mark_paths_uncertain(&paths, &error);
                    }
                    self.update_watcher_diagnostics();
                }
                Ok(Command::ScanProgress { revision, progress }) => {
                    self.record_progress(revision, progress);
                }
                Ok(Command::ScanFinished { revision, result }) => {
                    self.finish_scan(revision, result);
                }
                #[cfg(test)]
                Ok(Command::Barrier(done)) => {
                    let _ = done.send(());
                }
                Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.cancel_active();
                    self.record_stopped();
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.run_schedules(&mut watcher, &mut watched_roots);
                }
            }
        }
    }

    fn make_watcher(&self) -> Option<RecommendedWatcher> {
        let commands = self.commands.clone();
        RecommendedWatcher::new(
            move |result: notify::Result<notify::Event>| match result {
                Ok(event)
                    if matches!(
                        event.kind,
                        notify::EventKind::Create(_)
                            | notify::EventKind::Modify(_)
                            | notify::EventKind::Remove(_)
                    ) =>
                {
                    let _ = commands.send(Command::FilesystemEvent(event.paths));
                }
                Ok(_) => {}
                Err(error) => {
                    let paths = error.paths.clone();
                    let _ = commands.send(Command::WatchBackendError {
                        paths,
                        error: error.to_string(),
                    });
                }
            },
            notify::Config::default(),
        )
        .ok()
    }

    fn install_plan(
        &mut self,
        watcher: &mut Option<RecommendedWatcher>,
        watched_roots: &mut Vec<PathBuf>,
        receiver: &mpsc::Receiver<Command>,
        pending_commands: &mut VecDeque<Command>,
    ) -> bool {
        self.cancel_active();
        let plan = match ScanPlan::new(&self.config) {
            Ok(plan) => Arc::new(plan),
            Err(error) => {
                self.current_plan = None;
                let mut index = self.index.write().expect("index write");
                index.status = SearchIndexStatus::Unavailable;
                index.last_error = Some(error);
                index.progress_text = None;
                return true;
            }
        };
        self.current_plan = Some(Arc::clone(&plan));
        let load_result = self.store.load(&plan, &self.shutdown);
        match &load_result {
            Ok(Some(snapshot)) => {
                self.publish(snapshot.clone());
                self.last_full_rebuild_at = Some(Instant::now());
            }
            Ok(None) | Err(_) => {
                let mut index = self.index.write().expect("index write");
                index.status = SearchIndexStatus::Indexing;
                index.progress_text = Some("Indexing · preparing filesystem watcher…".to_string());
                index.active_root = None;
                index.active_root_index = 0;
                index.total_roots = plan.roots().len();
            }
        }
        if !self.configure_watcher(watcher, watched_roots, &plan, receiver, pending_commands) {
            return false;
        }
        match load_result {
            Ok(Some(_)) => {}
            Ok(None) => self.start_scan(None),
            Err(error) => {
                let mut index = self.index.write().expect("index write");
                index.last_error = Some(error);
                index.status = if self.has_snapshot {
                    SearchIndexStatus::Ready
                } else {
                    SearchIndexStatus::Indexing
                };
                drop(index);
                self.start_scan(None);
            }
        }
        true
    }

    fn configure_watcher(
        &mut self,
        watcher: &mut Option<RecommendedWatcher>,
        watched_roots: &mut Vec<PathBuf>,
        plan: &ScanPlan,
        receiver: &mpsc::Receiver<Command>,
        pending_commands: &mut VecDeque<Command>,
    ) -> bool {
        let mut setup_watcher = watcher.take();
        let mut setup_watched_roots = std::mem::take(watched_roots);
        let roots = plan.roots().to_vec();
        let now = self.watch_clock_started.elapsed();
        let hook = self.before_watch_setup.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let setup_cancelled = Arc::clone(&cancelled);
        let (finished_tx, finished_rx) = mpsc::channel();
        std::thread::spawn(move || {
            if let Some(hook) = hook {
                hook();
            }
            if setup_cancelled.load(Ordering::Acquire) {
                return;
            }
            if let Some(watcher) = setup_watcher.as_mut() {
                for root in setup_watched_roots.drain(..) {
                    let _ = watcher.unwatch(&root);
                }
            }
            let mut registry =
                WatchRegistry::new(roots.clone(), WATCH_RETRY_INITIAL, WATCH_RETRY_MAXIMUM);
            let mut unavailable_roots = Vec::new();
            for root in &roots {
                if setup_cancelled.load(Ordering::Acquire) {
                    return;
                }
                if !root.exists() {
                    registry.record_watch_failure(root, now, "configured root unavailable");
                    unavailable_roots.push(root.clone());
                    continue;
                }
                match setup_watcher.as_mut() {
                    Some(watcher) => match watcher.watch(root, RecursiveMode::Recursive) {
                        Ok(()) => {
                            registry.record_watch_success(root);
                            setup_watched_roots.push(root.clone());
                        }
                        Err(error) => registry.record_watch_failure(root, now, &error.to_string()),
                    },
                    None => registry.record_watch_failure(root, now, "watch backend unavailable"),
                }
            }
            if setup_cancelled.load(Ordering::Acquire) {
                return;
            }
            let _ = finished_tx.send(WatchSetupOutcome {
                watcher: setup_watcher,
                watched_roots: setup_watched_roots,
                registry,
                unavailable_roots,
            });
        });

        let outcome = loop {
            match finished_rx.try_recv() {
                Ok(outcome) => break outcome,
                Err(mpsc::TryRecvError::Disconnected) => return false,
                Err(mpsc::TryRecvError::Empty) => {}
            }
            match receiver.recv_timeout(Duration::from_millis(10)) {
                Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    cancelled.store(true, Ordering::Release);
                    self.cancel_active();
                    self.record_stopped();
                    return false;
                }
                Ok(command) => pending_commands.push_back(command),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        };
        cancelled.store(true, Ordering::Release);
        *watcher = outcome.watcher;
        *watched_roots = outcome.watched_roots;
        let desired_identity = self
            .desired_plan_identity
            .lock()
            .expect("desired plan identity");
        let setup_is_current = desired_identity.as_deref() == Some(plan.identity());
        drop(desired_identity);
        if !setup_is_current {
            return true;
        }
        for root in outcome.unavailable_roots {
            self.pending_paths.insert(root);
            self.filesystem_event_revision = self.filesystem_event_revision.wrapping_add(1);
            self.filesystem_rebuild_deadline = Some(Instant::now() + EVENT_DEBOUNCE);
        }
        self.watch_registry = Some(outcome.registry);
        self.index
            .write()
            .expect("index write")
            .watcher_last_setup_unix_secs = Some(current_unix_secs());
        self.update_watcher_diagnostics();
        true
    }

    fn update_watcher_diagnostics(&self) {
        let Some(registry) = self.watch_registry.as_ref() else {
            return;
        };
        let mut index = self.index.write().expect("index write");
        index.configured_root_count = registry.roots().count();
        index.watched_root_count = registry.watched_roots().len();
        index.watcher_ready = registry.ready_with(Path::exists);
        index.watcher_last_error = registry.last_error().map(str::to_string);
    }

    fn start_scan(&mut self, completion: Option<mpsc::Sender<Result<(), String>>>) {
        let Some(plan) = self.current_plan.as_ref().cloned() else {
            if let Some(completion) = completion {
                let _ = completion.send(Err("Search plan is unavailable".to_string()));
            }
            return;
        };
        self.cancel_active();
        self.revision = self.revision.wrapping_add(1);
        let revision = self.revision;
        let expected_epoch = self.fence.capture();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.active_scan = Some(ActiveScan {
            revision,
            expected_epoch,
            covered_event_revision: self.filesystem_event_revision,
            cancelled: Arc::clone(&cancelled),
            join: None,
            completion,
        });
        {
            let mut index = self.index.write().expect("index write");
            index.status = if self.has_snapshot {
                SearchIndexStatus::Ready
            } else {
                SearchIndexStatus::Indexing
            };
            index.last_error = None;
            index.progress_text = Some("Indexing · scanning…".to_string());
            index.building_indexed_items = 0;
            index.last_indexed_path = None;
            index.active_root = None;
            index.active_root_index = 0;
            index.total_roots = plan.roots().len();
            index.last_scan_started_unix_secs = Some(current_unix_secs());
            index.last_scan_duration_ms = None;
            index.last_progress_unix_secs = Some(current_unix_secs());
        }
        let join = (self.launcher)(plan, cancelled, revision, self.commands.clone());
        self.active_scan
            .as_mut()
            .expect("active scan exists after launch")
            .join = join;
    }

    fn cancel_active(&mut self) {
        if let Some(mut active) = self.active_scan.take() {
            active.cancelled.store(true, Ordering::Release);
            if let Some(join) = active.join.take() {
                let _ = join.join();
            }
            if let Some(completion) = active.completion.take() {
                let _ = completion.send(Err("Requested rebuild was cancelled".to_string()));
            }
        }
    }

    fn record_progress(&mut self, revision: u64, progress: ScanProgress) {
        if self.active_scan.as_ref().is_none_or(|active| {
            active.revision != revision || active.cancelled.load(Ordering::Acquire)
        }) {
            return;
        }
        let mut index = self.index.write().expect("index write");
        index.progress_text = Some(progress.message);
        index.active_root = Some(progress.active_root);
        index.active_root_index = progress.active_root_index;
        index.total_roots = progress.total_roots;
        index.building_indexed_items = progress.indexed_items;
        index.last_indexed_path = progress.last_indexed_path;
        index.last_progress_unix_secs = Some(current_unix_secs());
    }

    fn finish_scan(&mut self, revision: u64, result: Result<ScanOutcome, String>) {
        let Some(active) = self.active_scan.as_ref() else {
            return;
        };
        if active.revision != revision || active.cancelled.load(Ordering::Acquire) {
            return;
        }
        let mut active = self.active_scan.take().expect("matching active scan");
        if let Some(join) = active.join.take() {
            let _ = join.join();
        }
        let completion = active.completion.take();
        let complete = |result| {
            if let Some(completion) = completion.as_ref() {
                let _ = completion.send(result);
            }
        };
        match result {
            Ok(ScanOutcome::Completed(items)) => {
                let Some(plan) = self.current_plan.as_ref() else {
                    complete(Err(
                        "Search plan became unavailable before commit".to_string()
                    ));
                    return;
                };
                let expected_epoch = active.expected_epoch;
                let index = Arc::clone(&self.index);
                let mut publish = |snapshot: &GenerationSnapshot| {
                    publish_snapshot(&index, snapshot);
                };
                match self.store.commit_guarded(
                    plan,
                    IndexMutation::ReplaceAll(items),
                    &self.fence,
                    expected_epoch,
                    &mut publish,
                ) {
                    Ok(()) => {
                        self.has_snapshot = true;
                        self.last_full_rebuild_at = Some(Instant::now());
                        if active.covered_event_revision == self.filesystem_event_revision {
                            self.pending_paths.clear();
                            self.filesystem_rebuild_deadline = None;
                        } else if !self.pending_paths.is_empty() {
                            self.filesystem_rebuild_deadline =
                                Some(Instant::now() + EVENT_DEBOUNCE);
                        }
                        complete(Ok(()));
                    }
                    Err(StoreBackendError::ActivationCancelled) => {
                        if !self.pending_paths.is_empty() {
                            self.filesystem_rebuild_deadline =
                                Some(Instant::now() + EVENT_DEBOUNCE);
                        }
                        complete(Err("Requested rebuild was superseded".to_string()));
                    }
                    Err(StoreBackendError::Other(error)) => {
                        complete(Err(error.clone()));
                        self.record_failure(error);
                        if !self.pending_paths.is_empty() {
                            self.filesystem_rebuild_deadline =
                                Some(Instant::now() + EVENT_DEBOUNCE);
                        }
                    }
                }
            }
            Ok(ScanOutcome::Cancelled) => {
                complete(Err("Requested rebuild was cancelled".to_string()));
                if !self.pending_paths.is_empty() {
                    self.filesystem_rebuild_deadline = Some(Instant::now() + EVENT_DEBOUNCE);
                }
            }
            Err(error) => {
                complete(Err(error.clone()));
                self.record_failure(error);
                if !self.pending_paths.is_empty() {
                    self.filesystem_rebuild_deadline = Some(Instant::now() + EVENT_DEBOUNCE);
                }
            }
        }
    }

    fn publish(&mut self, snapshot: GenerationSnapshot) {
        publish_snapshot(&self.index, &snapshot);
        self.has_snapshot = true;
    }

    fn record_failure(&mut self, error: String) {
        let mut index = self.index.write().expect("index write");
        index.status = if self.has_snapshot {
            SearchIndexStatus::Ready
        } else {
            SearchIndexStatus::Unavailable
        };
        index.last_error = Some(error);
        index.progress_text = None;
        index.active_root = None;
        index.active_root_index = 0;
        index.last_progress_unix_secs = Some(current_unix_secs());
    }

    fn record_stopped(&self) {
        let mut index = self.index.write().expect("index write");
        index.status = if self.has_snapshot {
            SearchIndexStatus::Ready
        } else {
            SearchIndexStatus::Unavailable
        };
        if !self.has_snapshot && index.last_error.is_none() {
            index.last_error = Some("Search indexing is stopped".to_string());
        }
        index.progress_text = None;
        index.building_indexed_items = 0;
        index.active_root = None;
        index.active_root_index = 0;
        index.watched_root_count = 0;
        index.watcher_ready = false;
        index.last_progress_unix_secs = Some(current_unix_secs());
    }

    fn commit_mutation(&mut self, mutation: IndexMutation, expected_epoch: u64) -> bool {
        let Some(plan) = self.current_plan.as_ref() else {
            return false;
        };
        let index = Arc::clone(&self.index);
        let mut publish = |snapshot: &GenerationSnapshot| publish_snapshot(&index, snapshot);
        match self
            .store
            .commit_guarded(plan, mutation, &self.fence, expected_epoch, &mut publish)
        {
            Ok(()) => {
                self.has_snapshot = true;
                true
            }
            Err(StoreBackendError::ActivationCancelled) => false,
            Err(StoreBackendError::Other(error)) => {
                self.record_failure(error);
                false
            }
        }
    }

    fn apply_pending_delta(&mut self) {
        if self.pending_paths.is_empty() || self.active_scan.is_some() {
            return;
        }
        if self.pending_paths.len() > INCREMENTAL_REBUILD_THRESHOLD {
            self.start_scan(None);
            return;
        }
        let Some(plan) = self.current_plan.as_ref() else {
            return;
        };
        let expected_epoch = self.fence.capture();
        let paths = self.pending_paths.iter().cloned().collect::<Vec<_>>();
        let mut upserts = Vec::new();
        let mut remove_trees = Vec::new();
        let mut classified = BTreeSet::new();
        for path in paths {
            if plan.root_for(&path).is_none() {
                classified.insert(path);
                continue;
            }
            match crate::scanner::candidates_for_path(plan, &path) {
                Ok(items) => {
                    remove_trees.push(path.clone());
                    upserts.extend(items);
                    classified.insert(path);
                }
                Err(error) => {
                    if let Some(registry) = self.watch_registry.as_mut() {
                        registry.mark_paths_uncertain(std::slice::from_ref(&path), &error);
                    }
                }
            }
        }
        let committed = if !upserts.is_empty() || !remove_trees.is_empty() {
            self.commit_mutation(
                IndexMutation::Delta {
                    upserts,
                    remove_trees,
                },
                expected_epoch,
            )
        } else {
            true
        };
        if committed {
            for path in classified {
                self.pending_paths.remove(&path);
            }
        } else {
            self.filesystem_rebuild_deadline = Some(Instant::now() + EVENT_DEBOUNCE);
        }
        self.update_watcher_diagnostics();
    }

    fn recover_root(&mut self, root: &Path) -> bool {
        if self.active_scan.is_some() || !self.has_snapshot {
            return false;
        }
        let Some(plan) = self.current_plan.as_ref().cloned() else {
            return false;
        };
        let expected_epoch = self.fence.capture();
        if let Some(registry) = self.watch_registry.as_mut() {
            registry.begin_recovery(root);
        }
        let cancelled = AtomicBool::new(false);
        let result = crate::scanner::scan_root(&plan, root, &cancelled);
        let committed = match result {
            Ok(ScanOutcome::Completed(items)) => self.commit_mutation(
                IndexMutation::ReplaceRoot {
                    root: root.to_path_buf(),
                    items,
                },
                expected_epoch,
            ),
            Ok(ScanOutcome::Cancelled) => false,
            Err(error) => {
                self.record_failure(error);
                false
            }
        };
        if let Some(registry) = self.watch_registry.as_mut() {
            if committed {
                registry.complete_recovery(root);
            } else {
                registry.record_recovery_failure(
                    root,
                    self.watch_clock_started.elapsed(),
                    "root recovery did not commit",
                );
            }
        }
        committed
    }

    fn run_schedules(
        &mut self,
        watcher: &mut Option<RecommendedWatcher>,
        watched_roots: &mut Vec<PathBuf>,
    ) {
        let now = self.watch_clock_started.elapsed();
        let disappeared = self
            .watch_registry
            .as_ref()
            .map(WatchRegistry::watched_roots)
            .unwrap_or_default()
            .into_iter()
            .filter(|root| !root.exists())
            .collect::<Vec<_>>();
        for root in disappeared {
            if let Some(watcher) = watcher.as_mut() {
                let _ = watcher.unwatch(&root);
            }
            watched_roots.retain(|watched| watched != &root);
            if let Some(registry) = self.watch_registry.as_mut() {
                registry.record_watch_failure(&root, now, "configured root disappeared");
            }
            self.pending_paths.insert(root);
            self.filesystem_event_revision = self.filesystem_event_revision.wrapping_add(1);
            self.filesystem_rebuild_deadline = Some(Instant::now() + EVENT_DEBOUNCE);
        }
        let retries = self
            .watch_registry
            .as_ref()
            .map(|registry| registry.due_retries(now))
            .unwrap_or_default();
        for root in retries {
            if !root.exists() {
                if let Some(registry) = self.watch_registry.as_mut() {
                    registry.record_watch_failure(&root, now, "configured root unavailable");
                }
                continue;
            }
            let result = watcher
                .as_mut()
                .ok_or_else(|| "watch backend unavailable".to_string())
                .and_then(|watcher| {
                    watcher
                        .watch(&root, RecursiveMode::Recursive)
                        .map_err(|error| error.to_string())
                });
            if let Some(registry) = self.watch_registry.as_mut() {
                match result {
                    Ok(()) => {
                        registry.record_retry_watch_success(&root);
                        if !watched_roots.contains(&root) {
                            watched_roots.push(root.clone());
                        }
                    }
                    Err(error) => registry.record_watch_failure(&root, now, &error),
                }
            }
        }
        let recoveries = if self.has_snapshot && self.active_scan.is_none() {
            self.watch_registry
                .as_ref()
                .map(|registry| registry.recovery_roots(now))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        for root in recoveries {
            self.recover_root(&root);
        }
        self.update_watcher_diagnostics();

        if self
            .filesystem_rebuild_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
            && self.active_scan.is_none()
        {
            self.filesystem_rebuild_deadline = None;
            self.apply_pending_delta();
        }
        let refresh_secs = u64::from(self.config.refresh_interval_hours) * 3_600;
        if refresh_secs > 0
            && self
                .last_full_rebuild_at
                .is_some_and(|last| last.elapsed().as_secs() >= refresh_secs)
            && self.active_scan.is_none()
        {
            self.start_scan(None);
        }
        if self.config.manage_locate_db && crate::updatedb_available() {
            let locate_secs = u64::from(self.config.locate_update_hours) * 3_600;
            let due = if !self.locate_db_initialized {
                crate::locate_db_age_secs().is_none_or(|age| age > 3_600)
            } else {
                locate_secs > 0
                    && self
                        .last_updatedb_at
                        .is_some_and(|last| last.elapsed().as_secs() >= locate_secs)
            };
            self.locate_db_initialized = true;
            if due {
                let root = self
                    .current_plan
                    .as_ref()
                    .and_then(|plan| plan.roots().first())
                    .cloned()
                    .unwrap_or_else(ottrin_core::default_home_dir);
                crate::run_user_updatedb(root);
                self.last_updatedb_at = Some(Instant::now());
            }
        }
    }
}

fn publish_snapshot(index: &RwLock<IndexState>, snapshot: &GenerationSnapshot) {
    let items = snapshot
        .items()
        .iter()
        .map(|owned| owned.item().clone())
        .collect::<Vec<_>>();
    let count = items.len();
    let mut index = index.write().expect("index write");
    index.items = Arc::new(items);
    index.snapshot_revision = index.snapshot_revision.wrapping_add(1);
    index.status = SearchIndexStatus::Ready;
    index.last_error = None;
    index.progress_text = Some(format!("Ready · {count} indexed"));
    index.last_completed_indexed_items = Some(count);
    index.building_indexed_items = 0;
    index.active_root = None;
    index.active_root_index = 0;
    index.last_scan_completed_unix_secs = Some(current_unix_secs());
    index.last_progress_unix_secs = Some(current_unix_secs());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mutation::{GenerationSnapshot, OwnedItem};
    use ottrin_core::{EntryKind, SearchConfig, SearchIndexStatus, SearchResultItem};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Condvar, Mutex, mpsc};
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(2);

    fn config(root: &str) -> SearchConfig {
        SearchConfig {
            include_roots: vec![PathBuf::from(root)],
            include_hidden_system: true,
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        }
    }

    fn owned(root: &str, name: &str) -> OwnedItem {
        let path = PathBuf::from(root).join(name);
        let mut item = SearchResultItem {
            parent_path: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            name: name.to_string(),
            path,
            kind: EntryKind::File,
            size_bytes: Some(1),
            modified_unix_secs: Some(1),
            is_executable: false,
            symlink_target_is_dir: None,
            content_snippet: None,
            name_lower: String::new(),
            path_str: String::new(),
            path_lower: String::new(),
        };
        item.prepare();
        OwnedItem::new(PathBuf::from(root), item)
    }

    #[derive(Default)]
    struct MemoryStoreState {
        active: Option<GenerationSnapshot>,
        commits: Vec<String>,
        next_generation: i64,
    }

    struct MemoryStore(Arc<Mutex<MemoryStoreState>>);

    impl StoreBackend for MemoryStore {
        fn load(
            &mut self,
            plan: &ScanPlan,
            _cancelled: &AtomicBool,
        ) -> Result<Option<GenerationSnapshot>, String> {
            let state = self.0.lock().expect("memory store");
            Ok(state
                .active
                .as_ref()
                .filter(|snapshot| snapshot.plan_identity() == plan.identity())
                .cloned())
        }

        fn commit_guarded(
            &mut self,
            plan: &ScanPlan,
            mutation: IndexMutation,
            fence: &ActivationFence,
            expected_epoch: u64,
            publish: &mut dyn FnMut(&GenerationSnapshot),
        ) -> Result<(), StoreBackendError> {
            let guard = fence.lock();
            if !guard.matches(expected_epoch) {
                return Err(StoreBackendError::ActivationCancelled);
            }
            let mut state = self.0.lock().expect("memory store");
            state.next_generation += 1;
            let current = state.active.clone().unwrap_or_else(|| {
                GenerationSnapshot::new(0, plan.identity().to_string(), Vec::new())
            });
            let items = current.apply(mutation).items().to_vec();
            let snapshot =
                GenerationSnapshot::new(state.next_generation, plan.identity().to_string(), items);
            state.commits.push(plan.identity().to_string());
            state.active = Some(snapshot.clone());
            publish(&snapshot);
            Ok(())
        }
    }

    struct BlockingStore {
        inner: MemoryStore,
        entered: mpsc::Sender<()>,
        release: Arc<(Mutex<bool>, Condvar)>,
    }

    impl StoreBackend for BlockingStore {
        fn load(
            &mut self,
            plan: &ScanPlan,
            _cancelled: &AtomicBool,
        ) -> Result<Option<GenerationSnapshot>, String> {
            self.inner.load(plan, _cancelled)
        }

        fn commit_guarded(
            &mut self,
            plan: &ScanPlan,
            mutation: IndexMutation,
            fence: &ActivationFence,
            expected_epoch: u64,
            publish: &mut dyn FnMut(&GenerationSnapshot),
        ) -> Result<(), StoreBackendError> {
            self.entered.send(()).expect("signal commit entered");
            let (lock, ready) = &*self.release;
            let mut released = lock.lock().expect("release lock");
            while !*released {
                released = ready.wait(released).expect("release wait");
            }
            self.inner
                .commit_guarded(plan, mutation, fence, expected_epoch, publish)
        }
    }

    struct BlockingLoadStore {
        entered: mpsc::Sender<()>,
    }

    impl StoreBackend for BlockingLoadStore {
        fn load(
            &mut self,
            _plan: &ScanPlan,
            cancelled: &AtomicBool,
        ) -> Result<Option<GenerationSnapshot>, String> {
            self.entered.send(()).expect("signal load entered");
            while !cancelled.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err("generation load was cancelled".to_string())
        }

        fn commit_guarded(
            &mut self,
            _plan: &ScanPlan,
            _mutation: IndexMutation,
            _fence: &ActivationFence,
            _expected_epoch: u64,
            _publish: &mut dyn FnMut(&GenerationSnapshot),
        ) -> Result<(), StoreBackendError> {
            panic!("blocked startup load must not commit")
        }
    }

    struct OrderedStore {
        inner: MemoryStore,
        kinds: Arc<Mutex<Vec<&'static str>>>,
    }

    impl StoreBackend for OrderedStore {
        fn load(
            &mut self,
            plan: &ScanPlan,
            _cancelled: &AtomicBool,
        ) -> Result<Option<GenerationSnapshot>, String> {
            self.inner.load(plan, _cancelled)
        }

        fn commit_guarded(
            &mut self,
            plan: &ScanPlan,
            mutation: IndexMutation,
            fence: &ActivationFence,
            expected_epoch: u64,
            publish: &mut dyn FnMut(&GenerationSnapshot),
        ) -> Result<(), StoreBackendError> {
            self.kinds.lock().expect("kinds").push(match &mutation {
                IndexMutation::ReplaceAll(_) => "replace-all",
                IndexMutation::ReplaceRoot { .. } => "replace-root",
                IndexMutation::Delta { .. } => "delta",
            });
            self.inner
                .commit_guarded(plan, mutation, fence, expected_epoch, publish)
        }
    }

    struct ControlledScan {
        revision: u64,
        plan_identity: String,
        cancelled: Arc<AtomicBool>,
        commands: mpsc::Sender<Command>,
    }

    impl ControlledScan {
        fn complete(self, items: Vec<OwnedItem>) {
            self.commands
                .send(Command::ScanFinished {
                    revision: self.revision,
                    result: Ok(ScanOutcome::Completed(items)),
                })
                .expect("send completed scan");
            barrier(&self.commands);
        }

        fn fail(self, message: &str) {
            self.commands
                .send(Command::ScanFinished {
                    revision: self.revision,
                    result: Err(message.to_string()),
                })
                .expect("send failed scan");
            barrier(&self.commands);
        }
    }

    fn controlled_launcher() -> (ScanLauncher, mpsc::Receiver<ControlledScan>) {
        let (started_tx, started_rx) = mpsc::channel();
        let launcher = Arc::new(move |plan: Arc<ScanPlan>, cancelled, revision, commands| {
            started_tx
                .send(ControlledScan {
                    revision,
                    plan_identity: plan.identity().to_string(),
                    cancelled,
                    commands,
                })
                .expect("announce controlled scan");
            None
        });
        (launcher, started_rx)
    }

    fn barrier(commands: &mpsc::Sender<Command>) {
        let (tx, rx) = mpsc::channel();
        commands.send(Command::Barrier(tx)).expect("send barrier");
        rx.recv_timeout(WAIT).expect("worker barrier");
    }

    fn snapshot_items(index: &Arc<RwLock<IndexState>>) -> Vec<PathBuf> {
        index
            .read()
            .expect("index read")
            .items
            .iter()
            .map(|item| item.path.clone())
            .collect()
    }

    fn start_controlled(
        cfg: SearchConfig,
        store_state: Arc<Mutex<MemoryStoreState>>,
    ) -> (
        WorkerHandle,
        Arc<RwLock<IndexState>>,
        mpsc::Receiver<ControlledScan>,
    ) {
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, scans) = controlled_launcher();
        let worker = spawn_with(
            cfg,
            Arc::clone(&index),
            Box::new(MemoryStore(store_state)),
            launcher,
        );
        (worker, index, scans)
    }

    #[test]
    fn blocking_rebuild_completes_only_after_its_snapshot_commits() {
        let store = Arc::new(Mutex::new(MemoryStoreState::default()));
        let (worker, index, scans) = start_controlled(config("/root"), Arc::clone(&store));
        let initial = scans.recv_timeout(WAIT).expect("initial scan");
        let worker = Arc::new(worker);
        let waiting = Arc::clone(&worker);
        let completed = std::thread::spawn(move || waiting.rebuild_blocking());

        let requested = scans.recv_timeout(WAIT).expect("requested rebuild");
        assert!(initial.cancelled.load(Ordering::Acquire));
        assert!(!completed.is_finished());
        requested.complete(vec![owned("/root", "committed.txt")]);

        assert_eq!(completed.join().expect("join rebuild waiter"), Ok(()));
        assert_eq!(
            snapshot_items(&index),
            vec![PathBuf::from("/root/committed.txt")]
        );
        Arc::try_unwrap(worker)
            .unwrap_or_else(|_| panic!("worker references remain"))
            .stop();
    }

    #[test]
    fn cancelled_rebuild_never_reaches_store_or_publication() {
        let store = Arc::new(Mutex::new(MemoryStoreState::default()));
        let (worker, index, scans) = start_controlled(config("/old"), Arc::clone(&store));
        let old = scans.recv_timeout(WAIT).expect("initial scan");

        worker.request_rebuild();
        let _new = scans.recv_timeout(WAIT).expect("replacement scan");
        assert!(old.cancelled.load(Ordering::Acquire));
        old.complete(vec![owned("/old", "stale.txt")]);

        assert!(store.lock().expect("store").commits.is_empty());
        assert!(snapshot_items(&index).is_empty());
        worker.stop();
    }

    #[test]
    fn saved_snapshot_is_queryable_while_watcher_setup_is_blocked() {
        let cfg = config("/root");
        let plan = ScanPlan::new(&cfg).expect("plan");
        let prior = owned("/root", "prior.txt");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                4,
                plan.identity().to_string(),
                vec![prior.clone()],
            )),
            commits: Vec::new(),
            next_generation: 4,
        }));
        let entered = mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let hook_release = Arc::clone(&release);
        let hook = Arc::new(move || {
            entered.0.send(()).expect("signal watcher setup");
            let (lock, ready) = &*hook_release;
            let mut released = lock.lock().expect("watch setup release");
            while !*released {
                released = ready.wait(released).expect("watch setup wait");
            }
        });
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, _scans) = controlled_launcher();
        let worker = spawn_with_watch_setup_hook(
            cfg,
            Arc::clone(&index),
            Box::new(MemoryStore(store)),
            launcher,
            hook,
        );
        entered.1.recv_timeout(WAIT).expect("watch setup entered");

        let items_while_blocked = snapshot_items(&index);
        let status_while_blocked = index.read().expect("index").status;
        let (lock, ready) = &*release;
        *lock.lock().expect("watch setup release") = true;
        ready.notify_all();
        worker.stop();

        assert_eq!(items_while_blocked, vec![prior.item().path.clone()]);
        assert_eq!(status_while_blocked, SearchIndexStatus::Ready);
    }

    #[test]
    fn stop_completes_while_generation_load_is_blocked() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, scans) = controlled_launcher();
        let worker = spawn_with(
            config("/root"),
            index,
            Box::new(BlockingLoadStore {
                entered: entered_tx,
            }),
            launcher,
        );
        entered_rx.recv_timeout(WAIT).expect("load entered");
        let (stopped_tx, stopped_rx) = mpsc::channel();
        std::thread::spawn(move || {
            worker.stop();
            let _ = stopped_tx.send(());
        });

        stopped_rx
            .recv_timeout(WAIT)
            .expect("stop returned after cancelling generation load");
        assert!(
            scans.try_recv().is_err(),
            "shutdown cancellation must not launch a rebuild"
        );
    }

    #[test]
    fn stop_completes_while_watcher_setup_is_blocked() {
        let cfg = config("/root");
        let plan = ScanPlan::new(&cfg).expect("plan");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                4,
                plan.identity().to_string(),
                vec![owned("/root", "prior.txt")],
            )),
            commits: Vec::new(),
            next_generation: 4,
        }));
        let entered = mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let hook_release = Arc::clone(&release);
        let hook = Arc::new(move || {
            entered.0.send(()).expect("signal watcher setup");
            let (lock, ready) = &*hook_release;
            let mut released = lock.lock().expect("watch setup release");
            while !*released {
                released = ready.wait(released).expect("watch setup wait");
            }
        });
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, _scans) = controlled_launcher();
        let worker =
            spawn_with_watch_setup_hook(cfg, index, Box::new(MemoryStore(store)), launcher, hook);
        entered.1.recv_timeout(WAIT).expect("watch setup entered");
        let (stopped_tx, stopped_rx) = mpsc::channel();
        let stop_thread = std::thread::spawn(move || {
            worker.stop();
            stopped_tx.send(()).expect("signal stopped");
        });

        let stopped_while_blocked = stopped_rx.recv_timeout(Duration::from_millis(100)).is_ok();
        let (lock, ready) = &*release;
        *lock.lock().expect("watch setup release") = true;
        ready.notify_all();
        stop_thread.join().expect("stop thread");

        assert!(
            stopped_while_blocked,
            "worker stop waited for blocked watcher setup"
        );
    }

    #[test]
    fn superseded_watcher_setup_never_publishes_stale_diagnostics() {
        let initial = config("/root-a");
        let initial_plan = ScanPlan::new(&initial).expect("initial plan");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                4,
                initial_plan.identity().to_string(),
                vec![owned("/root-a", "prior.txt")],
            )),
            commits: Vec::new(),
            next_generation: 4,
        }));
        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (second_entered_tx, second_entered_rx) = mpsc::channel();
        let first_release = Arc::new((Mutex::new(false), Condvar::new()));
        let second_release = Arc::new((Mutex::new(false), Condvar::new()));
        let hook_first_release = Arc::clone(&first_release);
        let hook_second_release = Arc::clone(&second_release);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hook_calls = Arc::clone(&calls);
        let hook = Arc::new(move || {
            let (entered, release) = match hook_calls.fetch_add(1, Ordering::AcqRel) {
                0 => (&first_entered_tx, &hook_first_release),
                1 => (&second_entered_tx, &hook_second_release),
                _ => return,
            };
            entered.send(()).expect("signal watcher setup");
            let (lock, ready) = &**release;
            let mut released = lock.lock().expect("watch setup release");
            while !*released {
                released = ready.wait(released).expect("watch setup wait");
            }
        });
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, _scans) = controlled_launcher();
        let worker = spawn_with_watch_setup_hook(
            initial,
            Arc::clone(&index),
            Box::new(MemoryStore(store)),
            launcher,
            hook,
        );
        first_entered_rx
            .recv_timeout(WAIT)
            .expect("initial watcher setup entered");
        let mut replacement = config("/root-b");
        replacement.include_roots.push(PathBuf::from("/root-c"));
        worker.update_config(replacement);
        let (lock, ready) = &*first_release;
        *lock.lock().expect("first setup release") = true;
        ready.notify_all();
        second_entered_rx
            .recv_timeout(WAIT)
            .expect("replacement watcher setup entered");

        assert_eq!(index.read().expect("index").configured_root_count, 0);

        let (lock, ready) = &*second_release;
        *lock.lock().expect("second setup release") = true;
        ready.notify_all();
        let (barrier_tx, barrier_rx) = mpsc::channel();
        worker
            .commands
            .send(Command::Barrier(barrier_tx))
            .expect("queue barrier");
        barrier_rx.recv_timeout(WAIT).expect("worker barrier");
        assert_eq!(index.read().expect("index").configured_root_count, 2);
        worker.stop();
    }

    #[test]
    fn failed_scan_keeps_previous_generation_queryable() {
        let cfg = config("/root");
        let plan = ScanPlan::new(&cfg).expect("plan");
        let prior = owned("/root", "prior.txt");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                4,
                plan.identity().to_string(),
                vec![prior.clone()],
            )),
            commits: Vec::new(),
            next_generation: 4,
        }));
        let (worker, index, scans) = start_controlled(cfg, store);
        barrier(&worker.commands);
        worker.request_rebuild();
        scans
            .recv_timeout(WAIT)
            .expect("rebuild scan")
            .fail("scan failed");

        assert_eq!(snapshot_items(&index), vec![prior.item().path.clone()]);
        let state = index.read().expect("index");
        assert_eq!(state.status, SearchIndexStatus::Ready);
        assert_eq!(state.last_error.as_deref(), Some("scan failed"));
        drop(state);
        worker.stop();
    }

    #[test]
    fn non_membership_config_change_does_not_discard_active_scan() {
        let store = Arc::new(Mutex::new(MemoryStoreState::default()));
        let initial = config("/root");
        let (worker, index, scans) = start_controlled(initial.clone(), Arc::clone(&store));
        let scan = scans.recv_timeout(WAIT).expect("initial scan");

        let mut query_only = initial;
        query_only.result_limit = query_only.result_limit.saturating_add(1);
        worker.update_config(query_only);
        barrier(&worker.commands);
        assert!(!scan.cancelled.load(Ordering::Acquire));
        scan.complete(vec![owned("/root", "kept.txt")]);

        assert_eq!(store.lock().expect("store").commits.len(), 1);
        assert_eq!(
            snapshot_items(&index),
            vec![PathBuf::from("/root/kept.txt")]
        );
        worker.stop();
    }

    #[test]
    fn plan_change_cancels_old_scan_and_only_publishes_new_plan() {
        let store = Arc::new(Mutex::new(MemoryStoreState::default()));
        let (worker, index, scans) = start_controlled(config("/old"), Arc::clone(&store));
        let old = scans.recv_timeout(WAIT).expect("old scan");

        worker.update_config(config("/new"));
        let new = scans.recv_timeout(WAIT).expect("new scan");
        assert!(old.cancelled.load(Ordering::Acquire));
        assert_ne!(old.plan_identity, new.plan_identity);
        let new_identity = new.plan_identity.clone();
        old.complete(vec![owned("/old", "stale.txt")]);
        new.complete(vec![owned("/new", "fresh.txt")]);

        let commits = &store.lock().expect("store").commits;
        assert_eq!(commits, &[new_identity]);
        assert_eq!(
            snapshot_items(&index),
            vec![PathBuf::from("/new/fresh.txt")]
        );
        worker.stop();
    }

    #[test]
    fn successful_full_replace_removes_offline_deletion() {
        let cfg = config("/root");
        let plan = ScanPlan::new(&cfg).expect("plan");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                vec![owned("/root", "kept.txt"), owned("/root", "deleted.txt")],
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let (worker, index, scans) = start_controlled(cfg, store);
        barrier(&worker.commands);
        worker.request_rebuild();
        scans
            .recv_timeout(WAIT)
            .expect("full scan")
            .complete(vec![owned("/root", "kept.txt")]);

        assert_eq!(
            snapshot_items(&index),
            vec![PathBuf::from("/root/kept.txt")]
        );
        worker.stop();
    }

    #[test]
    fn same_count_replacement_advances_snapshot_revision() {
        let cfg = config("/root");
        let plan = ScanPlan::new(&cfg).expect("plan");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                vec![owned("/root", "old.txt")],
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let (worker, index, scans) = start_controlled(cfg, store);
        barrier(&worker.commands);
        let before = index.read().expect("index").snapshot_revision;

        worker.request_rebuild();
        scans
            .recv_timeout(WAIT)
            .expect("replacement scan")
            .complete(vec![owned("/root", "new.txt")]);

        let state = index.read().expect("index");
        assert_eq!(state.items[0].path, PathBuf::from("/root/new.txt"));
        assert_eq!(state.snapshot_revision, before.wrapping_add(1));
        drop(state);
        worker.stop();
    }

    #[test]
    fn readers_stay_on_old_complete_generation_until_store_commit_returns() {
        let cfg = config("/root");
        let plan = ScanPlan::new(&cfg).expect("plan");
        let state = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                vec![owned("/root", "old.txt")],
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let (entered_tx, entered_rx) = mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, scans) = controlled_launcher();
        let worker = spawn_with(
            cfg,
            Arc::clone(&index),
            Box::new(BlockingStore {
                inner: MemoryStore(Arc::clone(&state)),
                entered: entered_tx,
                release: Arc::clone(&release),
            }),
            launcher,
        );
        barrier(&worker.commands);
        worker.request_rebuild();
        let scan = scans.recv_timeout(WAIT).expect("scan");
        scan.commands
            .send(Command::ScanFinished {
                revision: scan.revision,
                result: Ok(ScanOutcome::Completed(vec![owned("/root", "new.txt")])),
            })
            .expect("finish scan");
        entered_rx.recv_timeout(WAIT).expect("commit entered");

        assert_eq!(snapshot_items(&index), vec![PathBuf::from("/root/old.txt")]);
        let (lock, ready) = &*release;
        *lock.lock().expect("release lock") = true;
        ready.notify_one();
        barrier(&scan.commands);
        assert_eq!(snapshot_items(&index), vec![PathBuf::from("/root/new.txt")]);
        worker.stop();
    }

    #[test]
    fn cancellation_during_commit_prevents_candidate_publication() {
        let cfg = config("/root");
        let plan = ScanPlan::new(&cfg).expect("plan");
        let state = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                vec![owned("/root", "old.txt")],
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let (entered_tx, entered_rx) = mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, scans) = controlled_launcher();
        let worker = spawn_with(
            cfg,
            Arc::clone(&index),
            Box::new(BlockingStore {
                inner: MemoryStore(Arc::clone(&state)),
                entered: entered_tx,
                release: Arc::clone(&release),
            }),
            launcher,
        );
        barrier(&worker.commands);
        worker.request_rebuild();
        let scan = scans.recv_timeout(WAIT).expect("scan");
        scan.commands
            .send(Command::ScanFinished {
                revision: scan.revision,
                result: Ok(ScanOutcome::Completed(vec![owned("/root", "obsolete.txt")])),
            })
            .expect("finish scan");
        entered_rx.recv_timeout(WAIT).expect("commit entered");

        worker.request_rebuild();
        let (lock, ready) = &*release;
        *lock.lock().expect("release lock") = true;
        ready.notify_one();
        barrier(&scan.commands);
        scans.recv_timeout(WAIT).expect("replacement scan");
        assert_eq!(snapshot_items(&index), vec![PathBuf::from("/root/old.txt")]);
        let durable = state.lock().expect("store state");
        assert!(durable.commits.is_empty());
        assert_eq!(durable.next_generation, 1);
        assert_eq!(
            durable
                .active
                .as_ref()
                .expect("previous active generation")
                .items()[0]
                .item()
                .path,
            PathBuf::from("/root/old.txt")
        );
        drop(durable);
        worker.stop();
    }

    #[test]
    fn notify_burst_commits_one_transactional_delta_generation() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-worker-delta-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(root.join("new.txt"), b"new").expect("write event path");
        let cfg = config(&root.to_string_lossy());
        let plan = ScanPlan::new(&cfg).expect("plan");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                vec![owned(&root.to_string_lossy(), "old.txt")],
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let (worker, index, scans) = start_controlled(cfg, Arc::clone(&store));
        barrier(&worker.commands);

        for _ in 0..3 {
            worker
                .commands
                .send(Command::FilesystemEvent(vec![root.join("new.txt")]))
                .expect("event");
        }
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline && store.lock().expect("store").commits.is_empty() {
            std::thread::sleep(Duration::from_millis(20));
        }

        assert_eq!(store.lock().expect("store").commits.len(), 1);
        assert_eq!(
            snapshot_items(&index),
            vec![root.join("new.txt"), root.join("old.txt")]
        );
        assert!(scans.try_recv().is_err(), "delta launched a full scan");
        worker.stop();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn filtered_rename_event_removes_previously_indexed_path() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-worker-filtered-rename-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&root).expect("create root");
        let old_path = root.join("visible.txt");
        let filtered_path = root.join("renamed.tmp");
        std::fs::write(&old_path, b"old").expect("write old");
        let mut cfg = config(&root.to_string_lossy());
        cfg.exclude_globs = vec!["*.tmp".to_string()];
        let plan = ScanPlan::new(&cfg).expect("plan");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                vec![owned(&root.to_string_lossy(), "visible.txt")],
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let (worker, index, _scans) = start_controlled(cfg, Arc::clone(&store));
        barrier(&worker.commands);
        std::fs::rename(&old_path, &filtered_path).expect("rename");
        worker
            .commands
            .send(Command::FilesystemEvent(vec![old_path, filtered_path]))
            .expect("rename event");
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline && store.lock().expect("store").commits.is_empty() {
            std::thread::sleep(Duration::from_millis(20));
        }

        assert!(snapshot_items(&index).is_empty());
        assert_eq!(store.lock().expect("store").commits.len(), 1);
        worker.stop();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn directory_notify_event_indexes_descendants_transactionally() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-worker-directory-event-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        let moved = root.join("moved");
        std::fs::create_dir_all(&moved).expect("create moved directory");
        std::fs::write(moved.join("child.txt"), b"child").expect("write child");
        let cfg = config(&root.to_string_lossy());
        let plan = ScanPlan::new(&cfg).expect("plan");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                Vec::new(),
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let (worker, index, _scans) = start_controlled(cfg, Arc::clone(&store));
        barrier(&worker.commands);
        worker
            .commands
            .send(Command::FilesystemEvent(vec![moved.clone()]))
            .expect("directory event");
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline && store.lock().expect("store").commits.is_empty() {
            std::thread::sleep(Duration::from_millis(20));
        }

        let paths = snapshot_items(&index);
        assert!(paths.contains(&moved));
        assert!(paths.contains(&moved.join("child.txt")));
        assert_eq!(store.lock().expect("store").commits.len(), 1);
        worker.stop();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn large_burst_uses_one_full_transactional_generation_commit() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-worker-large-burst-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&root).expect("create root");
        let cfg = config(&root.to_string_lossy());
        let plan = ScanPlan::new(&cfg).expect("plan");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                vec![owned(&root.to_string_lossy(), "old.txt")],
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let (worker, _index, scans) = start_controlled(cfg, Arc::clone(&store));
        barrier(&worker.commands);
        let paths = (0..=INCREMENTAL_REBUILD_THRESHOLD)
            .map(|index| root.join(format!("event-{index}")))
            .collect();
        worker
            .commands
            .send(Command::FilesystemEvent(paths))
            .expect("large burst");
        let scan = scans.recv_timeout(WAIT).expect("full scan");
        assert!(store.lock().expect("store").commits.is_empty());
        scan.complete(vec![owned(&root.to_string_lossy(), "fresh.txt")]);

        assert_eq!(store.lock().expect("store").commits.len(), 1);
        worker.stop();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_large_burst_rebuild_retains_dirty_trigger_until_success() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-worker-large-burst-retry-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&root).expect("create root");
        let cfg = config(&root.to_string_lossy());
        let plan = ScanPlan::new(&cfg).expect("plan");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                Vec::new(),
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let (worker, _index, scans) = start_controlled(cfg, Arc::clone(&store));
        barrier(&worker.commands);
        let paths = (0..=INCREMENTAL_REBUILD_THRESHOLD)
            .map(|index| root.join(format!("event-{index}")))
            .collect();
        worker
            .commands
            .send(Command::FilesystemEvent(paths))
            .expect("large burst");
        scans
            .recv_timeout(WAIT)
            .expect("first scan")
            .fail("scan failed");

        let retry = scans.recv_timeout(WAIT).expect("retry scan");
        retry.complete(vec![owned(&root.to_string_lossy(), "fresh.txt")]);
        assert_eq!(store.lock().expect("store").commits.len(), 1);
        worker.stop();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn notify_delta_is_durable_before_memory_publication() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-worker-order-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(root.join("new.txt"), b"new").expect("write new");
        let cfg = config(&root.to_string_lossy());
        let plan = ScanPlan::new(&cfg).expect("plan");
        let prior = owned(&root.to_string_lossy(), "old.txt");
        let state = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                vec![prior.clone()],
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let (entered_tx, entered_rx) = mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, _scans) = controlled_launcher();
        let worker = spawn_with(
            cfg,
            Arc::clone(&index),
            Box::new(BlockingStore {
                inner: MemoryStore(Arc::clone(&state)),
                entered: entered_tx,
                release: Arc::clone(&release),
            }),
            launcher,
        );
        barrier(&worker.commands);
        worker
            .commands
            .send(Command::FilesystemEvent(vec![root.join("new.txt")]))
            .expect("event");
        entered_rx.recv_timeout(WAIT).expect("delta commit entered");

        assert_eq!(snapshot_items(&index), vec![prior.item().path.clone()]);
        assert!(state.lock().expect("store").commits.is_empty());
        let (lock, ready) = &*release;
        *lock.lock().expect("release") = true;
        ready.notify_one();
        barrier(&worker.commands);
        assert_eq!(state.lock().expect("store").commits.len(), 1);
        assert_eq!(
            snapshot_items(&index),
            vec![root.join("new.txt"), root.join("old.txt")]
        );
        worker.stop();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recovered_root_commits_replace_root_before_queued_delta() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-worker-recover-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(root.join("scanned.txt"), b"scan").expect("write scan item");
        let cfg = config(&root.to_string_lossy());
        let plan = Arc::new(ScanPlan::new(&cfg).expect("plan"));
        let prior = owned(&root.to_string_lossy(), "prior.txt");
        let state = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                vec![prior],
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let kinds = Arc::new(Mutex::new(Vec::new()));
        let index = Arc::new(RwLock::new(IndexState::default()));
        publish_snapshot(
            &index,
            state
                .lock()
                .expect("state")
                .active
                .as_ref()
                .expect("active"),
        );
        let (commands, _receiver) = mpsc::channel();
        let mut registry =
            WatchRegistry::new(vec![root.clone()], WATCH_RETRY_INITIAL, WATCH_RETRY_MAXIMUM);
        registry.record_watch_success(&root);
        registry.mark_paths_uncertain(&[root.join("event")], "backend error");
        let mut worker = WorkerLoop {
            config: cfg,
            index,
            store: Box::new(OrderedStore {
                inner: MemoryStore(state),
                kinds: Arc::clone(&kinds),
            }),
            launcher: controlled_launcher().0,
            commands,
            fence: Arc::new(ActivationFence::new()),
            desired_plan_identity: Arc::new(Mutex::new(Some(plan.identity().to_string()))),
            shutdown: Arc::new(AtomicBool::new(false)),
            current_plan: Some(plan),
            active_scan: None,
            revision: 0,
            has_snapshot: true,
            last_full_rebuild_at: None,
            last_updatedb_at: None,
            locate_db_initialized: false,
            filesystem_rebuild_deadline: None,
            filesystem_event_revision: 0,
            pending_paths: BTreeSet::from([root.join("queued-delete.txt")]),
            watch_registry: Some(registry),
            watch_clock_started: Instant::now(),
            before_watch_setup: None,
        };

        assert!(worker.recover_root(&root));
        worker.apply_pending_delta();

        assert_eq!(
            kinds.lock().expect("kinds").as_slice(),
            &["replace-root", "delta"]
        );
        assert!(
            worker
                .watch_registry
                .as_ref()
                .expect("registry")
                .is_healthy(&root)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn every_obsoleting_command_advances_activation_epoch_before_enqueue() {
        let store = Arc::new(Mutex::new(MemoryStoreState::default()));
        let (worker, _index, _scans) = start_controlled(config("/root"), store);

        let before_config = worker.fence.capture();
        worker.update_config(config("/other"));
        assert_eq!(worker.fence.capture(), before_config.wrapping_add(1));

        let before_rebuild = worker.fence.capture();
        worker.request_rebuild();
        assert_eq!(worker.fence.capture(), before_rebuild.wrapping_add(1));

        let before_event = worker.fence.capture();
        worker.filesystem_event();
        assert_eq!(worker.fence.capture(), before_event);

        let fence = Arc::clone(&worker.fence);
        let before_stop = fence.capture();
        worker.stop();
        assert_eq!(fence.capture(), before_stop.wrapping_add(1));
    }

    #[test]
    fn replacement_waits_for_cancelled_scanner_thread_to_exit() {
        let store = Arc::new(Mutex::new(MemoryStoreState::default()));
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let releases = Arc::new(Mutex::new(Some(release_rx)));
        let launcher: ScanLauncher = Arc::new(move |plan, cancelled, revision, commands| {
            started_tx
                .send(plan.identity().to_string())
                .expect("started");
            let release = releases.lock().expect("release receiver").take()?;
            Some(std::thread::spawn(move || {
                release.recv().expect("release cancelled scanner");
                let _ = commands.send(Command::ScanFinished {
                    revision,
                    result: Ok(if cancelled.load(Ordering::Acquire) {
                        ScanOutcome::Cancelled
                    } else {
                        ScanOutcome::Completed(Vec::new())
                    }),
                });
            }))
        });
        let worker = spawn_with(
            config("/old"),
            index,
            Box::new(MemoryStore(store)),
            launcher,
        );
        started_rx.recv_timeout(WAIT).expect("old scan started");

        worker.update_config(config("/new"));
        assert!(
            started_rx.try_recv().is_err(),
            "new scan started before old scanner exited"
        );
        release_tx.send(()).expect("release old scanner");
        started_rx.recv_timeout(WAIT).expect("new scan started");
        worker.stop();
    }
}
