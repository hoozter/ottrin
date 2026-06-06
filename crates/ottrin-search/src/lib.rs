use globset::{Glob, GlobSet, GlobSetBuilder};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use ottrin_core::{
    EntryKind, SearchConfig, SearchIndexStatus, SearchQuery, SearchResponse, SearchResultItem,
    SearchScope, SearchSort,
};
use regex::{Regex, RegexBuilder};
use std::collections::HashSet;
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use walkdir::WalkDir;

mod db;
use db::{SearchDb, SearchDbWriter, search_db_path};

/// Publish this many indexed items to the shared index before the walk completes,
/// so queries return partial results immediately (FSearch-style streaming).
const STREAM_BATCH_SIZE: usize = 2_000;
const INDEX_STALL_SECS: u64 = 600;
const INDEX_STALL_RESTART_COOLDOWN_SECS: u64 = 1_800;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("search parser error: {0}")]
    Parse(String),
}

#[derive(Debug)]
struct IndexState {
    items: Vec<SearchResultItem>,
    status: SearchIndexStatus,
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
            items: Vec::new(),
            status: SearchIndexStatus::Indexing,
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

#[derive(Debug, Clone)]
pub struct SearchIndexDiagnostics {
    pub status: SearchIndexStatus,
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

#[derive(Debug, Clone)]
struct ProgressUpdate {
    message: String,
    active_root: PathBuf,
    active_root_index: usize,
    total_roots: usize,
}

/// A targeted filesystem change that can be applied to the index without a full rebuild.
#[derive(Debug)]
enum NotifyAction {
    /// A path was created or modified — upsert one entry.
    Upsert(PathBuf),
    /// A path was removed — delete it and all children from the index.
    Remove(PathBuf),
}

/// If a single debounce window delivers more events than this, fall back to a full rebuild
/// rather than applying thousands of individual deltas (e.g. large directory move/extract).
const INCREMENTAL_REBUILD_THRESHOLD: usize = 500;

pub struct SearchService {
    config: Arc<RwLock<SearchConfig>>,
    index: Arc<RwLock<IndexState>>,
    stop: Arc<AtomicBool>,
    rebuild: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl SearchService {
    pub fn new(config: SearchConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            index: Arc::new(RwLock::new(IndexState::default())),
            stop: Arc::new(AtomicBool::new(false)),
            rebuild: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        }
    }

