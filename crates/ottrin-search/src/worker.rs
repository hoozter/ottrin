use crate::db::{SearchStore, SearchStoreDiagnostics};
use crate::mutation::{ActivationFence, GenerationSnapshot, IndexMutation};
use crate::plan::ScanPlan;
#[cfg(not(test))]
use crate::scanner::{BatchedScanOutcome, scan_batched};
use crate::scanner::{ScanOutcome, ScanProgress};
use crate::watch::WatchRegistry;
use crate::{IndexState, current_unix_secs};
use notify::event::{ModifyKind, RenameMode};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use ottrin_core::{SearchConfig, SearchIndexStatus};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use walkdir::WalkDir;

const EVENT_DEBOUNCE: Duration = Duration::from_millis(500);
const WATCH_RETRY_INITIAL: Duration = Duration::from_secs(60);
const WATCH_RETRY_MAXIMUM: Duration = Duration::from_secs(3_600);
const MAX_INCREMENTAL_PATHS_PER_BATCH: usize = 500;
const MAINTENANCE_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) type DeltaPublisher<'a> =
    dyn FnMut(&[crate::mutation::OwnedItem], &[PathBuf], usize) + 'a;

pub(crate) trait StoreBackend: Send {
    fn load(
        &mut self,
        plan: &ScanPlan,
        cancelled: &AtomicBool,
    ) -> Result<Option<GenerationSnapshot>, String>;

    #[cfg_attr(test, allow(dead_code))]
    fn load_count(
        &mut self,
        plan: &ScanPlan,
        cancelled: &AtomicBool,
    ) -> Result<Option<usize>, String> {
        self.load(plan, cancelled)
            .map(|snapshot| snapshot.map(|snapshot| snapshot.items().len()))
    }

    fn begin_staged_generation(&mut self, _plan: &ScanPlan) -> Result<Option<i64>, String> {
        Ok(None)
    }

    fn append_staged_generation(
        &mut self,
        _plan: &ScanPlan,
        _generation: i64,
        _items: &[crate::mutation::OwnedItem],
    ) -> Result<(), String> {
        Err("staged generation storage is unavailable".to_string())
    }

    fn activate_staged_generation(
        &mut self,
        _plan: &ScanPlan,
        _generation: i64,
        _expected_count: usize,
        _fence: &ActivationFence,
        _expected_epoch: u64,
        _publish: &mut dyn FnMut(usize),
    ) -> Result<(), StoreBackendError> {
        Err(StoreBackendError::Other(
            "staged generation storage is unavailable".to_string(),
        ))
    }

    fn discard_staged_generation(&mut self, _generation: i64) -> Result<(), String> {
        Ok(())
    }

    fn commit_guarded(
        &mut self,
        plan: &ScanPlan,
        mutation: IndexMutation,
        fence: &ActivationFence,
        expected_epoch: u64,
        publish: &mut dyn FnMut(&GenerationSnapshot),
    ) -> Result<(), StoreBackendError>;

    fn commit_delta_guarded(
        &mut self,
        plan: &ScanPlan,
        upserts: Vec<crate::mutation::OwnedItem>,
        remove_trees: Vec<PathBuf>,
        fence: &ActivationFence,
        expected_epoch: u64,
        publish: &mut DeltaPublisher<'_>,
    ) -> Result<(), StoreBackendError> {
        let published_upserts = upserts.clone();
        let published_remove_trees = remove_trees.clone();
        let mut publish_snapshot = |snapshot: &GenerationSnapshot| {
            publish(
                &published_upserts,
                &published_remove_trees,
                snapshot.items().len(),
            );
        };
        self.commit_guarded(
            plan,
            IndexMutation::Delta {
                upserts,
                remove_trees,
            },
            fence,
            expected_epoch,
            &mut publish_snapshot,
        )
    }

    fn diagnostics(&mut self) -> Option<SearchStoreDiagnostics> {
        None
    }
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

    fn load_count(
        &mut self,
        plan: &ScanPlan,
        cancelled: &AtomicBool,
    ) -> Result<Option<usize>, String> {
        self.load_count_cancellable(plan, cancelled)
            .map_err(|error| error.to_string())
    }

