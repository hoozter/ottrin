use fs2::FileExt;
use globset::{Glob, GlobSet, GlobSetBuilder};
use ottrin_core::{
    EntryKind, SearchConfig, SearchIndexStatus, SearchQuery, SearchResponse, SearchResultItem,
    SearchScope, SearchSort,
};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::Mutex;
#[cfg(any(unix, target_os = "windows"))]
use std::sync::mpsc;
use std::sync::{Arc, RwLock, atomic::AtomicBool};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use walkdir::WalkDir;

#[cfg(unix)]
mod daemon;
#[cfg(any(unix, target_os = "windows"))]
mod daemon_client;
#[cfg(any(unix, target_os = "windows"))]
mod daemon_protocol;
#[cfg(any(unix, target_os = "windows"))]
mod daemon_server;
mod db;
#[allow(dead_code)] // Slice 2 mutation API is intentionally isolated until worker integration.
mod mutation;
#[cfg(target_os = "windows")]
mod path_identity;
mod plan;
mod scanner;
mod watch;
#[cfg(target_os = "windows")]
mod windows_identity;
mod windows_installer;
mod worker;
#[cfg(target_os = "windows")]
pub use windows_identity::IndexDaemonIdentity;
pub use windows_installer::IndexInstallerPhase;
#[cfg(target_os = "windows")]
pub use windows_installer::run as run_windows_index_installer_transaction;
#[cfg(target_os = "windows")]
mod windows_pipe;
#[cfg(target_os = "windows")]
pub use windows_pipe::{WindowsIndexPipeListener, WindowsIndexPipeStream};

use db::{
    SearchStore, SimpleTermPage, active_generation_contains_paths, generation_content_db_paths,
    query_indexed_content_page, query_simple_term_page, search_db_path,
    stream_generation_content_items, stream_generation_items,
};

#[cfg(test)]
static SEARCH_SERVICE_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
fn test_temp_dir() -> PathBuf {
    thread_local! {
        static ROOT: tempfile::TempDir = tempfile::Builder::new()
            .prefix("ottrin-test-")
            .tempdir()
            .expect("private search test directory");
    }
    // Windows TEMP can use an 8.3 alias. Hand-built indexed rows and scan plans
    // must use the same filesystem spelling when testing path ownership. The
    // test thread owns the directory and removes remaining stores on exit.
    ROOT.with(|root| std::fs::canonicalize(root.path()).expect("resolve test temporary directory"))
}

#[cfg(test)]
fn default_search_store_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    crate::test_temp_dir().join(format!(
        "ottrin-search-test-{}-{unique}.db",
        std::process::id()
    ))
}

#[cfg(not(test))]
fn default_search_store_path() -> PathBuf {
    search_db_path()
}

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("search parser error: {0}")]
    Parse(String),
}

#[derive(Debug)]
struct IndexState {
    items: Arc<Vec<SearchResultItem>>,
    snapshot_revision: u64,
    status: SearchIndexStatus,
    loading_saved_snapshot: bool,
    database_bytes: u64,
    wal_bytes: u64,
    freelist_pages: u64,
    freelist_bytes: u64,
    last_error: Option<String>,
    progress_text: Option<String>,
    last_completed_indexed_items: Option<usize>,
    estimated_total_items: Option<usize>,
    last_indexed_path: Option<PathBuf>,
    building_indexed_items: usize,
    configured_root_count: usize,
    watched_root_count: usize,
    watcher_ready: bool,
    watcher_last_setup_unix_secs: Option<u64>,
    watcher_last_error: Option<String>,
    active_root: Option<PathBuf>,
    active_root_index: usize,
    total_roots: usize,
    last_scan_started_unix_secs: Option<u64>,
    last_scan_completed_unix_secs: Option<u64>,
    last_scan_duration_ms: Option<u64>,
    last_progress_unix_secs: Option<u64>,
}

impl Default for IndexState {
    fn default() -> Self {
        Self {
            items: Arc::new(Vec::new()),
            snapshot_revision: 0,
            status: SearchIndexStatus::Indexing,
            loading_saved_snapshot: false,
            database_bytes: 0,
            wal_bytes: 0,
            freelist_pages: 0,
            freelist_bytes: 0,
            last_error: None,
            progress_text: None,
            last_completed_indexed_items: None,
            estimated_total_items: None,
            last_indexed_path: None,
            building_indexed_items: 0,
            configured_root_count: 0,
            watched_root_count: 0,
            watcher_ready: false,
            watcher_last_setup_unix_secs: None,
            watcher_last_error: None,
            active_root: None,
            active_root_index: 0,
            total_roots: 0,
            last_scan_started_unix_secs: None,
            last_scan_completed_unix_secs: None,
            last_scan_duration_ms: None,
            last_progress_unix_secs: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchIndexDiagnostics {
    pub status: SearchIndexStatus,
    /// True only while SQLite is deserializing a matching completed snapshot;
    /// it never represents a filesystem scan.
    pub loading_saved_snapshot: bool,
    pub database_bytes: u64,
    pub wal_bytes: u64,
    pub freelist_pages: u64,
    pub freelist_bytes: u64,
    /// Monotonically advances whenever a complete query snapshot is replaced.
    pub snapshot_revision: u64,
    pub detail: Option<String>,
    pub indexed_items: usize,
    pub last_completed_indexed_items: Option<usize>,
    pub estimated_total_items: Option<usize>,
    pub last_indexed_path: Option<PathBuf>,
    pub configured_root_count: usize,
    pub watched_root_count: usize,
    pub watcher_ready: bool,
    pub watcher_last_setup_unix_secs: Option<u64>,
    pub watcher_last_error: Option<String>,
    pub active_root: Option<PathBuf>,
    pub active_root_index: usize,
    pub total_roots: usize,
    pub last_scan_started_unix_secs: Option<u64>,
    pub last_scan_completed_unix_secs: Option<u64>,
    pub last_scan_duration_ms: Option<u64>,
    pub last_progress_unix_secs: Option<u64>,
    pub last_error: Option<String>,
}

impl SearchIndexDiagnostics {
    pub fn unavailable(error: impl Into<String>) -> Self {
        Self {
            status: SearchIndexStatus::Unavailable,
            loading_saved_snapshot: false,
            database_bytes: 0,
            wal_bytes: 0,
            freelist_pages: 0,
            freelist_bytes: 0,
            snapshot_revision: 0,
            detail: None,
            indexed_items: 0,
            last_completed_indexed_items: None,
            estimated_total_items: None,
            last_indexed_path: None,
            configured_root_count: 0,
            watched_root_count: 0,
            watcher_ready: false,
            watcher_last_setup_unix_secs: None,
            watcher_last_error: None,
            active_root: None,
            active_root_index: 0,
            total_roots: 0,
            last_scan_started_unix_secs: None,
            last_scan_completed_unix_secs: None,
            last_scan_duration_ms: None,
            last_progress_unix_secs: None,
            last_error: Some(error.into()),
        }
    }
}

#[cfg(unix)]
pub use daemon::{default_daemon_socket_path, run_search_daemon};
#[cfg(any(unix, target_os = "windows"))]
pub use daemon_client::SearchDaemonClient;
#[cfg(target_os = "windows")]
mod windows_daemon;
#[cfg(target_os = "windows")]
pub use windows_daemon::{ensure_windows_search_daemon, run_windows_search_daemon};

pub struct SearchService {
    config: Arc<RwLock<SearchConfig>>,
    index: Arc<RwLock<IndexState>>,
    worker: Option<worker::WorkerHandle>,
    writer_lock: Option<File>,
    store_path: PathBuf,
    launcher: worker::ScanLauncher,
    watch_filesystem: bool,
}

/// A cloneable, read-only view of the latest search snapshot.
///
/// Handles do not own worker lifecycle or rebuild/configuration controls, so they can be sent to
/// short-lived query tasks and can remain queryable after their [`SearchService`] owner stops.
#[derive(Clone)]
pub struct SearchHandle {
    config: Arc<RwLock<SearchConfig>>,
    index: Arc<RwLock<IndexState>>,
    store_path: Arc<PathBuf>,
}

/// A search response plus non-fatal presentation diagnostics.
///
/// This deliberately keeps [`SearchResponse`] source-compatible for downstream
/// callers that construct it with a struct literal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQueryResult {
    pub response: SearchResponse,
    /// Non-fatal information about intentionally excluded results.
    #[serde(default)]
    pub warning: Option<String>,
}

impl SearchService {
    pub fn new(config: SearchConfig) -> Self {
        Self::new_with_store_path(config, default_search_store_path())
    }

    /// Load the completed configured generation from Ottrin's normal store without opening
    /// writable storage or starting a worker.
    pub fn load_persisted_default(config: SearchConfig) -> Result<Self, String> {
        Self::load_persisted(config, default_search_store_path())
    }

    /// Creates a search service backed by the supplied SQLite store.
    ///
    /// Callers that run an isolated instance, such as a test harness, can use
    /// this to avoid contending with the user's active search index.
    pub fn new_with_store_path(config: SearchConfig, store_path: PathBuf) -> Self {
        let config = resolved_search_config(config);
        Self {
            config: Arc::new(RwLock::new(config)),
            index: Arc::new(RwLock::new(IndexState::default())),
            worker: None,
            writer_lock: None,
            store_path,
            launcher: worker::real_launcher(),
            watch_filesystem: true,
        }
    }

    /// Load the completed configured generation without opening writable storage or starting a
    /// worker. This is the read-only authority used by status and one-shot query clients.
    pub fn load_persisted(config: SearchConfig, store_path: PathBuf) -> Result<Self, String> {
        let plan = plan::ScanPlan::new(&config)?;
        let count = SearchStore::load_count_read_only(&store_path, &plan)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| {
                "No completed search index exists for the configured roots".to_string()
            })?;
        let service = Self::new_with_store_path(config, store_path);
        worker::publish_ready_count(&service.index, count);
        {
            let mut index = service.index.write().expect("index write");
            index.configured_root_count = plan.roots().len();
            index.watched_root_count = 0;
            index.watcher_ready = false;
            index.watcher_last_error =
                Some("Watcher health is unavailable from a read-only status snapshot".to_string());
        }
        Ok(service)
    }

    #[cfg(test)]
    fn new_with_runtime(
        config: SearchConfig,
        store_path: PathBuf,
        launcher: worker::ScanLauncher,
    ) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            index: Arc::new(RwLock::new(IndexState::default())),
            worker: None,
            writer_lock: None,
            store_path,
            launcher,
            watch_filesystem: false,
        }
    }

    pub fn start(&mut self) {
        if self.worker.is_some() {
            return;
        }
        let writer_lock = match acquire_writer_lock(&self.store_path) {
            Ok(lock) => lock,
            Err(error) => {
                self.record_unavailable(error);
                return;
            }
        };
        let store = match SearchStore::open(self.store_path.clone()) {
            Ok(store) => store,
            Err(error) => {
                self.record_unavailable(error.to_string());
                return;
            }
        };
        self.writer_lock = Some(writer_lock);
        let config = self.config.read().expect("config read").clone();
        self.worker = Some(worker::spawn_with_runtime(
            config,
            Arc::clone(&self.index),
            store,
            Arc::clone(&self.launcher),
            self.watch_filesystem,
        ));
    }

    fn record_unavailable(&self, error: String) {
        let mut index = self.index.write().expect("index write");
        index.status = SearchIndexStatus::Unavailable;
        index.last_error = Some(error);
        index.progress_text = None;
    }

    fn stop_worker(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop();
        }
    }

    fn release_writer_lock(&mut self) {
        if let Some(lock) = self.writer_lock.take() {
            let _ = FileExt::unlock(&lock);
        }
    }

    pub fn stop(&mut self) {
        self.stop_worker();
        self.release_writer_lock();
    }

    pub fn clear_storage(&mut self) -> Result<u64, String> {
        self.stop_worker();
        if self.writer_lock.is_none() {
            self.writer_lock = Some(acquire_writer_lock(&self.store_path)?);
        }
        let result = (|| {
            let mut freed = clear_sqlite_storage(&self.store_path)?;
            if self.store_path == search_db_path() {
                freed = freed.saturating_add(clear_search_storage_unlocked()?);
            }
            let mut index = self.index.write().expect("index write");
            *index = IndexState::default();
            index.status = SearchIndexStatus::Unavailable;
            index.last_error = Some("Search storage was cleared".to_string());
            index.progress_text = None;
            Ok(freed)
        })();
        self.release_writer_lock();
        result
    }

    pub fn update_config(&mut self, cfg: SearchConfig) {
        let cfg = resolved_search_config(cfg);
        if let Ok(mut c) = self.config.write() {
            *c = cfg.clone();
        }
        if let Some(worker) = self.worker.as_ref() {
            worker.update_config(cfg);
        }
    }

    /// Force an immediate reindex regardless of config changes.
    pub fn rebuild_index(&mut self) {
        if let Some(worker) = self.worker.as_ref() {
            worker.request_rebuild();
        }
    }

    /// Force a reindex and return only after that exact rebuild commits or fails.
    pub fn rebuild_index_blocking(&mut self) -> Result<(), String> {
        self.worker
            .as_ref()
            .ok_or_else(|| "Search service is not running".to_string())?
            .rebuild_blocking()
    }

    #[cfg(any(unix, target_os = "windows"))]
    fn begin_rebuild_index(&self) -> Result<mpsc::Receiver<Result<(), String>>, String> {
        self.worker
            .as_ref()
            .ok_or_else(|| "Search service is not running".to_string())?
            .begin_rebuild()
    }

    /// Reconcile every configured root and return only after that exact update commits or fails.
    ///
    /// Unlike a rebuild, this publishes a root-scoped delta so state outside the configured roots
    /// is not treated as part of the replacement candidate.
    pub fn update_index_blocking(&mut self) -> Result<(), String> {
        self.worker
            .as_ref()
            .ok_or_else(|| "Search service is not running".to_string())?
            .reconcile_blocking()
    }

    /// Run one headless update and release the worker and writer lock before returning.
    pub fn run_single_update(mut self, rebuild: bool) -> Result<SearchIndexDiagnostics, String> {
        self.watch_filesystem = false;
        self.start();
        let startup = self.diagnostics();
        let result = if matches!(startup.status, SearchIndexStatus::Unavailable) {
            Err(startup
                .last_error
                .unwrap_or_else(|| "search service unavailable".to_string()))
        } else if rebuild {
            self.rebuild_index_blocking()
        } else {
            self.update_index_blocking()
        };
        let result = result.map(|()| self.diagnostics());
        self.stop();
        result
    }

    #[cfg(any(unix, target_os = "windows"))]
    fn begin_index_reconcile(&self) -> Result<mpsc::Receiver<Result<(), String>>, String> {
        self.worker
            .as_ref()
            .ok_or_else(|| "Search service is not running".to_string())?
            .begin_reconcile()
    }

    /// Returns `true` if `plocate` or `locate` is on PATH (the system locate database is usable).
    pub fn system_locate_available() -> bool {
        system_locate_available()
    }

    /// Returns `true` if `updatedb` (mlocate) is on PATH.
    pub fn updatedb_available() -> bool {
        updatedb_available()
    }

    pub fn locate_schedule_status() -> LocateScheduleStatus {
        locate_schedule_status()
    }

    pub fn system_locate_schedule_status() -> SystemLocateScheduleStatus {
        system_locate_schedule_status()
    }

    pub fn sqlite_schedule_status() -> LocateScheduleStatus {
        sqlite_schedule_status()
    }

    /// Start the installed service, or a package-owned session daemon.
    pub fn start_sqlite_schedule() -> Result<String, String> {
        #[cfg(target_os = "linux")]
        {
            if sqlite_systemd_service_path().is_file() {
                return restart_sqlite_schedule();
            }
            let client = SearchDaemonClient::connect_default()?;
            if client.ping().is_ok() {
                return Ok("Session indexing is already running.".to_string());
            }
            let executable =
                find_ottrin_binary().ok_or("Cannot locate this package's index service")?;
            std::process::Command::new(executable)
                .arg("--index-daemon")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .spawn()
                .map_err(|error| format!("Cannot start the session index: {error}"))?;
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                if client.ping().is_ok() {
                    return Ok("Session indexing started.".to_string());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err("The session index did not become ready. Its startup diagnostics are available in the application log.".to_string())
        }
        #[cfg(target_os = "windows")]
        {
            let executable =
                find_ottrin_binary().ok_or("Cannot locate this package's index daemon")?;
            if query_sqlite_windows_task()?.is_some_and(|task| task.enabled) {
                return restart_sqlite_schedule();
            }
            prepare_running_windows_daemon()?;
            ensure_windows_search_daemon(&executable)?;
            Ok("Session indexing started / restarted.".to_string())
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        restart_sqlite_schedule()
    }

    /// Stop indexing without deleting the service registration or saved index.
    pub fn stop_sqlite_schedule() -> Result<String, String> {
        #[cfg(target_os = "linux")]
        {
            if sqlite_systemd_service_path().is_file()
                && !run_sqlite_systemd(SqliteSystemdOperation::Stop)?
            {
                return Err("Could not stop the background index service".to_string());
            }
            prepare_running_daemon_for_upgrade()?;
            Ok("Indexing stopped. Your saved index is kept. Opening Ottrin again starts indexing for that session.".to_string())
        }
        #[cfg(target_os = "windows")]
        {
            query_sqlite_windows_task()?; // Do not mutate runtime after an inspection failure.
            prepare_running_windows_daemon()?;
            stop_registered_windows_index_task()?;
            Ok("Indexing stopped. Your saved index and login setup are kept.".to_string())
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        Err("Stopping the background index service is not available on this platform.".to_string())
    }

    pub fn restart_sqlite_schedule() -> Result<String, String> {
        restart_sqlite_schedule()
    }

    /// Age of Ottrin's managed locate.db in seconds, if it exists.
    pub fn locate_db_age_secs() -> Option<u64> {
        locate_db_age_secs()
    }

    pub fn search_db_path() -> PathBuf {
        search_db_path()
    }

    /// Age of the system locate database in seconds, if detected.
    pub fn system_locate_db_age_secs() -> Option<u64> {
        system_locate_db_age_secs()
    }

    /// Install a persistent schedule (systemd user timer or crontab) to keep
    /// the locate database fresh. Returns a description or error message.
    pub fn install_locate_schedule(interval_hours: u32, root: &Path) -> Result<String, String> {
        install_locate_schedule(interval_hours, root)
    }

    pub fn install_sqlite_schedule(config: &SearchConfig) -> Result<String, String> {
        install_sqlite_schedule(config)
    }

    pub fn uninstall_sqlite_schedule() -> Result<String, String> {
        uninstall_sqlite_schedule()
    }

    /// Trigger an updatedb run for the given root now (non-blocking).
    pub fn run_user_updatedb(root: PathBuf) {
        run_user_updatedb(root);
    }

    /// Returns `true` if the process has `CAP_SYS_ADMIN`, which is one prerequisite for
    /// fanotify-based filesystem-level change notifications. On non-Linux platforms this always
    /// returns `false`.
    pub fn privileged_indexing_available() -> bool {
        #[cfg(target_os = "linux")]
        {
            // Parse CapEff from /proc/self/status — no extra deps required.
            if let Ok(text) = std::fs::read_to_string("/proc/self/status") {
                for line in text.lines() {
                    if let Some(rest) = line.strip_prefix("CapEff:\t")
                        && let Ok(cap_eff) = u64::from_str_radix(rest.trim(), 16)
                    {
                        const CAP_SYS_ADMIN: u64 = 1 << 21;
                        return cap_eff & CAP_SYS_ADMIN != 0;
                    }
                }
            }
            false
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    pub fn handle(&self) -> SearchHandle {
        SearchHandle {
            config: Arc::clone(&self.config),
            index: Arc::clone(&self.index),
            store_path: Arc::new(self.store_path.clone()),
        }
    }

    pub fn index_status(&self) -> (SearchIndexStatus, Option<String>) {
        self.handle().index_status()
    }

    pub fn diagnostics(&self) -> SearchIndexDiagnostics {
        self.handle().diagnostics()
    }

    pub fn query(&self, query: SearchQuery) -> SearchResponse {
        self.handle().query(query)
    }

    /// Run a query and retain non-fatal diagnostics in a serializable result.
    ///
    /// This is the forward-compatible API for callers that need to distinguish
    /// a complete result set from a fail-closed one with excluded unknown
    /// content. [`Self::query`] remains available for legacy callers that
    /// construct or serialize `SearchResponse` directly.
    pub fn query_with_diagnostics(&self, query: SearchQuery) -> SearchQueryResult {
        self.handle().query_with_diagnostics(query)
    }
}

impl SearchHandle {
    pub fn index_status(&self) -> (SearchIndexStatus, Option<String>) {
        let snapshot = self.diagnostics();
        (snapshot.status, snapshot.detail)
    }

    pub fn diagnostics(&self) -> SearchIndexDiagnostics {
        let idx = self.index.read().expect("index read");
        let detail = match idx.status {
            SearchIndexStatus::Loading => idx.progress_text.clone(),
            SearchIndexStatus::Indexing => idx.progress_text.clone(),
            SearchIndexStatus::Unavailable => idx.last_error.clone(),
            SearchIndexStatus::Ready => idx.progress_text.clone(),
        };
        SearchIndexDiagnostics {
            status: idx.status,
            loading_saved_snapshot: idx.loading_saved_snapshot,
            database_bytes: idx.database_bytes,
            wal_bytes: idx.wal_bytes,
            freelist_pages: idx.freelist_pages,
            freelist_bytes: idx.freelist_bytes,
            snapshot_revision: idx.snapshot_revision,
            detail,
            indexed_items: if matches!(idx.status, SearchIndexStatus::Indexing) {
                idx.building_indexed_items
            } else {
                idx.last_completed_indexed_items.unwrap_or(idx.items.len())
            },
            last_completed_indexed_items: idx.last_completed_indexed_items,
            estimated_total_items: idx.estimated_total_items,
            last_indexed_path: idx.last_indexed_path.clone(),
            configured_root_count: idx.configured_root_count,
            watched_root_count: idx.watched_root_count,
            watcher_ready: idx.watcher_ready,
            watcher_last_setup_unix_secs: idx.watcher_last_setup_unix_secs,
            watcher_last_error: idx.watcher_last_error.clone(),
            active_root: idx.active_root.clone(),
            active_root_index: idx.active_root_index,
            total_roots: idx.total_roots,
            last_scan_started_unix_secs: idx.last_scan_started_unix_secs,
            last_scan_completed_unix_secs: idx.last_scan_completed_unix_secs,
            last_scan_duration_ms: idx.last_scan_duration_ms,
            last_progress_unix_secs: idx.last_progress_unix_secs,
            last_error: idx.last_error.clone(),
        }
    }

    pub fn query(&self, query: SearchQuery) -> SearchResponse {
        self.query_with_system_locate(query, query_system_locate)
    }

    fn query_with_system_locate(
        &self,
        query: SearchQuery,
        system_locate: fn(&str, Option<&Path>, bool, usize) -> Vec<SearchResultItem>,
    ) -> SearchResponse {
        #[cfg(target_os = "windows")]
        let query = resolved_search_query(query);
        let (status, items) = {
            let idx = self.index.read().expect("index read");
            (idx.status, Arc::clone(&idx.items))
        };
        let cfg = self.config.read().expect("cfg read").clone();
        if query.text.trim().is_empty() {
            return SearchResponse {
                items: Vec::new(),
                total: 0,
                status,
                error: None,
            };
        }

        let expr = match parse_query(&query.text) {
            Ok(e) => e,
            Err(e) => {
                return SearchResponse {
                    items: Vec::new(),
                    total: 0,
                    status,
                    error: Some(e.to_string()),
                };
            }
        };

        if query.content_search && expr_contains_negated_content(&expr) {
            return self
                .query_negated_content_snapshot(&query, &expr, status, &cfg, None)
                .response;
        }

        let current_folder_outside_index = matches!(query.scope, SearchScope::CurrentFolder)
            && query
                .root_path
                .as_ref()
                .map(|root| !root_is_indexed(root, &cfg))
                .unwrap_or(true);
        let global_root_outside_index = matches!(query.scope, SearchScope::Global)
            && query
                .root_path
                .as_ref()
                .map(|root| !root_is_indexed(root, &cfg))
                .unwrap_or(false);

        let result_capacity = query.offset.saturating_add(query.limit).max(1);
        let mut streamed_index = false;
        let mut total_matches_override = None;
        let mut matched: Vec<(i64, SearchResultItem)> =
            if matches!(query.scope, SearchScope::CurrentFolder)
                && (matches!(
                    status,
                    SearchIndexStatus::Unavailable | SearchIndexStatus::Indexing
                ) || current_folder_outside_index)
            {
                query_current_folder_fallback(&query, &expr, &cfg)
            } else if matches!(query.scope, SearchScope::Global)
                && matches!(status, SearchIndexStatus::Indexing)
                && items.is_empty()
            {
                let roots = ordered_query_roots(&query, &cfg);
                let target_hits = query.limit.saturating_mul(6).max(200);
                query_global_fallback(
                    &query,
                    &expr,
                    &roots,
                    &cfg,
                    24_000,
                    target_hits,
                    Duration::from_millis(140),
                )
            } else if matches!(status, SearchIndexStatus::Ready) && self.store_path.exists() {
                let plan = match plan::ScanPlan::new(&cfg) {
                    Ok(plan) => plan,
                    Err(error) => {
                        return SearchResponse {
                            items: Vec::new(),
                            total: 0,
                            status,
                            error: Some(error),
                        };
                    }
                };
                let simple_page = if is_simple_single_word(&query.text)
                    && query.text.trim().chars().count() >= 3
                    && query.text.trim().is_ascii()
                    && !matches!(query.scope, SearchScope::CurrentFolder)
                    && !matches!(query.sort, SearchSort::Path)
                {
                    query_simple_term_page(
                        self.store_path.as_ref(),
                        &plan,
                        SimpleTermPage {
                            term: query.text.trim(),
                            include_hidden: query.include_hidden_system,
                            sort: query.sort,
                            limit: result_capacity,
                            offset: 0,
                        },
                    )
                    .ok()
                    .filter(|(_, total)| {
                        should_use_simple_term_page(*total, query.limit, query.content_search)
                    })
                } else {
                    None
                };
                if let Some((page, total)) = simple_page {
                    streamed_index = true;
                    total_matches_override = Some(total);
                    page.into_iter()
                        .map(|item| (relevance_score(&query, &item), item))
                        .collect()
                } else {
                    let fuzzy_query =
                        is_simple_single_word(&query.text).then(|| query.text.trim().to_string());
                    let mut exact = Vec::with_capacity(result_capacity);
                    let mut fuzzy = Vec::with_capacity(query.limit.saturating_mul(2).max(200));
                    let mut exact_total = 0usize;
                    let stream_result =
                        stream_generation_items(self.store_path.as_ref(), &plan, None, |item| {
                            if !scope_match(&query, &item)
                                || (!query.include_hidden_system && item.name.starts_with('.'))
                            {
                                return;
                            }
                            if eval_expr(&expr, &item) {
                                exact_total = exact_total.saturating_add(1);
                                push_bounded_result(
                                    &mut exact,
                                    (relevance_score(&query, &item), item),
                                    result_capacity,
                                    query.sort,
                                );
                            } else if let Some(fuzzy_query) = fuzzy_query.as_deref()
                                && fuzzy.len() < query.limit.saturating_mul(2).max(200)
                                && let Some(score) =
                                    fuzzy_subsequence_score(fuzzy_query, &item.name)
                            {
                                fuzzy.push((score, item));
                            }
                        });
                    if let Err(error) = stream_result {
                        return SearchResponse {
                            items: Vec::new(),
                            total: 0,
                            status,
                            error: Some(format!("Search index query failed: {error}")),
                        };
                    }
                    if exact_total < query.limit {
                        let remaining = query
                            .limit
                            .saturating_mul(2)
                            .max(200)
                            .saturating_sub(exact.len());
                        let fuzzy_added = fuzzy.len().min(remaining);
                        exact_total = exact_total.saturating_add(fuzzy_added);
                        for candidate in fuzzy.into_iter().take(fuzzy_added) {
                            push_bounded_result(&mut exact, candidate, result_capacity, query.sort);
                        }
                    }
                    streamed_index = true;
                    total_matches_override = Some(exact_total);
                    exact
                }
            } else {
                items
                    .iter()
                    // A replacement generation remains readable until its commit,
                    // but a removed root must never leak results from that prior
                    // generation while the replacement is in flight.
                    .filter(|item| root_is_indexed(&item.path, &cfg))
                    .filter(|item| scope_match(&query, item))
                    .filter(|item| query.include_hidden_system || !item.name.starts_with('.'))
                    .filter_map(|item| {
                        if eval_expr(&expr, item) {
                            Some((relevance_score(&query, item), item.clone()))
                        } else {
                            None
                        }
                    })
                    .collect()
            };
        let mut total_matches = total_matches_override.unwrap_or(matched.len());

        if global_root_outside_index && query.text.trim().len() >= 2 {
            let mut seen: HashSet<PathBuf> =
                matched.iter().map(|(_, item)| item.path.clone()).collect();
            for (score, item) in query_current_folder_fallback(&query, &expr, &cfg) {
                if seen.insert(item.path.clone()) {
                    matched.push((score, item));
                    total_matches = total_matches.saturating_add(1);
                }
                if matched.len() >= query.limit.saturating_mul(2).max(200) {
                    break;
                }
            }
        }

        // System filename discovery is a peer global source, not a sparse-index
        // fallback. Query it whenever enabled so native results cannot hide
        // broader system paths; exact-path deduplication still prefers native rows.
        if cfg.use_system_locate && query.text.trim().len() >= 2 {
            let scope_root = if matches!(query.scope, SearchScope::CurrentFolder) {
                query.root_path.as_deref()
            } else {
                None
            };
            let locate_hits = system_locate(
                &query.text,
                scope_root,
                query.include_hidden_system,
                result_capacity.max(50),
            );
            let persisted_native = if matches!(status, SearchIndexStatus::Ready)
                && items.is_empty()
                && self.store_path.exists()
            {
                plan::ScanPlan::new(&cfg).ok().and_then(|plan| {
                    let paths = locate_hits
                        .iter()
                        .map(|item| item.path.clone())
                        .collect::<Vec<_>>();
                    active_generation_contains_paths(self.store_path.as_ref(), &plan, &paths).ok()
                })
            } else {
                None
            };
            let mut seen: HashSet<PathBuf> =
                matched.iter().map(|(_, item)| item.path.clone()).collect();
            for (index, item) in locate_hits.into_iter().enumerate() {
                if persisted_native
                    .as_ref()
                    .and_then(|present| present.get(index))
                    .copied()
                    .unwrap_or(false)
                {
                    continue;
                }
                if eval_expr(&expr, &item) && seen.insert(item.path.clone()) {
                    matched.push((relevance_score(&query, &item), item));
                    total_matches = total_matches.saturating_add(1);
                }
            }
        }

        // ── Fuzzy pass: supplement with subsequence-matched filenames ─────────
        // Only runs when query is a single bare word and exact results are sparse.
        if !streamed_index && is_simple_single_word(&query.text) && matched.len() < query.limit {
            let q = query.text.trim();
            let mut seen: HashSet<PathBuf> =
                matched.iter().map(|(_, item)| item.path.clone()).collect();
            for item in items
                .iter()
                .filter(|item| scope_match(&query, item))
                .filter(|item| query.include_hidden_system || !item.name.starts_with('.'))
            {
                if seen.contains(&item.path) {
                    continue;
                }
                if let Some(score) = fuzzy_subsequence_score(q, &item.name) {
                    seen.insert(item.path.clone());
                    matched.push((score, item.clone()));
                    total_matches = total_matches.saturating_add(1);
                    if matched.len() >= query.limit.saturating_mul(2).max(200) {
                        break;
                    }
                }
            }
        }

        // ── Content search pass: query the active FTS generation first. ───────
        // The config is a privacy boundary, not a persistence preference: when
        // disabled, avoid both indexed payloads and the live ripgrep fallback.
        let mut live_content_error = None;
        if query.content_search && cfg.content_indexing_enabled && query.text.trim().len() >= 2 {
            let mut seen: HashSet<PathBuf> =
                matched.iter().map(|(_, item)| item.path.clone()).collect();
            let fts_query = fts_content_match(&expr);
            if matches!(status, SearchIndexStatus::Ready)
                && let Some(fts_query) = fts_query.as_deref()
                && let Ok(plan) = plan::ScanPlan::new(&cfg)
            {
                let desired_hits = query.offset.saturating_add(query.limit).max(1);
                let mut accepted_hits = 0;
                let mut fts_offset = 0;
                const FTS_PAGE_SIZE: usize = 50;
                loop {
                    let Ok(fts_hits) = query_indexed_content_page(
                        self.store_path.as_ref(),
                        &plan,
                        fts_query,
                        FTS_PAGE_SIZE,
                        fts_offset,
                    ) else {
                        break;
                    };
                    let fetched = fts_hits.len();
                    for hit in fts_hits {
                        let mut item = hit.item;
                        let filename_match = hit.metadata_indexed && eval_expr(&expr, &item);
                        // Evaluate against the bounded indexed content, not the short
                        // display snippet: boolean and negated terms can occur on a
                        // different line than the snippet selected for presentation.
                        item.content_snippet = Some(hit.content);
                        if scope_match(&query, &item)
                            && (query.include_hidden_system || !item.name.starts_with('.'))
                            && eval_expr(&expr, &item)
                        {
                            item.content_snippet = Some(hit.snippet);
                            let score = 8_000_i64.saturating_add(hit.relevance);
                            if let Some((existing_score, existing_item)) = matched
                                .iter_mut()
                                .find(|(_, existing)| existing.path == item.path)
                            {
                                *existing_score = (*existing_score).max(score);
                                *existing_item = item;
                            } else if seen.insert(item.path.clone()) {
                                push_bounded_result(
                                    &mut matched,
                                    (score, item),
                                    result_capacity,
                                    query.sort,
                                );
                                if !filename_match {
                                    total_matches = total_matches.saturating_add(1);
                                }
                            }
                            accepted_hits += 1;
                        }
                    }
                    if fetched < FTS_PAGE_SIZE || accepted_hits >= desired_hits {
                        break;
                    }
                    fts_offset = fts_offset.saturating_add(fetched);
                }
            }
            let roots = match query.scope {
                SearchScope::Global => cfg.content_include_roots.clone(),
                SearchScope::CurrentFolder => query.root_path.iter().cloned().collect(),
            };
            let content_limit = query.limit.max(50);
            let rg_items = query_content_ripgrep(
                &expr,
                &roots,
                &cfg,
                query.include_hidden_system,
                content_limit,
                &content_search_program(),
            )
            .unwrap_or_else(|error| {
                live_content_error = Some(error);
                Vec::new()
            });
            let filename_memberships = if streamed_index && !rg_items.is_empty() {
                let paths = rg_items
                    .iter()
                    .map(|item| item.path.clone())
                    .collect::<Vec<_>>();
                plan::ScanPlan::new(&cfg)
                    .and_then(|plan| {
                        active_generation_contains_paths(self.store_path.as_ref(), &plan, &paths)
                            .map_err(|error| error.to_string())
                    })
                    .unwrap_or_else(|error| {
                        live_content_error = Some(format!(
                            "Content search could not verify filename membership: {error}"
                        ));
                        vec![false; rg_items.len()]
                    })
            } else {
                vec![false; rg_items.len()]
            };
            for (item, filename_indexed) in rg_items.into_iter().zip(filename_memberships) {
                if scope_match(&query, &item) && seen.insert(item.path.clone()) {
                    let mut filename_item = item.clone();
                    filename_item.content_snippet = None;
                    let filename_match = filename_indexed && eval_expr(&expr, &filename_item);
                    // Content matches get a high base score — they are strong signals.
                    push_bounded_result(&mut matched, (8_000, item), result_capacity, query.sort);
                    if !filename_match {
                        total_matches = total_matches.saturating_add(1);
                    }
                }
            }
        }

        sort_results(&mut matched, query.sort);
        matched.truncate(result_capacity);
        let total = total_matches;
        let items = matched
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .map(|(_, mut item)| {
                item.parent_path = item
                    .path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_default();
                item
            })
            .collect();

        SearchResponse {
            items,
            total,
            status,
            error: live_content_error,
        }
    }

    /// Run a query and retain non-fatal diagnostics in a serializable result.
    ///
    /// This is the forward-compatible API for callers that need to distinguish
    /// a complete result set from a fail-closed one with excluded unknown
    /// content. [`Self::query`] remains available for legacy callers that
    /// construct or serialize `SearchResponse` directly.
    pub fn query_with_diagnostics(&self, query: SearchQuery) -> SearchQueryResult {
        #[cfg(target_os = "windows")]
        let query = resolved_search_query(query);
        if !query.content_search {
            return SearchQueryResult {
                response: self.query(query),
                warning: None,
            };
        }
        let (status, cfg) = {
            let status = self.index.read().expect("index read").status;
            let cfg = self.config.read().expect("config read").clone();
            (status, cfg)
        };
        let expr = match parse_query(&query.text) {
            Ok(expr) => expr,
            Err(error) => {
                return SearchQueryResult {
                    response: SearchResponse {
                        items: Vec::new(),
                        total: 0,
                        status,
                        error: Some(error.to_string()),
                    },
                    warning: None,
                };
            }
        };
        if expr_contains_negated_content(&expr) {
            self.query_negated_content_snapshot(&query, &expr, status, &cfg, None)
        } else {
            SearchQueryResult {
                response: self.query(query),
                warning: None,
            }
        }
    }
}

impl SearchHandle {
    /// Run a negated content query with cooperative cancellation. Other query
    /// modes retain the established synchronous route.
    pub fn query_cancellable(&self, query: SearchQuery, cancelled: &AtomicBool) -> SearchResponse {
        self.query_cancellable_with_diagnostics(query, cancelled)
            .response
    }

    /// Run a query with cooperative cancellation and non-fatal diagnostics.
    /// UI callers use this so a superseded request can promptly release its
    /// snapshot reader without changing the established `SearchResponse` API.
    pub fn query_cancellable_with_diagnostics(
        &self,
        query: SearchQuery,
        cancelled: &AtomicBool,
    ) -> SearchQueryResult {
        #[cfg(target_os = "windows")]
        let query = resolved_search_query(query);
        if !query.content_search {
            return self.query_with_diagnostics(query);
        }
        let (status, cfg) = {
            let status = self.index.read().expect("index read").status;
            let cfg = self.config.read().expect("config read").clone();
            (status, cfg)
        };
        let expr = match parse_query(&query.text) {
            Ok(expr) => expr,
            Err(error) => {
                return SearchQueryResult {
                    response: SearchResponse {
                        items: Vec::new(),
                        total: 0,
                        status,
                        error: Some(error.to_string()),
                    },
                    warning: None,
                };
            }
        };
        if expr_contains_negated_content(&expr) {
            self.query_negated_content_snapshot(&query, &expr, status, &cfg, Some(cancelled))
        } else {
            self.query_with_diagnostics(query)
        }
    }

    fn query_negated_content_snapshot(
        &self,
        query: &SearchQuery,
        expr: &Expr,
        status: SearchIndexStatus,
        cfg: &SearchConfig,
        cancelled: Option<&AtomicBool>,
    ) -> SearchQueryResult {
        if !matches!(status, SearchIndexStatus::Ready) {
            return SearchQueryResult {
                response: SearchResponse {
                    items: Vec::new(),
                    total: 0,
                    status,
                    error: Some(
                        "Negated content search requires a completed indexed generation."
                            .to_string(),
                    ),
                },
                warning: None,
            };
        }
        let plan = match plan::ScanPlan::new(cfg) {
            Ok(plan) => plan,
            Err(error) => {
                return SearchQueryResult {
                    response: SearchResponse {
                        items: Vec::new(),
                        total: 0,
                        status,
                        error: Some(error.to_string()),
                    },
                    warning: None,
                };
            }
        };
        let mut matched = Vec::new();
        let mut total_matches = 0usize;
        let mut unknown_items = 0usize;
        let result_capacity = query.offset.saturating_add(query.limit).max(1);
        let stats =
            stream_generation_content_items(self.store_path.as_ref(), &plan, cancelled, |record| {
                // Unknown content is excluded from every negated-content query. An
                // absent FTS row must never become evidence for a complement result.
                if !record.complete {
                    if scope_match(query, &record.item)
                        && (query.include_hidden_system || !record.item.name.starts_with('.'))
                        && eval_expr(expr, &record.item)
                    {
                        unknown_items = unknown_items.saturating_add(1);
                    }
                    return;
                }
                let mut item = record.item;
                item.content_snippet = record.content;
                if scope_match(query, &item)
                    && (query.include_hidden_system || !item.name.starts_with('.'))
                    && eval_expr(expr, &item)
                {
                    total_matches = total_matches.saturating_add(1);
                    matched.push((relevance_score(query, &item), item));
                    // The reader streams one item at a time; retain only the page
                    // window while scanning so a broad complement cannot clone the
                    // whole generation into query memory.
                    if matched.len() > result_capacity {
                        sort_results(&mut matched, query.sort);
                        matched.truncate(result_capacity);
                    }
                }
            });
        match stats {
            Ok(_) => {}
            Err(error) => {
                return SearchQueryResult {
                    response: SearchResponse {
                        items: Vec::new(),
                        total: 0,
                        status,
                        error: Some(format!(
                            "Negated content search could not read its indexed generation: {error}"
                        )),
                    },
                    warning: None,
                };
            }
        };
        sort_results(&mut matched, query.sort);
        let items = matched
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .map(|(_, mut item)| {
                item.parent_path = item
                    .path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_default();
                item
            })
            .collect();
        SearchQueryResult {
            response: SearchResponse {
                items,
                total: total_matches,
                status,
                error: None,
            },
            warning: unknown_content_warning(unknown_items),
        }
    }
}

fn unknown_content_warning(unknown_items: usize) -> Option<String> {
    (unknown_items > 0).then(|| {
        let count = if unknown_items > 999 {
            "999+".to_string()
        } else {
            unknown_items.to_string()
        };
        format!(
            "Excluded {count} item(s) with unknown indexed content from negated content search."
        )
    })
}

impl Drop for SearchService {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn resolved_search_config(config: SearchConfig) -> SearchConfig {
    #[cfg(target_os = "windows")]
    {
        let mut config = config;
        for roots in [
            &mut config.include_roots,
            &mut config.exclude_roots,
            &mut config.content_include_roots,
            &mut config.content_exclude_roots,
            &mut config.active_indexed_volume_roots,
        ] {
            for root in roots.iter_mut() {
                *root = path_identity::resolved_index_path(root);
            }
            roots.sort();
            roots.dedup();
        }
        config
    }
    #[cfg(not(target_os = "windows"))]
    {
        config
    }
}

#[cfg(target_os = "windows")]
fn resolved_search_query(mut query: SearchQuery) -> SearchQuery {
    query.root_path = query
        .root_path
        .map(|path| path_identity::resolved_index_path(&path));
    query
}

fn effective_roots(cfg: &SearchConfig) -> Vec<PathBuf> {
    let mut roots = cfg.include_roots.clone();
    roots.extend(cfg.active_indexed_volume_roots.iter().cloned());
    #[cfg(target_os = "windows")]
    for root in &mut roots {
        *root = path_identity::resolved_index_path(root);
    }
    roots.sort();
    roots.dedup();
    roots
}

fn ordered_query_roots(query: &SearchQuery, cfg: &SearchConfig) -> Vec<PathBuf> {
    let roots = effective_roots(cfg);
    let Some(preferred) = query.root_path.as_ref() else {
        return roots;
    };
    if !preferred.exists() {
        return roots;
    }

    let mut ordered = Vec::with_capacity(roots.len() + 1);
    let mut seen: HashSet<PathBuf> = HashSet::with_capacity(roots.len() + 1);
    if seen.insert(preferred.clone()) {
        ordered.push(preferred.clone());
    }
    for root in roots {
        if seen.insert(root.clone()) {
            ordered.push(root);
        }
    }
    ordered
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Index persistence ──────────────────────────────────────────────────────────

fn index_cache_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    let cache_root = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            ottrin_core::default_home_dir()
                .join("AppData")
                .join("Local")
        });
    #[cfg(not(target_os = "windows"))]
    let cache_root = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| ottrin_core::default_home_dir().join(".cache"));
    let dir = cache_root.join("ottrin");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("search_index.json")
}

fn remove_file_if_present(path: &Path) -> Result<u64, String> {
    match std::fs::metadata(path) {
        Ok(metadata) => {
            let len = metadata.len();
            std::fs::remove_file(path)
                .map_err(|error| format!("Failed to remove {}: {error}", path.display()))?;
            Ok(len)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(format!("Failed to inspect {}: {error}", path.display())),
    }
}

fn writer_lock_path(path: &Path) -> PathBuf {
    let mut lock_path = path.as_os_str().to_os_string();
    lock_path.push(".lock");
    PathBuf::from(lock_path)
}

fn acquire_writer_lock(store_path: &Path) -> Result<File, String> {
    let lock_path = writer_lock_path(store_path);
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create search lock directory: {error}"))?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| format!("Failed to open search writer lock: {error}"))?;
    lock.try_lock_exclusive().map_err(|error| {
        format!(
            "Search index is already owned by another Ottrin process ({}): {error}",
            lock_path.display()
        )
    })?;
    Ok(lock)
}

fn clear_sqlite_storage(path: &Path) -> Result<u64, String> {
    let mut freed = remove_file_if_present(path)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        freed = freed.saturating_add(remove_file_if_present(Path::new(&sidecar))?);
    }
    for content_path in generation_content_db_paths(path) {
        freed = freed.saturating_add(clear_sqlite_storage(&content_path)?);
    }
    Ok(freed)
}

fn clear_search_storage_unlocked() -> Result<u64, String> {
    let mut freed = clear_sqlite_storage(&search_db_path())?;
    #[cfg(target_os = "linux")]
    let mut paths = vec![index_cache_path(), locate_db_path()];
    #[cfg(target_os = "linux")]
    paths.push(appimage_index_wrapper_path());
    #[cfg(not(target_os = "linux"))]
    let paths = vec![index_cache_path(), locate_db_path()];
    for path in paths {
        freed = freed.saturating_add(remove_file_if_present(&path)?);
    }
    Ok(freed)
}

pub fn clear_search_storage() -> Result<u64, String> {
    let lock = acquire_writer_lock(&search_db_path())?;
    let result = clear_search_storage_unlocked();
    let _ = FileExt::unlock(&lock);
    result
}

// ── Locate database management ─────────────────────────────────────────────────
//
// Linux: Ottrin manages its own mlocate-format database at
//   ~/.cache/ottrin/locate.db
// This is built with:
//   updatedb -l 0 -U <root> -o ~/.cache/ottrin/locate.db
// and queried with:
//   locate -d ~/.cache/ottrin/locate.db -i <query>
// No root required. Runs as the current user. Opt-in.
//
// Windows can also use Task Scheduler for Ottrin's SQLite refresh. A deeper
// NTFS USN journal integration is intentionally outside the current v1 path.

/// Path where Ottrin stores its own mlocate database.
fn locate_db_path() -> PathBuf {
    index_cache_path().with_file_name("locate.db")
}

/// Returns true if `updatedb` (mlocate) is on PATH — required to build our own locate database.
pub fn updatedb_available() -> bool {
    command_exists("updatedb")
}

/// Returns true if `plocate` or `locate` is accessible on PATH.
pub fn system_locate_available() -> bool {
    command_exists("plocate") || command_exists("locate")
}

#[cfg(any(target_os = "windows", test))]
fn internal_command_creation_flags(is_windows: bool) -> u32 {
    if is_windows { 0x0800_0000 } else { 0 }
}

fn internal_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new(program);
        command.creation_flags(internal_command_creation_flags(true));
        command
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new(program)
    }
}