    pub fn start(&self) {
        if self.worker.lock().expect("worker lock").is_some() {
            return;
        }
        self.stop.store(false, Ordering::SeqCst);
        let config = Arc::clone(&self.config);
        let index = Arc::clone(&self.index);
        let stop = Arc::clone(&self.stop);
        let rebuild_flag = Arc::clone(&self.rebuild);
        let handle = std::thread::spawn(move || {
            let index_for_panic = Arc::clone(&index);
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
                let mut watcher = RecommendedWatcher::new(
                    move |res| {
                        let _ = tx.send(res);
                    },
                    notify::Config::default(),
                )
                .ok();
                let mut last_cfg: Option<SearchConfig> = None;
                let mut watched_roots: Vec<PathBuf> = Vec::new();
                let mut needs_rebuild = true;
                // Incremental update state: events are debounced over 500ms before applying.
                let mut pending: Vec<NotifyAction> = Vec::new();
                let mut last_event_at: Option<Instant> = None;
                // Tracks when the last full rebuild completed, for scheduled refresh.
                let mut last_full_rebuild_at: Option<Instant> = None;
                // Tracks when we last ran updatedb for the managed locate database.
                let mut last_updatedb_at: Option<Instant> = None;
                let mut locate_db_initialized = false;
                let mut last_stall_restart_at: Option<Instant> = None;

                // Pre-populate from disk cache for instant results on startup.
                // If the cache is fresh (< 30 min), defer the rebuild so we don't thrash.
                if let Some(cached) = load_index_from_disk() {
                    let count = cached.len();
                    let age = index_cache_age_secs().unwrap_or(u64::MAX);
                    let age_str = format_age(age);
                    if let Ok(mut i) = index.write() {
                        i.items = cached;
                        i.status = SearchIndexStatus::Ready;
                        if age < 1_800 {
                            // Fresh enough — skip the immediate rebuild, background watcher is enough.
                            i.progress_text =
                                Some(format!("Indexed {} files · {}", count, age_str));
                            needs_rebuild = false;
                            last_full_rebuild_at = Some(Instant::now()); // Treat as fresh rebuild.
                        } else {
                            i.progress_text = Some(format!(
                                "Cached · {} files · {} · refreshing…",
                                count, age_str
                            ));
                            // needs_rebuild stays true — silent background refresh begins.
                        }
                    }
                }

                loop {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let cfg = config.read().expect("cfg read").clone();
                    if last_cfg.as_ref() != Some(&cfg) {
                        needs_rebuild = true;
                    }
                    if rebuild_flag.swap(false, Ordering::SeqCst) {
                        needs_rebuild = true;
                    }

                    if needs_rebuild {
                        let scan_started = current_unix_secs();
                        let scan_started_instant = Instant::now();
                        let roots = effective_roots(&cfg);
                        {
                            let mut i = index.write().expect("index write");
                            i.status = SearchIndexStatus::Indexing;
                            i.last_error = None;
                            i.configured_root_count = roots.len();
                            i.watched_root_count = 0;
                            i.watcher_ready = false;
                            i.watcher_last_setup_unix_secs = None;
                            i.watcher_last_error = None;
                            i.last_indexed_path = None;
                            i.building_indexed_items = 0;
                            if i.last_completed_indexed_items.is_none() {
                                i.estimated_total_items = estimate_total_items(&cfg, &roots);
                            } else {
                                i.estimated_total_items = None;
                            }
                            i.active_root = None;
                            i.active_root_index = 0;
                            i.total_roots = roots.len();
                            i.last_scan_started_unix_secs = Some(scan_started);
                            i.last_scan_completed_unix_secs = None;
                            i.last_scan_duration_ms = None;
                            i.last_progress_unix_secs = Some(scan_started);
                            i.progress_text = Some("Indexing · scanning…".to_string());
                        }
                        // Reset pending incremental events before streaming a new index build.
                        pending.clear();
                        last_event_at = None;
                        let mut new_items: Vec<SearchResultItem> = Vec::new();
                        let db_writer = SearchDbWriter::open(search_db_path()).ok();
                        let db_writer_arc = Arc::new(Mutex::new(db_writer));
                        let db_writer_arc_for_batch = Arc::clone(&db_writer_arc);
                        match build_index(
                            &cfg,
                            &stop,
                            |progress| {
                                let mut i = index.write().expect("index write");
                                i.progress_text = Some(progress.message);
                                i.active_root = Some(progress.active_root);
                                i.active_root_index = progress.active_root_index;
                                i.total_roots = progress.total_roots;
                                i.last_progress_unix_secs = Some(current_unix_secs());
                            },
                            |batch| {
                                // Stream each batch into the shared index so queries return
                                // partial results before the walk finishes.
                                if let Ok(mut writer_guard) = db_writer_arc_for_batch.lock()
                                    && let Some(ref mut writer) = *writer_guard
                                {
                                    let _ = writer.insert_batch(&batch);
                                    if let Some(last) = batch.last()
                                        && let Some(root) = roots
                                            .iter()
                                            .find(|r: &&PathBuf| last.path.starts_with(r.as_path()))
                                    {
                                        let _ = writer.set_scan_cursor(
                                            root.as_path(),
                                            Some(last.path.as_path()),
                                        );
                                    }
                                }
                                let mut i = index.write().expect("index write");
                                if let Some(last) = batch.last() {
                                    i.last_indexed_path = Some(last.path.clone());
                                }
                                i.building_indexed_items =
                                    i.building_indexed_items.saturating_add(batch.len());
                                i.last_progress_unix_secs = Some(current_unix_secs());
                                new_items.extend(batch);
                            },
                        ) {
                            Ok(total) => {
                                if stop.load(Ordering::SeqCst) {
                                    break;
                                }
                                // Clear scan cursors using the same Arc<Mutex<>>.
                                if let Ok(mut writer_guard) = db_writer_arc.lock()
                                    && let Some(ref mut w) = *writer_guard
                                {
                                    let _ = w.clear_scan_cursors();
                                }
                                let items_snapshot = {
                                    let mut i = index.write().expect("index write");
                                    i.status = SearchIndexStatus::Ready;
                                    i.last_error = None;
                                    i.active_root = None;
                                    i.active_root_index = 0;
                                    i.last_scan_completed_unix_secs = Some(current_unix_secs());
                                    i.last_scan_duration_ms = Some(
                                        scan_started_instant
                                            .elapsed()
                                            .as_millis()
                                            .min(u128::from(u64::MAX))
                                            as u64,
                                    );
                                    i.last_progress_unix_secs = Some(current_unix_secs());
                                    i.progress_text = Some(format!("Ready · {} indexed", total));
                                    i.last_completed_indexed_items = Some(total);
                                    i.items = std::mem::take(&mut new_items);
                                    i.items.clone()
                                };
                                // Persist to disk in background — next launch gets instant results.
                                save_index_to_disk(items_snapshot);
                            }
                            Err(e) => {
                                if stop.load(Ordering::SeqCst) {
                                    break;
                                }
                                let mut i = index.write().expect("index write");
                                i.status = SearchIndexStatus::Unavailable;
                                i.last_error = Some(e.clone());
                                i.active_root = None;
                                i.active_root_index = 0;
                                i.last_scan_completed_unix_secs = Some(current_unix_secs());
                                i.last_scan_duration_ms = Some(
                                    scan_started_instant
                                        .elapsed()
                                        .as_millis()
                                        .min(u128::from(u64::MAX))
                                        as u64,
                                );
                                i.last_progress_unix_secs = Some(current_unix_secs());
                                i.progress_text = None;
                            }
                        }

                        if let Some(w) = watcher.as_mut() {
                            for root in watched_roots.drain(..) {
                                let _ = w.unwatch(&root);
                            }
                            let mut failed = 0usize;
                            for root in &roots {
                                if root.exists() {
                                    if w.watch(root, RecursiveMode::Recursive).is_ok() {
                                        watched_roots.push(root.clone());
                                    } else {
                                        failed = failed.saturating_add(1);
                                    }
                                }
                            }
                            let mut i = index.write().expect("index write");
                            i.watched_root_count = watched_roots.len();
                            i.watcher_ready = true;
                            i.watcher_last_setup_unix_secs = Some(current_unix_secs());
                            i.watcher_last_error = if failed > 0 {
                                Some(format!(
                                    "Failed to watch {} location{}",
                                    failed,
                                    if failed == 1 { "" } else { "s" }
                                ))
                            } else {
                                None
                            };
                        }

                        last_cfg = Some(cfg);
                        needs_rebuild = false;
                        last_full_rebuild_at = Some(Instant::now());
                    }

                    // Scheduled refresh: trigger a silent rebuild when the interval elapses.
                    {
                        let cfg_snap = config.read().expect("cfg read");
                        let (status, last_progress) = {
                            let idx = index.read().expect("index read");
                            (idx.status, idx.last_progress_unix_secs)
                        };
                        if matches!(status, SearchIndexStatus::Indexing) {
                            let now_secs = current_unix_secs();
                            if let Some(last) = last_progress {
                                let age = now_secs.saturating_sub(last);
                                let cooldown = last_stall_restart_at
                                    .map(|t| {
                                        t.elapsed().as_secs() < INDEX_STALL_RESTART_COOLDOWN_SECS
                                    })
                                    .unwrap_or(false);
                                if age > INDEX_STALL_SECS && !cooldown {
                                    last_stall_restart_at = Some(Instant::now());
                                    needs_rebuild = true;
                                    if let Ok(mut i) = index.write() {
                                        i.progress_text =
                                            Some("Stalled · restarting index…".to_string());
                                    }
                                }
                            }
                        }
                        let interval_secs = cfg_snap.refresh_interval_hours as u64 * 3_600;
                        if interval_secs > 0 {
                            let due = last_full_rebuild_at
                                .map(|t| t.elapsed().as_secs() >= interval_secs)
                                .unwrap_or(false);
                            if due {
                                needs_rebuild = true;
                            }
                        }

                        // Managed locate database: run updatedb on first use and on schedule.
                        // This gives near-instant locate query results, especially on cold start.
                        // Completely opt-in; no root required for the user's own directories.
                        //
                        // Fanotify interaction: if CAP_SYS_ADMIN is present, fanotify is catching
                        // all real-time changes, so we only need updatedb as a daily safety net
                        // (catches anything fanotify may have missed across reboots). Without
                        // fanotify, we run on the user-configured schedule.
                        if cfg_snap.manage_locate_db && updatedb_available() {
                            let has_fanotify = Self::privileged_indexing_available();
                            let effective_interval_secs = if has_fanotify {
                                // With fanotify: daily safety net regardless of user setting.
                                86_400u64
                            } else {
                                cfg_snap.locate_update_hours as u64 * 3_600
                            };

                            let locate_due = if !locate_db_initialized {
                                // First run: check age of existing DB; rebuild if missing or >1h old.
                                locate_db_age_secs().map(|a| a > 3_600).unwrap_or(true)
                            } else if effective_interval_secs > 0 {
                                last_updatedb_at
                                    .map(|t| t.elapsed().as_secs() >= effective_interval_secs)
                                    .unwrap_or(false)
                            } else {
                                false
                            };
                            locate_db_initialized = true;
                            if locate_due {
                                let roots = effective_roots(&cfg_snap);
                                let root = roots
                                    .into_iter()
                                    .next()
                                    .unwrap_or_else(ottrin_core::default_home_dir);
                                run_user_updatedb(root);
                                last_updatedb_at = Some(Instant::now());
                            }
                        }
                    }

                    // Collect notify events; use a short timeout so we can check debounce frequently.
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(Ok(event)) => {
                            for path in event.paths {
                                let action = if matches!(event.kind, notify::EventKind::Remove(_)) {
                                    NotifyAction::Remove(path)
                                } else {
                                    NotifyAction::Upsert(path)
                                };
                                pending.push(action);
                            }
                            last_event_at = Some(Instant::now());
                        }
                        Ok(Err(_)) => {}
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            std::thread::sleep(Duration::from_millis(200));
                        }
                    }

                    // After 500ms of quiet, apply pending events.
                    if last_event_at
                        .map(|t| t.elapsed() >= Duration::from_millis(500))
                        .unwrap_or(false)
                        && !pending.is_empty()
                    {
                        if pending.len() > INCREMENTAL_REBUILD_THRESHOLD {
                            // Too many events at once (large copy/extract) — full rebuild is cleaner.
                            needs_rebuild = true;
                        } else {
                            let cfg_snap = config.read().expect("cfg read").clone();
                            apply_incremental_to_db(&pending, &cfg_snap);
                            let snap = {
                                let mut idx = index.write().expect("index write");
                                apply_incremental(&mut idx.items, &pending, &cfg_snap);
                                let count = idx.items.len();
                                if matches!(idx.status, SearchIndexStatus::Ready) {
                                    idx.progress_text = Some(format!("Ready · {} indexed", count));
                                }
                                idx.items.clone()
                            };
                            // Persist updated index in background.
                            save_index_to_disk(snap);
                        }
                        pending.clear();
                        last_event_at = None;
                    }
                }
            }));
            if result.is_err() {
                let mut i = index_for_panic.write().expect("index write");
                i.status = SearchIndexStatus::Unavailable;
                i.last_error = Some("Indexer thread panicked".to_string());
                i.last_progress_unix_secs = Some(current_unix_secs());
                i.progress_text = Some("Indexer crashed · restart to retry".to_string());
            }
        });
        *self.worker.lock().expect("worker lock") = Some(handle);
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.worker.lock().expect("worker lock").take() {
            std::thread::spawn(move || {
                let _ = h.join();
            });
        }
    }

    pub fn update_config(&self, cfg: SearchConfig) {
        if let Ok(mut c) = self.config.write() {
            *c = cfg;
        }
    }

    /// Force an immediate reindex regardless of config changes.
    pub fn rebuild_index(&self) {
        self.rebuild.store(true, Ordering::SeqCst);
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

    /// Age of Ottrin's managed locate.db in seconds, if it exists.
    pub fn locate_db_age_secs() -> Option<u64> {
        locate_db_age_secs()
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

    /// Trigger an updatedb run for the given root now (non-blocking).
    pub fn run_user_updatedb(root: PathBuf) {
        run_user_updatedb(root);
    }

    /// Returns `true` if the process has `CAP_SYS_ADMIN` — required for fanotify filesystem-level
    /// change notifications. On non-Linux platforms always returns `false`.
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

    pub fn index_status(&self) -> (SearchIndexStatus, Option<String>) {
        let snapshot = self.diagnostics();
        (snapshot.status, snapshot.detail)
    }

    pub fn diagnostics(&self) -> SearchIndexDiagnostics {
        let idx = self.index.read().expect("index read");
        let detail = match idx.status {
            SearchIndexStatus::Indexing => idx.progress_text.clone(),
            SearchIndexStatus::Unavailable => idx.last_error.clone(),
            SearchIndexStatus::Ready => idx.progress_text.clone(),
        };
        SearchIndexDiagnostics {
            status: idx.status,
            detail,
            indexed_items: if matches!(idx.status, SearchIndexStatus::Indexing) {
                idx.building_indexed_items
            } else {
                idx.items.len()
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
        let idx = self.index.read().expect("index read");
        let status = idx.status;
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
                && idx.items.is_empty()
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
            } else {
                idx.items
                    .iter()
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

        if matches!(query.scope, SearchScope::Global)
            && matches!(status, SearchIndexStatus::Indexing)
            && !idx.items.is_empty()
            && query.text.trim().len() >= 2
        {
            let roots = ordered_query_roots(&query, &cfg);
            let mut seen: HashSet<PathBuf> =
                matched.iter().map(|(_, item)| item.path.clone()).collect();
            let supplement_hits = query.limit.clamp(64, 256);
            for (score, item) in query_global_fallback(
                &query,
                &expr,
                &roots,
                &cfg,
                30_000,
                supplement_hits,
                Duration::from_millis(120),
            ) {
                if seen.insert(item.path.clone()) {
                    matched.push((score, item));
                }
                if matched.len() >= query.limit.saturating_mul(2).max(200) {
                    break;
                }
            }
        }

        if global_root_outside_index && query.text.trim().len() >= 2 {
            let mut seen: HashSet<PathBuf> =
                matched.iter().map(|(_, item)| item.path.clone()).collect();
            for (score, item) in query_current_folder_fallback(&query, &expr, &cfg) {
                if seen.insert(item.path.clone()) {
                    matched.push((score, item));
                }
                if matched.len() >= query.limit.saturating_mul(2).max(200) {
                    break;
                }
            }
        }

        // Supplement with system locate/plocate when: query is non-trivial,
        // the user has opted in, and we have budget (index empty or still building).
        // Zero privilege required — we just read the database the OS already maintains.
        let budget_remaining = matched.len() < query.limit && query.text.trim().len() >= 2;
        let index_thin = matches!(status, SearchIndexStatus::Indexing) && idx.items.len() < 5_000;
        if cfg.use_system_locate && budget_remaining && (index_thin || matched.is_empty()) {
            let roots = effective_roots(&cfg);
            let locate_hits = query_system_locate(
                &query.text,
                &roots,
                query.include_hidden_system,
                query.limit.saturating_sub(matched.len()).max(50),
            );
            let mut seen: HashSet<PathBuf> =
                matched.iter().map(|(_, item)| item.path.clone()).collect();
            for item in locate_hits {
                if eval_expr(&expr, &item) && seen.insert(item.path.clone()) {
                    matched.push((relevance_score(&query, &item), item));
                }
            }
        }

        // ── Fuzzy pass: supplement with subsequence-matched filenames ─────────
        // Only runs when query is a single bare word and exact results are sparse.
        if is_simple_single_word(&query.text) && matched.len() < query.limit {
            let q = query.text.trim();
            let mut seen: HashSet<PathBuf> =
                matched.iter().map(|(_, item)| item.path.clone()).collect();
            for item in idx
                .items
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
                    if matched.len() >= query.limit.saturating_mul(2).max(200) {
                        break;
                    }
                }
            }
        }

        // ── Content search pass: use ripgrep when requested ───────────────────
        if query.content_search && query.text.trim().len() >= 2 {
            let roots = ordered_query_roots(&query, &cfg);
            let content_limit = query.limit.max(50);
            let rg_items = query_content_ripgrep(
                &expr,
                query.text.trim(),
                &roots,
                query.include_hidden_system,
                content_limit,
            );
            let mut seen: HashSet<PathBuf> =
                matched.iter().map(|(_, item)| item.path.clone()).collect();
            for item in rg_items {
                if scope_match(&query, &item) && seen.insert(item.path.clone()) {
                    // Content matches get a high base score — they are strong signals
                    matched.push((8_000, item));
                    if matched.len() >= query.limit.saturating_mul(2).max(200) {
                        break;
                    }
                }
            }
        }

        sort_results(&mut matched, query.sort);
        let total = matched.len();
        let items = matched
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .map(|(_, item)| item)
            .collect();

        SearchResponse {
            items,
            total,
            status,
            error: None,
        }
    }
}

impl Drop for SearchService {
    fn drop(&mut self) {
        self.stop();
    }
}

fn effective_roots(cfg: &SearchConfig) -> Vec<PathBuf> {
    let roots = if !cfg.include_roots.is_empty() {
        cfg.include_roots.clone()
    } else {
        default_roots()
    };
    #[cfg(target_os = "linux")]
    {
        let mut roots = roots;
        let mounts = Path::new("/proc/mounts");
        if mounts.exists()
            && let Ok(text) = std::fs::read_to_string(mounts)
        {
            let mut seen: HashSet<PathBuf> = roots.iter().cloned().collect();
            for root in roots.clone() {
                let root_dev = std::fs::metadata(&root).ok().map(|m| m.dev());
                for line in text.lines() {
                    let mut parts = line.split_whitespace();
                    let _src = parts.next();
                    let mnt = parts.next();
                    if let Some(m) = mnt {
                        let p = PathBuf::from(m);
                        if !p.starts_with(&root) || !seen.insert(p.clone()) {
                            continue;
                        }
                        if let (Some(rdev), Ok(mmeta)) = (root_dev, std::fs::metadata(&p))
                            && mmeta.dev() == rdev
                        {
                            continue; // same filesystem; already covered by root
                        }
                        roots.push(p);
                    }
                }
            }
        }
        roots
    }
    #[cfg(not(target_os = "linux"))]
    {
        roots
    }
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

pub fn default_roots() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let mut roots = Vec::new();
        for c in b'C'..=b'Z' {
            let p = PathBuf::from(format!("{}:\\", c as char));
            if p.exists() {
                roots.push(p);
            }
        }
        if roots.is_empty() {
            roots.push(ottrin_core::default_home_dir());
        }
        roots
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut roots = vec![ottrin_core::default_home_dir()];
        let mounts = Path::new("/proc/mounts");
        if mounts.exists()
            && let Ok(text) = std::fs::read_to_string(mounts)
        {
            let mut seen: HashSet<PathBuf> = roots.iter().cloned().collect();
            for line in text.lines() {
                let mut parts = line.split_whitespace();
                let _src = parts.next();
                let mnt = parts.next();
                if let Some(m) = mnt {
                    let p = PathBuf::from(m);
                    if (p.starts_with("/media")
                        || p.starts_with("/mnt")
                        || p.starts_with("/run/media"))
                        && seen.insert(p.clone())
                    {
                        roots.push(p);
                    }
                }
            }
        }
        roots
    }
}