    #[cfg(not(test))]
    fn begin_staged_generation(&mut self, plan: &ScanPlan) -> Result<Option<i64>, String> {
        SearchStore::begin_staged_generation(self, plan)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    #[cfg(test)]
    fn begin_staged_generation(&mut self, _plan: &ScanPlan) -> Result<Option<i64>, String> {
        // Worker tests intentionally retain test-only snapshots for publication assertions.
        Ok(None)
    }

    fn append_staged_generation(
        &mut self,
        plan: &ScanPlan,
        generation: i64,
        items: &[crate::mutation::OwnedItem],
    ) -> Result<(), String> {
        SearchStore::append_staged_generation(self, plan, generation, items)
            .map_err(|error| error.to_string())
    }

    fn activate_staged_generation(
        &mut self,
        plan: &ScanPlan,
        generation: i64,
        expected_count: usize,
        fence: &ActivationFence,
        expected_epoch: u64,
        publish: &mut dyn FnMut(usize),
    ) -> Result<(), StoreBackendError> {
        SearchStore::activate_staged_generation_then(
            self,
            plan,
            generation,
            expected_count,
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

    fn discard_staged_generation(&mut self, generation: i64) -> Result<(), String> {
        SearchStore::discard_staged_generation(self, generation).map_err(|error| error.to_string())
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

    fn commit_delta_guarded(
        &mut self,
        plan: &ScanPlan,
        upserts: Vec<crate::mutation::OwnedItem>,
        remove_trees: Vec<PathBuf>,
        fence: &ActivationFence,
        expected_epoch: u64,
        publish: &mut DeltaPublisher<'_>,
    ) -> Result<(), StoreBackendError> {
        self.commit_delta_guarded_then(
            plan,
            &upserts,
            &remove_trees,
            fence,
            expected_epoch,
            |count| publish(&upserts, &remove_trees, count),
        )
        .map_err(|error| match error {
            crate::db::SearchDbError::ActivationCancelled => StoreBackendError::ActivationCancelled,
            other => StoreBackendError::Other(other.to_string()),
        })
    }

    fn diagnostics(&mut self) -> Option<SearchStoreDiagnostics> {
        SearchStore::diagnostics(self).ok()
    }
}

pub(crate) type ScanLauncher = Arc<
    dyn Fn(Arc<ScanPlan>, Arc<AtomicBool>, u64, mpsc::Sender<Command>) -> Option<JoinHandle<()>>
        + Send
        + Sync
        + 'static,
>;

type WatchSetupHook = Arc<dyn Fn() + Send + Sync + 'static>;
type MemoryReclaimer = Arc<dyn Fn() + Send + Sync + 'static>;

pub(crate) enum Command {
    UpdateConfig(SearchConfig),
    Rebuild(Option<mpsc::Sender<Result<(), String>>>),
    Reconcile(mpsc::Sender<Result<(), String>>),
    FilesystemEvent {
        kind: EventKind,
        paths: Vec<PathBuf>,
    },
    WatchBackendError {
        paths: Vec<PathBuf>,
        error: String,
    },
    ScanProgress {
        revision: u64,
        progress: ScanProgress,
    },
    #[cfg_attr(test, allow(dead_code))]
    ScanBatch {
        revision: u64,
        items: Vec<crate::mutation::OwnedItem>,
        persisted: mpsc::SyncSender<Result<(), String>>,
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
        self.begin_rebuild()?
            .recv()
            .map_err(|_| "Search worker stopped before rebuild completed".to_string())?
    }

    pub(crate) fn begin_rebuild(&self) -> Result<mpsc::Receiver<Result<(), String>>, String> {
        let (done, completed) = mpsc::channel();
        self.fence.invalidate();
        self.commands
            .send(Command::Rebuild(Some(done)))
            .map_err(|_| "Search worker stopped before rebuild was requested".to_string())?;
        Ok(completed)
    }

    pub(crate) fn reconcile_blocking(&self) -> Result<(), String> {
        self.begin_reconcile()?
            .recv()
            .map_err(|_| "Search worker stopped before update completed".to_string())?
    }

    pub(crate) fn begin_reconcile(&self) -> Result<mpsc::Receiver<Result<(), String>>, String> {
        let (done, completed) = mpsc::channel();
        self.fence.invalidate();
        self.commands
            .send(Command::Reconcile(done))
            .map_err(|_| "Search worker stopped before update was requested".to_string())?;
        Ok(completed)
    }

    #[cfg(test)]
    pub(crate) fn filesystem_event(&self) {
        let _ = self.commands.send(Command::FilesystemEvent {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![PathBuf::from("/root/event")],
        });
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
    mode: ScanMode,
    staged_generation: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScanMode {
    Rebuild,
    Reconcile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PendingChange {
    Entry,
    Subtree,
}

impl PendingChange {
    fn for_event(kind: &EventKind) -> Self {
        match kind {
            EventKind::Modify(kind) if !matches!(kind, ModifyKind::Name(_)) => Self::Entry,
            _ => Self::Subtree,
        }
    }
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
    pending_paths: BTreeMap<PathBuf, PendingChange>,
    watch_registry: Option<WatchRegistry>,
    watch_clock_started: Instant,
    before_watch_setup: Option<WatchSetupHook>,
    memory_reclaimer: MemoryReclaimer,
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
        Arc::new(release_full_replacement_memory),
    )
}

fn release_full_replacement_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `malloc_trim` is a process-wide glibc allocator maintenance call.
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(not(test))]
pub(crate) fn real_launcher() -> ScanLauncher {
    Arc::new(|plan, cancelled, revision, commands| {
        Some(std::thread::spawn(move || {
            let progress_commands = commands.clone();
            let result = scan_batched(
                &plan,
                &cancelled,
                2_048,
                |items| send_scan_batch(&commands, revision, items, &cancelled),
                move |progress| {
                    let _ = progress_commands.send(Command::ScanProgress { revision, progress });
                },
            )
            .map(|outcome| match outcome {
                BatchedScanOutcome::Completed { indexed_items } => {
                    ScanOutcome::Staged { indexed_items }
                }
                BatchedScanOutcome::Cancelled => ScanOutcome::Cancelled,
            });
            let _ = commands.send(Command::ScanFinished { revision, result });
        }))
    })
}

fn send_scan_batch(
    commands: &mpsc::Sender<Command>,
    revision: u64,
    items: Vec<crate::mutation::OwnedItem>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let (ack, persisted) = mpsc::sync_channel(0);
    commands
        .send(Command::ScanBatch {
            revision,
            items,
            persisted: ack,
        })
        .map_err(|_| "Search worker stopped during staged indexing".to_string())?;
    loop {
        match persisted.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) if cancelled.load(Ordering::Acquire) => {
                return Err("staged indexing was cancelled".to_string());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Search worker stopped during staged indexing".to_string());
            }
        }
    }
}

#[cfg(test)]
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
    spawn_inner(config, index, store, launcher, false, None, Arc::new(|| {}))
}

#[cfg(test)]
fn spawn_with_reclaimer(
    config: SearchConfig,
    index: Arc<RwLock<IndexState>>,
    store: Box<dyn StoreBackend>,
    launcher: ScanLauncher,
    memory_reclaimer: MemoryReclaimer,
) -> WorkerHandle {
    spawn_inner(
        config,
        index,
        store,
        launcher,
        false,
        None,
        memory_reclaimer,
    )
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
        Arc::new(|| {}),
    )
}

fn spawn_inner(
    config: SearchConfig,
    index: Arc<RwLock<IndexState>>,
    store: Box<dyn StoreBackend>,
    launcher: ScanLauncher,
    watch_filesystem: bool,
    before_watch_setup: Option<WatchSetupHook>,
    memory_reclaimer: MemoryReclaimer,
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
            pending_paths: BTreeMap::new(),
            watch_registry: None,
            watch_clock_started: Instant::now(),
            before_watch_setup,
            memory_reclaimer,
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

fn watcher_config() -> notify::Config {
    // ScanPlan treats directory symlinks as leaf entries. Recursive watching
    // must use the same membership rule or Wine's dosdevices/z: link escapes
    // a home-root watch into the entire filesystem.
    notify::Config::default().with_follow_symlinks(false)
}

fn walk_plan_watch_paths(
    plan: &ScanPlan,
    configured_root: &Path,
    start: &Path,
    cancelled: &AtomicBool,
    mut visit: impl FnMut(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let walker = WalkDir::new(start)
        .follow_links(plan.follow_links())
        .same_file_system(plan.same_file_system())
        .into_iter()
        .filter_entry(|entry| {
            plan.includes_path_for_root(entry.path(), configured_root)
                && (!entry.file_type().is_symlink() || entry.depth() == 0)
        });
    for entry in walker {
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error)
                if (error.depth() > 0 || start != configured_root)
                    && matches!(
                        error.io_error().map(std::io::Error::kind),
                        Some(std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound)
                    ) =>
            {
                continue;
            }
            Err(error) => return Err(error.to_string()),
        };
        if (entry.depth() == 0 || entry.file_type().is_dir())
            && let Err(error) = visit(entry.path())
        {
            // A descendant may disappear between enumeration and inotify watch
            // registration. Its parent still reports the removal; this is not
            // lost coverage of the whole configured root.
            if entry.path() == configured_root || !watch_path_unavailable(entry.path()) {
                return Err(error);
            }
        }
    }
    Ok(())
}

fn watch_new_directory_subtrees(
    plan: &ScanPlan,
    kind: &EventKind,
    paths: &[PathBuf],
    cancelled: &AtomicBool,
    mut visit: impl FnMut(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let candidates: &[PathBuf] = match kind {
        EventKind::Create(_) => paths,
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if paths.len() >= 2 => {
            &paths[paths.len() - 1..]
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To | RenameMode::Any)) => paths,
        _ => return Ok(()),
    };
    for path in candidates {
        let Some(root) = plan.root_for(path) else {
            continue;
        };
        if plan.included_event_path_for_root(path, root).as_deref() != Some(path.as_path()) {
            continue;
        }
        // Do not use metadata(): directory symlinks are indexed as leaves, not watched.
        if !std::fs::symlink_metadata(path)
            .map(|metadata| metadata.file_type().is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        walk_plan_watch_paths(plan, root, path, cancelled, &mut visit)?;
    }
    Ok(())
}

fn included_watch_event_paths(plan: &ScanPlan, paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter_map(|path| {
            let root = plan.root_for(&path)?;
            plan.included_event_path_for_root(&path, root)
        })
        .collect()
}

fn watch_path_unavailable(path: &Path) -> bool {
    matches!(
        std::fs::read_dir(path),
        Err(error) if matches!(
            error.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
        )
    )
}

fn watch_error_requires_recovery(paths: &[PathBuf]) -> bool {
    // Pathless backend failures (including queue overflow) still require recovery.
    paths.is_empty() || paths.iter().any(|path| !watch_path_unavailable(path))
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

        let mut next_maintenance = Instant::now() + MAINTENANCE_INTERVAL;
        loop {
            let command = pending_commands.pop_front().map(Ok).unwrap_or_else(|| {
                receiver.recv_timeout(next_maintenance.saturating_duration_since(Instant::now()))
            });
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
                Ok(Command::Reconcile(completion)) => {
                    self.start_scan_with_mode(Some(completion), ScanMode::Reconcile)
                }
                Ok(Command::FilesystemEvent { kind, paths }) => {
                    let paths = self
                        .current_plan
                        .as_ref()
                        .map(|plan| included_watch_event_paths(plan, paths))
                        .unwrap_or_default();
                    if !paths.is_empty() {
                        if let (Some(plan), Some(watcher)) =
                            (self.current_plan.as_ref(), watcher.as_mut())
                        {
                            let watch_result = watch_new_directory_subtrees(
                                plan,
                                &kind,
                                &paths,
                                &self.shutdown,
                                |directory| {
                                    watcher
                                        .watch(directory, RecursiveMode::NonRecursive)
                                        .map_err(|error| error.to_string())
                                },
                            );
                            if let (Err(error), Some(registry)) =
                                (watch_result, self.watch_registry.as_mut())
                            {
                                registry.mark_paths_uncertain(&paths, &error);
                            }
                        }
                        self.filesystem_event_revision =
                            self.filesystem_event_revision.wrapping_add(1);
                        for path in paths {
                            let change = if self.current_plan.as_ref().is_some_and(|plan| {
                                plan.root_for(&path)
                                    .is_some_and(|root| !plan.includes_path_for_root(&path, root))
                            }) {
                                // Generated-output boundaries remove previously indexed trees.
                                PendingChange::Subtree
                            } else {
                                PendingChange::for_event(&kind)
                            };
                            self.pending_paths
                                .entry(path)
                                .and_modify(|pending| *pending = (*pending).max(change))
                                .or_insert(change);
                        }
                        // Coalesce a burst without postponing it indefinitely when
                        // another application writes continuously.
                        self.filesystem_rebuild_deadline
                            .get_or_insert_with(|| Instant::now() + EVENT_DEBOUNCE);
                    }
                }
                Ok(Command::WatchBackendError { paths, error }) => {
                    if let (true, Some(registry)) = (
                        watch_error_requires_recovery(&paths),
                        self.watch_registry.as_mut(),
                    ) {
                        registry.mark_paths_uncertain(&paths, &error);
                    }
                    self.update_watcher_diagnostics();
                }
                Ok(Command::ScanProgress { revision, progress }) => {
                    self.record_progress(revision, progress);
                }
                Ok(Command::ScanBatch {
                    revision,
                    items,
                    persisted,
                }) => {
                    let result = match (
                        self.active_scan
                            .as_ref()
                            .filter(|active| {
                                active.revision == revision
                                    && !active.cancelled.load(Ordering::Acquire)
                            })
                            .and_then(|active| active.staged_generation),
                        self.current_plan.as_ref(),
                    ) {
                        (Some(generation), Some(plan)) => self
                            .store
                            .append_staged_generation(plan, generation, &items),
                        _ => Err("staged scan batch is no longer current".to_string()),
                    };
                    let _ = persisted.send(result);
                }
                Ok(Command::ScanFinished { revision, result }) => {
                    if self.finish_scan(revision, result) {
                        (self.memory_reclaimer)();
                    }
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
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            // Maintenance must also run while filesystem events keep arriving.
            if Instant::now() >= next_maintenance {
                self.run_schedules(&mut watcher, &mut watched_roots);
                next_maintenance = Instant::now() + MAINTENANCE_INTERVAL;
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
                    let _ = commands.send(Command::FilesystemEvent {
                        kind: event.kind,
                        paths: event.paths,
                    });
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
            watcher_config(),
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
        {
            let mut index = self.index.write().expect("index write");
            index.status = SearchIndexStatus::Loading;
            index.loading_saved_snapshot = true;
            index.progress_text = Some("Loading saved search index…".to_string());
        }
        enum LoadedGeneration {
            #[cfg(test)]
            Snapshot(GenerationSnapshot),
            #[cfg(not(test))]
            Count(usize),
        }
        #[cfg(test)]
        let load_result = self
            .store
            .load(&plan, &self.shutdown)
            .map(|loaded| loaded.map(LoadedGeneration::Snapshot));
        #[cfg(not(test))]
        let load_result = self
            .store
            .load_count(&plan, &self.shutdown)
            .map(|loaded| loaded.map(LoadedGeneration::Count));
        self.index
            .write()
            .expect("index write")
            .loading_saved_snapshot = false;
        self.update_storage_diagnostics();
        let mut load_error = None;
        let loaded_snapshot = match load_result {
            #[cfg(test)]
            Ok(Some(LoadedGeneration::Snapshot(snapshot))) => {
                self.publish(snapshot);
                self.last_full_rebuild_at = Some(Instant::now());
                true
            }
            #[cfg(not(test))]
            Ok(Some(LoadedGeneration::Count(count))) => {
                publish_ready_count(&self.index, count);
                self.has_snapshot = true;
                self.last_full_rebuild_at = Some(Instant::now());
                true
            }
            Ok(None) => {
                let mut index = self.index.write().expect("index write");
                index.status = SearchIndexStatus::Indexing;
                index.progress_text = Some("Indexing · preparing filesystem watcher…".to_string());
                index.active_root = None;
                index.active_root_index = 0;
                index.total_roots = plan.roots().len();
                false
            }
            Err(error) => {
                let mut index = self.index.write().expect("index write");
                index.status = SearchIndexStatus::Indexing;
                index.progress_text = Some("Indexing · preparing filesystem watcher…".to_string());
                index.active_root = None;
                index.active_root_index = 0;
                index.total_roots = plan.roots().len();
                load_error = Some(error);
                false
            }
        };
        if !self.configure_watcher(watcher, watched_roots, &plan, receiver, pending_commands) {
            return false;
        }
        if !loaded_snapshot {
            if let Some(error) = load_error {
                let mut index = self.index.write().expect("index write");
                index.last_error = Some(error);
                index.status = if self.has_snapshot {
                    SearchIndexStatus::Ready
                } else {
                    SearchIndexStatus::Indexing
                };
            }
            self.start_scan(None);
        }
        true
    }

    fn configure_watcher(
        &mut self,
        watcher: &mut Option<RecommendedWatcher>,
        watched_roots: &mut Vec<PathBuf>,
        plan: &Arc<ScanPlan>,
        receiver: &mpsc::Receiver<Command>,
        pending_commands: &mut VecDeque<Command>,
    ) -> bool {
        let mut setup_watcher = watcher.take();
        let mut setup_watched_roots = std::mem::take(watched_roots);
        let roots = plan.roots().to_vec();
        let setup_plan = Arc::clone(plan);
        let watch_clock_started = self.watch_clock_started;
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
                    registry.record_watch_failure(
                        root,
                        watch_clock_started.elapsed(),
                        "configured root unavailable",
                    );
                    unavailable_roots.push(root.clone());
                    continue;
                }
                match setup_watcher.as_mut() {
                    Some(watcher) => match walk_plan_watch_paths(
                        &setup_plan,
                        root,
                        root,
                        &setup_cancelled,
                        |path| {
                            watcher
                                .watch(path, RecursiveMode::NonRecursive)
                                .map_err(|error| error.to_string())
                        },
                    ) {
                        Ok(()) => {
                            registry.record_watch_success(root);
                            setup_watched_roots.push(root.clone());
                        }
                        Err(error) => registry.record_watch_failure(
                            root,
                            watch_clock_started.elapsed(),
                            &error,
                        ),
                    },
                    None => registry.record_watch_failure(
                        root,
                        watch_clock_started.elapsed(),
                        "watch backend unavailable",
                    ),
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
            self.pending_paths.insert(root, PendingChange::Subtree);
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

    fn update_storage_diagnostics(&mut self) {
        let Some(storage) = self.store.diagnostics() else {
            return;
        };
        let mut index = self.index.write().expect("index write");
        index.database_bytes = storage.database_bytes;
        index.wal_bytes = storage.wal_bytes;
        index.freelist_pages = storage.freelist_pages;
        index.freelist_bytes = storage.freelist_bytes;
    }

    fn start_scan(&mut self, completion: Option<mpsc::Sender<Result<(), String>>>) {
        self.start_scan_with_mode(completion, ScanMode::Rebuild);
    }

    fn start_scan_with_mode(
        &mut self,
        completion: Option<mpsc::Sender<Result<(), String>>>,
        mode: ScanMode,
    ) {
        let Some(plan) = self.current_plan.as_ref().cloned() else {
            if let Some(completion) = completion {
                let _ = completion.send(Err("Search plan is unavailable".to_string()));
            }
            return;
        };
        self.cancel_active();
        let staged_generation = match self.store.begin_staged_generation(&plan) {
            Ok(generation) => generation,
            Err(error) => {
                if let Some(completion) = completion {
                    let _ = completion.send(Err(error));
                }
                return;
            }
        };
        let mode = if staged_generation.is_some() {
            ScanMode::Rebuild
        } else {
            mode
        };
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
            mode,
            staged_generation,
        });
        {
            let mut index = self.index.write().expect("index write");
            index.loading_saved_snapshot = false;
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
            if let Some(generation) = active.staged_generation {
                let _ = self.store.discard_staged_generation(generation);
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

    fn finish_scan(&mut self, revision: u64, result: Result<ScanOutcome, String>) -> bool {
        let Some(active) = self.active_scan.as_ref() else {
            return false;
        };
        if active.revision != revision || active.cancelled.load(Ordering::Acquire) {
            return false;
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
                self.discard_incomplete_stage(active.staged_generation);
                let Some(plan) = self.current_plan.as_ref() else {
                    complete(Err(
                        "Search plan became unavailable before commit".to_string()
                    ));
                    return false;
                };
                let expected_epoch = active.expected_epoch;
                let index = Arc::clone(&self.index);
                let mut publish = |snapshot: &GenerationSnapshot| {
                    publish_snapshot(&index, snapshot);
                };
                let mutation = match active.mode {
                    ScanMode::Rebuild => IndexMutation::ReplaceAll(items),
                    ScanMode::Reconcile if !self.has_snapshot => IndexMutation::ReplaceAll(items),
                    ScanMode::Reconcile => IndexMutation::Delta {
                        upserts: items,
                        remove_trees: plan.roots().to_vec(),
                    },
                };
                match self.store.commit_guarded(
                    plan,
                    mutation,
                    &self.fence,
                    expected_epoch,
                    &mut publish,
                ) {
                    Ok(()) => {
                        self.has_snapshot = true;
                        if matches!(active.mode, ScanMode::Rebuild) {
                            self.last_full_rebuild_at = Some(Instant::now());
                        }
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
                };
                false
            }
            Ok(ScanOutcome::Staged { indexed_items }) => {
                let Some(plan) = self.current_plan.as_ref() else {
                    self.discard_incomplete_stage(active.staged_generation);
                    complete(Err(
                        "Search plan became unavailable before staged activation".to_string(),
                    ));
                    return false;
                };
                let Some(generation) = active.staged_generation else {
                    complete(Err("staged scan did not reserve a generation".to_string()));
                    return false;
                };
                let index = Arc::clone(&self.index);
                let mut publish = |count| publish_ready_count(&index, count);
                match self.store.activate_staged_generation(
                    plan,
                    generation,
                    indexed_items,
                    &self.fence,
                    active.expected_epoch,
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
                        true
                    }
                    Err(StoreBackendError::ActivationCancelled) => {
                        self.discard_incomplete_stage(Some(generation));
                        complete(Err("Requested rebuild was superseded".to_string()));
                        false
                    }
                    Err(StoreBackendError::Other(error)) => {
                        self.discard_incomplete_stage(Some(generation));
                        complete(Err(error.clone()));
                        self.record_failure(error);
                        false
                    }
                }
            }
            Ok(ScanOutcome::Cancelled) => {
                self.discard_incomplete_stage(active.staged_generation);
                complete(Err("Requested rebuild was cancelled".to_string()));
                if !self.pending_paths.is_empty() {
                    self.filesystem_rebuild_deadline = Some(Instant::now() + EVENT_DEBOUNCE);
                }
                false
            }
            Err(error) => {
                self.discard_incomplete_stage(active.staged_generation);
                complete(Err(error.clone()));
                self.record_failure(error);
                if !self.pending_paths.is_empty() {
                    self.filesystem_rebuild_deadline = Some(Instant::now() + EVENT_DEBOUNCE);
                }
                false
            }
        }
    }

    #[cfg(test)]
    fn publish(&mut self, snapshot: GenerationSnapshot) {
        publish_snapshot(&self.index, &snapshot);
        self.has_snapshot = true;
    }

    fn discard_incomplete_stage(&mut self, generation: Option<i64>) {
        let Some(generation) = generation else {
            return;
        };
        if let Err(error) = self.store.discard_staged_generation(generation) {
            self.record_failure(format!(
                "Failed to discard incomplete staged generation {generation}: {error}"
            ));
        }
    }

    fn record_failure(&mut self, error: String) {
        let mut index = self.index.write().expect("index write");
        index.loading_saved_snapshot = false;
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
        index.loading_saved_snapshot = false;
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
        let result = match mutation {
            IndexMutation::Delta {
                upserts,
                remove_trees,
            } => {
                let mut publish = |upserts: &[crate::mutation::OwnedItem],
                                   remove_trees: &[PathBuf],
                                   count: usize| {
                    publish_delta(&index, upserts, remove_trees, count);
                };
                self.store.commit_delta_guarded(
                    plan,
                    upserts,
                    remove_trees,
                    &self.fence,
                    expected_epoch,
                    &mut publish,
                )
            }
            mutation => {
                let mut publish =
                    |snapshot: &GenerationSnapshot| publish_snapshot(&index, snapshot);
                self.store
                    .commit_guarded(plan, mutation, &self.fence, expected_epoch, &mut publish)
            }
        };
        match result {
            Ok(()) => {
                self.has_snapshot = true;
                self.update_storage_diagnostics();
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
        let Some(plan) = self.current_plan.as_ref() else {
            return;
        };
        let expected_epoch = self.fence.capture();
        // A busy directory must not turn a change burst into a full-root rebuild.
        // Keep each transaction bounded and leave the remainder queued.
        let paths = self
            .pending_paths
            .iter()
            .take(MAX_INCREMENTAL_PATHS_PER_BATCH)
            .map(|(path, change)| (path.clone(), *change))
            .collect::<Vec<_>>();
        let mut upserts = Vec::new();
        let mut remove_trees = Vec::new();
        let mut classified = BTreeSet::new();
        for (path, change) in paths {
            if plan.root_for(&path).is_none() {
                classified.insert(path);
                continue;
            }
            let candidates = match change {
                PendingChange::Subtree => crate::scanner::candidates_for_path(plan, &path),
                PendingChange::Entry => crate::scanner::candidate_for_path(plan, &path)
                    .map(|item| item.into_iter().collect()),
            };
            match candidates {
                Ok(items) => {
                    // Metadata changes update only this entry. Replacing its tree
                    // would rescan and rewrite every descendant of a busy directory.
                    if change == PendingChange::Subtree || items.is_empty() {
                        remove_trees.push(path.clone());
                    }
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
        }
        if !self.pending_paths.is_empty() {
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
            Ok(ScanOutcome::Staged { .. }) | Ok(ScanOutcome::Cancelled) => false,
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
            self.pending_paths.insert(root, PendingChange::Subtree);
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
            let result = match (watcher.as_mut(), self.current_plan.as_ref()) {
                (Some(watcher), Some(plan)) => {
                    walk_plan_watch_paths(plan, &root, &root, &self.shutdown, |path| {
                        watcher
                            .watch(path, RecursiveMode::NonRecursive)
                            .map_err(|error| error.to_string())
                    })
                }
                _ => Err("watch backend unavailable".to_string()),
            };
            if let Some(registry) = self.watch_registry.as_mut() {
                match result {
                    Ok(()) => {
                        registry.record_retry_watch_success(&root);
                        if !watched_roots.contains(&root) {
                            watched_roots.push(root.clone());
                        }
                    }
                    Err(error) => registry.record_watch_failure(
                        &root,
                        self.watch_clock_started.elapsed(),
                        &error,
                    ),
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

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn publish_ready_count(index: &RwLock<IndexState>, count: usize) {
    let mut index = index.write().expect("index write");
    index.items = Arc::new(Vec::new());
    index.snapshot_revision = index.snapshot_revision.wrapping_add(1);
    index.loading_saved_snapshot = false;
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
pub(crate) fn compact_query_item(
    mut item: ottrin_core::SearchResultItem,
) -> ottrin_core::SearchResultItem {
    item.parent_path = PathBuf::new();
    item.name_lower = String::new();
    item.path_str = String::new();
    item.path_lower = String::new();
    item
}

pub(crate) fn publish_delta(
    index: &RwLock<IndexState>,
    upserts: &[crate::mutation::OwnedItem],
    remove_trees: &[PathBuf],
    authoritative_count: usize,
) {
    let mut index = index.write().expect("index write");
    #[cfg(not(test))]
    {
        let _ = (upserts, remove_trees);
        index.items = Arc::new(Vec::new());
    }
    #[cfg(test)]
    {
        let items = Arc::make_mut(&mut index.items);
        let mut removals = remove_trees.to_vec();
        removals.sort();
        removals.dedup();
        for removed in removals.into_iter().rev() {
            let start = items.partition_point(|item| item.path < removed);
            let removed_len =
                items[start..].partition_point(|item| item.path.starts_with(&removed));
            items.drain(start..start + removed_len);
        }
        let normalized = upserts
            .iter()
            .map(|owned| {
                (
                    owned.item().path.clone(),
                    compact_query_item(owned.item().clone()),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        for (path, item) in normalized {
            match items.binary_search_by(|candidate| candidate.path.cmp(&path)) {
                Ok(position) => items[position] = item,
                Err(position) => items.insert(position, item),
            }
        }
        debug_assert_eq!(items.len(), authoritative_count);
    }
    let count = authoritative_count;
    index.snapshot_revision = index.snapshot_revision.wrapping_add(1);
    index.loading_saved_snapshot = false;
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

pub(crate) fn publish_snapshot(index: &RwLock<IndexState>, snapshot: &GenerationSnapshot) {
    let count = snapshot.items().len();
    let mut index = index.write().expect("index write");
    #[cfg(not(test))]
    {
        index.items = Arc::new(Vec::new());
    }
    #[cfg(test)]
    {
        index.items = Arc::new(
            snapshot
                .items()
                .iter()
                .map(|owned| compact_query_item(owned.item().clone()))
                .collect(),
        );
    }
    index.snapshot_revision = index.snapshot_revision.wrapping_add(1);
    index.loading_saved_snapshot = false;
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

    #[test]
    fn watcher_membership_does_not_follow_directory_symlinks() {
        assert!(!watcher_config().follow_symlinks());
    }

    #[cfg(unix)]
    #[test]
    fn event_watch_discovery_only_walks_new_real_directory_subtrees() {
        use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "ottrin-event-watch-discovery-{}",
            std::process::id()
        ));
        let existing = root.join("existing");
        let nested = existing.join("nested");
        let moved = root.join("moved");
        let linked = root.join("linked");
        std::fs::create_dir_all(&nested).expect("create nested directory");
        symlink(&existing, &linked).expect("create directory symlink");
        let plan = ScanPlan::new(&config(root.to_str().expect("utf8 root"))).expect("plan");
        let cancelled = AtomicBool::new(false);
        let discover = |kind, paths: Vec<PathBuf>| {
            let mut visited = Vec::new();
            watch_new_directory_subtrees(&plan, &kind, &paths, &cancelled, |path| {
                visited.push(path.to_path_buf());
                Ok(())
            })
            .expect("discover watches");
            visited
        };

        // Directory metadata/content updates and removals must not walk an old tree.
        assert!(
            discover(
                notify::EventKind::Modify(ModifyKind::Any),
                vec![existing.clone()]
            )
            .is_empty()
        );
        assert!(
            discover(
                notify::EventKind::Remove(RemoveKind::Any),
                vec![existing.clone()]
            )
            .is_empty()
        );

        // An already populated newly created tree needs watches on every descendant.
        assert_eq!(
            discover(
                notify::EventKind::Create(CreateKind::Folder),
                vec![existing.clone()]
            ),
            vec![existing.clone(), nested.clone()]
        );

        std::fs::rename(&existing, &moved).expect("rename populated tree");
        assert_eq!(
            discover(
                notify::EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                vec![existing, moved.clone()],
            ),
            vec![moved.clone(), moved.join("nested")]
        );

        assert!(
            discover(
                notify::EventKind::Create(CreateKind::Any),
                vec![linked.clone()]
            )
            .is_empty()
        );
        assert!(
            discover(
                notify::EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                vec![linked],
            )
            .is_empty(),
            "directory symlinks are leaf entries"
        );
        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn delta_publication_removes_only_the_component_subtree() {
        let root = "/tmp/root";
        let snapshot = GenerationSnapshot::new(
            1,
            "plan".to_string(),
            vec![
                owned(root, "foo"),
                owned(root, "foo/child"),
                owned(root, "foobar"),
            ],
        );
        let index = RwLock::new(IndexState::default());
        publish_snapshot(&index, &snapshot);
        index
            .write()
            .expect("index write")
            .last_scan_completed_unix_secs = None;

        publish_delta(&index, &[], &[PathBuf::from(root).join("foo")], 1);

        let index = index.read().expect("index read");
        assert!(index.last_scan_completed_unix_secs.is_some());
        assert!(index.items.iter().all(|item| {
            item.parent_path.as_os_str().is_empty()
                && item.name_lower.is_empty()
                && item.path_str.is_empty()
                && item.path_lower.is_empty()
        }));
        assert_eq!(
            index
                .items
                .iter()
                .map(|item| item.path.clone())
                .collect::<Vec<_>>(),
            vec![PathBuf::from(root).join("foobar")]
        );
    }

    #[test]
    fn watch_directory_walk_matches_scan_membership() {
        let root =
            std::env::temp_dir().join(format!("ottrin-watch-membership-{}", std::process::id()));
        let normal = root.join("Documents").join("nested");
        let hidden = root.join(".cache").join("churn");
        let generated = root.join("Projects").join("Ottrin").join("target");
        let ordinary_out = root.join("out").join("notes");
        let ninja_out = root
            .join("workspace")
            .join("chromium")
            .join("src")
            .join("out")
            .join("MinusWayland");
        std::fs::create_dir_all(&normal).expect("create normal tree");
        std::fs::create_dir_all(&hidden).expect("create hidden tree");
        std::fs::create_dir_all(&generated).expect("create generated tree");
        std::fs::create_dir_all(&ordinary_out).expect("create ordinary out tree");
        std::fs::create_dir_all(ninja_out.join("obj")).expect("create Ninja output tree");
        std::fs::write(normal.join("kept.txt"), b"kept").expect("write normal fixture");
        std::fs::write(ordinary_out.join("kept.txt"), b"kept").expect("write ordinary out fixture");
        std::fs::write(ninja_out.join("build.ninja"), b"generated").expect("write Ninja marker");
        std::fs::write(ninja_out.join("args.gn"), b"is_debug = true").expect("write GN marker");
        std::fs::write(ninja_out.join("obj").join("artifact.o"), b"generated")
            .expect("write generated fixture");
        let mut search_config = config(root.to_string_lossy().as_ref());
        search_config.include_hidden_system = false;
        let plan = ScanPlan::new(&search_config).expect("plan");
        let cancelled = AtomicBool::new(false);
        let mut watched = Vec::new();

        walk_plan_watch_paths(&plan, &root, &root, &cancelled, |path| {
            watched.push(path.to_path_buf());
            Ok(())
        })
        .expect("walk watch paths");
        let ScanOutcome::Completed(scanned) =
            crate::scanner::scan(&plan, &cancelled, |_| {}).expect("scan fixture")
        else {
            panic!("scan unexpectedly cancelled");
        };
        let scanned = scanned
            .into_iter()
            .map(|item| item.item().path.clone())
            .collect::<BTreeSet<_>>();

        assert!(watched.contains(&root));
        assert!(watched.contains(&root.join("Documents")));
        assert!(watched.contains(&normal));
        assert!(watched.contains(&root.join("out")));
        assert!(watched.contains(&ordinary_out));
        assert!(!watched.contains(&root.join(".cache")));
        assert!(!watched.contains(&hidden));
        assert!(!watched.contains(&generated));
        assert!(!watched.contains(&ninja_out));
        assert!(!watched.contains(&ninja_out.join("obj")));
        assert!(scanned.contains(&ordinary_out.join("kept.txt")));
        assert!(!scanned.contains(&ninja_out));
        assert!(!scanned.contains(&ninja_out.join("build.ninja")));
        assert!(!scanned.contains(&ninja_out.join("obj").join("artifact.o")));
        assert_eq!(
            included_watch_event_paths(
                &plan,
                vec![
                    normal.join("new.txt"),
                    hidden.join("cache.tmp"),
                    generated.join("artifact.o"),
                    ordinary_out.join("new.txt"),
                    ninja_out.join("obj").join("nested-event.o"),
                ],
            ),
            vec![
                normal.join("new.txt"),
                ordinary_out.join("new.txt"),
                ninja_out,
            ]
        );
        std::fs::remove_dir_all(root).expect("remove watch fixture");
    }

    #[test]
    fn watch_retry_backoff_starts_after_attempt_completion() {
        let root = PathBuf::from("root");
        let attempt_completed_at = Duration::from_secs(300);
        let mut registry =
            WatchRegistry::new(vec![root.clone()], WATCH_RETRY_INITIAL, WATCH_RETRY_MAXIMUM);
        registry.record_watch_failure(&root, attempt_completed_at, "backend refused");

        assert!(
            registry
                .due_retries(attempt_completed_at + WATCH_RETRY_INITIAL - Duration::from_millis(1))
                .is_empty()
        );
        assert_eq!(
            registry.due_retries(attempt_completed_at + WATCH_RETRY_INITIAL),
            vec![root]
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_watch_descendant_does_not_invalidate_the_accessible_index() {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!(
            "ottrin-unreadable-watch-path-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create unreadable directory");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
            .expect("remove directory permissions");

        assert!(!watch_error_requires_recovery(std::slice::from_ref(&path)));

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("restore directory permissions");
        std::fs::remove_dir(path).expect("remove unreadable directory");
    }

    struct WatchTestRoot(PathBuf);

    impl Drop for WatchTestRoot {
        fn drop(&mut self) {
            if let Err(error) = std::fs::remove_dir_all(&self.0) {
                eprintln!("watch test fixture cleanup failed: {error}");
                assert!(
                    std::thread::panicking(),
                    "watch test fixture cleanup failed"
                );
            }
        }
    }

    #[test]
    fn disappearing_watch_descendant_does_not_trigger_root_recovery() {
        let root =
            std::env::temp_dir().join(format!("ottrin-disappearing-watch-{}", std::process::id()));
        let child = root.join("short-lived");
        std::fs::create_dir_all(&child).expect("create descendant");
        let _cleanup = WatchTestRoot(root.clone());
        let plan = ScanPlan::new(&config(root.to_str().expect("utf8 root"))).expect("plan");
        let cancelled = AtomicBool::new(false);
        walk_plan_watch_paths(&plan, &root, &root, &cancelled, |path| {
            if path == child {
                std::fs::remove_dir(&child).expect("remove between discovery and registration");
                return Err("No path was found".to_string());
            }
            Ok(())
        })
        .expect("disappearing descendant must not invalidate root coverage");
        assert!(!watch_error_requires_recovery(std::slice::from_ref(&child)));
        assert!(watch_error_requires_recovery(std::slice::from_ref(&root)));
        assert!(watch_error_requires_recovery(&[]));
        // A queued creation may already have been removed before discovery starts.
        walk_plan_watch_paths(&plan, &root, &child, &cancelled, |_| {
            panic!("removed path must not be watched")
        })
        .expect("removed discovery start is harmless");
    }

    #[test]
    fn watch_registration_failure_on_existing_directory_is_preserved() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-existing-watch-failure-{}",
            std::process::id()
        ));
        let child = root.join("child");
        std::fs::create_dir_all(&child).expect("create fixture");
        let _cleanup = WatchTestRoot(root.clone());
        let plan = ScanPlan::new(&config(root.to_str().expect("utf8 root"))).expect("plan");
        let cancelled = AtomicBool::new(false);
        assert_eq!(
            walk_plan_watch_paths(&plan, &root, &root, &cancelled, |path| {
                if path == child {
                    Err("watch limit reached".to_string())
                } else {
                    Ok(())
                }
            }),
            Err("watch limit reached".to_string())
        );
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

    #[test]
    fn scanner_waits_for_batch_persistence_before_finishing() {
        let (commands, receiver) = mpsc::channel();
        let (finished, completion) = mpsc::channel();
        let cancelled = AtomicBool::new(false);
        let producer = std::thread::spawn(move || {
            send_scan_batch(&commands, 7, vec![owned("/root", "item.txt")], &cancelled)
                .expect("worker persists batch");
            finished.send(()).expect("report scanner completion");
        });

        let Command::ScanBatch {
            revision,
            items,
            persisted,
        } = receiver.recv().expect("scan batch")
        else {
            panic!("expected scan batch");
        };
        assert_eq!(revision, 7);
        assert_eq!(items.len(), 1);
        assert!(matches!(
            completion.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));

        persisted.send(Ok(())).expect("acknowledge persisted batch");

        completion.recv_timeout(WAIT).expect("scanner finishes");
        producer.join().expect("producer joins");
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

    struct FailingStagedStore {
        discarded: Arc<Mutex<Vec<i64>>>,
    }

    struct SuccessfulStagedStore {
        lifecycle: Arc<Mutex<Vec<&'static str>>>,
    }

    impl StoreBackend for SuccessfulStagedStore {
        fn load(
            &mut self,
            _plan: &ScanPlan,
            _cancelled: &AtomicBool,
        ) -> Result<Option<GenerationSnapshot>, String> {
            Ok(None)
        }

        fn begin_staged_generation(&mut self, _plan: &ScanPlan) -> Result<Option<i64>, String> {
            Ok(Some(7))
        }

        fn append_staged_generation(
            &mut self,
            _plan: &ScanPlan,
            _generation: i64,
            _items: &[OwnedItem],
        ) -> Result<(), String> {
            Ok(())
        }

        fn activate_staged_generation(
            &mut self,
            _plan: &ScanPlan,
            _generation: i64,
            _expected_count: usize,
            _fence: &ActivationFence,
            _expected_epoch: u64,
            publish: &mut dyn FnMut(usize),
        ) -> Result<(), StoreBackendError> {
            self.lifecycle.lock().expect("lifecycle").push("activate");
            publish(1);
            Ok(())
        }

        fn commit_guarded(
            &mut self,
            _plan: &ScanPlan,
            _mutation: IndexMutation,
            _fence: &ActivationFence,
            _expected_epoch: u64,
            _publish: &mut dyn FnMut(&GenerationSnapshot),
        ) -> Result<(), StoreBackendError> {
            panic!("staged scan must not use legacy commit")
        }
    }

    impl StoreBackend for FailingStagedStore {
        fn load(
            &mut self,
            _plan: &ScanPlan,
            _cancelled: &AtomicBool,
        ) -> Result<Option<GenerationSnapshot>, String> {
            Ok(None)
        }

        fn begin_staged_generation(&mut self, _plan: &ScanPlan) -> Result<Option<i64>, String> {
            Ok(Some(7))
        }

        fn append_staged_generation(
            &mut self,
            _plan: &ScanPlan,
            _generation: i64,
            _items: &[OwnedItem],
        ) -> Result<(), String> {
            Ok(())
        }

        fn activate_staged_generation(
            &mut self,
            _plan: &ScanPlan,
            _generation: i64,
            _expected_count: usize,
            _fence: &ActivationFence,
            _expected_epoch: u64,
            _publish: &mut dyn FnMut(usize),
        ) -> Result<(), StoreBackendError> {
            Err(StoreBackendError::Other(
                "injected activation failure".to_string(),
            ))
        }

        fn discard_staged_generation(&mut self, generation: i64) -> Result<(), String> {
            self.discarded
                .lock()
                .expect("discarded generations")
                .push(generation);
            Ok(())
        }

        fn commit_guarded(
            &mut self,
            _plan: &ScanPlan,
            _mutation: IndexMutation,
            _fence: &ActivationFence,
            _expected_epoch: u64,
            _publish: &mut dyn FnMut(&GenerationSnapshot),
        ) -> Result<(), StoreBackendError> {
            panic!("staged scan must not use legacy commit")
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

        fn staged(self, indexed_items: usize) {
            self.commands
                .send(Command::ScanFinished {
                    revision: self.revision,
                    result: Ok(ScanOutcome::Staged { indexed_items }),
                })
                .expect("send staged scan");
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
    fn successful_staged_replacement_releases_allocator_after_activation() {
        let lifecycle = Arc::new(Mutex::new(Vec::new()));
        let reclaimed_lifecycle = Arc::clone(&lifecycle);
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, scans) = controlled_launcher();
        let worker = spawn_with_reclaimer(
            config("/root"),
            index,
            Box::new(SuccessfulStagedStore {
                lifecycle: Arc::clone(&lifecycle),
            }),
            launcher,
            Arc::new(move || {
                reclaimed_lifecycle
                    .lock()
                    .expect("lifecycle")
                    .push("reclaim");
            }),
        );
        let startup = scans.recv_timeout(WAIT).expect("startup staged scan");

        startup.staged(1);

        assert_eq!(
            lifecycle.lock().expect("lifecycle").as_slice(),
            &["activate", "reclaim"]
        );
        worker.stop();
    }

    #[test]
    fn completed_non_staged_scan_does_not_release_allocator() {
        let reclaim_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed_count = Arc::clone(&reclaim_count);
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, scans) = controlled_launcher();
        let worker = spawn_with_reclaimer(
            config("/root"),
            index,
            Box::new(MemoryStore(Arc::new(Mutex::new(
                MemoryStoreState::default(),
            )))),
            launcher,
            Arc::new(move || {
                observed_count.fetch_add(1, Ordering::AcqRel);
            }),
        );
        let startup = scans.recv_timeout(WAIT).expect("startup scan");

        startup.complete(vec![owned("/root", "item.txt")]);

        assert_eq!(reclaim_count.load(Ordering::Acquire), 0);
        worker.stop();
    }

    #[test]
    fn failed_staged_activation_discards_without_releasing_allocator() {
        let discarded = Arc::new(Mutex::new(Vec::new()));
        let reclaim_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed_count = Arc::clone(&reclaim_count);
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, scans) = controlled_launcher();
        let worker = spawn_with_reclaimer(
            config("/root"),
            index,
            Box::new(FailingStagedStore {
                discarded: Arc::clone(&discarded),
            }),
            launcher,
            Arc::new(move || {
                observed_count.fetch_add(1, Ordering::AcqRel);
            }),
        );
        let startup = scans.recv_timeout(WAIT).expect("startup staged scan");

        startup.staged(1);

        assert_eq!(
            discarded.lock().expect("discarded generations").as_slice(),
            &[7]
        );
        assert_eq!(reclaim_count.load(Ordering::Acquire), 0);
        worker.stop();
    }

    #[test]
    fn failed_staged_scan_discards_the_incomplete_generation() {
        let discarded = Arc::new(Mutex::new(Vec::new()));
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, scans) = controlled_launcher();
        let worker = spawn_with(
            config("/root"),
            index,
            Box::new(FailingStagedStore {
                discarded: Arc::clone(&discarded),
            }),
            launcher,
        );
        let startup = scans.recv_timeout(WAIT).expect("startup staged scan");

        startup.fail("injected scanner failure");

        assert_eq!(
            discarded.lock().expect("discarded generations").as_slice(),
            &[7]
        );
        worker.stop();
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
    fn fresh_blocking_reconcile_publishes_an_initial_replace_all_generation() {
        let state = Arc::new(Mutex::new(MemoryStoreState::default()));
        let kinds = Arc::new(Mutex::new(Vec::new()));
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, scans) = controlled_launcher();
        let worker = spawn_with(
            config("/root"),
            Arc::clone(&index),
            Box::new(OrderedStore {
                inner: MemoryStore(state),
                kinds: Arc::clone(&kinds),
            }),
            launcher,
        );
        let startup = scans.recv_timeout(WAIT).expect("startup scan");
        let worker = Arc::new(worker);
        let waiting = Arc::clone(&worker);
        let completed = std::thread::spawn(move || waiting.reconcile_blocking());

        let requested = scans.recv_timeout(WAIT).expect("requested reconcile");
        assert!(startup.cancelled.load(Ordering::Acquire));
        requested.complete(vec![owned("/root", "initial.txt")]);

        assert_eq!(completed.join().expect("join reconcile waiter"), Ok(()));
        assert_eq!(
            kinds.lock().expect("mutation kinds").as_slice(),
            ["replace-all"]
        );
        assert_eq!(
            snapshot_items(&index),
            vec![PathBuf::from("/root/initial.txt")]
        );
        Arc::try_unwrap(worker)
            .unwrap_or_else(|_| panic!("worker references remain"))
            .stop();
    }

    #[test]
    fn blocking_reconcile_completes_after_an_incremental_root_replacement_commits() {
        let cfg = config("/root");
        let plan = ScanPlan::new(&cfg).expect("plan");
        let outside = owned("/outside", "kept.txt");
        let stale = owned("/root", "stale.txt");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                4,
                plan.identity().to_string(),
                vec![outside.clone(), stale],
            )),
            commits: Vec::new(),
            next_generation: 4,
        }));
        let (worker, index, scans) = start_controlled(cfg, store);
        barrier(&worker.commands);
        let worker = Arc::new(worker);
        let waiting = Arc::clone(&worker);
        let completed = std::thread::spawn(move || waiting.reconcile_blocking());

        let requested = scans.recv_timeout(WAIT).expect("requested reconcile");
        assert!(!completed.is_finished());
        requested.complete(vec![owned("/root", "fresh.txt")]);

        assert_eq!(completed.join().expect("join reconcile waiter"), Ok(()));
        assert_eq!(
            snapshot_items(&index),
            vec![
                PathBuf::from("/outside/kept.txt"),
                PathBuf::from("/root/fresh.txt")
            ]
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
        let (launcher, scans) = controlled_launcher();
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
        assert!(
            scans.try_recv().is_err(),
            "a matching cached snapshot must not start a filesystem scan"
        );
    }

    #[test]
    fn stop_completes_while_generation_load_is_blocked() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, scans) = controlled_launcher();
        let worker = spawn_with(
            config("/root"),
            Arc::clone(&index),
            Box::new(BlockingLoadStore {
                entered: entered_tx,
            }),
            launcher,
        );
        entered_rx.recv_timeout(WAIT).expect("load entered");
        assert_eq!(
            index.read().expect("index").status,
            SearchIndexStatus::Loading,
            "SQLite snapshot deserialization is loading, not a filesystem scan"
        );
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

    struct TestWorker(Option<WorkerHandle>);

    impl std::ops::Deref for TestWorker {
        type Target = WorkerHandle;
        fn deref(&self) -> &WorkerHandle {
            self.0.as_ref().expect("test worker")
        }
    }

    impl Drop for TestWorker {
        fn drop(&mut self) {
            if let Some(worker) = self.0.take() {
                worker.stop();
            }
        }
    }

    #[test]
    fn directory_metadata_changes_preserve_children_without_scanning_them() {
        let root =
            std::env::temp_dir().join(format!("ottrin-directory-metadata-{}", std::process::id()));
        let directory = root.join("busy");
        std::fs::create_dir_all(&directory).expect("create fixture");
        let _cleanup = WatchTestRoot(root.clone());
        std::fs::write(directory.join("known.txt"), "known").expect("known child");
        let cfg = config(&root.to_string_lossy());
        let plan = ScanPlan::new(&cfg).expect("plan");
        let initial = crate::scanner::candidates_for_path(&plan, &directory).expect("initial tree");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                initial,
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let (worker, index, scans) = start_controlled(cfg, Arc::clone(&store));
        let worker = TestWorker(Some(worker));
        barrier(&worker.commands);
        let unseen = directory.join("unseen.txt");
        std::fs::write(&unseen, "unseen").expect("unreported child");
        worker
            .commands
            .send(Command::FilesystemEvent {
                kind: EventKind::Modify(ModifyKind::Metadata(notify::event::MetadataKind::Any)),
                paths: vec![directory.clone()],
            })
            .expect("metadata event");
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline && store.lock().expect("store").commits.is_empty() {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(store.lock().expect("store").commits.len(), 1);
        assert_eq!(
            snapshot_items(&index),
            vec![directory.clone(), directory.join("known.txt")]
        );
        // Creation/move discovery must still include populated descendants, and
        // a later metadata event must not downgrade that queued subtree work.
        worker
            .commands
            .send(Command::FilesystemEvent {
                kind: EventKind::Create(notify::event::CreateKind::Folder),
                paths: vec![directory.clone()],
            })
            .expect("subtree event");
        worker
            .commands
            .send(Command::FilesystemEvent {
                kind: EventKind::Modify(ModifyKind::Any),
                paths: vec![directory.clone()],
            })
            .expect("coalesced metadata event");
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline && store.lock().expect("store").commits.len() < 2 {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(snapshot_items(&index).contains(&unseen));
        assert!(
            scans.try_recv().is_err(),
            "event must not launch a full scan"
        );
        drop(worker);
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
                .send(Command::FilesystemEvent {
                    kind: EventKind::Create(notify::event::CreateKind::File),
                    paths: vec![root.join("new.txt")],
                })
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
    fn large_notify_burst_drains_without_a_full_scan_or_lost_changes() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-worker-large-burst-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&root).expect("create root");
        let count = MAX_INCREMENTAL_PATHS_PER_BATCH * 2 + 1;
        let paths = (0..count)
            .map(|number| root.join(format!("new-{number:04}.txt")))
            .collect::<Vec<_>>();
        for path in &paths {
            std::fs::write(path, b"new").expect("write event path");
        }
        let cfg = config(&root.to_string_lossy());
        let plan = ScanPlan::new(&cfg).expect("plan");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                vec![
                    owned(&root.to_string_lossy(), "unchanged.txt"),
                    owned(&root.to_string_lossy(), "deleted.txt"),
                ],
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let (worker, index, scans) = start_controlled(cfg, Arc::clone(&store));
        barrier(&worker.commands);
        let mut changed = paths.clone();
        changed.push(root.join("deleted.txt"));
        worker
            .commands
            .send(Command::FilesystemEvent {
                kind: EventKind::Modify(ModifyKind::Any),
                paths: changed,
            })
            .expect("burst");

        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline && snapshot_items(&index).len() != count + 1 {
            std::thread::sleep(Duration::from_millis(20));
        }
        let items = snapshot_items(&index);
        let transactions = store.lock().expect("store").commits.len();
        let full_scan = scans.try_recv().is_ok();
        worker.stop();
        std::fs::remove_dir_all(&root).expect("remove fixture");

        assert!(!full_scan, "ordinary changes launched a full-root scan");
        assert_eq!(transactions, 3, "burst must drain in bounded batches");
        assert_eq!(items.len(), count + 1);
        assert!(items.contains(&root.join("unchanged.txt")));
        assert!(!items.contains(&root.join("deleted.txt")));
        assert!(paths.iter().all(|path| items.contains(path)));
    }

    #[test]
    fn continuous_events_do_not_starve_incremental_updates() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-worker-continuous-events-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("busy.txt");
        std::fs::write(&path, b"busy").expect("write event path");
        let cfg = config(&root.to_string_lossy());
        let plan = ScanPlan::new(&cfg).expect("plan");
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                vec![],
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let (worker, index, scans) = start_controlled(cfg, Arc::clone(&store));
        barrier(&worker.commands);
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline && snapshot_items(&index).is_empty() {
            worker
                .commands
                .send(Command::FilesystemEvent {
                    kind: EventKind::Modify(ModifyKind::Any),
                    paths: vec![path.clone()],
                })
                .expect("continuous event");
            std::thread::sleep(Duration::from_millis(2));
        }
        let items = snapshot_items(&index);
        let full_scan = scans.try_recv().is_ok();
        worker.stop();
        std::fs::remove_dir_all(root).expect("remove fixture");
        assert_eq!(items, vec![path], "updates waited for an idle event queue");
        assert!(!full_scan);
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
            .send(Command::FilesystemEvent {
                kind: EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                paths: vec![old_path, filtered_path],
            })
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
            .send(Command::FilesystemEvent {
                kind: EventKind::Create(notify::event::CreateKind::Folder),
                paths: vec![moved.clone()],
            })
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

    struct FailOnceStore {
        inner: MemoryStore,
        fail_next: bool,
    }

    impl StoreBackend for FailOnceStore {
        fn load(
            &mut self,
            plan: &ScanPlan,
            cancelled: &AtomicBool,
        ) -> Result<Option<GenerationSnapshot>, String> {
            self.inner.load(plan, cancelled)
        }

        fn commit_guarded(
            &mut self,
            plan: &ScanPlan,
            mutation: IndexMutation,
            fence: &ActivationFence,
            expected_epoch: u64,
            publish: &mut dyn FnMut(&GenerationSnapshot),
        ) -> Result<(), StoreBackendError> {
            if std::mem::take(&mut self.fail_next) {
                return Err(StoreBackendError::Other(
                    "injected delta commit failure".to_string(),
                ));
            }
            self.inner
                .commit_guarded(plan, mutation, fence, expected_epoch, publish)
        }
    }

    #[test]
    fn failed_large_burst_delta_keeps_changes_queued_until_commit_succeeds() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-worker-large-burst-retry-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&root).expect("create root");
        let cfg = config(&root.to_string_lossy());
        let plan = ScanPlan::new(&cfg).expect("plan");
        let count = MAX_INCREMENTAL_PATHS_PER_BATCH + 1;
        let paths = (0..count)
            .map(|number| root.join(format!("event-{number:04}.txt")))
            .collect::<Vec<_>>();
        for path in &paths {
            std::fs::write(path, b"new").expect("write event file");
        }
        let store = Arc::new(Mutex::new(MemoryStoreState {
            active: Some(GenerationSnapshot::new(
                1,
                plan.identity().to_string(),
                Vec::new(),
            )),
            commits: Vec::new(),
            next_generation: 1,
        }));
        let index = Arc::new(RwLock::new(IndexState::default()));
        let (launcher, scans) = controlled_launcher();
        let worker = spawn_with(
            cfg,
            Arc::clone(&index),
            Box::new(FailOnceStore {
                inner: MemoryStore(Arc::clone(&store)),
                fail_next: true,
            }),
            launcher,
        );
        barrier(&worker.commands);
        worker
            .commands
            .send(Command::FilesystemEvent {
                kind: EventKind::Create(notify::event::CreateKind::File),
                paths: paths.clone(),
            })
            .expect("large burst");
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline && snapshot_items(&index).len() != count {
            std::thread::sleep(Duration::from_millis(20));
        }
        let items = snapshot_items(&index);
        let commits = store.lock().expect("store").commits.len();
        let full_scan = scans.try_recv().is_ok();
        worker.stop();
        std::fs::remove_dir_all(root).expect("remove fixture");
        assert_eq!(items, paths);
        assert_eq!(commits, 2);
        assert!(!full_scan);
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
            .send(Command::FilesystemEvent {
                kind: EventKind::Create(notify::event::CreateKind::File),
                paths: vec![root.join("new.txt")],
            })
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
            pending_paths: BTreeMap::from([(
                root.join("queued-delete.txt"),
                PendingChange::Subtree,
            )]),
            watch_registry: Some(registry),
            watch_clock_started: Instant::now(),
            before_watch_setup: None,
            memory_reclaimer: Arc::new(|| {}),
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