fn content_search_program() -> std::ffi::OsString {
    let binary = if cfg!(windows) { "rg.exe" } else { "rg" };
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
    {
        // Windows and portable Linux keep the provider beside the real app.
        // Native Linux and AppImage layouts use the private libexec directory.
        for candidate in [
            parent.join("../libexec/ottrin").join(binary),
            parent.join(binary),
        ] {
            if candidate.is_file() {
                return candidate.into_os_string();
            }
        }
    }
    // Development and CI can use their explicitly installed test dependency.
    "rg".into()
}

fn command_exists(name: &str) -> bool {
    internal_command(name)
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// Spawn a background thread that runs `updatedb` for the given root, writing
/// into Ottrin's own locate.db. Zero-privilege for paths the current user owns.
/// Returns immediately; the actual updatedb run may take a few seconds.
pub fn run_user_updatedb(root: PathBuf) {
    std::thread::spawn(move || {
        let db = locate_db_path();
        let _ = std::process::Command::new("updatedb")
            .args(["-l", "0"])
            .arg("-U")
            .arg(&root)
            .arg("-o")
            .arg(&db)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    });
}

/// How many seconds ago Ottrin's locate.db was last updated (mtime).
pub fn locate_db_age_secs() -> Option<u64> {
    let mtime = std::fs::metadata(locate_db_path()).ok()?.modified().ok()?;
    SystemTime::now()
        .duration_since(mtime)
        .ok()
        .map(|d| d.as_secs())
}

/// Age of the system locate database in seconds (Linux), if found.
pub fn system_locate_db_age_secs() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let candidates = [
            "/var/lib/plocate/plocate.db",
            "/var/lib/mlocate/mlocate.db",
            "/var/lib/locate/locatedb",
            "/var/lib/locate/locate.db",
        ];
        for path in candidates {
            let mtime = std::fs::metadata(path).ok()?.modified().ok()?;
            if let Ok(age) = SystemTime::now().duration_since(mtime) {
                return Some(age.as_secs());
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[derive(Debug, Clone)]
pub struct LocateScheduleStatus {
    pub source: String,
    pub enabled: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct SystemLocateScheduleStatus {
    pub source: Option<String>,
    pub detail: Option<String>,
    pub needs_setup: bool,
    pub managed_by_ottrin: bool,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SqliteWindowsTaskRegistration {
    enabled: bool,
}

#[cfg(any(target_os = "windows", test))]
fn parse_sqlite_windows_task_xml(output: &[u8]) -> Result<SqliteWindowsTaskRegistration, String> {
    let xml = if output.starts_with(&[0xff, 0xfe]) {
        let units = output[2..]
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units)
            .map_err(|error| format!("invalid Task Scheduler XML: {error}"))?
    } else if output.starts_with(&[0xfe, 0xff]) {
        let units = output[2..]
            .chunks_exact(2)
            .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units)
            .map_err(|error| format!("invalid Task Scheduler XML: {error}"))?
    } else {
        String::from_utf8(output.to_vec())
            .map_err(|error| format!("invalid Task Scheduler XML: {error}"))?
    };
    if !xml.contains("<Task") {
        return Err("Task Scheduler returned malformed task XML".to_string());
    }
    // Trigger enablement and task enablement are independent. Task Scheduler
    // emits trigger flags before Settings, so never read the first global flag.
    let settings = match xml.split_once("<Settings>") {
        Some((_, rest)) => rest
            .split_once("</Settings>")
            .map(|(settings, _)| settings)
            .ok_or_else(|| "Task Scheduler XML has unterminated Settings".to_string())?,
        None => "", // The schema defaults task enablement to true.
    };
    let enabled = match settings.split_once("<Enabled>") {
        None => true,
        Some((_, rest)) => {
            let (value, _) = rest.split_once("</Enabled>").ok_or_else(|| {
                "Task Scheduler XML has an unterminated Enabled value".to_string()
            })?;
            match value.trim() {
                "true" => true,
                "false" => false,
                _ => return Err("Task Scheduler XML has an invalid Enabled value".to_string()),
            }
        }
    };
    Ok(SqliteWindowsTaskRegistration { enabled })
}

#[cfg(any(target_os = "windows", test))]
fn sqlite_windows_task_status_with(
    query: impl FnOnce() -> Result<Option<SqliteWindowsTaskRegistration>, String>,
) -> LocateScheduleStatus {
    match query() {
        Ok(Some(registration)) if registration.enabled => LocateScheduleStatus {
            source: "Windows Task Scheduler".to_string(),
            enabled: true,
            detail: "Enabled at login".to_string(),
        },
        Ok(Some(_)) => LocateScheduleStatus {
            source: "Windows Task Scheduler".to_string(),
            enabled: false,
            detail: "Installed but disabled".to_string(),
        },
        Ok(None) => LocateScheduleStatus {
            source: "none".to_string(),
            enabled: false,
            detail: "No background Ottrin index task installed".to_string(),
        },
        Err(error) => LocateScheduleStatus {
            source: "Windows Task Scheduler".to_string(),
            enabled: false,
            detail: format!("Status unavailable: {error}"),
        },
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SqliteWindowsTaskOperation {
    Query,
    Run,
    Delete,
}

#[cfg(any(target_os = "windows", test))]
impl SqliteWindowsTaskOperation {
    fn args(self, task_name: &str) -> Option<Vec<&str>> {
        match self {
            Self::Query => None,
            Self::Run => Some(vec!["/Run", "/TN", task_name]),
            Self::Delete => Some(vec!["/Delete", "/F", "/TN", task_name]),
        }
    }

    #[cfg(target_os = "windows")]
    fn label(self) -> &'static str {
        match self {
            Self::Query => "query the Ottrin Index task",
            Self::Run => "start the Ottrin Index task",
            Self::Delete => "delete the Ottrin Index task",
        }
    }
}

#[cfg(target_os = "windows")]
const WINDOWS_INDEX_IDENTITY_SCRIPT: &str =
    include_str!("../../../packaging/windows/windows-index-identity.ps1");

#[cfg(target_os = "windows")]
fn windows_index_task_name() -> Result<String, String> {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if let Some(name) = NAME.get() {
        return Ok(name.clone());
    }
    let script = format!(
        "$ErrorActionPreference='Stop'; {WINDOWS_INDEX_IDENTITY_SCRIPT}\n[Console]::Out.Write((Get-OttrinIndexTaskName))"
    );
    let output = internal_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|error| format!("Failed to resolve index task identity: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Failed to resolve index task identity: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let name = String::from_utf8(output.stdout)
        .map_err(|error| format!("Invalid index task identity: {error}"))?;
    if name.is_empty() {
        return Err("Index task identity returned no name".to_owned());
    }
    let _ = NAME.set(name.clone());
    Ok(name)
}

#[cfg(target_os = "windows")]
fn prepare_running_windows_daemon() -> Result<(), String> {
    let client = SearchDaemonClient::connect_default()?;
    if client.ping().is_ok() {
        client.prepare_upgrade_and_wait()?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn stop_registered_windows_index_task() -> Result<(), String> {
    let script = format!(
        "try {{ $ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; {WINDOWS_INDEX_IDENTITY_SCRIPT}\n$service=New-Object -ComObject Schedule.Service; $service.Connect(); $task=Get-OttrinIndexTask ($service.GetFolder('\\')); if ($null -ne $task) {{ Stop-OttrinIndexTask $task }} }} catch {{ [Console]::Error.Write($_.Exception.GetBaseException().Message); exit 1 }}"
    );
    let output = internal_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|error| format!("Cannot stop the registered index task: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Cannot stop the registered index task: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_sqlite_windows_task(operation: SqliteWindowsTaskOperation) -> Result<bool, String> {
    if operation == SqliteWindowsTaskOperation::Query {
        return query_sqlite_windows_task().map(|task| task.is_some());
    }
    let task_name = windows_index_task_name()?;
    prepare_running_windows_daemon()?;
    stop_registered_windows_index_task()?;
    let args = operation
        .args(&task_name)
        .ok_or("Task query requires authenticated inspection")?;
    internal_command("schtasks")
        .args(args)
        .status()
        .map(|status| status.success())
        .map_err(|error| format!("Failed to {}: {error}", operation.label()))
}

#[cfg(target_os = "windows")]
fn query_sqlite_windows_task() -> Result<Option<SqliteWindowsTaskRegistration>, String> {
    let script = format!(
        "try {{ $ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); {WINDOWS_INDEX_IDENTITY_SCRIPT}\n$service=New-Object -ComObject Schedule.Service; $service.Connect(); $task=Get-OttrinIndexTask ($service.GetFolder('\\')); if ($null -ne $task) {{ Assert-OttrinTaskOwner $task; [Console]::Out.Write($task.Xml) }} }} catch {{ [Console]::Error.Write($_.Exception.GetBaseException().Message); exit 1 }}"
    );
    let output = internal_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|error| format!("Failed to inspect the Ottrin Index task: {error}"))?;
    parse_sqlite_windows_task_query(output.status.success(), &output.stdout, &output.stderr)
}

#[cfg(any(target_os = "windows", test))]
fn parse_sqlite_windows_task_query(
    success: bool,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<Option<SqliteWindowsTaskRegistration>, String> {
    if !success {
        let detail = String::from_utf8_lossy(if stderr.is_empty() { stdout } else { stderr });
        return Err(format!(
            "Failed to inspect the Ottrin Index task: {}",
            detail.trim()
        ));
    }
    if stdout.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    parse_sqlite_windows_task_xml(stdout).map(Some)
}

#[cfg(any(target_os = "windows", test))]
fn restart_sqlite_windows_task_with(
    mut run: impl FnMut(SqliteWindowsTaskOperation) -> Result<bool, String>,
) -> Result<String, String> {
    if !run(SqliteWindowsTaskOperation::Query)? {
        return Err("Persistent Ottrin index task is not installed".to_string());
    }
    if !run(SqliteWindowsTaskOperation::Run)? {
        return Err("Task Scheduler failed to start the Ottrin Index task".to_string());
    }
    Ok("Background Ottrin index task started.".to_string())
}

#[cfg(any(target_os = "windows", test))]
fn uninstall_sqlite_windows_task_with(
    mut run: impl FnMut(SqliteWindowsTaskOperation) -> Result<bool, String>,
    stop_session: impl FnOnce() -> Result<(), String>,
) -> Result<String, String> {
    if !run(SqliteWindowsTaskOperation::Query)? {
        stop_session()?;
        return Ok(
            "Session indexing stopped; no background Ottrin index task was installed.".to_string(),
        );
    }
    if !run(SqliteWindowsTaskOperation::Delete)? {
        return Err("Task Scheduler failed to delete the Ottrin Index task".to_string());
    }
    Ok("Background Ottrin index task removed.".to_string())
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SqliteSystemdOperation {
    IsEnabled,
    IsActive,
    Restart,
    Stop,
    DisableNowService,
}

#[cfg(target_os = "linux")]
impl SqliteSystemdOperation {
    fn args(self) -> &'static [&'static str] {
        match self {
            Self::IsEnabled => &["--user", "is-enabled", "--quiet", "ottrin-index.service"],
            Self::IsActive => &["--user", "is-active", "--quiet", "ottrin-index.service"],
            Self::Restart => &["--user", "restart", "ottrin-index.service"],
            Self::Stop => &["--user", "stop", "ottrin-index.service"],
            Self::DisableNowService => &["--user", "disable", "--now", "ottrin-index.service"],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::IsEnabled => "query whether the service is enabled",
            Self::IsActive => "query whether the service is active",
            Self::Restart => "restart the service",
            Self::Stop => "stop the service",
            Self::DisableNowService => "disable and stop the service",
        }
    }
}

#[cfg(target_os = "linux")]
fn run_sqlite_systemd(operation: SqliteSystemdOperation) -> Result<bool, String> {
    std::process::Command::new("systemctl")
        .args(operation.args())
        .status()
        .map(|status| status.success())
        .map_err(|error| format!("Failed to {}: {error}", operation.label()))
}

#[cfg(target_os = "linux")]
fn sqlite_systemd_status_with(
    unit_exists: bool,
    mut run: impl FnMut(SqliteSystemdOperation) -> Result<bool, String>,
) -> LocateScheduleStatus {
    if !unit_exists {
        return LocateScheduleStatus {
            source: "none".to_string(),
            enabled: false,
            detail: "No persistent Ottrin index daemon detected".to_string(),
        };
    }
    let unavailable = |error: String| LocateScheduleStatus {
        source: "systemd user service".to_string(),
        enabled: false,
        detail: format!("Status unavailable: {error}"),
    };
    let enabled = match run(SqliteSystemdOperation::IsEnabled) {
        Ok(enabled) => enabled,
        Err(error) => return unavailable(error),
    };
    let active = match run(SqliteSystemdOperation::IsActive) {
        Ok(active) => active,
        Err(error) => return unavailable(error),
    };
    let detail = match (enabled, active) {
        (true, true) => "Enabled and running",
        (true, false) => "Enabled but stopped",
        (false, true) => "Running but disabled",
        (false, false) => "Installed but disabled and stopped",
    };
    LocateScheduleStatus {
        source: "systemd user service".to_string(),
        enabled,
        detail: detail.to_string(),
    }
}

fn systemd_user_unit_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| ottrin_core::default_home_dir().join(".config"))
        .join("systemd/user")
}

pub fn sqlite_systemd_service_path() -> PathBuf {
    systemd_user_unit_dir().join("ottrin-index.service")
}

/// Detect whether Ottrin's primary SQLite index has a persistent daemon installed.
pub fn sqlite_schedule_status() -> LocateScheduleStatus {
    #[cfg(target_os = "linux")]
    {
        let unit_dir = systemd_user_unit_dir();
        let service_path = unit_dir.join("ottrin-index.service");
        if service_path.exists() {
            if command_exists("systemctl") {
                return sqlite_systemd_status_with(true, run_sqlite_systemd);
            }
            return LocateScheduleStatus {
                source: "systemd user service".to_string(),
                enabled: false,
                detail: "Installed but systemctl is unavailable".to_string(),
            };
        }
    }

    #[cfg(target_os = "windows")]
    {
        sqlite_windows_task_status_with(query_sqlite_windows_task)
    }

    #[cfg(not(target_os = "windows"))]
    {
        LocateScheduleStatus {
            source: "none".to_string(),
            enabled: false,
            detail: "No persistent Ottrin index daemon detected".to_string(),
        }
    }
}

/// Detect whether Ottrin's user-level locate schedule is installed (systemd user timer or crontab).
pub fn locate_schedule_status() -> LocateScheduleStatus {
    let unit_dir = systemd_user_unit_dir();
    let timer_path = unit_dir.join("ottrin-updatedb.timer");
    if timer_path.exists() {
        let enabled = if command_exists("systemctl") {
            std::process::Command::new("systemctl")
                .args(["--user", "is-enabled", "ottrin-updatedb.timer"])
                .status()
                .map(|s| s.success())
                .unwrap_or(true)
        } else {
            true
        };
        return LocateScheduleStatus {
            source: "systemd user timer".to_string(),
            enabled,
            detail: if enabled {
                "Enabled".to_string()
            } else {
                "Installed but disabled".to_string()
            },
        };
    }

    let crontab = std::process::Command::new("crontab")
        .arg("-l")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    if crontab.contains("# ottrin-updatedb") {
        return LocateScheduleStatus {
            source: "crontab".to_string(),
            enabled: true,
            detail: "Entry found (# ottrin-updatedb)".to_string(),
        };
    }

    LocateScheduleStatus {
        source: "none".to_string(),
        enabled: false,
        detail: "No schedule detected".to_string(),
    }
}

/// Detect whether healthy system locate database maintenance is scheduled.
pub fn system_locate_schedule_status() -> SystemLocateScheduleStatus {
    #[cfg(target_os = "linux")]
    {
        system_locate_schedule_status_with(
            |path| std::fs::metadata(path).is_ok(),
            |timer| {
                std::process::Command::new("systemctl")
                    .args(["is-enabled", "--quiet", timer])
                    .status()
                    .is_ok_and(|status| status.success())
            },
        )
    }

    #[cfg(not(target_os = "linux"))]
    SystemLocateScheduleStatus {
        source: None,
        detail: None,
        needs_setup: true,
        managed_by_ottrin: false,
    }
}

#[cfg(any(target_os = "linux", test))]
fn system_locate_schedule_status_with(
    mut path_exists: impl FnMut(&str) -> bool,
    mut systemd_timer_enabled: impl FnMut(&str) -> bool,
) -> SystemLocateScheduleStatus {
    let ottrin_cron = "/etc/cron.daily/ottrin-updatedb";
    if path_exists(ottrin_cron) {
        return SystemLocateScheduleStatus {
            source: Some("cron".to_string()),
            detail: Some(ottrin_cron.to_string()),
            needs_setup: false,
            managed_by_ottrin: true,
        };
    }

    let timer_names = ["plocate-updatedb.timer", "mlocate.timer", "updatedb.timer"];
    for name in timer_names {
        if systemd_timer_enabled(name) {
            return SystemLocateScheduleStatus {
                source: Some("systemd".to_string()),
                detail: Some(name.to_string()),
                needs_setup: false,
                managed_by_ottrin: false,
            };
        }
    }

    let cron_paths = [
        "/etc/cron.daily/plocate",
        "/etc/cron.daily/mlocate",
        "/etc/cron.daily/updatedb",
        "/etc/cron.weekly/updatedb",
        "/etc/cron.weekly/plocate",
    ];
    for path in cron_paths {
        if path_exists(path) {
            return SystemLocateScheduleStatus {
                source: Some("cron".to_string()),
                detail: Some(path.to_string()),
                needs_setup: false,
                managed_by_ottrin: false,
            };
        }
    }

    SystemLocateScheduleStatus {
        source: None,
        detail: None,
        needs_setup: true,
        managed_by_ottrin: false,
    }
}

#[cfg(target_os = "linux")]
fn restart_sqlite_systemd_with(
    unit_exists: bool,
    mut run: impl FnMut(SqliteSystemdOperation) -> Result<bool, String>,
) -> Result<String, String> {
    if !unit_exists {
        return Err("Persistent Ottrin index service is not installed".to_string());
    }
    if !run(SqliteSystemdOperation::Restart)? {
        return Err("systemctl failed to restart the Ottrin index service".to_string());
    }
    Ok("Persistent Ottrin index daemon restarted.".to_string())
}

pub fn restart_sqlite_schedule() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let service_path = sqlite_systemd_service_path();
        if !command_exists("systemctl") {
            return Err(
                "Cannot restart background indexing because systemctl is unavailable".to_string(),
            );
        }
        restart_sqlite_systemd_with(service_path.is_file(), run_sqlite_systemd)
    }

    #[cfg(target_os = "windows")]
    {
        restart_sqlite_windows_task_with(run_sqlite_windows_task)?;
        let executable = find_ottrin_binary().ok_or("Cannot locate this package's index daemon")?;
        windows_daemon::wait_for_windows_search_daemon(&executable)?;
        Ok("Background Ottrin index service restarted and ready.".to_string())
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err(
            "Restarting the persistent index service is not supported on this platform yet."
                .to_string(),
        )
    }
}

#[cfg(target_os = "linux")]
fn prepare_sqlite_systemd_upgrade_with(
    unit_exists: bool,
    prepare_daemon: impl FnOnce() -> Result<(), String>,
    mut run: impl FnMut(SqliteSystemdOperation) -> Result<bool, String>,
) -> Result<(), String> {
    prepare_daemon()?;
    if !unit_exists {
        return Ok(());
    }
    if !run(SqliteSystemdOperation::Stop)? {
        return Err(
            "systemctl failed to stop the existing Ottrin index service for upgrade".to_string(),
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn prepare_running_daemon_for_upgrade() -> Result<(), String> {
    let socket = match default_daemon_socket_path() {
        Ok(socket) if socket.exists() => socket,
        _ => return Ok(()),
    };
    let client = SearchDaemonClient::new(socket.clone());
    if client.ping().is_err() {
        return Ok(());
    }
    client.prepare_upgrade_and_wait()
}

pub fn install_sqlite_schedule(config: &SearchConfig) -> Result<String, String> {
    let ottrin = find_ottrin_binary()
        .ok_or_else(|| "Could not find this package's index executable.".to_string())?;

    #[cfg(target_os = "linux")]
    {
        let _ = config;
        let has_systemd = std::process::Command::new("systemctl")
            .args(["--user", "is-system-running"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success() || s.code() == Some(1))
            .unwrap_or(false);
        if has_systemd {
            return install_sqlite_systemd_user_daemon(&ottrin);
        }
        Err("Persistent background indexing requires a systemd user service on Linux; Ottrin will still start the daemon with the app for this session.".to_string())
    }

    #[cfg(target_os = "windows")]
    {
        query_sqlite_windows_task()?; // Inspection failures must precede any runtime changes.
        let client = SearchDaemonClient::connect_default()?;
        let was_running = client.ping().is_ok();
        prepare_running_windows_daemon()?;
        let message = install_sqlite_windows_task(&ottrin)?;
        let registration =
            query_sqlite_windows_task()?.ok_or("The index task disappeared after setup")?;
        if registration.enabled {
            if !run_sqlite_windows_task(SqliteWindowsTaskOperation::Run)? {
                return Err(
                    "The index task was installed but Task Scheduler could not start it"
                        .to_string(),
                );
            }
            let client = windows_daemon::wait_for_windows_search_daemon(&ottrin)?;
            client.update_config(config.clone())?;
            Ok(format!("{message} Index daemon is ready."))
        } else {
            if was_running {
                ensure_windows_search_daemon(&ottrin)?.update_config(config.clone())?;
            }
            Ok(message)
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = (config, ottrin);
        Err(
            "Background SQLite refresh scheduling is not supported on this platform yet."
                .to_string(),
        )
    }
}

#[cfg(target_os = "linux")]
fn stop_sqlite_systemd_for_removal_with(
    service_exists: bool,
    mut run: impl FnMut(SqliteSystemdOperation) -> Result<bool, String>,
) -> Result<(), String> {
    if service_exists {
        let operation = SqliteSystemdOperation::DisableNowService;
        if !run(operation)? {
            return Err(format!("systemctl failed to {}", operation.label()));
        }
    }
    Ok(())
}

pub fn uninstall_sqlite_schedule() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let unit_dir = systemd_user_unit_dir();
        let service_path = unit_dir.join("ottrin-index.service");
        let mut changed = false;
        if service_path.is_file() {
            if command_exists("systemctl") {
                stop_sqlite_systemd_for_removal_with(true, run_sqlite_systemd)?;
            } else {
                return Err(
                    "Cannot remove background indexing safely because systemctl is unavailable"
                        .to_string(),
                );
            }
            remove_file_if_present(&service_path)?;
            let status = std::process::Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .status()
                .map_err(|error| format!("Failed to reload systemd user units: {error}"))?;
            if !status.success() {
                return Err(format!("systemctl daemon-reload exited with {status}"));
            }
            changed = true;
        }
        if changed {
            Ok("Background Ottrin index schedule removed.".to_string())
        } else {
            prepare_running_daemon_for_upgrade()?;
            Ok(
                "Session indexing stopped; no background Ottrin index schedule was installed."
                    .to_string(),
            )
        }
    }

    #[cfg(target_os = "windows")]
    {
        uninstall_sqlite_windows_task_with(run_sqlite_windows_task, prepare_running_windows_daemon)
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Ok("No background schedule support exists for this platform yet.".to_string())
    }
}

fn find_ottrin_binary() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let current = std::env::current_exe().ok()?;
        let dedicated = current.parent()?.join("ottrin-indexd.exe");
        dedicated.is_file().then_some(dedicated)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let exe_name = "ottrin";
        #[cfg(target_os = "linux")]
        if let Some(wrapper) = create_appimage_index_wrapper() {
            return Some(wrapper);
        }
        if let Ok(current) = std::env::current_exe() {
            #[cfg(target_os = "linux")]
            if let Some(directory) = current.parent() {
                let dedicated = directory.join("ottrin-indexd");
                if dedicated.is_file() {
                    return Some(dedicated);
                }
            }
            let current_name = current.file_name().and_then(|name| name.to_str());
            if matches!(
                current_name,
                Some("ottrin" | "ottrin.exe" | "ottrin-app" | "ottrin-app.exe")
            ) {
                return Some(current);
            }
            if let Some(dir) = current.parent() {
                let sibling = dir.join(exe_name);
                if sibling.exists() {
                    return Some(sibling);
                }
            }
        }
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(exe_name))
                .find(|candidate| candidate.exists())
        })
    }
}

#[cfg(target_os = "linux")]
fn appimage_index_wrapper_path() -> PathBuf {
    let cache_root = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| ottrin_core::default_home_dir().join(".cache"));
    cache_root.join("ottrin").join("ottrin-index-appimage")
}

#[cfg(target_os = "linux")]
fn create_appimage_index_wrapper() -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from)?;
    if !appimage.is_file() {
        return None;
    }
    let wrapper = appimage_index_wrapper_path();
    if let Some(parent) = wrapper.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    let script = format!(
        "#!/bin/sh\nexec {} \"$@\"\n",
        shell_quote(&appimage.display().to_string())
    );
    std::fs::write(&wrapper, script).ok()?;
    let mut perms = std::fs::metadata(&wrapper).ok()?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&wrapper, perms).ok()?;
    Some(wrapper)
}

