use crate::db::SearchStore;
use crate::mutation::{ActivationFence, GenerationSnapshot, IndexMutation, OwnedItem};
use crate::plan::ScanPlan;
use crate::scanner::{ScanOutcome, ScanProgress};
use crate::{IndexState, current_unix_secs};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use ottrin_core::{SearchConfig, SearchIndexStatus};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub(crate) trait StoreBackend: Send {
    fn load(&mut self, plan: &ScanPlan) -> Result<Option<GenerationSnapshot>, String>;
    fn replace_all_guarded(
        &mut self,
        plan: &ScanPlan,
        items: Vec<OwnedItem>,
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
    fn load(&mut self, plan: &ScanPlan) -> Result<Option<GenerationSnapshot>, String> {
        SearchStore::load(self, plan).map_err(|error| error.to_string())
    }

    fn replace_all_guarded(
        &mut self,
        plan: &ScanPlan,
        items: Vec<OwnedItem>,
        fence: &ActivationFence,
        expected_epoch: u64,
        publish: &mut dyn FnMut(&GenerationSnapshot),
    ) -> Result<(), StoreBackendError> {
        self.commit_guarded_then(
            plan,
            IndexMutation::ReplaceAll(items),
            fence,
            expected_epoch,
            publish,
        )
        .map(|_| ())
        .map_err(|error| match error {
            crate::db::SearchDbError::ActivationCancelled => StoreBackendError::ActivationCancelled,
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

pub(crate) enum Command {
    UpdateConfig(SearchConfig),
    Rebuild,
    FilesystemEvent,
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
    desired_plan_identity: Mutex<Option<String>>,
    join: Mutex<Option<JoinHandle<()>>>,
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
        self.invalidate_and_send(Command::Rebuild);
    }

    #[cfg(test)]
    pub(crate) fn filesystem_event(&self) {
        self.invalidate_and_send(Command::FilesystemEvent);
    }

    pub(crate) fn stop(&self) {
        self.invalidate_and_send(Command::Stop);
        if let Some(join) = self.join.lock().expect("worker join lock").take() {
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
    cancelled: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

struct WorkerLoop {
    config: SearchConfig,
    index: Arc<RwLock<IndexState>>,
    store: Box<dyn StoreBackend>,
    launcher: ScanLauncher,
    commands: mpsc::Sender<Command>,
    fence: Arc<ActivationFence>,
    current_plan: Option<Arc<ScanPlan>>,
    active_scan: Option<ActiveScan>,
    revision: u64,
    has_snapshot: bool,
    last_full_rebuild_at: Option<Instant>,
    last_updatedb_at: Option<Instant>,
    locate_db_initialized: bool,
    filesystem_rebuild_deadline: Option<Instant>,
}

pub(crate) fn spawn(
    config: SearchConfig,
    index: Arc<RwLock<IndexState>>,
    store: SearchStore,
) -> WorkerHandle {
    spawn_inner(config, index, Box::new(store), real_launcher(), true)
}

fn real_launcher() -> ScanLauncher {
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
    spawn_inner(config, index, store, launcher, false)
}

fn spawn_inner(
    config: SearchConfig,
    index: Arc<RwLock<IndexState>>,
    store: Box<dyn StoreBackend>,
    launcher: ScanLauncher,
    watch_filesystem: bool,
) -> WorkerHandle {
    let desired_plan_identity = ScanPlan::new(&config)
        .ok()
        .map(|plan| plan.identity().to_string());
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
            current_plan: None,
            active_scan: None,
            revision: 0,
            has_snapshot: false,
            last_full_rebuild_at: None,
            last_updatedb_at: None,
            locate_db_initialized: false,
            filesystem_rebuild_deadline: None,
        };
        worker.run(receiver, watch_filesystem);
    });
    WorkerHandle {
        commands,
        fence,
        desired_plan_identity: Mutex::new(desired_plan_identity),
        join: Mutex::new(Some(join)),
    }
}

impl WorkerLoop {
    fn run(&mut self, receiver: mpsc::Receiver<Command>, watch_filesystem: bool) {
        let mut watcher = watch_filesystem.then(|| self.make_watcher()).flatten();
        let mut watched_roots = Vec::new();
        self.install_plan(&mut watcher, &mut watched_roots);

        loop {
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(Command::UpdateConfig(config)) => {
                    let identity_changed = ScanPlan::new(&config)
                        .ok()
                        .map(|plan| plan.identity().to_string())
                        != self
                            .current_plan
                            .as_ref()
                            .map(|plan| plan.identity().to_string());
                    self.config = config;
                    if identity_changed {
                        self.install_plan(&mut watcher, &mut watched_roots);
                    }
                }
                Ok(Command::Rebuild) => self.start_scan(),
                Ok(Command::FilesystemEvent) => {
                    self.filesystem_rebuild_deadline =
                        Some(Instant::now() + Duration::from_millis(500));
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
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => self.run_schedules(),
            }
        }
    }

    fn make_watcher(&self) -> Option<RecommendedWatcher> {
        let commands = self.commands.clone();
        let fence = Arc::clone(&self.fence);
        RecommendedWatcher::new(
            move |result: notify::Result<notify::Event>| {
                if result.as_ref().is_ok_and(|event| {
                    matches!(
                        event.kind,
                        notify::EventKind::Create(_)
                            | notify::EventKind::Modify(_)
                            | notify::EventKind::Remove(_)
                    )
                }) {
                    fence.invalidate();
                    let _ = commands.send(Command::FilesystemEvent);
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
    ) {
        self.cancel_active();
        let plan = match ScanPlan::new(&self.config) {
            Ok(plan) => Arc::new(plan),
            Err(error) => {
                self.current_plan = None;
                let mut index = self.index.write().expect("index write");
                index.status = SearchIndexStatus::Unavailable;
                index.last_error = Some(error);
                index.progress_text = None;
                return;
            }
        };
        self.configure_watcher(watcher, watched_roots, &plan);
        self.current_plan = Some(Arc::clone(&plan));
        match self.store.load(&plan) {
            Ok(Some(snapshot)) => {
                self.publish(snapshot);
                self.last_full_rebuild_at = Some(Instant::now());
            }
            Ok(None) => self.start_scan(),
            Err(error) => {
                let mut index = self.index.write().expect("index write");
                index.last_error = Some(error);
                index.status = if self.has_snapshot {
                    SearchIndexStatus::Ready
                } else {
                    SearchIndexStatus::Indexing
                };
                drop(index);
                self.start_scan();
            }
        }
    }

    fn configure_watcher(
        &self,
        watcher: &mut Option<RecommendedWatcher>,
        watched_roots: &mut Vec<PathBuf>,
        plan: &ScanPlan,
    ) {
        let Some(watcher) = watcher.as_mut() else {
            return;
        };
        for root in watched_roots.drain(..) {
            let _ = watcher.unwatch(&root);
        }
        let mut failures = 0;
        for root in plan.roots() {
            if root.exists() && watcher.watch(root, RecursiveMode::Recursive).is_ok() {
                watched_roots.push(root.clone());
            } else if root.exists() {
                failures += 1;
            }
        }
        let mut index = self.index.write().expect("index write");
        index.configured_root_count = plan.roots().len();
        index.watched_root_count = watched_roots.len();
        index.watcher_ready = true;
        index.watcher_last_setup_unix_secs = Some(current_unix_secs());
        index.watcher_last_error =
            (failures > 0).then(|| format!("Failed to watch {failures} locations"));
    }

    fn start_scan(&mut self) {
        let Some(plan) = self.current_plan.as_ref().cloned() else {
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
            cancelled: Arc::clone(&cancelled),
            join: None,
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
        match result {
            Ok(ScanOutcome::Completed(items)) => {
                let Some(plan) = self.current_plan.as_ref() else {
                    return;
                };
                let expected_epoch = active.expected_epoch;
                let index = Arc::clone(&self.index);
                let mut publish = |snapshot: &GenerationSnapshot| {
                    publish_snapshot(&index, snapshot);
                };
                match self.store.replace_all_guarded(
                    plan,
                    items,
                    &self.fence,
                    expected_epoch,
                    &mut publish,
                ) {
                    Ok(()) => {
                        self.has_snapshot = true;
                        self.last_full_rebuild_at = Some(Instant::now());
                    }
                    Err(StoreBackendError::ActivationCancelled) => {}
                    Err(StoreBackendError::Other(error)) => self.record_failure(error),
                }
            }
            Ok(ScanOutcome::Cancelled) => {}
            Err(error) => self.record_failure(error),
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

    fn run_schedules(&mut self) {
        if self
            .filesystem_rebuild_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.filesystem_rebuild_deadline = None;
            self.start_scan();
        }
        let refresh_secs = u64::from(self.config.refresh_interval_hours) * 3_600;
        if refresh_secs > 0
            && self
                .last_full_rebuild_at
                .is_some_and(|last| last.elapsed().as_secs() >= refresh_secs)
            && self.active_scan.is_none()
        {
            self.start_scan();
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
    index.items = items;
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
        fn load(&mut self, plan: &ScanPlan) -> Result<Option<GenerationSnapshot>, String> {
            let state = self.0.lock().expect("memory store");
            Ok(state
                .active
                .as_ref()
                .filter(|snapshot| snapshot.plan_identity() == plan.identity())
                .cloned())
        }

        fn replace_all_guarded(
            &mut self,
            plan: &ScanPlan,
            items: Vec<OwnedItem>,
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
        fn load(&mut self, plan: &ScanPlan) -> Result<Option<GenerationSnapshot>, String> {
            self.inner.load(plan)
        }

        fn replace_all_guarded(
            &mut self,
            plan: &ScanPlan,
            items: Vec<OwnedItem>,
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
                .replace_all_guarded(plan, items, fence, expected_epoch, publish)
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
    fn filesystem_event_conservatively_requests_full_rebuild() {
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
        let (worker, _index, scans) = start_controlled(cfg, store);
        barrier(&worker.commands);

        worker.filesystem_event();
        worker.filesystem_event();
        worker.filesystem_event();
        let scan = scans.recv_timeout(WAIT).expect("full rebuild scan");
        assert_eq!(scan.plan_identity, plan.identity());
        barrier(&worker.commands);
        assert!(
            scans.try_recv().is_err(),
            "watcher burst launched duplicates"
        );
        worker.stop();
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
        assert_eq!(worker.fence.capture(), before_event.wrapping_add(1));

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