/// Walk the configured roots, calling `on_batch` with every `STREAM_BATCH_SIZE` items so the UI
/// can show progress while the rebuild runs.
fn build_index<F, G>(
    cfg: &SearchConfig,
    stop: &AtomicBool,
    mut on_progress: F,
    mut on_batch: G,
) -> Result<usize, String>
where
    F: FnMut(ProgressUpdate),
    G: FnMut(Vec<SearchResultItem>),
{
    let debug = std::env::var("OTTRIN_INDEX_DEBUG").ok().is_some();
    let mut batch: Vec<SearchResultItem> = Vec::with_capacity(STREAM_BATCH_SIZE);
    let mut total_indexed = 0usize;
    let roots = effective_roots(cfg);
    let excluded_roots = cfg.exclude_roots.clone();
    let roots_total = roots.len().max(1);
    if debug {
        eprintln!(
            "build_index: roots={} include_globs={} exclude_globs={}",
            roots.len(),
            cfg.include_globs.len(),
            cfg.exclude_globs.len()
        );
        for r in &roots {
            eprintln!("build_index: root={} exists={}", r.display(), r.exists());
        }
    }
    let placeholder_root = roots.first().cloned().unwrap_or_else(PathBuf::new);
    on_progress(ProgressUpdate {
        message: "Indexing · preparing filters…".to_string(),
        active_root: placeholder_root,
        active_root_index: 0,
        total_roots: roots_total,
    });
    let include = build_globset(&cfg.include_globs)?;
    let exclude = build_globset(&cfg.exclude_globs)?;
    let resume_cursors = SearchDb::open(search_db_path())
        .ok()
        .and_then(|db| db.scan_cursors().ok())
        .unwrap_or_default();
    if debug {
        eprintln!("build_index: globsets built");
    }

    for (root_idx, root) in roots.into_iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let total_before_root = total_indexed;
        let mut root_seen = 0usize;
        on_progress(ProgressUpdate {
            message: format_progress(root_idx + 1, roots_total, total_indexed, root_seen),
            active_root: root.clone(),
            active_root_index: root_idx + 1,
            total_roots: roots_total,
        });
        if !root.exists() {
            if debug {
                eprintln!("build_index: skip missing root {}", root.display());
            }
            continue;
        }
        let resume_cursor = resume_cursors.get(&root).cloned();
        let mut cursor_found = false;
        let passes = if resume_cursor.is_some() { 2 } else { 1 };
        for pass in 0..passes {
            let mut skipping = resume_cursor.is_some() && pass == 0;
            let walker = WalkDir::new(&root)
                .follow_links(false)
                .same_file_system(true)
                .into_iter()
                .filter_entry(|entry| {
                    should_keep_path(
                        entry.path(),
                        &root,
                        cfg.include_hidden_system,
                        &include,
                        &exclude,
                        &excluded_roots,
                    )
                });
            for entry in walker.filter_map(Result::ok) {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let path = entry.path();
                if path == root {
                    continue;
                }
                if skipping {
                    if let Some(cursor) = resume_cursor.as_ref()
                        && path == cursor.as_path()
                    {
                        skipping = false;
                        cursor_found = true;
                    }
                    continue;
                }
                root_seen = root_seen.saturating_add(1);
                let name = entry.file_name().to_string_lossy().to_string();
                if !cfg.include_hidden_system && name.starts_with('.') {
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
                batch.push(item);
                total_indexed += 1;

                if batch.len() >= STREAM_BATCH_SIZE {
                    on_progress(ProgressUpdate {
                        message: format_progress(
                            root_idx + 1,
                            roots_total,
                            total_indexed,
                            root_seen,
                        ),
                        active_root: root.clone(),
                        active_root_index: root_idx + 1,
                        total_roots: roots_total,
                    });
                    on_batch(std::mem::take(&mut batch));
                    batch = Vec::with_capacity(STREAM_BATCH_SIZE);
                }
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
            if pass == 0 && resume_cursor.is_some() && !cursor_found {
                root_seen = 0;
                total_indexed = total_before_root;
            } else {
                break;
            }
        }
    }
    // Flush final partial batch
    if !batch.is_empty() {
        on_batch(batch);
    }
    Ok(total_indexed)
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

fn estimate_total_items(cfg: &SearchConfig, roots: &[PathBuf]) -> Option<usize> {
    if roots.is_empty() {
        return None;
    }
    if !system_locate_available() && !cfg.manage_locate_db {
        return None;
    }
    let root = roots[0].clone();
    locate_count(&root)
}

fn locate_count(root: &Path) -> Option<usize> {
    let root_str = root.display().to_string();
    let output = if command_exists("plocate") {
        std::process::Command::new("plocate")
            .args(["-c", &root_str])
            .output()
            .ok()?
    } else if command_exists("locate") {
        std::process::Command::new("locate")
            .args(["-c", &root_str])
            .output()
            .ok()?
    } else {
        return None;
    };
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse::<usize>().ok()
}

/// Serialize the index to disk in a background thread so the UI is never blocked.
fn save_index_to_disk(items: Vec<SearchResultItem>) {
    std::thread::spawn(move || {
        let path = index_cache_path();
        if let Ok(data) = serde_json::to_vec(&items) {
            let _ = std::fs::write(&path, &data);
        }
    });
}

/// Load a previously persisted index from disk. Returns `None` if unavailable or corrupt.
fn load_index_from_disk() -> Option<Vec<SearchResultItem>> {
    let db_path = search_db_path();
    if let Ok(db) = SearchDb::open(db_path)
        && let Ok(items) = db.load_items()
        && !items.is_empty()
    {
        return Some(items);
    }
    let path = index_cache_path();
    let data = std::fs::read(&path).ok()?;
    let mut items: Vec<SearchResultItem> = serde_json::from_slice(&data).ok()?;
    for item in &mut items {
        item.prepare();
    }
    Some(items)
}

/// Returns how many seconds ago the on-disk index was last written.
fn index_cache_age_secs() -> Option<u64> {
    let db_path = search_db_path();
    if let Ok(mtime) = std::fs::metadata(&db_path).and_then(|m| m.modified())
        && let Ok(age) = SystemTime::now().duration_since(mtime)
    {
        return Some(age.as_secs());
    }
    let path = index_cache_path();
    let mtime = std::fs::metadata(&path).ok()?.modified().ok()?;
    SystemTime::now()
        .duration_since(mtime)
        .ok()
        .map(|d| d.as_secs())
}

/// Human-readable age string used in status messages.
fn format_age(secs: u64) -> String {
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3_600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3_600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
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
// Windows: the equivalent is the NTFS USN journal.
//   DeviceIoControl(volume, FSCTL_ENUM_USN_DATA, ...) gives the entire MFT
//   in seconds. DeviceIoControl(volume, FSCTL_READ_USN_JOURNAL, ...) gives
//   real-time change events. This is how "Everything" works and would replace
//   both the directory walk and inotify on Windows entirely.
//   See: crates/ottrin-search/src/platform/windows_usn.rs (TODO)

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

fn command_exists(name: &str) -> bool {
    std::process::Command::new(name)
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
}

/// Detect whether Ottrin's user-level locate schedule is installed (systemd user timer or crontab).
pub fn locate_schedule_status() -> LocateScheduleStatus {
    let home = ottrin_core::default_home_dir();
    let unit_dir = home.join(".config/systemd/user");
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

/// Detect whether the system locate database is scheduled (systemd timer or cron).
pub fn system_locate_schedule_status() -> SystemLocateScheduleStatus {
    let timer_names = ["plocate-updatedb.timer", "mlocate.timer", "updatedb.timer"];
    if command_exists("systemctl")
        && let Ok(out) = std::process::Command::new("systemctl")
            .args(["list-timers", "--all", "--no-legend"])
            .output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        for name in &timer_names {
            if text.contains(name) {
                return SystemLocateScheduleStatus {
                    source: Some("systemd".to_string()),
                    detail: Some((*name).to_string()),
                    needs_setup: false,
                };
            }
        }
    }

    let cron_paths = [
        "/etc/cron.daily/plocate",
        "/etc/cron.daily/mlocate",
        "/etc/cron.daily/updatedb",
        "/etc/cron.weekly/updatedb",
        "/etc/cron.weekly/plocate",
    ];
    for path in &cron_paths {
        if std::fs::metadata(path).is_ok() {
            return SystemLocateScheduleStatus {
                source: Some("cron".to_string()),
                detail: Some((*path).to_string()),
                needs_setup: false,
            };
        }
    }

    SystemLocateScheduleStatus {
        source: None,
        detail: None,
        needs_setup: true,
    }
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
    let home = ottrin_core::default_home_dir();
    let unit_dir = home.join(".config/systemd/user");
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
    roots: &[PathBuf],
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

    let effective_roots: Vec<PathBuf> = if roots.is_empty() {
        vec![ottrin_core::default_home_dir()]
    } else {
        roots.to_vec()
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let path = PathBuf::from(line.trim());
            if !effective_roots.iter().any(|r| path.starts_with(r)) {
                return None;
            }
            if !include_hidden && path.file_name()?.to_str()?.starts_with('.') {
                return None;
            }
            make_item_from_path(&path)
        })
        .take(limit)
        .collect()
}

// ── Incremental index updates ──────────────────────────────────────────────────

/// Build a single `SearchResultItem` by statting `path`. Returns `None` if the path no longer
/// exists or we can't read its metadata (transient race — handled gracefully).
fn make_item_from_path(path: &Path) -> Option<SearchResultItem> {
    let meta = std::fs::symlink_metadata(path).ok()?;
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

/// Apply a batch of `NotifyAction`s to the index without re-walking the filesystem.
/// Handles create/modify (upsert) and delete (remove + all children) efficiently.
fn apply_incremental(
    items: &mut Vec<SearchResultItem>,
    actions: &[NotifyAction],
    cfg: &SearchConfig,
) {
    let include = build_globset(&cfg.include_globs).unwrap_or_else(|_| empty_globset());
    let exclude = build_globset(&cfg.exclude_globs).unwrap_or_else(|_| empty_globset());
    let roots = effective_roots(cfg);

    for action in actions {
        match action {
            NotifyAction::Remove(path) => {
                // Remove the exact path and everything under it (directory removal).
                items.retain(|item| item.path != *path && !item.path.starts_with(path));
            }
            NotifyAction::Upsert(path) => {
                // Remove stale entry (may or may not exist).
                items.retain(|item| item.path != *path);
                // Find which configured root owns this path.
                let Some(root) = roots.iter().find(|r| path.starts_with(r.as_path())) else {
                    continue; // Outside all indexed roots — ignore.
                };
                // Apply the same filters used during full index build.
                if !should_keep_path(
                    path,
                    root,
                    cfg.include_hidden_system,
                    &include,
                    &exclude,
                    &cfg.exclude_roots,
                ) {
                    continue;
                }
                if let Some(item) = make_item_from_path(path)
                    && (cfg.include_hidden_system || !item.name.starts_with('.'))
                {
                    items.push(item);
                }
            }
        }
    }
}

fn apply_incremental_to_db(actions: &[NotifyAction], cfg: &SearchConfig) {
    let mut writer = match SearchDbWriter::open(search_db_path()) {
        Ok(writer) => writer,
        Err(_) => return,
    };
    let include = build_globset(&cfg.include_globs).unwrap_or_else(|_| empty_globset());
    let exclude = build_globset(&cfg.exclude_globs).unwrap_or_else(|_| empty_globset());
    let roots = effective_roots(cfg);

    for action in actions {
        match action {
            NotifyAction::Remove(path) => {
                let _ = writer.remove_path_tree(path);
            }
            NotifyAction::Upsert(path) => {
                let Some(root) = roots.iter().find(|r| path.starts_with(r.as_path())) else {
                    continue;
                };
                if !should_keep_path(
                    path,
                    root,
                    cfg.include_hidden_system,
                    &include,
                    &exclude,
                    &cfg.exclude_roots,
                ) {
                    continue;
                }
                if let Some(item) = make_item_from_path(path)
                    && (cfg.include_hidden_system || !item.name.starts_with('.'))
                {
                    let _ = writer.upsert_item(&item);
                }
            }
        }
    }
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
    let rel = normalized_rel_path(path, root);
    !exclude.is_match(rel.as_str()) || include.is_match(rel.as_str())
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
    p.parse_expr()
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

fn eval_term(t: &Term, item: &SearchResultItem) -> bool {
    // Use pre-computed lowercase fields — zero allocations in the hot query path.
    let name_l = &item.name_lower;
    let path_l = &item.path_lower;
    match t {
        Term::Contains { field, value } => match field {
            // Substring match on name OR path — grep-like behavior.
            // "cat" matches "angelcat.pdf", "cat_photo.jpg", "/cats/file.txt".
            // Multi-term AND queries ("angel cat pdf") match all terms independently.
            Field::Any => {
                name_l.contains(value.as_str())
                    || path_l.contains(value.as_str())
                    || item
                        .content_snippet
                        .as_ref()
                        .map(|snippet| snippet.to_ascii_lowercase().contains(value.as_str()))
                        .unwrap_or(false)
            }
            Field::Name => name_l.contains(value.as_str()),
            Field::Path => path_l.contains(value.as_str()),
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
                    || regex.is_match(&item.path_str)
                    || item
                        .content_snippet
                        .as_ref()
                        .map(|snippet| regex.is_match(snippet))
                        .unwrap_or(false)
            }
            Field::Name => regex.is_match(&item.name),
            Field::Path => regex.is_match(&item.path_str),
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

fn relevance_score(query: &SearchQuery, item: &SearchResultItem) -> i64 {
    let q = query.text.to_ascii_lowercase();
    // Use pre-computed lowercase fields — zero allocations.
    let name = &item.name_lower;
    if name == &q {
        10_000
    } else if name.starts_with(&*q) {
        7_500
    } else if name.contains(&*q) {
        5_000
    } else if item.path_lower.contains(&*q) {
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

/// Run ripgrep to find files whose *contents* match whitespace-separated literal terms.
/// Returns items with `content_snippet` populated (first matching line, trimmed).
/// Regex-style queries are intentionally left unchanged here.
fn query_content_ripgrep(
    expr: &Expr,
    pattern: &str,
    roots: &[PathBuf],
    include_hidden: bool,
    limit: usize,
) -> Vec<SearchResultItem> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() || roots.is_empty() {
        return Vec::new();
    }
    if trimmed.starts_with('/') && trimmed.ends_with('/') {
        return Vec::new();
    }

    let mut terms = Vec::new();
    let mut seen_terms = HashSet::new();
    for term in trimmed.split_whitespace() {
        let term = term.to_ascii_lowercase();
        if term.len() >= 2 && seen_terms.insert(term.clone()) {
            terms.push(term);
        }
    }
    if terms.is_empty() {
        return Vec::new();
    }

    let mut matching_paths: Option<HashSet<PathBuf>> = None;
    for term in &terms {
        let mut cmd = std::process::Command::new("rg");
        cmd.arg("--files-with-matches")
            .arg("--fixed-strings")
            .arg("--ignore-case")
            .arg("--max-count=1");
        if include_hidden {
            cmd.arg("--hidden");
        }
        cmd.arg("-e").arg(term);
        cmd.arg("--");
        for root in roots {
            cmd.arg(root);
        }
        let file_output = match cmd.output() {
            Ok(o) if o.status.success() || !o.stdout.is_empty() => o,
            _ => return Vec::new(),
        };
        let term_paths: HashSet<PathBuf> = std::str::from_utf8(&file_output.stdout)
            .unwrap_or("")
            .lines()
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .collect();
        if term_paths.is_empty() {
            return Vec::new();
        }
        matching_paths = Some(match matching_paths.take() {
            Some(existing) => existing.intersection(&term_paths).cloned().collect(),
            None => term_paths,
        });
        if matching_paths.as_ref().is_none_or(|set| set.is_empty()) {
            return Vec::new();
        }
    }

    let mut matching_paths: Vec<PathBuf> = matching_paths.unwrap_or_default().into_iter().collect();
    matching_paths.sort();
    let mut items: Vec<SearchResultItem> = matching_paths
        .into_iter()
        .filter_map(|path| {
            let meta = std::fs::metadata(&path).ok()?;
            let content = std::fs::read(&path).ok()?;
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
    items
}

fn sort_results(v: &mut [(i64, SearchResultItem)], sort: SearchSort) {
    match sort {
        SearchSort::Relevance => {
            v.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)))
        }
        SearchSort::Name => v.sort_by(|a, b| a.1.name_lower.cmp(&b.1.name_lower)),
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
    fn parser_rejects_invalid_regex() {
        let err = parse_query("/[/").expect_err("should reject invalid regex");
        assert!(err.to_string().contains("Invalid regex"));
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
    fn fallback_walk_respects_scope_and_query() {
        let root = std::env::temp_dir().join(format!("ottrin-search-test-{}", std::process::id()));
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
            std::env::temp_dir().join(format!("ottrin-indexed-{}", std::process::id()));
        let outside_root =
            std::env::temp_dir().join(format!("ottrin-outside-{}", std::process::id()));
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
            idx.items.clear();
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
            std::env::temp_dir().join(format!("ottrin-search-home-{}", std::process::id()));
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
            std::env::temp_dir().join(format!("ottrin-content-search-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");

        let file_path = root.join("alpha.txt");
        fs::write(&file_path, b"first line\nneedle appears here\nthird line\n")
            .expect("create test file");

        let service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items.clear();
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
    fn content_search_matches_multi_word_queries() {
        let root = std::env::temp_dir().join(format!(
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
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items.clear();
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
        let root = std::env::temp_dir().join(format!(
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
        let results =
            query_content_ripgrep(&expr, "needle", std::slice::from_ref(&root), false, 10);

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
        let root = std::env::temp_dir().join(format!(
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
            ..SearchConfig::default()
        });
        {
            let mut idx = service.index.write().expect("index write");
            idx.status = SearchIndexStatus::Ready;
            idx.items.clear();
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