#[cfg(target_os = "linux")]
fn install_sqlite_systemd_user_daemon(indexer: &Path) -> Result<String, String> {
    let unit_dir = systemd_user_unit_dir();
    std::fs::create_dir_all(&unit_dir)
        .map_err(|e| format!("Cannot create systemd unit dir: {}", e))?;
    let service_path = unit_dir.join("ottrin-index.service");

    prepare_sqlite_systemd_upgrade_with(
        service_path.is_file(),
        prepare_running_daemon_for_upgrade,
        run_sqlite_systemd,
    )?;

    let service = sqlite_daemon_systemd_unit(indexer);
    let temporary_path = unit_dir.join("ottrin-index.service.tmp");
    std::fs::write(&temporary_path, service)
        .map_err(|error| format!("Failed to write {}: {error}", temporary_path.display()))?;
    std::fs::rename(&temporary_path, &service_path)
        .map_err(|error| format!("Failed to install {}: {error}", service_path.display()))?;
    for args in [
        vec!["--user", "daemon-reload"],
        vec!["--user", "enable", "--now", "ottrin-index.service"],
    ] {
        let status = std::process::Command::new("systemctl")
            .args(&args)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("systemctl {:?} exited with {}", args, status));
        }
    }
    Ok("Persistent Ottrin index daemon enabled with a systemd user service.".to_string())
}

#[cfg(target_os = "linux")]
fn sqlite_daemon_systemd_unit(indexer: &Path) -> String {
    include_str!("../../../packaging/linux/ottrin-index.service.in")
        .replace("@OTTRIN_EXECUTABLE@", &systemd_exec_quote(indexer))
}

#[cfg(target_os = "linux")]
fn write_administrator_index_config(path: &Path, config: &SearchConfig) -> Result<(), String> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let mut protected = config.clone();
    protected.content_indexing_enabled = false;
    protected.content_include_roots.clear();
    protected.content_exclude_roots.clear();
    let bytes = serde_json::to_vec_pretty(&protected).map_err(|error| {
        format!("Could not serialize Administrator index configuration: {error}")
    })?;
    let parent = path
        .parent()
        .ok_or_else(|| "Administrator index configuration path has no parent".to_string())?;
    match std::fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => {
            return Err(format!(
                "Refusing non-directory Administrator configuration namespace at {}",
                parent.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!("Could not create protected Administrator configuration directory: {error}")
            })?;
        }
        Err(error) => {
            return Err(format!(
                "Could not inspect Administrator configuration directory: {error}"
            ));
        }
    }
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).map_err(|error| {
        format!("Could not protect Administrator configuration directory: {error}")
    })?;
    let temporary = parent.join(format!(".search.json.{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| {
            format!("Could not create protected Administrator configuration: {error}")
        })?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!(
            "Could not write protected Administrator configuration: {error}"
        ));
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!(
            "Could not publish protected Administrator configuration: {error}"
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
struct AdministratorIndexPaths {
    unit: PathBuf,
    config: PathBuf,
    state: PathBuf,
    runtime: PathBuf,
}

#[cfg(target_os = "linux")]
impl AdministratorIndexPaths {
    fn system() -> Self {
        Self {
            unit: PathBuf::from("/etc/systemd/system/ottrin-administrator-index.service"),
            config: PathBuf::from("/etc/ottrin-administrator/search.json"),
            state: PathBuf::from("/var/lib/ottrin-administrator"),
            runtime: PathBuf::from("/run/ottrin-administrator"),
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdministratorSystemdOperation {
    DaemonReload,
    EnableNow,
    IsEnabled,
    IsActive,
    Restart,
    DisableNow,
}

#[cfg(target_os = "linux")]
impl AdministratorSystemdOperation {
    fn args(self) -> &'static [&'static str] {
        match self {
            Self::DaemonReload => &["daemon-reload"],
            Self::EnableNow => &["enable", "--now", "ottrin-administrator-index.service"],
            Self::IsEnabled => &[
                "is-enabled",
                "--quiet",
                "ottrin-administrator-index.service",
            ],
            Self::IsActive => &["is-active", "--quiet", "ottrin-administrator-index.service"],
            Self::Restart => &["restart", "ottrin-administrator-index.service"],
            Self::DisableNow => &["disable", "--now", "ottrin-administrator-index.service"],
        }
    }
}

#[cfg(target_os = "linux")]
fn write_administrator_index_unit(path: &Path, contents: &str) -> Result<(), String> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    if let Ok(existing) = std::fs::read_to_string(path)
        && !existing.starts_with("# Managed by Ottrin\n")
    {
        return Err(format!(
            "Refusing to replace non-Ottrin service at {}",
            path.display()
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| "Service path has no parent".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create service directory: {error}"))?;
    let temporary = parent.join(format!(
        ".ottrin-administrator-index.{}.tmp",
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&temporary)
        .map_err(|error| format!("Could not create Administrator service unit: {error}"))?;
    if let Err(error) = file
        .write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
    {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!(
            "Could not write Administrator service unit: {error}"
        ));
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!(
            "Could not publish Administrator service unit: {error}"
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_administrator_index_service_with(
    paths: &AdministratorIndexPaths,
    indexer: &Path,
    config: &SearchConfig,
    mut run: impl FnMut(AdministratorSystemdOperation) -> Result<bool, String>,
) -> Result<(), String> {
    if config.include_roots.is_empty() {
        return Err(
            "Select at least one location before enabling the persistent Administrator index."
                .to_string(),
        );
    }
    write_administrator_index_config(&paths.config, config)?;
    write_administrator_index_unit(&paths.unit, &administrator_index_systemd_unit(indexer))?;
    for operation in [
        AdministratorSystemdOperation::DaemonReload,
        AdministratorSystemdOperation::EnableNow,
    ] {
        if !run(operation)? {
            return Err(format!("systemctl {:?} failed", operation.args()));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_administrator_systemd(operation: AdministratorSystemdOperation) -> Result<bool, String> {
    std::process::Command::new("systemctl")
        .args(operation.args())
        .status()
        .map(|status| status.success())
        .map_err(|error| format!("Failed to run systemctl {:?}: {error}", operation.args()))
}

#[cfg(target_os = "linux")]
fn require_administrator_process() -> Result<(), String> {
    // SAFETY: geteuid has no preconditions and no side effects.
    if unsafe { libc::geteuid() } == 0 {
        Ok(())
    } else {
        Err(
            "Persistent Administrator indexing can be managed only from an Administrator session."
                .to_string(),
        )
    }
}

#[cfg(target_os = "linux")]
fn find_dedicated_indexer() -> Option<PathBuf> {
    let current = std::env::current_exe().ok()?;
    if current.file_name().and_then(|name| name.to_str()) == Some("ottrin-indexd") {
        return Some(current);
    }
    current
        .parent()
        .map(|parent| parent.join("ottrin-indexd"))
        .filter(|candidate| candidate.is_file())
}

#[cfg(target_os = "linux")]
pub fn administrator_index_service_status() -> LocateScheduleStatus {
    let paths = AdministratorIndexPaths::system();
    if !paths.unit.is_file() {
        return LocateScheduleStatus {
            source: "none".to_string(),
            enabled: false,
            detail: "Persistent Administrator index service is not installed".to_string(),
        };
    }
    if !command_exists("systemctl") {
        return LocateScheduleStatus {
            source: "systemd system service".to_string(),
            enabled: false,
            detail: "Installed but systemctl is unavailable".to_string(),
        };
    }
    let enabled = run_administrator_systemd(AdministratorSystemdOperation::IsEnabled);
    let active = run_administrator_systemd(AdministratorSystemdOperation::IsActive);
    match (enabled, active) {
        (Ok(enabled), Ok(active)) => LocateScheduleStatus {
            source: "systemd system service".to_string(),
            enabled,
            detail: match (enabled, active) {
                (true, true) => "Enabled and running",
                (true, false) => "Enabled but stopped",
                (false, true) => "Running but disabled",
                (false, false) => "Installed but disabled and stopped",
            }
            .to_string(),
        },
        (Err(error), _) | (_, Err(error)) => LocateScheduleStatus {
            source: "systemd system service".to_string(),
            enabled: false,
            detail: format!("Status unavailable: {error}"),
        },
    }
}

#[cfg(not(target_os = "linux"))]
pub fn administrator_index_service_status() -> LocateScheduleStatus {
    LocateScheduleStatus {
        source: "none".to_string(),
        enabled: false,
        detail: "Persistent Administrator indexing is supported only on Linux".to_string(),
    }
}

#[cfg(target_os = "linux")]
pub fn administrator_index_database_bytes() -> u64 {
    std::fs::read_dir("/var/lib/ottrin-administrator")
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("search.db"))
        .filter_map(|entry| entry.metadata().ok())
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
        .sum()
}

#[cfg(not(target_os = "linux"))]
pub fn administrator_index_database_bytes() -> u64 {
    0
}

#[cfg(target_os = "linux")]
pub fn install_administrator_index_service(config: &SearchConfig) -> Result<String, String> {
    require_administrator_process()?;
    if !command_exists("systemctl") {
        return Err("Persistent Administrator indexing requires systemd.".to_string());
    }
    let indexer = find_dedicated_indexer().ok_or_else(|| {
        "Could not find the dedicated ottrin-indexd executable beside Ottrin.".to_string()
    })?;
    install_administrator_index_service_with(
        &AdministratorIndexPaths::system(),
        &indexer,
        config,
        run_administrator_systemd,
    )?;
    Ok("Persistent Administrator index enabled with an isolated system service.".to_string())
}

#[cfg(not(target_os = "linux"))]
pub fn install_administrator_index_service(_config: &SearchConfig) -> Result<String, String> {
    Err("Persistent Administrator indexing is supported only on Linux.".to_string())
}

#[cfg(target_os = "linux")]
pub fn restart_administrator_index_service() -> Result<String, String> {
    require_administrator_process()?;
    let paths = AdministratorIndexPaths::system();
    if !paths.unit.is_file() {
        return Err("Persistent Administrator index service is not installed.".to_string());
    }
    if !run_administrator_systemd(AdministratorSystemdOperation::Restart)? {
        return Err("systemctl failed to restart the Administrator index service.".to_string());
    }
    Ok("Persistent Administrator index service restarted.".to_string())
}

#[cfg(not(target_os = "linux"))]
pub fn restart_administrator_index_service() -> Result<String, String> {
    Err("Persistent Administrator indexing is supported only on Linux.".to_string())
}

#[cfg(target_os = "linux")]
fn remove_administrator_directory(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => std::fs::remove_dir_all(path)
            .map_err(|error| format!("Could not remove {}: {error}", path.display())),
        Ok(_) => Err(format!(
            "Refusing to purge non-directory Administrator state at {}",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not inspect {}: {error}", path.display())),
    }
}

#[cfg(target_os = "linux")]
fn uninstall_administrator_index_service_with(
    paths: &AdministratorIndexPaths,
    purge: bool,
    mut run: impl FnMut(AdministratorSystemdOperation) -> Result<bool, String>,
) -> Result<String, String> {
    if paths.unit.exists() {
        let unit = std::fs::read_to_string(&paths.unit)
            .map_err(|error| format!("Could not inspect Administrator service: {error}"))?;
        if !unit.starts_with("# Managed by Ottrin\n") {
            return Err("Refusing to remove a non-Ottrin Administrator service.".to_string());
        }
        if !run(AdministratorSystemdOperation::DisableNow)? {
            return Err(
                "systemctl failed to stop and disable the Administrator index service.".to_string(),
            );
        }
        std::fs::remove_file(&paths.unit)
            .map_err(|error| format!("Could not remove Administrator service unit: {error}"))?;
        if !run(AdministratorSystemdOperation::DaemonReload)? {
            return Err(
                "systemctl failed to reload units after Administrator service removal.".to_string(),
            );
        }
    }
    if purge {
        if let Some(config_directory) = paths.config.parent() {
            remove_administrator_directory(config_directory)?;
        }
        remove_administrator_directory(&paths.state)?;
        remove_administrator_directory(&paths.runtime)?;
    }
    Ok(if purge {
        "Persistent Administrator index service and its protected index data were removed."
            .to_string()
    } else {
        "Persistent Administrator index service removed; protected configuration and index data were retained."
            .to_string()
    })
}

#[cfg(target_os = "linux")]
pub fn uninstall_administrator_index_service(purge: bool) -> Result<String, String> {
    require_administrator_process()?;
    uninstall_administrator_index_service_with(
        &AdministratorIndexPaths::system(),
        purge,
        run_administrator_systemd,
    )
}

#[cfg(not(target_os = "linux"))]
pub fn uninstall_administrator_index_service(_purge: bool) -> Result<String, String> {
    Err("Persistent Administrator indexing is supported only on Linux.".to_string())
}

#[cfg(target_os = "linux")]
fn administrator_index_systemd_unit(indexer: &Path) -> String {
    format!(
        "# Managed by Ottrin\n[Unit]\nDescription=Ottrin persistent Administrator search index\n\n[Service]\nType=simple\nExecStart={} --administrator-index-daemon\nRestart=on-failure\nRestartSec=2s\nRuntimeDirectory=ottrin-administrator\nRuntimeDirectoryMode=0700\nStateDirectory=ottrin-administrator\nStateDirectoryMode=0700\nConfigurationDirectory=ottrin-administrator\nConfigurationDirectoryMode=0700\nUMask=0077\n\n[Install]\nWantedBy=multi-user.target\n",
        systemd_exec_quote(indexer),
    )
}

#[cfg(target_os = "linux")]
fn systemd_exec_quote(path: &Path) -> String {
    let value = path.display().to_string();
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    )
}

#[cfg(target_os = "windows")]
fn install_sqlite_windows_task(ottrin: &Path) -> Result<String, String> {
    let output = internal_command("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "{WINDOWS_INDEX_IDENTITY_SCRIPT}\n{}",
                include_str!("windows_index_task.ps1")
            ),
        ])
        .env("OTTRIN_TASK_EXECUTABLE", ottrin)
        .output()
        .map_err(|error| format!("Failed to configure the index task: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Index task setup failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let message = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if message.is_empty() {
        return Err("Index task setup returned no result".to_string());
    }
    Ok(message)
}

#[cfg(target_os = "linux")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Install a persistent schedule that keeps the locate database fresh.
///
/// Tries (in order):
///  1. systemd user timer  → `~/.config/systemd/user/ottrin-updatedb.{service,timer}`
///  2. crontab entry       → `(crontab -l; echo "...") | crontab -`
///
/// Returns a human-readable description of what was installed, or an error.
pub fn install_locate_schedule(interval_hours: u32, root: &Path) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let db = locate_db_path();
        let root_str = root.to_string_lossy();
        let db_str = db.to_string_lossy();

        // Try systemd --user first
        let has_systemd = std::process::Command::new("systemctl")
            .args(["--user", "is-system-running"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success() || s.code() == Some(1)) // degraded still works
            .unwrap_or(false);

        if has_systemd {
            return install_systemd_user_timer(interval_hours, &root_str, &db_str);
        }

        // Fall back to crontab
        install_crontab_entry(interval_hours, &root_str, &db_str)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (interval_hours, root);
        Err("Scheduled locate updates are only supported on Linux.".to_string())
    }
}

#[cfg(target_os = "linux")]
fn install_systemd_user_timer(interval_hours: u32, root: &str, db: &str) -> Result<String, String> {
    let unit_dir = systemd_user_unit_dir();
    std::fs::create_dir_all(&unit_dir)
        .map_err(|e| format!("Cannot create systemd unit dir: {}", e))?;

    let service = format!(
        "[Unit]\nDescription=Ottrin locate database update\n\n\
         [Service]\nType=oneshot\n\
         ExecStart=/usr/bin/updatedb -l 0 -U {root} -o {db}\n"
    );
    let timer = format!(
        "[Unit]\nDescription=Ottrin locate database update timer\n\n\
         [Timer]\nOnBootSec=2min\nOnUnitActiveSec={interval}h\n\n\
         [Install]\nWantedBy=timers.target\n",
        interval = interval_hours
    );

    std::fs::write(unit_dir.join("ottrin-updatedb.service"), service).map_err(|e| e.to_string())?;
    std::fs::write(unit_dir.join("ottrin-updatedb.timer"), timer).map_err(|e| e.to_string())?;

    // Reload and enable
    for args in [
        vec!["--user", "daemon-reload"],
        vec!["--user", "enable", "--now", "ottrin-updatedb.timer"],
    ] {
        let _ = std::process::Command::new("systemctl").args(&args).status();
    }

    Ok(format!(
        "Systemd user timer installed.\nUpdatedb will run every {}h and on boot.\n\
         Unit files: ~/.config/systemd/user/ottrin-updatedb.{{service,timer}}",
        interval_hours
    ))
}

#[cfg(target_os = "linux")]
fn install_crontab_entry(interval_hours: u32, root: &str, db: &str) -> Result<String, String> {
    // Read existing crontab
    let existing = std::process::Command::new("crontab")
        .arg("-l")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    let tag = "# ottrin-updatedb";
    if existing.contains(tag) {
        return Ok("Crontab entry already installed.".to_string());
    }

    let entry = format!(
        "{tag}\n0 */{interval} * * * updatedb -l 0 -U {root} -o {db}\n",
        tag = tag,
        interval = interval_hours,
        root = root,
        db = db
    );
    let new_crontab = format!("{}\n{}", existing.trim_end(), entry);

    let mut child = std::process::Command::new("crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("crontab not available: {}", e))?;
    if let Some(stdin) = child.stdin.take() {
        use std::io::Write;
        let mut stdin = stdin;
        let _ = stdin.write_all(new_crontab.as_bytes());
    }
    child.wait().map_err(|e| e.to_string())?;

    Ok(format!(
        "Crontab entry installed.\nUpdatedb will run every {}h.",
        interval_hours
    ))
}

/// Query the locate database — prefers Ottrin's own DB, falls back to
/// plocate/locate if no managed DB exists yet.
fn query_system_locate(
    text: &str,
    scope_root: Option<&Path>,
    include_hidden: bool,
    limit: usize,
) -> Vec<SearchResultItem> {
    let our_db = locate_db_path();

    // Build the command: our own DB takes priority so results are always fresh.
    let output = if our_db.exists() && command_exists("locate") {
        std::process::Command::new("locate")
            .arg("-d")
            .arg(&our_db)
            .args(["-i", text])
            .output()
    } else if command_exists("plocate") {
        std::process::Command::new("plocate")
            .args(["-i", "--limit", &(limit * 4).to_string(), text])
            .output()
    } else if command_exists("locate") {
        std::process::Command::new("locate")
            .args(["-i", text])
            .output()
    } else {
        return Vec::new();
    };

    let output = match output {
        Ok(o) if !o.stdout.is_empty() => o,
        _ => return Vec::new(),
    };

    parse_system_locate_output(&output.stdout, scope_root, include_hidden, limit)
        .into_iter()
        .filter_map(|path| make_system_locate_item(&path))
        .take(limit)
        .collect()
}

fn parse_system_locate_output(
    output: &[u8],
    scope_root: Option<&Path>,
    include_hidden: bool,
    limit: usize,
) -> Vec<PathBuf> {
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| scope_root.is_none_or(|root| path.starts_with(root)))
        .filter(|path| {
            include_hidden
                || path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| !name.starts_with('.'))
        })
        .take(limit)
        .collect()
}

fn make_system_locate_item(path: &Path) -> Option<SearchResultItem> {
    make_system_locate_item_with(path, |path| std::fs::symlink_metadata(path))
}

fn make_system_locate_item_with(
    path: &Path,
    metadata: impl FnOnce(&Path) -> std::io::Result<std::fs::Metadata>,
) -> Option<SearchResultItem> {
    match metadata(path) {
        Ok(metadata) => make_item_from_metadata(path, metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => make_path_only_item(path),
    }
}

fn make_path_only_item(path: &Path) -> Option<SearchResultItem> {
    let mut item = SearchResultItem {
        path: path.to_path_buf(),
        parent_path: path.parent().map(Path::to_path_buf).unwrap_or_default(),
        name: path.file_name()?.to_string_lossy().into_owned(),
        kind: EntryKind::Other,
        is_executable: false,
        symlink_target_is_dir: None,
        size_bytes: None,
        modified_unix_secs: None,
        content_snippet: None,
        name_lower: String::new(),
        path_str: String::new(),
        path_lower: String::new(),
    };
    item.prepare();
    Some(item)
}

// ── Incremental index updates ──────────────────────────────────────────────────

fn make_item_from_metadata(path: &Path, meta: std::fs::Metadata) -> Option<SearchResultItem> {
    let file_type = meta.file_type();
    let kind = if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    let name = path.file_name()?.to_string_lossy().to_string();
    let size = if matches!(kind, EntryKind::File) {
        Some(meta.len())
    } else {
        None
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let mut item = SearchResultItem {
        path: path.to_path_buf(),
        parent_path: path.parent().map(Path::to_path_buf).unwrap_or_default(),
        name,
        kind,
        is_executable: detect_executable(&meta),
        symlink_target_is_dir: if matches!(kind, EntryKind::Symlink) {
            Some(meta.is_dir())
        } else {
            None
        },
        size_bytes: size,
        modified_unix_secs: modified,
        content_snippet: None,
        name_lower: String::new(),
        path_str: String::new(),
        path_lower: String::new(),
    };
    item.prepare();
    Some(item)
}

fn build_globset(patterns: &[String]) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        if p.trim().is_empty() {
            continue;
        }
        let g = Glob::new(p).map_err(|e| format!("Invalid glob '{}': {}", p, e))?;
        builder.add(g);
    }
    builder.build().map_err(|e| e.to_string())
}

fn scope_match(query: &SearchQuery, item: &SearchResultItem) -> bool {
    match query.scope {
        SearchScope::Global => true,
        SearchScope::CurrentFolder => {
            if let Some(root) = &query.root_path {
                item.path.starts_with(root)
            } else {
                false
            }
        }
    }
}

fn query_current_folder_fallback(
    query: &SearchQuery,
    expr: &Expr,
    cfg: &SearchConfig,
) -> Vec<(i64, SearchResultItem)> {
    let Some(root) = &query.root_path else {
        return Vec::new();
    };
    if !root.exists() {
        return Vec::new();
    }

    let include = build_globset(&cfg.include_globs).unwrap_or_else(|_| empty_globset());
    let exclude = build_globset(&cfg.exclude_globs).unwrap_or_else(|_| empty_globset());

    let mut out = Vec::new();
    let mut visited = 0usize;
    let max_visited = 40_000usize;
    let target_hits = query.limit.saturating_mul(4).max(200);
    let started = Instant::now();
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            should_keep_path(
                entry.path(),
                root,
                query.include_hidden_system,
                &include,
                &exclude,
                &cfg.exclude_roots,
            )
        });
    for entry in walker.filter_map(Result::ok) {
        if visited >= max_visited
            || out.len() >= target_hits
            || started.elapsed() >= Duration::from_millis(120)
        {
            break;
        }
        let path = entry.path();
        if path == root {
            continue;
        }
        visited = visited.saturating_add(1);
        let name = entry.file_name().to_string_lossy().to_string();
        if !query.include_hidden_system && name.starts_with('.') {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let file_type = entry.file_type();
        let kind = if file_type.is_symlink() {
            EntryKind::Symlink
        } else if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        let size = if matches!(kind, EntryKind::File) {
            Some(meta.len())
        } else {
            None
        };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let mut item = SearchResultItem {
            path: path.to_path_buf(),
            parent_path: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            name,
            kind,
            is_executable: detect_executable(&meta),
            symlink_target_is_dir: if matches!(kind, EntryKind::Symlink) {
                Some(meta.is_dir())
            } else {
                None
            },
            size_bytes: size,
            modified_unix_secs: modified,
            content_snippet: None,
            name_lower: String::new(),
            path_str: String::new(),
            path_lower: String::new(),
        };
        item.prepare();
        if eval_expr(expr, &item) {
            out.push((relevance_score(query, &item), item));
        }
    }
    out
}

fn query_global_fallback(
    query: &SearchQuery,
    expr: &Expr,
    roots: &[PathBuf],
    cfg: &SearchConfig,
    max_visited: usize,
    target_hits: usize,
    max_time: Duration,
) -> Vec<(i64, SearchResultItem)> {
    let mut visited = 0usize;
    let mut out = Vec::new();
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    let started = Instant::now();
    let include = build_globset(&cfg.include_globs).unwrap_or_else(|_| empty_globset());
    let exclude = build_globset(&cfg.exclude_globs).unwrap_or_else(|_| empty_globset());
    // Pass 1: shallow scan to surface likely user-visible hits quickly.
    query_global_fallback_pass(
        roots,
        query,
        expr,
        cfg,
        &include,
        &exclude,
        Some(3),
        max_visited,
        target_hits,
        max_time,
        started,
        &mut visited,
        &mut out,
        &mut seen_paths,
    );
    // Pass 2: deeper scan with remaining budget.
    if visited < max_visited && out.len() < target_hits && started.elapsed() < max_time {
        query_global_fallback_pass(
            roots,
            query,
            expr,
            cfg,
            &include,
            &exclude,
            None,
            max_visited,
            target_hits,
            max_time,
            started,
            &mut visited,
            &mut out,
            &mut seen_paths,
        );
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn query_global_fallback_pass(
    roots: &[PathBuf],
    query: &SearchQuery,
    expr: &Expr,
    cfg: &SearchConfig,
    include: &GlobSet,
    exclude: &GlobSet,
    max_depth: Option<usize>,
    max_visited: usize,
    target_hits: usize,
    max_time: Duration,
    started: Instant,
    visited: &mut usize,
    out: &mut Vec<(i64, SearchResultItem)>,
    seen_paths: &mut HashSet<PathBuf>,
) {
    for root in roots {
        if !root.exists() {
            continue;
        }
        let mut builder = WalkDir::new(root).follow_links(false);
        if let Some(depth) = max_depth {
            builder = builder.max_depth(depth);
        }
        let walker = builder.into_iter().filter_entry(|entry| {
            should_keep_path(
                entry.path(),
                root,
                query.include_hidden_system,
                include,
                exclude,
                &cfg.exclude_roots,
            )
        });
        for entry in walker.filter_map(Result::ok) {
            if *visited >= max_visited || out.len() >= target_hits || started.elapsed() >= max_time
            {
                return;
            }
            let path = entry.path();
            if path == root {
                continue;
            }
            *visited += 1;

            let name = entry.file_name().to_string_lossy().to_string();
            if !query.include_hidden_system && name.starts_with('.') {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let file_type = entry.file_type();
            let kind = if file_type.is_symlink() {
                EntryKind::Symlink
            } else if file_type.is_dir() {
                EntryKind::Directory
            } else if file_type.is_file() {
                EntryKind::File
            } else {
                EntryKind::Other
            };
            let size = if matches!(kind, EntryKind::File) {
                Some(meta.len())
            } else {
                None
            };
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            let mut item = SearchResultItem {
                path: path.to_path_buf(),
                parent_path: path.parent().map(Path::to_path_buf).unwrap_or_default(),
                name,
                kind,
                is_executable: detect_executable(&meta),
                symlink_target_is_dir: if matches!(kind, EntryKind::Symlink) {
                    Some(meta.is_dir())
                } else {
                    None
                },
                size_bytes: size,
                modified_unix_secs: modified,
                content_snippet: None,
                name_lower: String::new(),
                path_str: String::new(),
                path_lower: String::new(),
            };
            item.prepare();
            if eval_expr(expr, &item) && seen_paths.insert(item.path.clone()) {
                out.push((relevance_score(query, &item), item));
            }
        }
    }
}

fn format_progress(
    root_idx: usize,
    roots_total: usize,
    indexed: usize,
    _root_seen: usize,
) -> String {
    // Show a clear count-based progress instead of a misleading percentage.
    // We can't know total files ahead of time, so show indexed count + root progress.
    if roots_total <= 1 {
        format!("Indexing · {} files found", indexed)
    } else {
        format!(
            "Indexing · root {}/{} · {} files found",
            root_idx, roots_total, indexed
        )
    }
}

fn detect_executable(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

fn empty_globset() -> GlobSet {
    GlobSetBuilder::new()
        .build()
        .expect("building empty globset should not fail")
}

fn root_is_indexed(root: &Path, cfg: &SearchConfig) -> bool {
    effective_roots(cfg)
        .into_iter()
        .any(|indexed_root| root.starts_with(indexed_root))
}

fn should_keep_path(
    path: &Path,
    root: &Path,
    include_hidden_system: bool,
    include: &GlobSet,
    exclude: &GlobSet,
    excluded_roots: &[PathBuf],
) -> bool {
    if path == root {
        return true;
    }
    if excluded_roots.iter().any(|r| path.starts_with(r)) {
        return false;
    }
    if !include_hidden_system && path_has_hidden_component(path, root) {
        return false;
    }
    if path_has_builtin_excluded_component(path, root) {
        return false;
    }
    let rel = normalized_rel_path(path, root);
    !exclude.is_match(rel.as_str()) || include.is_match(rel.as_str())
}

fn path_has_builtin_excluded_component(path: &Path, root: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(
            name.as_ref(),
            ".git" | ".svn" | ".hg" | "node_modules" | "target" | "__pycache__" | ".Trash"
        ) || name.starts_with(".Trash-")
            || name.eq_ignore_ascii_case("$Recycle.Bin")
            || name.eq_ignore_ascii_case("System Volume Information")
    })
}

fn normalized_rel_path(path: &Path, root: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let mut s = rel.to_string_lossy().into_owned();
    if std::path::MAIN_SEPARATOR != '/' {
        s = s.replace('\\', "/");
    }
    s
}

fn path_has_hidden_component(path: &Path, root: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    rel.components().any(|c| {
        c.as_os_str()
            .to_str()
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
    })
}

#[derive(Debug, Clone)]
enum Expr {
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
    Term(Term),
}

#[derive(Debug, Clone)]
enum Term {
    Contains { field: Field, value: String },
    Regex { field: Field, regex: Regex },
    TypeIs(String),
}

#[derive(Debug, Clone, Copy)]
enum Field {
    Any,
    Name,
    Path,
    Ext,
}

fn parse_query(input: &str) -> Result<Expr, SearchError> {
    let tokens = tokenize(input)?;
    let mut p = Parser { tokens, pos: 0 };
    let expr = p.parse_expr()?;
    if p.peek().is_some() {
        return Err(SearchError::Parse("Unexpected trailing token".to_string()));
    }
    Ok(expr)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    LParen,
    RParen,
    Or,
    Not,
    Word(String),
}

fn tokenize(s: &str) -> Result<Vec<Tok>, SearchError> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '|' => {
                out.push(Tok::Or);
                i += 1;
            }
            '!' => {
                out.push(Tok::Not);
                i += 1;
            }
            '"' => {
                i += 1;
                let start = i;
                while i < chars.len() && chars[i] != '"' {
                    i += 1;
                }
                if i >= chars.len() {
                    return Err(SearchError::Parse("Unclosed quote".to_string()));
                }
                out.push(Tok::Word(chars[start..i].iter().collect()));
                i += 1;
            }
            '/' => {
                let start = i;
                i += 1;
                while i < chars.len() && chars[i] != '/' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i >= chars.len() {
                    return Err(SearchError::Parse("Unclosed regex literal".to_string()));
                }
                i += 1;
                out.push(Tok::Word(chars[start..i].iter().collect()));
            }
            _ => {
                let start = i;
                while i < chars.len()
                    && !chars[i].is_whitespace()
                    && !['(', ')', '|', '!'].contains(&chars[i])
                {
                    i += 1;
                }
                out.push(Tok::Word(chars[start..i].iter().collect()));
            }
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn parse_expr(&mut self) -> Result<Expr, SearchError> {
        self.parse_or()
    }
    fn parse_or(&mut self) -> Result<Expr, SearchError> {
        let mut parts = vec![self.parse_and()?];
        while self.peek() == Some(&Tok::Or) {
            self.pos += 1;
            parts.push(self.parse_and()?);
        }
        if parts.len() == 1 {
            Ok(parts.remove(0))
        } else {
            Ok(Expr::Or(parts))
        }
    }
    fn parse_and(&mut self) -> Result<Expr, SearchError> {
        let mut parts = Vec::new();
        while let Some(tok) = self.peek() {
            if tok == &Tok::RParen || tok == &Tok::Or {
                break;
            }
            parts.push(self.parse_unary()?);
        }
        if parts.is_empty() {
            Err(SearchError::Parse("Expected expression".to_string()))
        } else if parts.len() == 1 {
            Ok(parts.remove(0))
        } else {
            Ok(Expr::And(parts))
        }
    }
    fn parse_unary(&mut self) -> Result<Expr, SearchError> {
        if self.peek() == Some(&Tok::Not) {
            self.pos += 1;
            Ok(Expr::Not(Box::new(self.parse_unary()?)))
        } else {
            self.parse_primary()
        }
    }
    fn parse_primary(&mut self) -> Result<Expr, SearchError> {
        match self.next() {
            Some(Tok::LParen) => {
                let e = self.parse_expr()?;
                match self.next() {
                    Some(Tok::RParen) => Ok(e),
                    _ => Err(SearchError::Parse("Missing ')'".to_string())),
                }
            }
            Some(Tok::Word(w)) => Ok(Expr::Term(parse_term(&w)?)),
            _ => Err(SearchError::Parse("Unexpected token".to_string())),
        }
    }
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }
}

fn parse_term(raw: &str) -> Result<Term, SearchError> {
    if raw.starts_with('/') && raw.ends_with('/') && raw.len() >= 2 {
        let body = &raw[1..raw.len() - 1];
        let re = RegexBuilder::new(body)
            .case_insensitive(true)
            .build()
            .map_err(|e| SearchError::Parse(format!("Invalid regex: {}", e)))?;
        return Ok(Term::Regex {
            field: Field::Any,
            regex: re,
        });
    }
    if let Some((k, v)) = raw.split_once(':') {
        match k.to_ascii_lowercase().as_str() {
            "name" => {
                return Ok(Term::Contains {
                    field: Field::Name,
                    value: v.to_ascii_lowercase(),
                });
            }
            "path" => {
                return Ok(Term::Contains {
                    field: Field::Path,
                    value: v.to_ascii_lowercase(),
                });
            }
            "ext" => {
                return Ok(Term::Contains {
                    field: Field::Ext,
                    value: v.to_ascii_lowercase(),
                });
            }
            "type" => return Ok(Term::TypeIs(v.to_ascii_lowercase())),
            _ => {}
        }
    }
    Ok(Term::Contains {
        field: Field::Any,
        value: raw.to_ascii_lowercase(),
    })
}

fn eval_expr(expr: &Expr, item: &SearchResultItem) -> bool {
    match expr {
        Expr::And(v) => v.iter().all(|e| eval_expr(e, item)),
        Expr::Or(v) => v.iter().any(|e| eval_expr(e, item)),
        Expr::Not(e) => !eval_expr(e, item),
        Expr::Term(t) => eval_term(t, item),
    }
}

fn ascii_contains_ignore_case(haystack: &str, needle_lower: &str) -> bool {
    let haystack = haystack.as_bytes();
    let needle = needle_lower.as_bytes();
    needle.is_empty()
        || haystack
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle))
}

fn cached_contains_ignore_case(primary: &str, lowercase_cache: &str, needle_lower: &str) -> bool {
    if lowercase_cache.is_empty() {
        ascii_contains_ignore_case(primary, needle_lower)
    } else {
        lowercase_cache.contains(needle_lower)
    }
}

fn eval_term(t: &Term, item: &SearchResultItem) -> bool {
    let path = if item.path_str.is_empty() {
        item.path.to_string_lossy()
    } else {
        std::borrow::Cow::Borrowed(item.path_str.as_str())
    };
    let name_contains =
        |value: &str| cached_contains_ignore_case(&item.name, &item.name_lower, value);
    let path_contains =
        |value: &str| cached_contains_ignore_case(path.as_ref(), &item.path_lower, value);
    match t {
        Term::Contains { field, value } => match field {
            // Substring match on name OR path — grep-like behavior.
            // "cat" matches "angelcat.pdf", "cat_photo.jpg", "/cats/file.txt".
            // Multi-term AND queries ("angel cat pdf") match all terms independently.
            Field::Any => {
                name_contains(value.as_str())
                    || path_contains(value.as_str())
                    || item
                        .content_snippet
                        .as_ref()
                        .map(|snippet| ascii_contains_ignore_case(snippet, value.as_str()))
                        .unwrap_or(false)
            }
            Field::Name => name_contains(value.as_str()),
            Field::Path => path_contains(value.as_str()),
            Field::Ext => item
                .path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case(value))
                .unwrap_or(false),
        },
        Term::Regex { field, regex } => match field {
            Field::Any => {
                regex.is_match(&item.name)
                    || regex.is_match(path.as_ref())
                    || item
                        .content_snippet
                        .as_ref()
                        .map(|snippet| regex.is_match(snippet))
                        .unwrap_or(false)
            }
            Field::Name => regex.is_match(&item.name),
            Field::Path => regex.is_match(path.as_ref()),
            Field::Ext => item
                .path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| regex.is_match(e))
                .unwrap_or(false),
        },
        Term::TypeIs(v) => match item.kind {
            EntryKind::File => v == "file",
            EntryKind::Directory => v == "dir" || v == "folder" || v == "directory",
            EntryKind::Symlink => v == "symlink" || v == "link",
            EntryKind::Other => v == "other",
        },
    }
}

/// Extract the unqualified literal terms that may match indexed file content.
/// Field filters still run against each returned item, but must not become FTS
/// terms themselves (for example, `name:notes needle` searches file content for
/// `needle` and then applies the name filter).
fn fts_content_match(expr: &Expr) -> Option<String> {
    fn quote(value: &str) -> String {
        format!("\"{}\"", value.replace('"', "\"\""))
    }

    fn build(expr: &Expr) -> Option<String> {
        match expr {
            Expr::And(parts) | Expr::Or(parts) => {
                let operator = if matches!(expr, Expr::And(_)) {
                    " AND "
                } else {
                    " OR "
                };
                let parts = parts.iter().filter_map(build).collect::<Vec<_>>();
                match parts.len() {
                    0 => None,
                    1 => parts.into_iter().next(),
                    _ => Some(format!("({})", parts.join(operator))),
                }
            }
            Expr::Term(Term::Contains {
                field: Field::Any,
                value,
            }) if value.len() >= 2 => Some(quote(value)),
            // A negated content term cannot be used to obtain a safe positive FTS
            // candidate set. The ordinary indexed/fallback paths remain available.
            Expr::Not(_) | Expr::Term(_) => None,
        }
    }
    build(expr)
}

/// Only negated terms that can inspect file content need the complete indexed
/// snapshot. Negated filename, path, extension, and type filters retain the
/// ordinary content-search path (including its established fallback behavior).
fn expr_contains_negated_content(expr: &Expr) -> bool {
    expr_has_negated_content(expr, false)
}

fn expr_has_negated_content(expr: &Expr, negated: bool) -> bool {
    match expr {
        Expr::And(parts) | Expr::Or(parts) => parts
            .iter()
            .any(|part| expr_has_negated_content(part, negated)),
        Expr::Not(inner) => expr_has_negated_content(inner, !negated),
        Expr::Term(Term::Contains {
            field: Field::Any, ..
        })
        | Expr::Term(Term::Regex {
            field: Field::Any, ..
        }) => negated,
        Expr::Term(_) => false,
    }
}

fn relevance_score(query: &SearchQuery, item: &SearchResultItem) -> i64 {
    let q = query.text.to_ascii_lowercase();
    let name_is_exact = if item.name_lower.is_empty() {
        item.name.eq_ignore_ascii_case(&q)
    } else {
        item.name_lower == q
    };
    let name_starts = if item.name_lower.is_empty() {
        item.name
            .get(..q.len())
            .map(|prefix| prefix.eq_ignore_ascii_case(&q))
            .unwrap_or(false)
    } else {
        item.name_lower.starts_with(&q)
    };
    let path = if item.path_str.is_empty() {
        item.path.to_string_lossy()
    } else {
        std::borrow::Cow::Borrowed(item.path_str.as_str())
    };
    if name_is_exact {
        10_000
    } else if name_starts {
        7_500
    } else if cached_contains_ignore_case(&item.name, &item.name_lower, &q) {
        5_000
    } else if cached_contains_ignore_case(path.as_ref(), &item.path_lower, &q) {
        2_500
    } else {
        0
    }
}

/// Fuzzy subsequence score: returns Some(score) if every character of `query` appears
/// in `text` in order (case-insensitive). Scores are in the 100–999 range, below all
/// exact-match scores so fuzzy results sort after precise matches.
fn fuzzy_subsequence_score(query: &str, text: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(100);
    }
    let q: Vec<char> = query.chars().map(|c| c.to_ascii_lowercase()).collect();
    let t: Vec<char> = text.chars().map(|c| c.to_ascii_lowercase()).collect();
    let mut qi = 0usize;
    let mut first_match: Option<usize> = None;
    let mut last_match = 0usize;
    for (ti, &tc) in t.iter().enumerate() {
        if qi < q.len() && tc == q[qi] {
            first_match.get_or_insert(ti);
            last_match = ti;
            qi += 1;
        }
    }
    if qi < q.len() {
        return None; // Not all query chars found in text
    }
    let first = first_match.unwrap_or(0);
    let span = (last_match - first + 1) as i64;
    let q_len = q.len() as i64;
    // Compact matches score higher; start-of-string bonus
    let compactness = (q_len * 200) / span;
    let start_bonus: i64 = if first == 0 {
        300
    } else if first < 3 {
        150
    } else {
        0
    };
    Some((compactness + start_bonus).clamp(100, 999))
}

/// Returns true when the query is a single bare word (no operators, no field prefixes).
/// Used to decide whether to run a fuzzy supplemental pass.
fn is_simple_single_word(text: &str) -> bool {
    let t = text.trim();
    t.len() >= 2
        && !t.contains(' ')
        && !t.contains('(')
        && !t.contains('|')
        && !t.contains(':')
        && !t.starts_with('!')
        && !t.starts_with('/')
        && !t.starts_with('"')
}

/// A SQLite simple-term page is authoritative for name/path matches, even when
/// it contains fewer rows than the requested limit. Falling back in that case
/// streams the whole persisted generation solely for optional fuzzy matches.
fn should_use_simple_term_page(_total: usize, _limit: usize, content_search: bool) -> bool {
    !content_search
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContentRipgrepTraversal {
    root: PathBuf,
    exclude_globs: Vec<String>,
}

/// Return ripgrep working directories that cannot traverse outside the content
/// policy. Excluded children become ripgrep ignore globs relative to their
/// working directory; a path that cannot be represented as a glob is omitted
/// rather than searched.
fn content_ripgrep_traversal_plan(
    query_roots: &[PathBuf],
    config: &SearchConfig,
) -> Vec<ContentRipgrepTraversal> {
    if !config.content_indexing_enabled || config.content_file_groups.is_empty() {
        return Vec::new();
    }

    let mut plan = Vec::new();
    for query_root in query_roots {
        for content_root in &config.content_include_roots {
            let root = if content_root.starts_with(query_root) {
                content_root.clone()
            } else if query_root.starts_with(content_root) {
                query_root.clone()
            } else {
                continue;
            };
            if config
                .content_exclude_roots
                .iter()
                .any(|excluded| root.starts_with(excluded))
            {
                continue;
            }

            let mut exclude_globs = Vec::new();
            let mut representable = true;
            for excluded in config
                .content_exclude_roots
                .iter()
                .filter(|excluded| excluded.starts_with(&root))
            {
                let Some(relative) = excluded.strip_prefix(&root).ok() else {
                    continue;
                };
                let Some(globs) = relative_path_to_ripgrep_globs(relative) else {
                    representable = false;
                    break;
                };
                exclude_globs.extend(globs);
            }
            if !representable {
                continue;
            }
            exclude_globs.sort();
            exclude_globs.dedup();
            plan.push(ContentRipgrepTraversal {
                root,
                exclude_globs,
            });
        }
    }
    plan.sort_by(|left, right| left.root.cmp(&right.root));
    plan.dedup_by(|left, right| left.root == right.root);
    plan
}

fn relative_path_to_ripgrep_globs(path: &Path) -> Option<[String; 2]> {
    let mut components = Vec::new();
    for component in path.components() {
        let std::path::Component::Normal(component) = component else {
            return None;
        };
        let component = component.to_str()?;
        let escaped = component
            .chars()
            .flat_map(|character| match character {
                '\\' | '*' | '?' | '[' | ']' | '{' | '}' | '!' => ['\\', character],
                _ => ['\0', character],
            })
            .filter(|character| *character != '\0')
            .collect::<String>();
        components.push(escaped);
    }
    let path = components.join("/");
    (!path.is_empty()).then(|| [format!("!{path}"), format!("!{path}/**")])
}

/// Run ripgrep to find files whose *contents* match positive parsed content literals.
/// Returns items with `content_snippet` populated (first matching line, trimmed).
/// Regex-style queries are intentionally left unchanged here.
fn query_content_ripgrep(
    expr: &Expr,
    roots: &[PathBuf],
    config: &SearchConfig,
    include_hidden: bool,
    limit: usize,
    program: &std::ffi::OsStr,
) -> Result<Vec<SearchResultItem>, String> {
    if roots.is_empty() {
        return Ok(Vec::new());
    }

    let mut terms = Vec::new();
    let mut seen_terms = HashSet::new();
    collect_positive_content_literals(expr, false, &mut terms, &mut seen_terms);
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let traversal_plan = content_ripgrep_traversal_plan(roots, config);
    if traversal_plan.is_empty() {
        return Ok(Vec::new());
    }

    let mut matching_paths: Option<HashSet<PathBuf>> = None;
    for term in &terms {
        let mut term_paths = HashSet::new();
        for traversal in &traversal_plan {
            let root = std::fs::metadata(&traversal.root).map_err(|error| format!("Content folder '{}' is unavailable: {error}. Reconnect it or change the folder in Settings → Search.", traversal.root.display()))?;
            if !root.is_dir() {
                return Err(format!(
                    "Content folder '{}' is not a directory. Change it in Settings → Search.",
                    traversal.root.display()
                ));
            }
            let mut cmd = internal_command(program);
            cmd.current_dir(&traversal.root)
                .arg("--no-config")
                .arg("--files-with-matches")
                .arg("--null")
                .arg("--fixed-strings")
                .arg("--ignore-case")
                .arg("--max-count=1")
                .arg("--max-filesize")
                .arg(config.content_max_file_size_bytes.to_string());
            if include_hidden {
                cmd.arg("--hidden");
            }
            for extension in config
                .content_file_groups
                .iter()
                .flat_map(|group| group.extensions())
            {
                cmd.arg("--glob").arg(format!("*.{extension}"));
            }
            // Explicit exclusions take priority over positive type globs.
            for glob in &traversal.exclude_globs {
                cmd.arg("--glob").arg(glob);
            }
            cmd.arg("-e").arg(term).arg("--").arg(".");
            let file_output = cmd.output().map_err(|error| {
                format!("Live content search could not start: {error}. Check access to the selected folders or repair Ottrin.")
            })?;
            if !file_output.status.success() && file_output.status.code() != Some(1) {
                let detail = String::from_utf8_lossy(&file_output.stderr)
                    .trim()
                    .chars()
                    .take(300)
                    .collect::<String>();
                return Err(format!(
                    "Live content search could not finish ({}): {detail}. Check access to the selected folders or repair Ottrin.",
                    file_output.status
                ));
            }
            if (file_output.status.success() && file_output.stdout.is_empty())
                || (!file_output.stdout.is_empty() && file_output.stdout.last() != Some(&0))
            {
                return Err(
                    "Live content search returned invalid output. Repair or reinstall Ottrin."
                        .into(),
                );
            }
            for bytes in file_output
                .stdout
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
            {
                #[cfg(unix)]
                let relative = {
                    use std::os::unix::ffi::OsStrExt;
                    Path::new(std::ffi::OsStr::from_bytes(bytes))
                };
                #[cfg(not(unix))]
                let relative = Path::new(std::str::from_utf8(bytes).map_err(|_| {
                    "Live content search returned an invalid filename. Repair or reinstall Ottrin.".to_string()
                })?);
                if relative.components().any(|part| {
                    matches!(
                        part,
                        std::path::Component::Prefix(_)
                            | std::path::Component::RootDir
                            | std::path::Component::ParentDir
                    )
                }) {
                    return Err("Live content search returned a path outside the selected folder. Repair or reinstall Ottrin.".into());
                }
                let path = traversal.root.join(relative);
                if content_indexing_allows_path(config, &path) {
                    term_paths.insert(path);
                }
            }
        }
        if term_paths.is_empty() {
            return Ok(Vec::new());
        }
        matching_paths = Some(match matching_paths.take() {
            Some(existing) => existing.intersection(&term_paths).cloned().collect(),
            None => term_paths,
        });
        if matching_paths.as_ref().is_none_or(|set| set.is_empty()) {
            return Ok(Vec::new());
        }
    }

    let mut matching_paths: Vec<PathBuf> = matching_paths.unwrap_or_default().into_iter().collect();
    matching_paths.sort();
    let mut items: Vec<SearchResultItem> = matching_paths
        .into_iter()
        .filter_map(|path| {
            let meta = std::fs::metadata(&path).ok()?;
            if meta.len() > config.content_max_file_size_bytes {
                return None;
            }
            let mut content = Vec::new();
            File::open(&path)
                .ok()?
                .take(config.content_max_file_size_bytes.saturating_add(1))
                .read_to_end(&mut content)
                .ok()?;
            if content.len() as u64 > config.content_max_file_size_bytes {
                return None;
            }
            let content = String::from_utf8_lossy(&content).into_owned();
            let name = path.file_name()?.to_string_lossy().into_owned();
            let parent = path.parent().unwrap_or(Path::new("")).to_path_buf();
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            let mut item = SearchResultItem {
                path: path.clone(),
                parent_path: parent,
                name,
                kind: EntryKind::File,
                is_executable: false,
                symlink_target_is_dir: None,
                size_bytes: Some(meta.len()),
                modified_unix_secs: modified,
                content_snippet: None,
                name_lower: String::new(),
                path_str: String::new(),
                path_lower: String::new(),
            };
            item.prepare();
            item.content_snippet = Some(content.clone());
            if !eval_expr(expr, &item) {
                return None;
            }
            let snippet = content
                .lines()
                .find(|line| {
                    let line_l = line.to_ascii_lowercase();
                    terms.iter().all(|term| line_l.contains(term))
                })
                .or_else(|| {
                    content.lines().find(|line| {
                        let line_l = line.to_ascii_lowercase();
                        terms.iter().any(|term| line_l.contains(term))
                    })
                })
                .map(|line| line.trim().to_string())
                .or_else(|| {
                    content
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .map(|line| line.trim().to_string())
                });
            item.content_snippet = snippet;
            Some(item)
        })
        .collect();
    items.truncate(limit);
    Ok(items)
}

fn content_indexing_allows_path(config: &SearchConfig, path: &Path) -> bool {
    config.content_indexing_enabled
        && config
            .content_include_roots
            .iter()
            .any(|root| path.starts_with(root))
        && !config
            .content_exclude_roots
            .iter()
            .any(|root| path.starts_with(root))
        && config
            .content_file_groups
            .iter()
            .any(|group| group.supports_path(path))
}

fn collect_positive_content_literals(
    expr: &Expr,
    negated: bool,
    terms: &mut Vec<String>,
    seen_terms: &mut HashSet<String>,
) {
    match expr {
        Expr::And(parts) | Expr::Or(parts) => {
            for part in parts {
                collect_positive_content_literals(part, negated, terms, seen_terms);
            }
        }
        Expr::Not(inner) => collect_positive_content_literals(inner, !negated, terms, seen_terms),
        Expr::Term(Term::Contains {
            field: Field::Any,
            value,
        }) if !negated && value.len() >= 2 && seen_terms.insert(value.clone()) => {
            terms.push(value.clone());
        }
        Expr::Term(_) => {}
    }
}

fn compare_ascii_case_insensitive(left: &str, right: &str) -> std::cmp::Ordering {
    left.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(right.bytes().map(|byte| byte.to_ascii_lowercase()))
}

fn push_bounded_result(
    results: &mut Vec<(i64, SearchResultItem)>,
    candidate: (i64, SearchResultItem),
    capacity: usize,
    sort: SearchSort,
) {
    if capacity == 0 {
        return;
    }
    results.push(candidate);
    if results.len() > capacity.saturating_mul(2) {
        sort_results(results, sort);
        results.truncate(capacity);
    }
}

fn sort_results(v: &mut [(i64, SearchResultItem)], sort: SearchSort) {
    match sort {
        SearchSort::Relevance => {
            v.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)))
        }
        SearchSort::Name => v.sort_by(|a, b| compare_ascii_case_insensitive(&a.1.name, &b.1.name)),
        SearchSort::Path => v.sort_by(|a, b| a.1.path.cmp(&b.1.path)),
        SearchSort::Modified => v.sort_by(|a, b| {
            b.1.modified_unix_secs
                .unwrap_or(0)
                .cmp(&a.1.modified_unix_secs.unwrap_or(0))
        }),
        SearchSort::Size => v.sort_by(|a, b| {
            b.1.size_bytes
                .unwrap_or(0)
                .cmp(&a.1.size_bytes.unwrap_or(0))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn headless_single_update_commits_files_and_releases_the_writer_for_the_next_run() {
        let fixture = temp_service_path("single-update").with_extension("fixture");
        std::fs::create_dir(&fixture).unwrap();
        let files = fixture.join("files");
        std::fs::create_dir(&files).unwrap();
        std::fs::write(files.join("first.txt"), "first").unwrap();
        let store = fixture.join("search.db");
        let config = SearchConfig {
            include_roots: vec![files.clone()],
            use_system_locate: false,
            manage_locate_db: false,
            ..SearchConfig::default()
        };
        let first = SearchService::new_with_store_path(config.clone(), store.clone())
            .run_single_update(false)
            .expect("first headless update");
        assert_eq!(first.status, SearchIndexStatus::Ready);
        assert_eq!(first.indexed_items, 1);
        std::fs::write(files.join("second.txt"), "second").unwrap();
        let second = SearchService::new_with_store_path(config, store)
            .run_single_update(false)
            .expect("second run acquires the released writer");
        assert_eq!(second.status, SearchIndexStatus::Ready);
        assert_eq!(second.indexed_items, 2);
        assert_eq!(std::fs::read(files.join("first.txt")).unwrap(), b"first");
        std::fs::remove_dir_all(fixture).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn administrator_index_configuration_is_private_and_metadata_only_by_default() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = crate::test_temp_dir().join(format!(
            "ottrin-administrator-config-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("create test directory");
        let path = directory.join("search.json");
        let config = SearchConfig {
            include_roots: vec![PathBuf::from("/etc")],
            content_indexing_enabled: true,
            content_include_roots: vec![PathBuf::from("/etc")],
            ..SearchConfig::default()
        };

        write_administrator_index_config(&path, &config).expect("write protected config");

        let stored: SearchConfig =
            serde_json::from_slice(&fs::read(&path).expect("read config")).expect("parse config");
        assert_eq!(stored.include_roots, vec![PathBuf::from("/etc")]);
        assert!(!stored.content_indexing_enabled);
        assert!(stored.content_include_roots.is_empty());
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn administrator_index_configuration_refuses_a_symlink_namespace() {
        use std::os::unix::fs::symlink;

        let directory = crate::test_temp_dir().join(format!(
            "ottrin-administrator-config-symlink-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(directory.join("target")).unwrap();
        symlink(directory.join("target"), directory.join("namespace")).unwrap();
        let config = SearchConfig {
            include_roots: vec![PathBuf::from("/etc")],
            ..SearchConfig::default()
        };

        let error =
            write_administrator_index_config(&directory.join("namespace/search.json"), &config)
                .expect_err("symlink namespace must be rejected");

        assert!(error.contains("directory"), "unexpected error: {error}");
        assert!(!directory.join("target/search.json").exists());
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn administrator_index_install_writes_owned_files_before_enabling_service() {
        let directory = crate::test_temp_dir().join(format!(
            "ottrin-administrator-install-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let paths = AdministratorIndexPaths {
            unit: directory.join("ottrin-administrator-index.service"),
            config: directory.join("etc/search.json"),
            state: directory.join("state"),
            runtime: directory.join("runtime"),
        };
        let config = SearchConfig {
            include_roots: vec![PathBuf::from("/etc")],
            ..SearchConfig::default()
        };
        let mut operations = Vec::new();

        install_administrator_index_service_with(
            &paths,
            Path::new("/opt/Ottrin/ottrin-indexd"),
            &config,
            |operation| {
                assert!(paths.unit.is_file());
                assert!(paths.config.is_file());
                operations.push(operation);
                Ok(true)
            },
        )
        .expect("install Administrator service");

        assert_eq!(
            operations,
            vec![
                AdministratorSystemdOperation::DaemonReload,
                AdministratorSystemdOperation::EnableNow,
            ]
        );
        assert!(
            fs::read_to_string(&paths.unit)
                .unwrap()
                .starts_with("# Managed by Ottrin\n")
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn administrator_index_removal_stops_service_before_optional_purge() {
        let directory = crate::test_temp_dir().join(format!(
            "ottrin-administrator-remove-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        let paths = AdministratorIndexPaths {
            unit: directory.join("ottrin-administrator-index.service"),
            config: directory.join("etc/search.json"),
            state: directory.join("state"),
            runtime: directory.join("runtime"),
        };
        fs::create_dir_all(paths.config.parent().unwrap()).unwrap();
        fs::create_dir_all(&paths.state).unwrap();
        fs::create_dir_all(&paths.runtime).unwrap();
        fs::write(&paths.unit, "# Managed by Ottrin\n[Unit]\n").unwrap();
        fs::write(&paths.config, "{}").unwrap();
        fs::write(paths.state.join("search.db"), b"index").unwrap();
        let mut operations = Vec::new();

        uninstall_administrator_index_service_with(&paths, true, |operation| {
            operations.push(operation);
            Ok(true)
        })
        .expect("remove Administrator service and data");

        assert_eq!(
            operations,
            vec![
                AdministratorSystemdOperation::DisableNow,
                AdministratorSystemdOperation::DaemonReload,
            ]
        );
        assert!(!paths.unit.exists());
        assert!(!paths.config.parent().unwrap().exists());
        assert!(!paths.state.exists());
        assert!(!paths.runtime.exists());
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn administrator_index_unit_uses_fixed_private_namespaces() {
        let unit = administrator_index_systemd_unit(Path::new("/opt/Ottrin/ottrin-indexd"));

        assert!(
            unit.contains("ExecStart=\"/opt/Ottrin/ottrin-indexd\" --administrator-index-daemon")
        );
        assert!(unit.contains("RuntimeDirectory=ottrin-administrator"));
        assert!(unit.contains("StateDirectory=ottrin-administrator"));
        assert!(unit.contains("ConfigurationDirectory=ottrin-administrator"));
        assert!(unit.contains("UMask=0077"));
        assert!(!unit.contains("--user"));
    }

    #[test]
    fn enabled_distro_updatedb_timer_is_reused_without_duplicate_setup() {
        let status = system_locate_schedule_status_with(
            |_| false,
            |timer| timer == "plocate-updatedb.timer",
        );

        assert_eq!(status.source.as_deref(), Some("systemd"));
        assert_eq!(status.detail.as_deref(), Some("plocate-updatedb.timer"));
        assert!(!status.needs_setup);
        assert!(!status.managed_by_ottrin);
    }

    #[test]
    fn disabled_distro_updatedb_timer_does_not_hide_missing_maintenance() {
        let status = system_locate_schedule_status_with(|_| false, |_| false);

        assert_eq!(status.source, None);
        assert_eq!(status.detail, None);
        assert!(status.needs_setup);
        assert!(!status.managed_by_ottrin);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sqlite_background_install_renders_one_persistent_daemon_unit() {
        let unit = sqlite_daemon_systemd_unit(Path::new("/opt/Ottrin 100%/ottrin"));

        assert!(unit.contains("Type=simple"));
        assert!(unit.contains("ExecStart=\"/opt/Ottrin 100%%/ottrin\" --index-daemon"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(!unit.contains("Type=oneshot"));
        assert!(!unit.contains("index update"));
        assert!(!unit.contains("[Timer]"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sqlite_background_removal_controls_only_the_current_service() {
        let mut operations = Vec::new();
        stop_sqlite_systemd_for_removal_with(true, |operation| {
            operations.push(operation);
            Ok(true)
        })
        .expect("stop current service");

        assert_eq!(operations, vec![SqliteSystemdOperation::DisableNowService]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sqlite_background_removal_reports_stop_failure_before_file_deletion() {
        let error = stop_sqlite_systemd_for_removal_with(true, |operation| {
            assert_eq!(operation, SqliteSystemdOperation::DisableNowService);
            Ok(false)
        })
        .expect_err("failed service stop must block removal");

        assert!(
            error.contains("disable and stop"),
            "unexpected error: {error}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sqlite_background_status_distinguishes_enabled_but_stopped() {
        let status = sqlite_systemd_status_with(true, |operation| match operation {
            SqliteSystemdOperation::IsEnabled => Ok(true),
            SqliteSystemdOperation::IsActive => Ok(false),
            _ => panic!("status must not mutate the service"),
        });

        assert!(status.enabled);
        assert_eq!(status.detail, "Enabled but stopped");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sqlite_background_status_reports_an_unavailable_systemd_probe() {
        let status = sqlite_systemd_status_with(true, |_| Err("user bus unavailable".to_string()));

        assert!(!status.enabled);
        assert!(status.detail.contains("user bus unavailable"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sqlite_background_restart_reports_systemctl_failure() {
        let mut operations = Vec::new();
        let error = restart_sqlite_systemd_with(true, |operation| {
            operations.push(operation);
            Ok(false)
        })
        .expect_err("failed restart must be reported");

        assert_eq!(operations, vec![SqliteSystemdOperation::Restart]);
        assert!(error.contains("restart"), "unexpected error: {error}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn first_service_install_prepares_existing_session_daemon() {
        let prepared = std::cell::Cell::new(false);
        prepare_sqlite_systemd_upgrade_with(
            false,
            || {
                prepared.set(true);
                Ok(())
            },
            |_| panic!("No existing unit to stop"),
        )
        .unwrap();
        assert!(prepared.get());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sqlite_background_upgrade_prepares_daemon_before_stopping_service() {
        let events = std::cell::RefCell::new(Vec::new());

        prepare_sqlite_systemd_upgrade_with(
            true,
            || {
                events.borrow_mut().push("prepare");
                Ok(())
            },
            |operation| {
                assert_eq!(operation, SqliteSystemdOperation::Stop);
                events.borrow_mut().push("stop");
                Ok(true)
            },
        )
        .expect("prepare upgrade");

        assert_eq!(events.into_inner(), vec!["prepare", "stop"]);
    }

    #[test]
    fn windows_background_status_reports_an_absent_task() {
        let status = sqlite_windows_task_status_with(|| Ok(None));

        assert_eq!(status.source, "none");
        assert!(!status.enabled);
        assert_eq!(status.detail, "No background Ottrin index task installed");
    }

    #[test]
    fn windows_background_status_reports_an_enabled_task_as_ready() {
        let status = sqlite_windows_task_status_with(|| {
            Ok(Some(SqliteWindowsTaskRegistration { enabled: true }))
        });

        assert_eq!(status.source, "Windows Task Scheduler");
        assert!(status.enabled);
        assert_eq!(status.detail, "Enabled at login");
    }

    #[test]
    fn windows_background_status_does_not_mask_query_errors() {
        let status =
            sqlite_windows_task_status_with(|| Err("could not start schtasks".to_string()));

        assert_eq!(status.source, "Windows Task Scheduler");
        assert!(!status.enabled);
        assert_eq!(
            status.detail,
            "Status unavailable: could not start schtasks"
        );
    }

    #[test]
    fn windows_background_status_reports_a_disabled_task() {
        let status = sqlite_windows_task_status_with(|| {
            Ok(Some(SqliteWindowsTaskRegistration { enabled: false }))
        });

        assert_eq!(status.source, "Windows Task Scheduler");
        assert!(!status.enabled);
        assert_eq!(status.detail, "Installed but disabled");
    }

    #[test]
    fn windows_task_query_distinguishes_absence_from_access_and_parse_failures() {
        assert!(
            parse_sqlite_windows_task_query(true, b"", b"")
                .unwrap()
                .is_none()
        );
        assert!(
            parse_sqlite_windows_task_query(false, b"", b"Access is denied")
                .unwrap_err()
                .contains("Access is denied")
        );
        assert!(parse_sqlite_windows_task_query(true, b"not XML", b"").is_err());
    }

    #[test]
    fn windows_task_management_stops_on_query_errors() {
        for remove in [false, true] {
            let mut calls = Vec::new();
            let mut run = |operation| {
                calls.push(operation);
                Err("Access is denied".to_string())
            };
            let result = if remove {
                uninstall_sqlite_windows_task_with(&mut run, || {
                    panic!("inspection failure must not stop a session daemon")
                })
            } else {
                restart_sqlite_windows_task_with(&mut run)
            };
            assert_eq!(result.unwrap_err(), "Access is denied");
            assert_eq!(calls, [SqliteWindowsTaskOperation::Query]);
        }
    }

    #[test]
    fn windows_task_management_uses_only_the_selected_user_identity() {
        for operation in [
            SqliteWindowsTaskOperation::Run,
            SqliteWindowsTaskOperation::Delete,
        ] {
            let first = operation.args("Ottrin Index S-1-5-21-101").unwrap();
            let second = operation.args("Ottrin Index S-1-5-21-102").unwrap();
            assert_ne!(first, second);
            assert!(first.contains(&"Ottrin Index S-1-5-21-101"));
            assert!(!first.contains(&"Ottrin Index"));
        }
    }

    #[test]
    fn windows_task_xml_reads_the_invariant_enabled_flag_from_utf16() {
        let xml = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Settings><Enabled>false</Enabled></Settings>
</Task>"#;
        let mut output = vec![0xff, 0xfe];
        output.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));

        let registration = parse_sqlite_windows_task_xml(&output).expect("parse task XML");

        assert!(!registration.enabled);
    }

    #[test]
    fn windows_task_status_uses_task_settings_instead_of_trigger_enablement() {
        for (trigger, task, expected) in [("true", "false", false), ("false", "true", true)] {
            let xml = format!(
                "<Task><Triggers><CalendarTrigger><Enabled>{trigger}</Enabled></CalendarTrigger></Triggers><Settings><Enabled>{task}</Enabled></Settings></Task>"
            );
            let registration =
                parse_sqlite_windows_task_xml(xml.as_bytes()).expect("valid task XML");
            let status = sqlite_windows_task_status_with(|| Ok(Some(registration)));
            assert_eq!(status.enabled, expected);
            assert_eq!(
                status.detail,
                if expected {
                    "Enabled at login"
                } else {
                    "Installed but disabled"
                }
            );
        }
    }

    #[test]
    fn windows_task_xml_rejects_unterminated_settings() {
        assert!(
            parse_sqlite_windows_task_xml(b"<Task><Settings><Enabled>true</Enabled></Task>")
                .is_err()
        );
    }

    #[test]
    fn windows_task_xml_rejects_an_unknown_enabled_value() {
        let error = parse_sqlite_windows_task_xml(
            br#"<Task><Settings><Enabled>sometimes</Enabled></Settings></Task>"#,
        )
        .expect_err("unknown enabled value must not be reported ready");

        assert!(error.contains("Enabled"), "unexpected error: {error}");
    }

    #[test]
    fn windows_background_restart_requires_an_existing_task_and_runs_it() {
        let mut operations = Vec::new();
        let message = restart_sqlite_windows_task_with(|operation| {
            operations.push(operation);
            Ok(true)
        })
        .expect("restart existing task");

        assert_eq!(
            operations,
            vec![
                SqliteWindowsTaskOperation::Query,
                SqliteWindowsTaskOperation::Run,
            ]
        );
        assert!(message.contains("started"), "unexpected message: {message}");
    }

    #[test]
    fn windows_unregistered_removal_stops_session_and_propagates_shutdown_failure() {
        for fail in [false, true] {
            let stopped = std::cell::Cell::new(false);
            let result = uninstall_sqlite_windows_task_with(
                |operation| {
                    assert_eq!(operation, SqliteWindowsTaskOperation::Query);
                    Ok(false)
                },
                || {
                    stopped.set(true);
                    if fail {
                        Err("daemon did not exit".into())
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(stopped.get());
            if fail {
                assert_eq!(result.unwrap_err(), "daemon did not exit");
            } else {
                assert!(result.unwrap().contains("Session indexing stopped"));
            }
        }
    }

    #[test]
    fn windows_background_removal_does_not_mask_task_scheduler_failure() {
        let mut operations = Vec::new();
        let error = uninstall_sqlite_windows_task_with(
            |operation| {
                operations.push(operation);
                match operation {
                    SqliteWindowsTaskOperation::Query => Ok(true),
                    SqliteWindowsTaskOperation::Delete => Ok(false),
                    _ => panic!("removal must only query and delete"),
                }
            },
            || panic!("registered removal owns daemon shutdown"),
        )
        .expect_err("failed deletion must block uninstall");

        assert_eq!(
            operations,
            vec![
                SqliteWindowsTaskOperation::Query,
                SqliteWindowsTaskOperation::Delete,
            ]
        );
        assert!(error.contains("delete"), "unexpected error: {error}");
    }

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn windows_internal_commands_request_no_console_window() {
        assert_eq!(internal_command_creation_flags(true), 0x0800_0000);
        assert_eq!(internal_command_creation_flags(false), 0);
    }

    fn temp_service_path(name: &str) -> PathBuf {
        crate::test_temp_dir().join(format!(
            "ottrin-search-service-{name}-{}-{}.db",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos()
        ))
    }

    fn blocking_launcher(starts: Arc<AtomicUsize>) -> worker::ScanLauncher {
        Arc::new(move |_plan, cancelled, _revision, _commands| {
            starts.fetch_add(1, Ordering::AcqRel);
            Some(std::thread::spawn(move || {
                while !cancelled.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }))
        })
    }

    fn tracked_blocking_launcher(
        starts: Arc<AtomicUsize>,
        exits: Arc<AtomicUsize>,
    ) -> worker::ScanLauncher {
        Arc::new(move |_plan, cancelled, _revision, _commands| {
            starts.fetch_add(1, Ordering::AcqRel);
            let exits = Arc::clone(&exits);
            Some(std::thread::spawn(move || {
                while !cancelled.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(1));
                }
                exits.fetch_add(1, Ordering::AcqRel);
            }))
        })
    }

    fn query_by_name(name: &str) -> SearchQuery {
        SearchQuery {
            text: name.to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: true,
            sort: SearchSort::Relevance,
            limit: 50,
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
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(predicate(), "condition was not met before timeout");
    }

    #[cfg(windows)]
    #[test]
    fn windows_index_watches_allow_directory_moves_and_follow_new_subtrees() {
        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                if let Err(error) = fs::remove_dir_all(&self.0) {
                    eprintln!("Windows watch fixture cleanup failed: {error}");
                    assert!(std::thread::panicking(), "watch fixture cleanup failed");
                }
            }
        }
        let fixture =
            Fixture(temp_service_path("windows-directory-watch").with_extension("fixture"));
        let root = fixture.0.join("root");
        fs::create_dir_all(root.join("source/nested")).expect("create nested source");
        let root = root.canonicalize().expect("canonical root");
        fs::write(root.join("source/nested/original.txt"), b"original").expect("seed source");
        let mut config = SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        };
        // Declared after the fixture so panic unwinding stops all watches before cleanup.
        let mut service =
            SearchService::new_with_store_path(config.clone(), fixture.0.join("search.db"));
        service.start();
        service
            .rebuild_index_blocking()
            .expect("initial generation");
        assert!(service.diagnostics().watcher_ready);
        fs::rename(root.join("source"), root.join("moved"))
            .expect("an active indexer must not prevent moving a populated directory");
        wait_until(Duration::from_secs(5), || {
            let response = service.query(query_by_name("original.txt"));
            response.total == 1
                && response.items.len() == 1
                && response.items[0].path == root.join("moved/nested/original.txt")
        });
        fs::create_dir_all(root.join("new/deeper")).expect("create new subtree");
        fs::write(root.join("new/deeper/created.txt"), b"created").expect("write new descendant");
        wait_until(Duration::from_secs(5), || {
            service.query(query_by_name("created.txt")).total == 1
        });
        fs::rename(root.join("new"), root.join("new-moved"))
            .expect("new descendants must not acquire move-blocking watches");
        wait_until(Duration::from_secs(5), || {
            let response = service.query(query_by_name("created.txt"));
            response.total == 1
                && response.items.len() == 1
                && response.items[0].path == root.join("new-moved/deeper/created.txt")
        });
        fs::create_dir(root.join("node_modules")).expect("create excluded subtree");
        fs::write(root.join("node_modules/ignored-noise.txt"), b"excluded")
            .expect("write excluded descendant");
        fs::write(root.join("barrier.txt"), b"included").expect("write included barrier");
        wait_until(Duration::from_secs(5), || {
            service.query(query_by_name("barrier.txt")).total == 1
        });
        assert_eq!(service.query(query_by_name("ignored-noise.txt")).total, 0);
        let replacement = fixture.0.join("replacement");
        fs::create_dir(&replacement).expect("create replacement root");
        config.include_roots = vec![replacement];
        service.update_config(config);
        service
            .rebuild_index_blocking()
            .expect("replacement generation");
        fs::rename(root.join("moved"), root.join("after-config-change"))
            .expect("reconfiguration must release every watch from the previous plan");
        service.stop();
    }

    fn mk_item(path: &str, name: &str, kind: EntryKind) -> SearchResultItem {
        let mut item = SearchResultItem {
            path: PathBuf::from(path),
            parent_path: PathBuf::from(path)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
            name: name.to_string(),
            kind,
            is_executable: false,
            symlink_target_is_dir: None,
            size_bytes: None,
            modified_unix_secs: None,
            content_snippet: None,
            name_lower: String::new(),
            path_str: String::new(),
            path_lower: String::new(),
        };
        item.prepare();
        item
    }

    #[test]
    fn default_test_service_never_uses_the_user_search_database() {
        let service = SearchService::new(SearchConfig::default());

        assert_ne!(service.store_path, search_db_path());
    }

    #[test]
    fn partial_sqlite_simple_term_page_is_used_instead_of_streaming_the_generation() {
        assert!(should_use_simple_term_page(1, 10, false));
    }

    #[test]
    fn persisted_simple_term_no_match_does_not_fall_back_to_generation_wide_fuzzy_results() {
        let _guard = SEARCH_SERVICE_TEST_LOCK.lock().expect("test lock");
        let root = crate::test_temp_dir().join(format!(
            "ottrin-persisted-simple-term-no-fuzzy-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let db_path = temp_service_path("persisted-simple-term-no-fuzzy");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create fixture root");
        let config = SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        };
        let plan = plan::ScanPlan::new(&config).expect("valid fixture plan");
        let items = (0..10_000)
            .map(|index| {
                // Every name is a fuzzy subsequence match for "needle", but none contains
                // it as a name or path substring. The old persisted fallback scanned all of
                // these rows solely to return optional fuzzy results.
                let name = format!("n-{index:05}-e-e-d-l-e.txt");
                let path = root.join(&name);
                mutation::OwnedItem::new(
                    root.clone(),
                    mk_item(&path.to_string_lossy(), &name, EntryKind::File),
                )
            })
            .collect();
        let mut store = SearchStore::open(db_path.clone()).expect("open fixture store");
        store
            .commit(&plan, mutation::IndexMutation::ReplaceAll(items))
            .expect("commit persisted fixture generation");
        drop(store);

        let reader = SearchService::load_persisted(config, db_path.clone())
            .expect("load persisted fixture generation");
        let response = reader.query(query_by_name("needle"));

        assert!(
            response.error.is_none(),
            "unexpected error: {:?}",
            response.error
        );
        assert!(response.items.is_empty());
        assert_eq!(response.total, 0);

        let _ = clear_sqlite_storage(&db_path);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn persisted_snapshot_can_be_read_without_starting_or_locking_a_writer() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-read-only-status-root-{}",
            std::process::id()
        ));
        let db_path = temp_service_path("read-only-status");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(root.join("marker.txt"), b"marker").expect("write marker");
        let config = SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        };
        let mut writer = SearchService::new_with_store_path(config.clone(), db_path.clone());
        writer.start();
        writer
            .rebuild_index_blocking()
            .expect("commit persisted generation");
        writer.stop();

        let reader = SearchService::load_persisted(config.clone(), db_path.clone())
            .expect("load persisted generation read-only");
        assert_eq!(reader.diagnostics().status, SearchIndexStatus::Ready);
        assert_eq!(reader.diagnostics().indexed_items, 1);
        assert!(reader.index.read().expect("index read").items.is_empty());
        let response = reader.query(SearchQuery {
            text: "marker".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 10,
            offset: 0,
            content_search: false,
        });
        assert_eq!(response.total, 1);
        assert_eq!(response.items[0].path, root.join("marker.txt"));

        let mut next_writer = SearchService::new_with_store_path(config, db_path.clone());
        next_writer.start();
        assert!(!matches!(
            next_writer.diagnostics().last_error.as_deref(),
            Some(error) if error.contains("already in use")
        ));
        next_writer.stop();
        let _ = clear_sqlite_storage(&db_path);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn persisted_queries_preserve_component_non_utf8_scope_and_ascii_case_semantics() {
        use std::os::unix::ffi::OsStringExt;

        let root = crate::test_temp_dir().join(format!(
            "ottrin-persisted-scope-root-{}",
            std::process::id()
        ));
        let db_path = temp_service_path("persisted-scope");
        let foo = root.join("foo");
        let foobar = root.join("foobar");
        let non_utf8 = root.join(std::ffi::OsString::from_vec(b"non-utf8-\xff".to_vec()));
        for directory in [&foo, &foobar, &non_utf8] {
            std::fs::create_dir_all(directory).expect("create scoped directory");
        }
        std::fs::write(foo.join("needle.txt"), b"needle").expect("write scoped file");
        std::fs::write(foobar.join("needle.txt"), b"needle").expect("write sibling file");
        std::fs::write(non_utf8.join("marker.txt"), b"marker").expect("write non-UTF8 file");
        std::fs::write(root.join("ÉCOLE.txt"), b"case").expect("write Unicode file");
        let config = SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        };
        let mut writer = SearchService::new_with_store_path(config.clone(), db_path.clone());
        writer.start();
        writer
            .rebuild_index_blocking()
            .expect("commit persisted generation");
        writer.stop();
        let reader = SearchService::load_persisted(config, db_path.clone())
            .expect("load persisted generation");

        let scoped = reader.query(SearchQuery {
            text: "needle".to_string(),
            scope: SearchScope::CurrentFolder,
            root_path: Some(foo.clone()),
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 10,
            offset: 0,
            content_search: false,
        });
        assert_eq!(scoped.total, 1, "foo scope must not include foobar");
        assert!(scoped.items[0].path.starts_with(&foo));

        let raw_scope = reader.query(SearchQuery {
            text: "marker".to_string(),
            scope: SearchScope::CurrentFolder,
            root_path: Some(non_utf8.clone()),
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 10,
            offset: 0,
            content_search: false,
        });
        assert_eq!(raw_scope.total, 1);
        assert!(raw_scope.items[0].path.starts_with(&non_utf8));

        let unicode_case = reader.query(SearchQuery {
            text: "école".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 10,
            offset: 0,
            content_search: false,
        });
        assert_eq!(unicode_case.total, 0, "legacy matching is ASCII-case-only");

        let _ = clear_sqlite_storage(&db_path);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn start_is_idempotent() {
        let starts = Arc::new(AtomicUsize::new(0));
        let db_path = temp_service_path("start-idempotent");
        let mut service = SearchService::new_with_runtime(
            SearchConfig {
                include_roots: vec![PathBuf::from("/controlled")],
                ..SearchConfig::default()
            },
            db_path.clone(),
            blocking_launcher(Arc::clone(&starts)),
        );

        service.start();
        wait_until(Duration::from_secs(1), || {
            starts.load(Ordering::Acquire) == 1
        });
        service.start();
        std::thread::sleep(Duration::from_millis(20));

        assert_eq!(starts.load(Ordering::Acquire), 1);
        service.stop();
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn one_writer_owns_storage_across_process_boundaries() {
        let starts = Arc::new(AtomicUsize::new(0));
        let db_path = temp_service_path("writer-lock");
        let config = SearchConfig {
            include_roots: vec![PathBuf::from("/controlled")],
            ..SearchConfig::default()
        };
        let mut first = SearchService::new_with_runtime(
            config.clone(),
            db_path.clone(),
            blocking_launcher(Arc::clone(&starts)),
        );
        let mut second = SearchService::new_with_runtime(
            config,
            db_path.clone(),
            blocking_launcher(Arc::clone(&starts)),
        );

        first.start();
        wait_until(Duration::from_secs(1), || {
            starts.load(Ordering::Acquire) == 1
        });
        second.start();
        assert_eq!(second.diagnostics().status, SearchIndexStatus::Unavailable);
        assert!(
            second
                .diagnostics()
                .last_error
                .is_some_and(|error| error.contains("already owned"))
        );
        assert!(second.clear_storage().is_err());
        assert_eq!(starts.load(Ordering::Acquire), 1);

        first.stop();
        second.start();
        wait_until(Duration::from_secs(1), || {
            starts.load(Ordering::Acquire) == 2
        });
        second.stop();
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn lifecycle_stress_loop() {
        let starts = Arc::new(AtomicUsize::new(0));
        let exits = Arc::new(AtomicUsize::new(0));
        let db_path = temp_service_path("stress");
        let mut service = SearchService::new_with_runtime(
            SearchConfig {
                include_roots: vec![PathBuf::from("/controlled")],
                ..SearchConfig::default()
            },
            db_path.clone(),
            tracked_blocking_launcher(Arc::clone(&starts), Arc::clone(&exits)),
        );

        for expected in 1..=100 {
            service.start();
            service.start();
            wait_until(Duration::from_secs(1), || {
                starts.load(Ordering::Acquire) == expected
            });
            service.stop();
            service.stop();
        }

        assert_eq!(starts.load(Ordering::Acquire), 100);
        assert_eq!(exits.load(Ordering::Acquire), 100);
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn stop_is_idempotent_and_joins_worker_and_scan() {
        let starts = Arc::new(AtomicUsize::new(0));
        let exits = Arc::new(AtomicUsize::new(0));
        let db_path = temp_service_path("stop-idempotent");
        let mut service = SearchService::new_with_runtime(
            SearchConfig {
                include_roots: vec![PathBuf::from("/controlled")],
                ..SearchConfig::default()
            },
            db_path.clone(),
            tracked_blocking_launcher(Arc::clone(&starts), Arc::clone(&exits)),
        );
        service.start();
        wait_until(Duration::from_secs(1), || {
            starts.load(Ordering::Acquire) == 1
        });

        service.stop();
        service.stop();

        assert_eq!(exits.load(Ordering::Acquire), 1, "stop must join the scan");
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn persisted_restart_reconciles_files_changed_while_stopped() {
        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).expect("remove offline-change fixture");
            }
        }
        let fixture = Fixture(temp_service_path("offline-changes").with_extension("fixture"));
        let corpus = fixture.0.join("corpus");
        std::fs::create_dir_all(&corpus).unwrap();
        let edited = corpus.join("edited.txt");
        let deleted = corpus.join("deleted.txt");
        let renamed = corpus.join("before-rename.txt");
        for path in [&edited, &deleted, &renamed] {
            std::fs::write(path, "original").unwrap();
        }
        let cfg = SearchConfig {
            include_roots: vec![corpus.clone()],
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        };
        let database = fixture.0.join("search.db");
        let mut first = SearchService::new_with_store_path(cfg.clone(), database.clone());
        first.start();
        first.rebuild_index_blocking().unwrap();
        first.stop();
        std::fs::write(&edited, "changed while indexing was stopped").unwrap();
        std::fs::remove_file(&deleted).unwrap();
        std::fs::rename(&renamed, corpus.join("after-rename.txt")).unwrap();
        std::fs::write(corpus.join("created.txt"), "created offline").unwrap();
        let mut restarted = SearchService::new_with_store_path(cfg, database);
        restarted.start();
        wait_until(Duration::from_secs(5), || {
            let edited_result = restarted.query(query_by_name("edited.txt"));
            restarted.query(query_by_name("created.txt")).total == 1
                && restarted.query(query_by_name("deleted.txt")).total == 0
                && restarted.query(query_by_name("before-rename.txt")).total == 0
                && restarted.query(query_by_name("after-rename.txt")).total == 1
                && edited_result.items.first().is_some_and(|item| {
                    item.size_bytes == Some("changed while indexing was stopped".len() as u64)
                })
        });
        restarted.stop();
    }

    #[test]
    fn service_restarts_after_stop() {
        let starts = Arc::new(AtomicUsize::new(0));
        let db_path = temp_service_path("restart");
        let mut service = SearchService::new_with_runtime(
            SearchConfig {
                include_roots: vec![PathBuf::from("/controlled")],
                ..SearchConfig::default()
            },
            db_path.clone(),
            blocking_launcher(Arc::clone(&starts)),
        );
        service.start();
        wait_until(Duration::from_secs(1), || {
            starts.load(Ordering::Acquire) == 1
        });
        service.stop();
        service.start();
        wait_until(Duration::from_secs(1), || {
            starts.load(Ordering::Acquire) == 2
        });
        service.stop();
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn drop_cancels_and_joins_a_blocked_scan_bounded() {
        let starts = Arc::new(AtomicUsize::new(0));
        let exits = Arc::new(AtomicUsize::new(0));
        let db_path = temp_service_path("drop-blocked");
        let mut service = SearchService::new_with_runtime(
            SearchConfig {
                include_roots: vec![PathBuf::from("/controlled")],
                ..SearchConfig::default()
            },
            db_path.clone(),
            tracked_blocking_launcher(Arc::clone(&starts), Arc::clone(&exits)),
        );
        service.start();
        wait_until(Duration::from_secs(1), || {
            starts.load(Ordering::Acquire) == 1
        });

        let handle = service.handle();
        let began = Instant::now();
        drop(service);

        assert!(began.elapsed() < Duration::from_secs(1));
        assert_eq!(exits.load(Ordering::Acquire), 1, "drop must join the scan");
        let diagnostics = handle.diagnostics();
        assert!(matches!(diagnostics.status, SearchIndexStatus::Unavailable));

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn concurrent_query_handle_does_not_block_stop() {
        let starts = Arc::new(AtomicUsize::new(0));
        let db_path = temp_service_path("query-stop");
        let mut service = SearchService::new_with_runtime(
            SearchConfig {
                include_roots: vec![PathBuf::from("/controlled")],
                ..SearchConfig::default()
            },
            db_path.clone(),
            blocking_launcher(Arc::clone(&starts)),
        );
        service.start();
        wait_until(Duration::from_secs(1), || {
            starts.load(Ordering::Acquire) == 1
        });
        let query = service.handle();
        let running = Arc::new(AtomicBool::new(true));
        let queries = Arc::new(AtomicUsize::new(0));
        let query_running = Arc::clone(&running);
        let query_count = Arc::clone(&queries);
        let query_thread = std::thread::spawn(move || {
            while query_running.load(Ordering::Acquire) {
                let _ = query.query(query_by_name("needle"));
                query_count.fetch_add(1, Ordering::AcqRel);
            }
        });
        wait_until(Duration::from_secs(1), || {
            queries.load(Ordering::Acquire) > 0
        });

        let began = Instant::now();
        service.stop();
        assert!(began.elapsed() < Duration::from_secs(1));

        running.store(false, Ordering::Release);
        query_thread.join().expect("query thread");
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn clear_storage_stops_owner_and_leaves_empty_unavailable_state() {
        let starts = Arc::new(AtomicUsize::new(0));
        let exits = Arc::new(AtomicUsize::new(0));
        let db_path = temp_service_path("clear-storage");
        let mut service = SearchService::new_with_runtime(
            SearchConfig {
                include_roots: vec![PathBuf::from("/controlled")],
                ..SearchConfig::default()
            },
            db_path.clone(),
            tracked_blocking_launcher(Arc::clone(&starts), Arc::clone(&exits)),
        );
        service.start();
        wait_until(Duration::from_secs(1), || {
            starts.load(Ordering::Acquire) == 1
        });
        assert!(db_path.exists());
        let sidecars = ["-wal", "-shm", "-journal"].map(|suffix| {
            let mut path = db_path.as_os_str().to_os_string();
            path.push(suffix);
            let path = PathBuf::from(path);
            // A running SQLite owner may already map its real WAL/SHM files.
            // Seed only missing orphan sidecars; never truncate live storage.
            if !path.exists() {
                fs::write(&path, b"stale sqlite sidecar").expect("create sidecar");
            }
            path
        });
        let mut content_name = db_path.as_os_str().to_os_string();
        content_name.push(".content-42.db");
        let content_db = PathBuf::from(content_name);
        fs::write(&content_db, b"generation content database").expect("create content database");
        let content_sidecars = ["-wal", "-shm", "-journal"].map(|suffix| {
            let mut path = content_db.as_os_str().to_os_string();
            path.push(suffix);
            let path = PathBuf::from(path);
            fs::write(&path, b"content sqlite sidecar").expect("create content sidecar");
            path
        });
        let expected_content_freed = [&content_db]
            .into_iter()
            .chain(content_sidecars.iter())
            .map(|path| fs::metadata(path).expect("storage metadata").len())
            .sum::<u64>();

        let freed = service.clear_storage().expect("clear owned storage");

        assert_eq!(exits.load(Ordering::Acquire), 1);
        assert!(!db_path.exists());
        assert!(sidecars.iter().all(|path| !path.exists()));
        assert!(!content_db.exists());
        assert!(content_sidecars.iter().all(|path| !path.exists()));
        assert!(freed >= expected_content_freed);
        let diagnostics = service.diagnostics();
        assert!(matches!(diagnostics.status, SearchIndexStatus::Unavailable));
        assert_eq!(diagnostics.indexed_items, 0);
        assert!(service.query(query_by_name("anything")).items.is_empty());
    }

    #[test]
    fn clear_sqlite_storage_removes_and_reports_all_generation_content_files() {
        let db_path = temp_service_path("clear-generation-storage");
        let mut paths = vec![db_path.clone()];
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut path = db_path.as_os_str().to_os_string();
            path.push(suffix);
            paths.push(PathBuf::from(path));
        }
        for generation in [3, 9] {
            let mut path = db_path.as_os_str().to_os_string();
            path.push(format!(".content-{generation}.db"));
            let content_db = PathBuf::from(path);
            paths.push(content_db.clone());
            for suffix in ["-wal", "-shm", "-journal"] {
                let mut sidecar = content_db.as_os_str().to_os_string();
                sidecar.push(suffix);
                paths.push(PathBuf::from(sidecar));
            }
        }
        for (index, path) in paths.iter().enumerate() {
            fs::write(path, vec![b'x'; index + 1]).expect("create storage fixture");
        }
        let expected_freed = paths
            .iter()
            .map(|path| fs::metadata(path).expect("storage metadata").len())
            .sum::<u64>();

        assert_eq!(
            clear_sqlite_storage(&db_path).expect("clear complete sqlite storage"),
            expected_freed
        );
        assert!(paths.iter().all(|path| !path.exists()));
    }

    #[test]
    fn parser_rejects_invalid_regex() {
        let err = parse_query("/[/").expect_err("should reject invalid regex");
        assert!(err.to_string().contains("Invalid regex"));
    }

    #[test]
    fn parser_rejects_malformed_syntax_without_accepting_a_prefix() {
        for query in ["foo) !bar", "(foo", "()", "foo |", "!", "foo bar)"] {
            let error = parse_query(query).expect_err(query);
            assert!(
                matches!(error, SearchError::Parse(_)),
                "{query:?} should produce a parse error"
            );
        }
    }

    #[test]
    fn parser_preserves_valid_neighboring_syntax() {
        for query in [
            "foo bar",
            "foo | bar",
            "!(foo | bar)",
            "!!name:report",
            "name:\"quarterly report\" path:C:\\work",
            "/foo\\\\d+/ ext:rs",
        ] {
            parse_query(query).unwrap_or_else(|error| panic!("{query:?} should parse: {error}"));
        }
    }

    #[test]
    fn malformed_query_returns_an_empty_error_response_before_search_execution() {
        let service = SearchService::new(SearchConfig::default());
        let response = service.query(query_by_name("foo) !bar"));

        assert!(response.items.is_empty());
        assert_eq!(response.total, 0);
        assert!(
            response
                .error
                .as_deref()
                .is_some_and(|error| error.contains("search parser error"))
        );
    }

    #[test]
    fn default_index_roots_remain_home_only() {
        let home = ottrin_core::default_home_dir();
        #[cfg(target_os = "windows")]
        let home = path_identity::resolved_index_path(&home);
        assert_eq!(effective_roots(&SearchConfig::default()), vec![home]);
    }

    #[test]
    fn explicit_zero_native_roots_remain_empty() {
        let config = SearchConfig {
            include_roots: Vec::new(),
            ..SearchConfig::default()
        };

        assert!(effective_roots(&config).is_empty());
    }

    #[test]
    fn unavailable_volume_intent_does_not_keep_a_stale_root_in_the_scan_plan() {
        let mut config = SearchConfig {
            include_roots: vec![PathBuf::from("/home/alice")],
            active_indexed_volume_roots: vec![PathBuf::from("/media/alice/usb")],
            ..SearchConfig::default()
        };
        assert_eq!(
            effective_roots(&config),
            vec![
                PathBuf::from("/home/alice"),
                PathBuf::from("/media/alice/usb")
            ]
        );

        config.active_indexed_volume_roots.clear();
        assert_eq!(effective_roots(&config), vec![PathBuf::from("/home/alice")]);
    }

    #[test]
    fn dropping_query_handles_does_not_stop_owner_rebuilds() {
        let _guard = SEARCH_SERVICE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = crate::test_temp_dir().join(format!(
            "ottrin-search-handle-owner-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("create handle ownership root");
        fs::write(root.join("initial.txt"), b"initial").expect("create initial file");

        let mut service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        });
        service.start();
        wait_until(Duration::from_secs(5), || {
            matches!(service.diagnostics().status, SearchIndexStatus::Ready)
        });

        drop(service.handle());
        let handles = vec![service.handle(), service.handle(), service.handle()];
        drop(handles);

        fs::write(root.join("explicit-revision.txt"), b"revision").expect("create revision file");
        service.rebuild_index();
        wait_until(Duration::from_secs(5), || {
            service
                .query(query_by_name("explicit-revision"))
                .items
                .iter()
                .any(|item| item.name == "explicit-revision.txt")
        });

        service.stop();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn query_handle_outlives_stopped_and_dropped_owner_snapshot() {
        let mut service = SearchService::new(SearchConfig::default());
        {
            let mut index = service.index.write().expect("index write");
            index.status = SearchIndexStatus::Ready;
            index.items = Arc::new(vec![mk_item(
                "/snapshot/retained-result.txt",
                "retained-result.txt",
                EntryKind::File,
            )]);
        }
        let handle = service.handle();

        service.stop();
        drop(service);

        let response = handle.query(query_by_name("retained-result"));
        assert!(matches!(response.status, SearchIndexStatus::Ready));
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].name, "retained-result.txt");
        assert_eq!(handle.diagnostics().indexed_items, 1);
    }

    #[test]
    fn current_folder_scope_filters_by_prefix() {
        let query = SearchQuery {
            text: "foo".to_string(),
            scope: SearchScope::CurrentFolder,
            root_path: Some(PathBuf::from("/tmp/a")),
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 100,
            offset: 0,
            content_search: false,
        };
        let in_scope = mk_item("/tmp/a/b/foo.txt", "foo.txt", EntryKind::File);
        let out_scope = mk_item("/tmp/c/foo.txt", "foo.txt", EntryKind::File);
        assert!(scope_match(&query, &in_scope));
        assert!(!scope_match(&query, &out_scope));
    }

    #[test]
    fn global_search_merges_system_results_when_native_results_already_exist() {
        fn system_results(
            _text: &str,
            _scope_root: Option<&Path>,
            _include_hidden: bool,
            _limit: usize,
        ) -> Vec<SearchResultItem> {
            vec![mk_item(
                "/opt/shared/shared-needle.txt",
                "shared-needle.txt",
                EntryKind::File,
            )]
        }

        let service = SearchService::new(SearchConfig {
            include_roots: vec![PathBuf::from("/home/alice")],
            use_system_locate: true,
            ..SearchConfig::default()
        });
        {
            let mut index = service.index.write().expect("index write");
            index.status = SearchIndexStatus::Ready;
            index.items = Arc::new(vec![mk_item(
                "/home/alice/native-needle.txt",
                "native-needle.txt",
                EntryKind::File,
            )]);
        }

        let response = service.handle().query_with_system_locate(
            SearchQuery {
                text: "needle".to_string(),
                scope: SearchScope::Global,
                root_path: None,
                include_hidden_system: false,
                sort: SearchSort::Relevance,
                limit: 20,
                offset: 0,
                content_search: false,
            },
            system_results,
        );

        assert_eq!(response.total, 2);
        assert!(
            response
                .items
                .iter()
                .any(|item| item.path == Path::new("/home/alice/native-needle.txt"))
        );
        assert!(
            response
                .items
                .iter()
                .any(|item| item.path == Path::new("/opt/shared/shared-needle.txt"))
        );
    }

    #[test]
    fn persisted_native_rows_deduplicate_system_results_outside_the_loaded_page() {
        fn native_root() -> PathBuf {
            crate::test_temp_dir().join(format!(
                "ottrin-persisted-locate-dedupe-{}",
                std::process::id()
            ))
        }

        fn duplicate_system_result(
            _text: &str,
            _scope_root: Option<&Path>,
            _include_hidden: bool,
            _limit: usize,
        ) -> Vec<SearchResultItem> {
            vec![mk_item(
                native_root()
                    .join("z-needle.txt")
                    .to_str()
                    .expect("test path is UTF-8"),
                "z-needle.txt",
                EntryKind::Other,
            )]
        }

        let root = native_root();
        let db_path = temp_service_path("persisted-locate-dedupe");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create native root");
        fs::write(root.join("a-needle.txt"), b"first").expect("write first native item");
        fs::write(root.join("z-needle.txt"), b"native metadata")
            .expect("write duplicate native item");
        let config = SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: true,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        };
        let mut writer = SearchService::new_with_store_path(config.clone(), db_path.clone());
        writer.start();
        writer
            .rebuild_index_blocking()
            .expect("persist native generation");
        writer.stop();
        let reader = SearchService::load_persisted(config, db_path.clone())
            .expect("load persisted generation");

        let response = reader.handle().query_with_system_locate(
            SearchQuery {
                text: "needle".to_string(),
                scope: SearchScope::Global,
                root_path: None,
                include_hidden_system: false,
                sort: SearchSort::Relevance,
                limit: 1,
                offset: 0,
                content_search: false,
            },
            duplicate_system_result,
        );

        assert_eq!(
            response.total, 2,
            "exact native path must not be counted twice"
        );
        let _ = clear_sqlite_storage(&db_path);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn global_locate_output_is_not_bounded_to_native_roots() {
        let paths = parse_system_locate_output(
            b"/home/alice/native/needle.txt\n/opt/shared/needle.txt\n",
            None,
            true,
            20,
        );

        assert_eq!(
            paths,
            vec![
                PathBuf::from("/home/alice/native/needle.txt"),
                PathBuf::from("/opt/shared/needle.txt"),
            ]
        );
    }

    #[test]
    fn current_folder_locate_output_cannot_escape_its_scope() {
        let paths = parse_system_locate_output(
            b"/home/alice/current/needle.txt\n/home/alice/currently/needle.txt\n/opt/shared/needle.txt\n",
            Some(Path::new("/home/alice/current")),
            true,
            20,
        );

        assert_eq!(paths, vec![PathBuf::from("/home/alice/current/needle.txt")]);
    }

    #[test]
    fn inaccessible_system_locate_path_remains_a_truthful_path_only_result() {
        let path = Path::new("/protected/reports/needle.txt");

        let item = make_system_locate_item_with(path, |_| {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        })
        .expect("database-visible inaccessible path remains discoverable");

        assert_eq!(item.path, path);
        assert_eq!(item.parent_path, Path::new("/protected/reports"));
        assert_eq!(item.name, "needle.txt");
        assert_eq!(item.kind, EntryKind::Other);
        assert!(!item.is_executable);
        assert_eq!(item.symlink_target_is_dir, None);
        assert_eq!(item.size_bytes, None);
        assert_eq!(item.modified_unix_secs, None);
        assert_eq!(item.path_lower, "/protected/reports/needle.txt");
    }

    #[test]
    fn provably_absent_system_locate_path_is_omitted_as_stale() {
        let item = make_system_locate_item_with(Path::new("/stale/needle.txt"), |_| {
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        });

        assert_eq!(item, None);
    }

    #[test]
    fn fallback_walk_respects_scope_and_query() {
        let root =
            crate::test_temp_dir().join(format!("ottrin-search-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sub")).expect("create test dirs");
        fs::write(root.join("sub/needle.txt"), b"ok").expect("create test file");
        fs::write(root.join("sub/other.bin"), b"ok").expect("create non-match file");

        let query = SearchQuery {
            text: "needle".to_string(),
            scope: SearchScope::CurrentFolder,
            root_path: Some(root.clone()),
            include_hidden_system: true,
            sort: SearchSort::Relevance,
            limit: 100,
            offset: 0,
            content_search: false,
        };
        let expr = parse_query(&query.text).expect("parse query");
        let hits = query_current_folder_fallback(&query, &expr, &SearchConfig::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1.name, "needle.txt");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn current_folder_outside_index_uses_fallback_even_when_index_ready() {
        let indexed_root =
            crate::test_temp_dir().join(format!("ottrin-indexed-{}", std::process::id()));
        let outside_root =
            crate::test_temp_dir().join(format!("ottrin-outside-{}", std::process::id()));
        let _ = fs::remove_dir_all(&indexed_root);
        let _ = fs::remove_dir_all(&outside_root);
        fs::create_dir_all(&indexed_root).expect("create indexed root");
        fs::create_dir_all(outside_root.join("sub")).expect("create outside root");
        fs::write(outside_root.join("sub/needle.txt"), b"ok").expect("create outside file");

        let service = SearchService::new(SearchConfig {
            include_roots: vec![indexed_root.clone()],
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items = Arc::new(Vec::new());
            idx.configured_root_count = 1;
            idx.watched_root_count = 1;
            idx.total_roots = 1;
        }

        let response = service.query(SearchQuery {
            text: "needle".to_string(),
            scope: SearchScope::CurrentFolder,
            root_path: Some(outside_root.clone()),
            include_hidden_system: true,
            sort: SearchSort::Relevance,
            limit: 50,
            offset: 0,
            content_search: false,
        });

        assert_eq!(response.total, 1);
        assert_eq!(response.items[0].name, "needle.txt");

        let _ = fs::remove_dir_all(&indexed_root);
        let _ = fs::remove_dir_all(&outside_root);
    }

    #[test]
    fn global_fallback_prioritizes_active_folder_hint() {
        let home_like =
            crate::test_temp_dir().join(format!("ottrin-search-home-{}", std::process::id()));
        let preferred = home_like.join("pcloud");
        let _ = fs::remove_dir_all(&home_like);
        fs::create_dir_all(preferred.join("docs")).expect("create preferred dirs");
        fs::write(preferred.join("docs/needle.txt"), b"ok").expect("create preferred file");

        let query = SearchQuery {
            text: "needle".to_string(),
            scope: SearchScope::Global,
            root_path: Some(preferred.clone()),
            include_hidden_system: true,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: false,
        };
        let roots = ordered_query_roots(
            &query,
            &SearchConfig {
                include_roots: vec![home_like.clone()],
                ..SearchConfig::default()
            },
        );

        assert_eq!(roots.first(), Some(&preferred));

        let expr = parse_query(&query.text).expect("parse query");
        let hits = query_global_fallback(
            &query,
            &expr,
            &roots,
            &SearchConfig {
                include_roots: vec![home_like.clone()],
                ..SearchConfig::default()
            },
            2_000,
            20,
            Duration::from_millis(250),
        );

        assert!(
            hits.iter()
                .any(|(_, item)| item.path.ends_with("needle.txt"))
        );

        let _ = fs::remove_dir_all(&home_like);
    }

    #[test]
    fn content_search_returns_content_matches_with_snippets() {
        let root =
            crate::test_temp_dir().join(format!("ottrin-content-search-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");

        let file_path = root.join("alpha.txt");
        fs::write(&file_path, b"first line\nneedle appears here\nthird line\n")
            .expect("create test file");

        let service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items = Arc::new(Vec::new());
        }

        let filename_only = service.query(SearchQuery {
            text: "needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: false,
        });
        assert!(filename_only.items.is_empty());

        let base_query = SearchQuery {
            text: "needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: false,
        };
        let response = service.query(SearchQuery {
            content_search: true,
            ..base_query
        });

        assert_eq!(response.total, 1);
        assert_eq!(response.items.len(), 1);
        let item = &response.items[0];
        assert_eq!(item.path, file_path);
        assert_eq!(item.name, "alpha.txt");
        assert_eq!(item.content_snippet.as_deref(), Some("needle appears here"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn live_content_provider_failures_are_not_empty_matches() {
        let root = crate::test_temp_dir().join("provider-errors");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("document.txt"), b"provider-needle").unwrap();
        let mut config = SearchConfig {
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        };
        let expr = parse_query("provider-needle").unwrap();
        let missing = root.join("missing-provider");
        let error = query_content_ripgrep(
            &expr,
            std::slice::from_ref(&root),
            &config,
            false,
            10,
            missing.as_os_str(),
        )
        .expect_err("missing provider must report a failure");
        assert!(error.contains("repair Ottrin"));

        // A real executable that rejects the search-provider arguments exercises
        // nonzero process exit handling on both Unix and Windows.
        let other_program = std::env::current_exe().unwrap();
        let error = query_content_ripgrep(
            &expr,
            std::slice::from_ref(&root),
            &config,
            false,
            10,
            other_program.as_os_str(),
        )
        .expect_err("failed provider must report a failure");
        assert!(error.contains("could not finish"));

        // Disabled content search must never try to start even a missing tool.
        config.content_indexing_enabled = false;
        assert!(
            query_content_ripgrep(
                &expr,
                std::slice::from_ref(&root),
                &config,
                false,
                10,
                missing.as_os_str(),
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn live_content_roots_are_independent_of_metadata_roots_and_honor_folder_scope() {
        let root = crate::test_temp_dir();
        let content_root = root.join("content");
        let other_folder = root.join("other");
        fs::create_dir(&content_root).unwrap();
        fs::create_dir(&other_folder).unwrap();
        fs::write(content_root.join("document.txt"), b"independent-body-probe").unwrap();
        let service = SearchService::new(SearchConfig {
            include_roots: Vec::new(),
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![content_root.clone()],
            ..SearchConfig::default()
        });
        let mut query = query_by_name("independent-body-probe");
        query.content_search = true;
        let global = service.query(query.clone());
        assert!(global.error.is_none(), "{:?}", global.error);
        assert_eq!(global.items.len(), 1);
        assert_eq!(global.items[0].path, content_root.join("document.txt"));
        query.scope = SearchScope::CurrentFolder;
        query.root_path = Some(other_folder);
        let local = service.query(query);
        assert!(local.error.is_none());
        assert!(local.items.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn live_content_provider_preserves_newlines_and_non_utf8_filenames() {
        use std::os::unix::ffi::OsStringExt;
        let root = crate::test_temp_dir().join("provider-filenames");
        fs::create_dir(&root).unwrap();
        let names = [
            std::ffi::OsString::from("two\nlines.txt"),
            std::ffi::OsString::from_vec(b"non-utf8-\xff.txt".to_vec()),
        ];
        for name in &names {
            fs::write(root.join(name), b"filename-probe").unwrap();
        }
        let config = SearchConfig {
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        };
        let expr = parse_query("filename-probe").unwrap();
        let results = query_content_ripgrep(
            &expr,
            std::slice::from_ref(&root),
            &config,
            false,
            10,
            &content_search_program(),
        )
        .unwrap();
        assert_eq!(results.len(), 2);
        for name in names {
            assert!(results.iter().any(|item| item.path == root.join(&name)));
        }
    }

    #[cfg(unix)]
    #[test]
    fn live_content_provider_exclusions_override_file_types_before_reading() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let root = crate::test_temp_dir().join("provider-exclusions");
        fs::create_dir(&root).unwrap();
        let private = root.join("private.txt");
        fs::write(&private, b"privacy-probe").unwrap();
        fs::set_permissions(&private, fs::Permissions::from_mode(0o0)).unwrap();
        fs::write(root.join("included.txt"), b"privacy-probe").unwrap();
        let config = SearchConfig {
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            content_exclude_roots: vec![private],
            ..SearchConfig::default()
        };
        let expr = parse_query("privacy-probe").unwrap();
        // Opening the excluded body would make the real provider fail with
        // Permission Denied, even if a later result filter hid that pathname.
        let results = query_content_ripgrep(
            &expr,
            std::slice::from_ref(&root),
            &config,
            false,
            10,
            &content_search_program(),
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, root.join("included.txt"));
    }

    #[test]
    fn content_fallback_honors_selected_file_groups() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-content-groups-search-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        fs::write(root.join("notes.txt"), b"documentneedle").expect("create document");
        fs::write(root.join("main.rs"), b"sourceneedle").expect("create source");
        let service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            content_file_groups: vec![ottrin_core::ContentFileGroup::TextDocuments],
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items = Arc::new(Vec::new());
        }

        let query = |text: &str| {
            service.query(SearchQuery {
                text: text.to_string(),
                scope: SearchScope::Global,
                root_path: None,
                include_hidden_system: false,
                sort: SearchSort::Relevance,
                limit: 20,
                offset: 0,
                content_search: true,
            })
        };

        assert_eq!(query("documentneedle").items.len(), 1);
        assert!(query("sourceneedle").items.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn content_fallback_honors_configured_extraction_size_limit() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-content-limit-search-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        fs::write(root.join("large.txt"), b"largeneedle").expect("create test file");
        let service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            content_max_file_size_bytes: 4,
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items = Arc::new(Vec::new());
        }

        let response = service.query(SearchQuery {
            text: "largeneedle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });

        assert!(response.items.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn disabled_content_indexing_never_searches_file_bodies() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-disabled-content-search-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        fs::write(root.join("body-only.txt"), "private-body-needle")
            .expect("write body-only content");

        let service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: false,
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items = Arc::new(Vec::new());
        }

        let response = service.query(SearchQuery {
            text: "private-body-needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });

        assert!(response.items.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_content_search_excludes_content_policy_roots() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-content-policy-live-search-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let excluded = root.join("excluded");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&excluded).expect("create content roots");
        fs::write(root.join("included.txt"), "shared-content-needle")
            .expect("write included content");
        fs::write(excluded.join("private.txt"), "shared-content-needle")
            .expect("write excluded content");

        let service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            content_exclude_roots: vec![excluded],
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items = Arc::new(Vec::new());
        }

        let response = service.query(SearchQuery {
            text: "shared-content-needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].name, "included.txt");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_content_search_excludes_a_configured_directory_after_it_becomes_a_file() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-content-policy-replaced-root-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let excluded = root.join("was-directory");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create content root");
        fs::write(root.join("included.txt"), "replacement-content-needle")
            .expect("write included content");
        fs::write(&excluded, "replacement-content-needle")
            .expect("replace configured directory with file");

        let service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            content_exclude_roots: vec![excluded],
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items = Arc::new(Vec::new());
        }

        let response = service.query(SearchQuery {
            text: "replacement-content-needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].name, "included.txt");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_content_search_excludes_hidden_policy_roots_when_hidden_search_is_enabled() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-content-policy-hidden-search-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let excluded = root.join(".excluded");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&excluded).expect("create hidden content roots");
        fs::write(root.join(".included.txt"), "hidden-content-needle")
            .expect("write included hidden content");
        fs::write(excluded.join("private.txt"), "hidden-content-needle")
            .expect("write excluded hidden content");

        let service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            content_exclude_roots: vec![excluded],
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items = Arc::new(Vec::new());
        }

        let response = service.query(SearchQuery {
            text: "hidden-content-needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: true,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].name, ".included.txt");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_content_search_honors_nested_include_and_exclude_roots() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-content-policy-nested-search-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let included = root.join("documents");
        let excluded = included.join("private");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&excluded).expect("create nested content roots");
        fs::write(included.join("public.txt"), "nested-content-needle")
            .expect("write included nested content");
        fs::write(excluded.join("private.txt"), "nested-content-needle")
            .expect("write excluded nested content");
        fs::write(root.join("outside.txt"), "nested-content-needle")
            .expect("write content outside include root");

        let service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![included],
            content_exclude_roots: vec![excluded],
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items = Arc::new(Vec::new());
        }

        let response = service.query(SearchQuery {
            text: "nested-content-needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].name, "public.txt");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_content_search_skips_a_scope_beneath_an_excluded_ancestor() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-content-policy-ancestor-search-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let excluded = root.join("excluded");
        let selected = excluded.join("selected");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&selected).expect("create excluded selected root");
        fs::write(selected.join("private.txt"), "ancestor-content-needle")
            .expect("write excluded selected content");

        let service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            content_exclude_roots: vec![excluded],
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items = Arc::new(Vec::new());
        }

        let response = service.query(SearchQuery {
            text: "ancestor-content-needle".to_string(),
            scope: SearchScope::CurrentFolder,
            root_path: Some(selected),
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });

        assert!(response.items.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_content_search_does_not_cross_disjoint_query_and_content_roots() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-content-policy-disjoint-search-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let content_root = root.join("content");
        let selected = root.join("selected");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&content_root).expect("create content root");
        fs::create_dir_all(&selected).expect("create selected root");
        fs::write(content_root.join("other.txt"), "disjoint-content-needle")
            .expect("write content in configured root");
        fs::write(selected.join("selected.txt"), "disjoint-content-needle")
            .expect("write content in selected root");

        let service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![content_root],
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items = Arc::new(Vec::new());
        }

        let response = service.query(SearchQuery {
            text: "disjoint-content-needle".to_string(),
            scope: SearchScope::CurrentFolder,
            root_path: Some(selected),
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });

        assert!(response.items.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn content_ripgrep_exclude_globs_keep_exact_files_and_descendants_out_of_rg() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-content-ripgrep-globs-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let excluded_file = root.join("private.txt");
        #[cfg(unix)]
        let excluded_metachar_file = root.join("private[?]*!.txt");
        #[cfg(windows)]
        let excluded_metachar_file = root.join("private[!]file.txt");
        let excluded_dir = root.join("private-dir");
        let plan = content_ripgrep_traversal_plan(
            std::slice::from_ref(&root),
            &SearchConfig {
                content_indexing_enabled: true,
                content_include_roots: vec![root.clone()],
                content_exclude_roots: vec![
                    excluded_file.clone(),
                    excluded_metachar_file.clone(),
                    excluded_dir.clone(),
                ],
                ..SearchConfig::default()
            },
        );

        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].root, root);
        fs::create_dir_all(&excluded_dir).expect("create excluded directory");
        fs::write(root.join("included.txt"), "content-policy-needle").expect("write included file");
        fs::write(&excluded_file, "content-policy-needle").expect("write excluded file");
        fs::write(&excluded_metachar_file, "content-policy-needle")
            .expect("write excluded metacharacter file");
        fs::write(excluded_dir.join("child.txt"), "content-policy-needle")
            .expect("write excluded child");

        let mut command = internal_command("rg");
        command
            .current_dir(&root)
            .arg("--files-with-matches")
            .arg("--fixed-strings")
            .arg("--ignore-case")
            .arg("--max-count=1");
        for glob in &plan[0].exclude_globs {
            command.arg("--glob").arg(glob);
        }
        let output = command
            .arg("-e")
            .arg("content-policy-needle")
            .arg("--")
            .arg(".")
            .output()
            .expect("run ripgrep");
        assert!(output.status.success());
        let matches = std::str::from_utf8(&output.stdout)
            .expect("ripgrep stdout")
            .lines()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        assert_eq!(matches, vec![PathBuf::from(".").join("included.txt")]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_content_total_counts_filename_shaped_hits_outside_metadata() {
        let root = crate::test_temp_dir().join("live-content-total");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("needle.txt");
        fs::write(&file, "needle").unwrap();
        let service = SearchService::new_with_store_path(
            SearchConfig {
                include_roots: Vec::new(),
                content_indexing_enabled: true,
                content_include_roots: vec![root.clone()],
                use_system_locate: false,
                ..SearchConfig::default()
            },
            root.join("unused.db"),
        );
        let answer = service.query(SearchQuery {
            text: "needle".into(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });
        assert!(answer.error.is_none(), "{:?}", answer.error);
        assert_eq!(answer.total, 1);
        assert_eq!(answer.items.len(), 1);
        assert_eq!(
            fs::canonicalize(&answer.items[0].path).unwrap(),
            fs::canonicalize(file).unwrap()
        );
    }

    #[test]
    fn indexed_and_live_content_merge_never_counts_a_path_twice() {
        let root = crate::test_temp_dir().join("content-page-dedupe");
        fs::create_dir_all(&root).unwrap();
        let bodies = root.join("bodies");
        fs::create_dir(&bodies).unwrap();
        for index in 0..70 {
            fs::write(bodies.join(format!("document-{index}.txt")), "bodyneedle").unwrap();
        }
        let config = SearchConfig {
            include_roots: Vec::new(),
            content_indexing_enabled: true,
            content_include_roots: vec![bodies],
            use_system_locate: false,
            ..SearchConfig::default()
        };
        let store = root.join("search.db");
        let mut service = SearchService::new_with_store_path(config.clone(), store.clone());
        service.start();
        service.rebuild_index_blocking().unwrap();
        service.stop();
        let loaded = SearchService::load_persisted(config, store).unwrap();
        let answer = loaded.query(SearchQuery {
            text: "bodyneedle".into(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });
        assert!(answer.error.is_none(), "{:?}", answer.error);
        assert_eq!(answer.items.len(), 20);
        assert!(
            answer.total >= 20 && answer.total <= 70,
            "{} results for 70 paths",
            answer.total
        );
    }

    #[test]
    fn filename_query_paths_share_completed_metadata_membership() {
        let root = crate::test_temp_dir().join("metadata-snapshot-membership");
        let native = root.join("native");
        let build = native.join("build");
        fs::create_dir_all(&build).unwrap();
        let file = build.join("logged.txt");
        fs::write(&file, "original").unwrap();
        let config = SearchConfig {
            include_roots: vec![native],
            use_system_locate: false,
            manage_locate_db: false,
            ..SearchConfig::default()
        };
        let store = root.join("search.db");
        let mut service = SearchService::new_with_store_path(config.clone(), store.clone());
        service.start();
        service.rebuild_index_blocking().unwrap();
        service.stop();
        fs::write(build.join("build.ninja"), "new generated-output marker").unwrap();
        let loaded = SearchService::load_persisted(config, store).unwrap();
        for text in ["logged", "logged name:logged"] {
            let response = loaded.query(SearchQuery {
                text: text.into(),
                scope: SearchScope::Global,
                root_path: None,
                include_hidden_system: false,
                sort: SearchSort::Relevance,
                limit: 20,
                offset: 0,
                content_search: false,
            });
            assert!(response.error.is_none(), "{:?}", response.error);
            assert_eq!(
                response.total, 1,
                "query {text} must use the same completed membership"
            );
            assert_eq!(response.items[0].path, file);
        }
    }

    #[test]
    fn independent_content_roots_persist_without_filename_membership() {
        for (label, nested, no_metadata) in [
            ("disjoint", false, false),
            ("excluded-nested", true, false),
            ("content-only", false, true),
        ] {
            let root = crate::test_temp_dir().join(format!("saved-content-{label}"));
            let metadata = root.join("native");
            let bodies = if nested {
                metadata.join("bodies")
            } else {
                root.join("bodies")
            };
            fs::create_dir_all(&metadata).unwrap();
            fs::create_dir_all(&bodies).unwrap();
            fs::write(
                metadata.join("catalog-only.txt"),
                "metadata body is not admitted",
            )
            .unwrap();
            let file = bodies.join("needle.txt");
            fs::write(&file, "needle persisted content").unwrap();
            fs::write(bodies.join("unsupported.bin"), "needle not admitted").unwrap();
            let config = SearchConfig {
                include_roots: if no_metadata {
                    Vec::new()
                } else {
                    vec![metadata]
                },
                exclude_roots: vec![bodies.clone()],
                content_indexing_enabled: true,
                content_include_roots: vec![bodies.clone()],
                content_file_groups: vec![ottrin_core::ContentFileGroup::TextDocuments],
                use_system_locate: false,
                manage_locate_db: false,
                refresh_interval_hours: 0,
                ..SearchConfig::default()
            };
            let store = root.join("search.db");
            let mut service = SearchService::new_with_store_path(config.clone(), store.clone());
            service.start();
            service.rebuild_index_blocking().unwrap();
            service.stop();
            // A read-only reload with the source absent proves saved extraction,
            // rather than the live provider, supplies this hit.
            fs::remove_file(&file).unwrap();
            let loaded = SearchService::load_persisted(config.clone(), store).unwrap();
            let query = |text: &str, content_search| SearchQuery {
                text: text.into(),
                content_search,
                scope: SearchScope::Global,
                root_path: None,
                include_hidden_system: false,
                sort: SearchSort::Relevance,
                limit: 20,
                offset: 0,
            };
            for text in ["needle", "needle name:needle"] {
                let names = loaded.query(query(text, false));
                assert!(names.error.is_none(), "{label}: {:?}", names.error);
                assert_eq!(names.total, 0, "content-only metadata leaked: {label}");
            }
            let content = loaded.query(query("needle", true));
            assert!(content.error.is_none(), "{label}: {:?}", content.error);
            assert_eq!(content.total, 1, "content hit total: {label}");
            assert_eq!(content.items.len(), 1, "content hit count: {label}");
            assert_eq!(content.items[0].path, file, "{label}");
            assert!(
                content.items[0]
                    .content_snippet
                    .as_ref()
                    .unwrap()
                    .contains("persisted")
            );
            let plan = plan::ScanPlan::new(&config).unwrap();
            let mut paths = Vec::new();
            db::stream_generation_items(loaded.store_path.as_ref(), &plan, None, |item| {
                paths.push(item.path)
            })
            .unwrap();
            assert!(
                !paths.iter().any(|path| path.starts_with(&bodies)),
                "{label}"
            );
            assert_eq!(paths.len(), usize::from(!no_metadata));
        }
    }

    #[test]
    fn nested_content_root_keeps_its_metadata_directory_and_unique_rows() {
        let root = crate::test_temp_dir().join("shared-nested-content");
        let bodies = root.join("bodies");
        fs::create_dir_all(&bodies).unwrap();
        fs::write(bodies.join("needle.txt"), "needle").unwrap();
        let config = SearchConfig {
            include_roots: vec![root.clone()],
            content_indexing_enabled: true,
            content_include_roots: vec![bodies.clone()],
            use_system_locate: false,
            manage_locate_db: false,
            ..SearchConfig::default()
        };
        let plan = plan::ScanPlan::new(&config).unwrap();
        let scanned = scanner::scan(&plan, &AtomicBool::new(false), |_| {}).unwrap();
        let scanner::ScanOutcome::Completed(items) = scanned else {
            panic!("scan incomplete");
        };
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|item| item.item().path == bodies));
        assert_eq!(
            items
                .iter()
                .filter(|item| item.item().name == "needle.txt")
                .count(),
            1
        );
    }

    #[test]
    fn unavailable_content_root_keeps_metadata_usable_and_recovers_its_watch() {
        let root = crate::test_temp_dir().join("content-root-availability");
        let native = root.join("native");
        let bodies = root.join("content-return");
        fs::create_dir_all(&native).unwrap();
        let catalog = native.join("catalog.txt");
        fs::write(&catalog, "metadata").unwrap();
        let config = SearchConfig {
            include_roots: vec![native],
            content_indexing_enabled: true,
            content_include_roots: vec![bodies.clone()],
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        };
        let store = root.join("search.db");
        let mut service = SearchService::new_with_store_path(config.clone(), store.clone());
        service.start();
        service.rebuild_index_blocking().unwrap();
        let answer = service.query(SearchQuery {
            text: "catalog".into(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Name,
            limit: 20,
            offset: 0,
            content_search: false,
        });
        assert!(answer.error.is_none(), "{:?}", answer.error);
        assert_eq!(answer.items.len(), 1);
        assert_eq!(answer.items[0].path, catalog);
        let diagnostics = service.diagnostics();
        assert!(!diagnostics.watcher_ready);
        assert!(diagnostics.watcher_last_error.is_some());
        fs::create_dir(&bodies).unwrap();
        let file = bodies.join("document.txt");
        fs::write(&file, "returnneedle").unwrap();
        let plan = plan::ScanPlan::new(&config).unwrap();
        // Preserve the production 60-second registration backoff.
        let until = Instant::now() + Duration::from_secs(75);
        loop {
            let hits =
                db::query_indexed_content_page(&store, &plan, "returnneedle", 20, 0).unwrap();
            if hits.len() == 1 && service.diagnostics().watcher_ready {
                assert_eq!(hits[0].item.path, file);
                break;
            }
            assert!(
                Instant::now() < until,
                "returning folder did not recover: {:?}; hits {}",
                service.diagnostics(),
                hits.len()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        fs::write(&file, "afterreturnneedle").unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if db::query_indexed_content_page(&store, &plan, "afterreturnneedle", 20, 0)
                .unwrap()
                .len()
                == 1
            {
                break;
            }
            assert!(
                Instant::now() < until,
                "reattached watch misses later writes"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        service.stop();
    }

    #[test]
    fn content_only_watcher_updates_moves_deletes_and_detaches_when_disabled() {
        let root = crate::test_temp_dir().join("content-only-live-watcher");
        let bodies = root.join("bodies");
        fs::create_dir_all(&bodies).unwrap();
        let mut config = SearchConfig {
            include_roots: Vec::new(),
            content_indexing_enabled: true,
            content_include_roots: vec![bodies.clone()],
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        };
        let store = root.join("search.db");
        let mut service = SearchService::new_with_store_path(config.clone(), store.clone());
        service.start();
        service.rebuild_index_blocking().unwrap();
        let wait = |term: &str, expected: &[PathBuf]| {
            let plan = plan::ScanPlan::new(&config).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let hits = db::query_indexed_content_page(&store, &plan, term, 20, 0).unwrap();
                let got = hits
                    .into_iter()
                    .map(|hit| hit.item.path)
                    .collect::<Vec<_>>();
                if got == expected {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "saved watcher misses {term}: {got:?}, expected {expected:?}"
                );
                std::thread::sleep(Duration::from_millis(30));
            }
        };
        let directory = bodies.join("new");
        fs::create_dir(&directory).unwrap();
        let file = directory.join("document.txt");
        fs::write(&file, "olderneedle").unwrap();
        wait("olderneedle", std::slice::from_ref(&file));
        fs::write(&file, "freshneedle").unwrap();
        wait("freshneedle", std::slice::from_ref(&file));
        wait("olderneedle", &[]);
        let moved = bodies.join("moved");
        fs::rename(&directory, &moved).unwrap();
        wait("freshneedle", &[moved.join("document.txt")]);
        fs::remove_dir_all(&moved).unwrap();
        wait("freshneedle", &[]);
        let revision = service.diagnostics().snapshot_revision;
        config.content_indexing_enabled = false;
        service.update_config(config.clone());
        service.rebuild_index_blocking().unwrap();
        let diagnostics = service.diagnostics();
        assert!(diagnostics.snapshot_revision > revision);
        assert_eq!(diagnostics.configured_root_count, 0);
        assert_eq!(diagnostics.watched_root_count, 0);
        assert_eq!(diagnostics.last_completed_indexed_items, Some(0));
        service.stop();
    }

    #[test]
    fn content_search_uses_active_fts_generation_before_ripgrep_fallback() {
        let _guard = SEARCH_SERVICE_TEST_LOCK.lock().expect("test lock");
        let root = crate::test_temp_dir().join(format!(
            "ottrin-fts-content-search-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        let file_path = root.join("indexed.txt");
        fs::write(&file_path, "first line\nneedle from indexed generation\n")
            .expect("write indexed content");

        let mut service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        });
        service.start();
        service
            .rebuild_index_blocking()
            .expect("commit indexed generation");
        fs::remove_file(&file_path).expect("remove source after indexing");

        let response = service.query(SearchQuery {
            text: "needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].path, file_path);
        assert_eq!(
            response.items[0].content_snippet.as_deref(),
            Some("needle from indexed generation")
        );

        service.stop();
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&service.store_path);
    }

    #[test]
    fn indexed_content_search_applies_name_filters_without_querying_them_as_content() {
        let _guard = SEARCH_SERVICE_TEST_LOCK.lock().expect("test lock");
        let root = crate::test_temp_dir().join(format!(
            "ottrin-fts-filtered-content-search-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        let file_path = root.join("alpha.txt");
        fs::write(&file_path, "needle from indexed generation\n").expect("write indexed content");

        let mut service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        });
        service.start();
        service
            .rebuild_index_blocking()
            .expect("commit indexed generation");
        fs::remove_file(&file_path).expect("remove source after indexing");

        let response = service.query(SearchQuery {
            text: "name:alpha needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].path, file_path);
        assert_eq!(
            response.items[0].content_snippet.as_deref(),
            Some("needle from indexed generation")
        );

        service.stop();
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&service.store_path);
    }

    #[test]
    fn indexed_content_search_keeps_multi_word_matches_when_terms_are_on_separate_lines() {
        let _guard = SEARCH_SERVICE_TEST_LOCK.lock().expect("test lock");
        let root = crate::test_temp_dir().join(format!(
            "ottrin-fts-multiword-content-search-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        let file_path = root.join("notes.txt");
        fs::write(&file_path, "alpha on one line\nbeta on another line\n")
            .expect("write indexed content");

        let mut service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        });
        service.start();
        service
            .rebuild_index_blocking()
            .expect("commit indexed generation");
        fs::remove_file(&file_path).expect("remove source after indexing");

        let response = service.query(SearchQuery {
            text: "alpha beta".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].path, file_path);

        service.stop();
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&service.store_path);
    }

    #[test]
    fn indexed_content_search_merges_fts_evidence_into_filename_matches() {
        let _guard = SEARCH_SERVICE_TEST_LOCK.lock().expect("test lock");
        let root = crate::test_temp_dir().join(format!(
            "ottrin-fts-merge-content-search-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        let file_path = root.join("needle.txt");
        fs::write(&file_path, "needle appears in indexed content\n")
            .expect("write indexed content");

        let mut service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        });
        service.start();
        service
            .rebuild_index_blocking()
            .expect("commit indexed generation");
        fs::remove_file(&file_path).expect("remove source after indexing");

        let response = service.query(SearchQuery {
            text: "needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].path, file_path);
        assert!(
            response.items[0]
                .content_snippet
                .as_deref()
                .is_some_and(|snippet| snippet.contains("needle"))
        );

        service.stop();
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&service.store_path);
    }

    #[test]
    fn negated_content_search_requires_a_completed_generation() {
        let service = SearchService::new(SearchConfig {
            include_roots: vec![PathBuf::from("/snapshot")],
            use_system_locate: false,
            ..SearchConfig::default()
        });
        let item = mk_item("/snapshot/foo.txt", "foo.txt", EntryKind::File);
        {
            let mut index = service.index.write().expect("index write");
            index.status = SearchIndexStatus::Indexing;
            index.items = Arc::new(vec![item.clone()]);
        }
        let response = service.query(SearchQuery {
            text: "!bar".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });
        assert!(response.items.is_empty());
        assert_eq!(response.total, 0);
        assert!(
            response
                .error
                .as_deref()
                .is_some_and(|error| error.contains("completed indexed generation"))
        );
    }

    #[test]
    fn negated_filename_filter_keeps_content_fallback_available_while_indexing() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-negated-name-content-fallback-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        let wanted = root.join("wanted.txt");
        let excluded = root.join("excluded.txt");
        fs::write(&wanted, "needle in the wanted document\n").expect("write wanted content");
        fs::write(&excluded, "needle in the excluded document\n").expect("write excluded content");

        let service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        });
        service.index.write().expect("index write").status = SearchIndexStatus::Indexing;

        let response = service.query(SearchQuery {
            text: "!name:excluded needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Path,
            limit: 20,
            offset: 0,
            content_search: true,
        });

        assert!(response.error.is_none());
        assert_eq!(response.total, 1);
        assert_eq!(response.items[0].path, wanted);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn quoted_negated_content_is_treated_as_a_literal_not_fts_syntax() {
        let _guard = SEARCH_SERVICE_TEST_LOCK.lock().expect("test lock");
        let root = crate::test_temp_dir().join(format!(
            "ottrin-negated-content-literal-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        let literal = root.join("literal.txt");
        let other = root.join("other.txt");
        fs::write(&literal, "alpha OR beta\n").expect("write literal content");
        fs::write(&other, "alpha beta without the literal operator\n")
            .expect("write other content");

        let mut service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        });
        service.start();
        service
            .rebuild_index_blocking()
            .expect("commit indexed generation");

        let response = service.query(SearchQuery {
            text: "!\"alpha OR beta\"".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Path,
            limit: 20,
            offset: 0,
            content_search: true,
        });

        assert!(response.error.is_none());
        assert_eq!(response.total, 1);
        assert_eq!(response.items[0].path, other);

        service.stop();
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(&service.store_path);
    }

    #[test]
    fn negated_content_search_returns_only_generation_complete_content_absences() {
        use crate::mutation::{IndexMutation, OwnedItem};
        use crate::plan::ScanPlan;

        let root = crate::test_temp_dir().join(format!(
            "ottrin-negated-fts-completeness-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        let complete = root.join("complete.txt");
        let empty = root.join("empty.txt");
        let binary = root.join("binary.txt");
        let oversized = root.join("oversized.txt");
        let vanished = root.join("vanished.txt");
        let hidden_binary = root.join(".hidden-binary.txt");
        fs::write(&complete, "ordinary searchable text").expect("write complete content");
        fs::write(&empty, "").expect("write empty content");
        fs::write(&binary, b"needle\0binary").expect("write binary content");
        fs::write(&oversized, vec![b'x'; 512 * 1024 + 1]).expect("write oversized content");
        fs::write(&hidden_binary, b"needle\0hidden").expect("write hidden binary content");

        let config = SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        };
        let service = SearchService::new(config.clone());
        let plan = ScanPlan::new(&config).expect("plan");
        let make_owned = |path: &Path| {
            let mut item = mk_item(
                path.to_str().expect("UTF-8 temporary path"),
                path.file_name()
                    .and_then(|name| name.to_str())
                    .expect("file name"),
                EntryKind::File,
            );
            item.size_bytes = fs::metadata(path).ok().map(|metadata| metadata.len());
            OwnedItem::new(root.clone(), item)
        };
        let mut store = SearchStore::open(service.store_path.clone()).expect("open real DB");
        let mut items = vec![
            make_owned(&complete),
            make_owned(&empty),
            make_owned(&binary),
            make_owned(&oversized),
            make_owned(&hidden_binary),
        ];
        let mut vanished_item = mk_item(
            vanished.to_str().expect("UTF-8 temporary path"),
            "vanished.txt",
            EntryKind::File,
        );
        vanished_item.size_bytes = Some(10);
        items.push(OwnedItem::new(root.clone(), vanished_item));
        store
            .commit(&plan, IndexMutation::ReplaceAll(items))
            .expect("commit generation");
        {
            let mut index = service.index.write().expect("index write");
            index.status = SearchIndexStatus::Ready;
        }

        let result = service.query_with_diagnostics(SearchQuery {
            text: "!needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Path,
            limit: 20,
            offset: 0,
            content_search: true,
        });
        let response = result.response;

        assert!(response.error.is_none());
        assert_eq!(
            result.warning.as_deref(),
            Some("Excluded 3 item(s) with unknown indexed content from negated content search.")
        );
        assert_eq!(
            response
                .items
                .iter()
                .map(|item| &item.path)
                .collect::<Vec<_>>(),
            vec![&complete, &empty]
        );
        let bounded_response = service.query(SearchQuery {
            limit: 1,
            ..SearchQuery {
                text: "!needle".to_string(),
                scope: SearchScope::Global,
                root_path: None,
                include_hidden_system: false,
                sort: SearchSort::Path,
                limit: 20,
                offset: 0,
                content_search: true,
            }
        });
        assert_eq!(bounded_response.total, 2);
        assert_eq!(bounded_response.items.len(), 1);
        assert_eq!(bounded_response.items[0].path, complete);
        let cancelled = AtomicBool::new(true);
        let cancelled_response = service.handle().query_cancellable(
            SearchQuery {
                text: "!needle".to_string(),
                scope: SearchScope::Global,
                root_path: None,
                include_hidden_system: false,
                sort: SearchSort::Path,
                limit: 20,
                offset: 0,
                content_search: true,
            },
            &cancelled,
        );
        assert!(cancelled_response.items.is_empty());
        assert!(
            cancelled_response
                .error
                .as_deref()
                .is_some_and(|error| error.contains("was cancelled"))
        );

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(&service.store_path);
    }

    #[test]
    fn unknown_content_warning_is_truthfully_bounded() {
        assert_eq!(unknown_content_warning(0), None);
        assert_eq!(
            unknown_content_warning(1_000).as_deref(),
            Some("Excluded 999+ item(s) with unknown indexed content from negated content search.")
        );
    }

    #[test]
    fn diagnostic_query_result_is_serializable_without_changing_search_response_shape() {
        fn assert_serializable<T: Serialize>() {}
        assert_serializable::<SearchQueryResult>();
        assert_serializable::<SearchResponse>();
    }

    #[test]
    fn indexed_content_search_preserves_or_branch_filters() {
        let _guard = SEARCH_SERVICE_TEST_LOCK.lock().expect("test lock");
        let root = crate::test_temp_dir().join(format!(
            "ottrin-fts-or-content-search-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        let foo = root.join("foo.txt");
        let good_bar = root.join("good-bar.txt");
        let bad_bar = root.join("bad-bar.txt");
        fs::write(&foo, "foo only\n").expect("write foo content");
        fs::write(&good_bar, "bar only\n").expect("write good bar content");
        fs::write(&bad_bar, "bar only\n").expect("write bad bar content");

        let mut service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        });
        service.start();
        service
            .rebuild_index_blocking()
            .expect("commit indexed generation");
        for path in [&foo, &good_bar, &bad_bar] {
            fs::remove_file(path).expect("remove source after indexing");
        }

        let response = service.query(SearchQuery {
            text: "foo | (bar name:good)".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });
        let paths = response
            .items
            .iter()
            .map(|item| item.path.clone())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(paths, std::collections::HashSet::from([foo, good_bar]));

        service.stop();
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&service.store_path);
    }

    #[test]
    fn indexed_content_search_pages_past_rejected_fts_candidates() {
        let _guard = SEARCH_SERVICE_TEST_LOCK.lock().expect("test lock");
        let root = crate::test_temp_dir().join(format!(
            "ottrin-fts-paged-content-search-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        for index in 0..51 {
            fs::write(root.join(format!("other-{index:02}.txt")), "foo\n")
                .expect("write rejected content");
        }
        let target = root.join("target.txt");
        fs::write(&target, "foo\n").expect("write target content");

        let mut service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        });
        service.start();
        service
            .rebuild_index_blocking()
            .expect("commit indexed generation");
        fs::remove_dir_all(&root).expect("remove sources after indexing");

        let response = service.query(SearchQuery {
            text: "name:target foo".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 1,
            offset: 0,
            content_search: true,
        });
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].path, target);

        service.stop();
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&service.store_path);
    }

    #[test]
    fn content_search_matches_multi_word_queries() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-content-search-multiword-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");

        let file_path = root.join("notes.txt");
        fs::write(
            &file_path,
            b"alpha on the first line\nbeta on the second line\n",
        )
        .expect("create test file");

        let service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items = Arc::new(Vec::new());
        }

        let response = service.query(SearchQuery {
            text: "alpha beta".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });

        assert_eq!(response.total, 1);
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].path, file_path);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn content_search_snippets_handle_colons_in_paths() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-content-search-colons-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");

        #[cfg(unix)]
        let file_path = root.join("alpha:beta.txt");
        #[cfg(windows)]
        let file_path = root.join("alpha-beta.txt");
        fs::write(&file_path, b"first line\nneedle appears here\nthird line\n")
            .expect("create test file");

        let expr = parse_query("needle").expect("parse query");
        let config = SearchConfig {
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        };
        let results = query_content_ripgrep(
            &expr,
            std::slice::from_ref(&root),
            &config,
            false,
            10,
            &content_search_program(),
        )
        .expect("live content search");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, file_path);
        assert_eq!(
            results[0].content_snippet.as_deref(),
            Some("needle appears here")
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn content_search_honors_query_filters() {
        let root = crate::test_temp_dir().join(format!(
            "ottrin-content-search-filters-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");

        let file_path = root.join("alpha.txt");
        fs::write(&file_path, b"needle inside a file\n").expect("create test file");

        let service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items = Arc::new(Vec::new());
        }

        let response = service.query(SearchQuery {
            text: "type:dir needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 20,
            offset: 0,
            content_search: true,
        });

        assert!(response.items.is_empty());
        assert_eq!(response.total, 0);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plain_term_matches_name_and_path_as_substring() {
        let expr = parse_query("angel").expect("parse query");
        // Direct name match
        let name_match = mk_item(
            "/home/user/projects/angelcat",
            "angelcat",
            EntryKind::Directory,
        );
        // Path match: "angel" appears inside a parent directory name
        let path_match = mk_item(
            "/home/user/angelcat/deep/file.txt",
            "file.txt",
            EntryKind::File,
        );
        // No match: "angel" appears neither in the name nor the path
        let no_match = mk_item(
            "/home/user/pictures/sunset.jpg",
            "sunset.jpg",
            EntryKind::File,
        );
        assert!(eval_expr(&expr, &name_match));
        assert!(eval_expr(&expr, &path_match));
        assert!(!eval_expr(&expr, &no_match));
    }

    #[test]
    fn built_in_generated_folder_excludes_are_recursive() {
        let root = Path::new("/home/campbell");
        assert!(path_has_builtin_excluded_component(
            Path::new("/home/campbell/Projects/Ottrin/target/debug/deps/file.d"),
            root
        ));
        assert!(path_has_builtin_excluded_component(
            Path::new("/home/campbell/site/node_modules/pkg/index.js"),
            root
        ));
        assert!(!path_has_builtin_excluded_component(
            Path::new("/home/campbell/Documents/target-notes.txt"),
            root
        ));
    }

    #[test]
    fn built_in_windows_system_folder_excludes_are_recursive_and_case_insensitive() {
        let root = Path::new("/scan");
        assert!(path_has_builtin_excluded_component(
            Path::new("/scan/$Recycle.Bin/S-1-5-18/desktop.ini"),
            root
        ));
        assert!(path_has_builtin_excluded_component(
            Path::new("/scan/$RECYCLE.BIN/S-1-5-21/deleted.txt"),
            root
        ));
        assert!(path_has_builtin_excluded_component(
            Path::new("/scan/System Volume Information/tracking.log"),
            root
        ));
    }

    /// Benchmark: 500k synthetic items. Debug/test builds keep this as a
    /// correctness and regression smoke test; the strict budget is enforced in
    /// release where optimizer noise is low enough to be meaningful.
    /// Run with: cargo test -p ottrin-search --release -- bench_query_500k --nocapture
    #[test]
    fn bench_query_500k() {
        const N: usize = 500_000;
        // Build a realistic synthetic index: mix of files and directories.
        let mut items: Vec<SearchResultItem> = (0..N)
            .map(|i| {
                let dir = format!(
                    "/home/user/projects/repo_{}/src/module_{}",
                    i / 1000,
                    i % 100
                );
                let name = format!("file_{:06}.rs", i);
                let path = format!("{}/{}", dir, name);
                mk_item(&path, &name, EntryKind::File)
            })
            .collect();
        // Sprinkle in a few items that match the query "needle".
        items[12345] = mk_item("/home/user/docs/needle.txt", "needle.txt", EntryKind::File);
        items[99999] = mk_item(
            "/home/user/projects/needle_lib/main.rs",
            "main.rs",
            EntryKind::File,
        );

        let query = SearchQuery {
            text: "needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Relevance,
            limit: 50,
            offset: 0,
            content_search: false,
        };
        let expr = parse_query(&query.text).expect("parse query");

        let start = std::time::Instant::now();
        let matched: Vec<_> = items
            .iter()
            .filter(|item| !item.name.starts_with('.'))
            .filter_map(|item| {
                if eval_expr(&expr, item) {
                    Some((relevance_score(&query, item), item))
                } else {
                    None
                }
            })
            .collect();
        let elapsed = start.elapsed();

        println!(
            "500k query: {}ms, {} hits",
            elapsed.as_millis(),
            matched.len()
        );
        let budget_ms = if cfg!(debug_assertions) { 250 } else { 100 };
        assert!(
            elapsed.as_millis() < budget_ms,
            "Query over 500k items took {}ms — exceeds {}ms budget",
            elapsed.as_millis(),
            budget_ms
        );
        assert!(
            matched.len() >= 2,
            "Expected at least 2 needle matches, got {}",
            matched.len()
        );
    }
}
