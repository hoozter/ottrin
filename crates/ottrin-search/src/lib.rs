use fs2::FileExt;
use globset::{Glob, GlobSet, GlobSetBuilder};
use ottrin_core::{
    EntryKind, SearchConfig, SearchIndexStatus, SearchQuery, SearchResponse, SearchResultItem,
    SearchScope, SearchSort,
};
use regex::{Regex, RegexBuilder};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::Mutex;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use walkdir::WalkDir;

mod db;
#[allow(dead_code)] // Slice 2 mutation API is intentionally isolated until worker integration.
mod mutation;
mod plan;
mod scanner;
mod watch;
mod worker;
use db::{
    SearchStore, query_indexed_content_page, search_db_path, stream_generation_content_items,
};

#[cfg(test)]
static SEARCH_SERVICE_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
fn default_search_store_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
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

impl SearchService {
    pub fn new(config: SearchConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            index: Arc::new(RwLock::new(IndexState::default())),
            worker: None,
            writer_lock: None,
            store_path: default_search_store_path(),
            launcher: worker::real_launcher(),
            watch_filesystem: true,
        }
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

    pub fn install_sqlite_schedule(
        interval_hours: u32,
        roots: &[PathBuf],
    ) -> Result<String, String> {
        install_sqlite_schedule(interval_hours, roots)
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
}

impl SearchHandle {
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
            snapshot_revision: idx.snapshot_revision,
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
                warning: None,
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
                    warning: None,
                };
            }
        };

        if query.content_search && expr_contains_negation(&expr) {
            return self.query_negated_content_snapshot(&query, &expr, status, &cfg);
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
        let index_thin = matches!(status, SearchIndexStatus::Indexing) && items.len() < 5_000;
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
                    if matched.len() >= query.limit.saturating_mul(2).max(200) {
                        break;
                    }
                }
            }
        }

        // ── Content search pass: query the active FTS generation first. ───────
        // Ripgrep remains a supplemental fallback for content which is deliberately
        // not indexed (unreadable, binary, oversized, or outside indexed roots).
        if query.content_search && query.text.trim().len() >= 2 {
            let fts_query = fts_content_match(&expr);
            if matches!(status, SearchIndexStatus::Ready)
                && let Some(fts_query) = fts_query.as_deref()
                && let Ok(plan) = plan::ScanPlan::new(&cfg)
            {
                let mut by_path = items
                    .iter()
                    .map(|item| (item.path.as_path(), item))
                    .collect::<std::collections::HashMap<_, _>>();
                let mut seen: HashSet<PathBuf> =
                    matched.iter().map(|(_, item)| item.path.clone()).collect();
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
                        let Some(indexed) = by_path.remove(hit.path.as_path()) else {
                            continue;
                        };
                        let mut item = indexed.clone();
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
                                matched.push((score, item));
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
            warning: None,
        }
    }
}

impl SearchHandle {
    fn query_negated_content_snapshot(
        &self,
        query: &SearchQuery,
        expr: &Expr,
        status: SearchIndexStatus,
        cfg: &SearchConfig,
    ) -> SearchResponse {
        if !matches!(status, SearchIndexStatus::Ready) {
            return SearchResponse {
                items: Vec::new(),
                total: 0,
                status,
                error: Some(
                    "Negated content search requires a completed indexed generation.".to_string(),
                ),
                warning: None,
            };
        }
        let plan = match plan::ScanPlan::new(cfg) {
            Ok(plan) => plan,
            Err(error) => {
                return SearchResponse {
                    items: Vec::new(),
                    total: 0,
                    status,
                    error: Some(error.to_string()),
                    warning: None,
                };
            }
        };
        let mut matched = Vec::new();
        let mut total_matches = 0usize;
        let result_capacity = query.offset.saturating_add(query.limit).max(1);
        let stats = stream_generation_content_items(self.store_path.as_ref(), &plan, |record| {
            // Unknown content is excluded from every negated-content query. An
            // absent FTS row must never become evidence for a complement result.
            if !record.complete {
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
        let stats = match stats {
            Ok(stats) => stats,
            Err(error) => {
                return SearchResponse {
                    items: Vec::new(),
                    total: 0,
                    status,
                    error: Some(format!(
                        "Negated content search could not read its indexed generation: {error}"
                    )),
                    warning: None,
                };
            }
        };
        sort_results(&mut matched, query.sort);
        let items = matched
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .map(|(_, item)| item)
            .collect();
        SearchResponse {
            items,
            total: total_matches,
            status,
            error: None,
            warning: (stats.unknown_items > 0).then(|| {
                format!(
                    "Excluded {} item(s) with unknown indexed content from negated content search.",
                    stats.unknown_items.min(999)
                )
            }),
        }
    }
}

impl Drop for SearchService {
    fn drop(&mut self) {
        self.stop();
    }
}

fn effective_roots(cfg: &SearchConfig) -> Vec<PathBuf> {
    let mut roots = cfg.include_roots.clone();
    roots.extend(cfg.active_indexed_volume_roots.iter().cloned());
    if !roots.is_empty() {
        roots.sort();
        roots.dedup();
        roots
    } else {
        default_roots()
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
    vec![ottrin_core::default_home_dir()]
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
        if let Ok(meta) = std::fs::metadata(&path) {
            let len = meta.len();
            std::fs::remove_file(&path)
                .map_err(|e| format!("Failed to remove {}: {}", path.display(), e))?;
            freed = freed.saturating_add(len);
        }
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

fn internal_command(program: &str) -> std::process::Command {
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

/// Detect whether Ottrin's primary SQLite index has a background refresh schedule.
pub fn sqlite_schedule_status() -> LocateScheduleStatus {
    #[cfg(target_os = "linux")]
    {
        let home = ottrin_core::default_home_dir();
        let unit_dir = home.join(".config/systemd/user");
        let timer_path = unit_dir.join("ottrin-index.timer");
        if timer_path.exists() {
            let enabled = if command_exists("systemctl") {
                std::process::Command::new("systemctl")
                    .args(["--user", "is-enabled", "ottrin-index.timer"])
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
        if crontab.contains("# ottrin-index") {
            return LocateScheduleStatus {
                source: "crontab".to_string(),
                enabled: true,
                detail: "Entry found (# ottrin-index)".to_string(),
            };
        }
    }

    #[cfg(target_os = "windows")]
    {
        let output = internal_command("schtasks")
            .args(["/Query", "/TN", "Ottrin Index"])
            .output();
        if output.map(|o| o.status.success()).unwrap_or(false) {
            return LocateScheduleStatus {
                source: "Windows Task Scheduler".to_string(),
                enabled: true,
                detail: "Task found: Ottrin Index".to_string(),
            };
        }
    }

    LocateScheduleStatus {
        source: "none".to_string(),
        enabled: false,
        detail: "No background SQLite refresh schedule detected".to_string(),
    }
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
    let ottrin_cron = "/etc/cron.daily/ottrin-updatedb";
    if std::fs::metadata(ottrin_cron).is_ok() {
        return SystemLocateScheduleStatus {
            source: Some("cron".to_string()),
            detail: Some(ottrin_cron.to_string()),
            needs_setup: false,
            managed_by_ottrin: true,
        };
    }

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
                    managed_by_ottrin: false,
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

pub fn install_sqlite_schedule(interval_hours: u32, roots: &[PathBuf]) -> Result<String, String> {
    let interval_hours = interval_hours.max(1);
    let roots = if roots.is_empty() {
        vec![ottrin_core::default_home_dir()]
    } else {
        roots.to_vec()
    };
    let indexer = find_ottrin_index_binary()
        .ok_or_else(|| "Could not find ottrin-index next to Ottrin or on PATH.".to_string())?;

    #[cfg(target_os = "linux")]
    {
        let has_systemd = std::process::Command::new("systemctl")
            .args(["--user", "is-system-running"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success() || s.code() == Some(1))
            .unwrap_or(false);
        if has_systemd {
            return install_sqlite_systemd_user_timer(interval_hours, &indexer, &roots);
        }
        install_sqlite_crontab_entry(interval_hours, &indexer, &roots)
    }

    #[cfg(target_os = "windows")]
    {
        install_sqlite_windows_task(interval_hours, &indexer, &roots)
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = (interval_hours, roots, indexer);
        Err(
            "Background SQLite refresh scheduling is not supported on this platform yet."
                .to_string(),
        )
    }
}

pub fn uninstall_sqlite_schedule() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let home = ottrin_core::default_home_dir();
        let unit_dir = home.join(".config/systemd/user");
        let service_path = unit_dir.join("ottrin-index.service");
        let timer_path = unit_dir.join("ottrin-index.timer");
        let mut changed = false;
        if timer_path.exists() || service_path.exists() {
            if command_exists("systemctl") {
                let _ = std::process::Command::new("systemctl")
                    .args(["--user", "disable", "--now", "ottrin-index.timer"])
                    .status();
            }
            let _ = std::fs::remove_file(&timer_path);
            let _ = std::fs::remove_file(&service_path);
            if command_exists("systemctl") {
                let _ = std::process::Command::new("systemctl")
                    .args(["--user", "daemon-reload"])
                    .status();
            }
            changed = true;
        }
        changed |= remove_crontab_block("# ottrin-index")?;
        changed |= remove_appimage_index_wrapper()?;
        if changed {
            Ok("Background Ottrin index schedule removed.".to_string())
        } else {
            Ok("No background Ottrin index schedule was installed.".to_string())
        }
    }

    #[cfg(target_os = "windows")]
    {
        let status = internal_command("schtasks")
            .args(["/Delete", "/F", "/TN", "Ottrin Index"])
            .status()
            .map_err(|e| format!("Failed to run schtasks: {}", e))?;
        if status.success() {
            Ok("Background Ottrin index task removed.".to_string())
        } else {
            Ok("No background Ottrin index task was installed.".to_string())
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Ok("No background schedule support exists for this platform yet.".to_string())
    }
}

#[cfg(target_os = "linux")]
fn remove_crontab_block(tag: &str) -> Result<bool, String> {
    let output = std::process::Command::new("crontab")
        .arg("-l")
        .output()
        .map_err(|e| format!("crontab not available: {}", e))?;
    let existing = String::from_utf8_lossy(&output.stdout);
    if !existing.contains(tag) {
        return Ok(false);
    }
    let mut skip_next = false;
    let mut kept = Vec::new();
    for line in existing.lines() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if line.trim() == tag {
            skip_next = true;
            continue;
        }
        kept.push(line);
    }
    let new_crontab = kept.join("\n");
    let mut child = std::process::Command::new("crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("crontab not available: {}", e))?;
    if let Some(stdin) = child.stdin.take() {
        use std::io::Write;
        let mut stdin = stdin;
        let _ = stdin.write_all(new_crontab.as_bytes());
        let _ = stdin.write_all(b"\n");
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(true)
    } else {
        Err(format!("crontab exited with {}", status))
    }
}

fn find_ottrin_index_binary() -> Option<PathBuf> {
    let exe_name = if cfg!(target_os = "windows") {
        "ottrin-index.exe"
    } else {
        "ottrin-index"
    };
    #[cfg(target_os = "linux")]
    if let Some(wrapper) = create_appimage_index_wrapper() {
        return Some(wrapper);
    }
    if let Ok(current) = std::env::current_exe()
        && let Some(dir) = current.parent()
    {
        let sibling = dir.join(exe_name);
        if sibling.exists() {
            return Some(sibling);
        }
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(exe_name))
            .find(|candidate| candidate.exists())
    })
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
        "#!/bin/sh\nexec {} --ottrin-index \"$@\"\n",
        shell_quote(&appimage.display().to_string())
    );
    std::fs::write(&wrapper, script).ok()?;
    let mut perms = std::fs::metadata(&wrapper).ok()?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&wrapper, perms).ok()?;
    Some(wrapper)
}

#[cfg(target_os = "linux")]
fn remove_appimage_index_wrapper() -> Result<bool, String> {
    let path = appimage_index_wrapper_path();
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(format!("Failed to remove {}: {}", path.display(), err)),
    }
}

#[cfg(target_os = "linux")]
fn install_sqlite_systemd_user_timer(
    interval_hours: u32,
    indexer: &Path,
    roots: &[PathBuf],
) -> Result<String, String> {
    let home = ottrin_core::default_home_dir();
    let unit_dir = home.join(".config/systemd/user");
    std::fs::create_dir_all(&unit_dir)
        .map_err(|e| format!("Cannot create systemd unit dir: {}", e))?;
    let command = sqlite_refresh_shell_command(indexer, roots);
    let service = format!(
        "[Unit]\nDescription=Ottrin SQLite search index refresh\n\n\
         [Service]\nType=oneshot\nExecStart=/bin/sh -lc {}\n",
        shell_quote(&command)
    );
    let timer = format!(
        "[Unit]\nDescription=Ottrin SQLite search index refresh timer\n\n\
         [Timer]\nOnBootSec=2min\nOnUnitActiveSec={interval}h\nPersistent=true\n\n\
         [Install]\nWantedBy=timers.target\n",
        interval = interval_hours
    );
    std::fs::write(unit_dir.join("ottrin-index.service"), service).map_err(|e| e.to_string())?;
    std::fs::write(unit_dir.join("ottrin-index.timer"), timer).map_err(|e| e.to_string())?;
    for args in [
        vec!["--user", "daemon-reload"],
        vec!["--user", "enable", "--now", "ottrin-index.timer"],
    ] {
        let status = std::process::Command::new("systemctl")
            .args(&args)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("systemctl {:?} exited with {}", args, status));
        }
    }
    Ok(format!(
        "Background Ottrin index refresh enabled every {}h with systemd user timer.",
        interval_hours
    ))
}

#[cfg(target_os = "linux")]
fn install_sqlite_crontab_entry(
    interval_hours: u32,
    indexer: &Path,
    roots: &[PathBuf],
) -> Result<String, String> {
    let existing = std::process::Command::new("crontab")
        .arg("-l")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let tag = "# ottrin-index";
    if existing.contains(tag) {
        return Ok("Background Ottrin index crontab entry already installed.".to_string());
    }
    let command = sqlite_refresh_shell_command(indexer, roots);
    let entry = format!(
        "{tag}\n17 */{interval} * * * {command}\n",
        interval = interval_hours
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
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("crontab exited with {}", status));
    }
    Ok(format!(
        "Background Ottrin index refresh enabled every {}h with crontab.",
        interval_hours
    ))
}

#[cfg(target_os = "windows")]
fn install_sqlite_windows_task(
    interval_hours: u32,
    indexer: &Path,
    roots: &[PathBuf],
) -> Result<String, String> {
    let mut command = format!("\"{}\" --rebuild", indexer.display());
    for root in roots {
        command.push(' ');
        command.push('"');
        command.push_str(&root.display().to_string());
        command.push('"');
    }
    let status = internal_command("schtasks")
        .args([
            "/Create",
            "/F",
            "/SC",
            "HOURLY",
            "/MO",
            &interval_hours.to_string(),
            "/TN",
            "Ottrin Index",
            "/TR",
            &command,
        ])
        .status()
        .map_err(|e| format!("Failed to run schtasks: {}", e))?;
    if status.success() {
        Ok(format!(
            "Background Ottrin index refresh enabled every {}h with Windows Task Scheduler.",
            interval_hours
        ))
    } else {
        Err(format!("schtasks exited with {}", status))
    }
}

#[cfg(target_os = "linux")]
fn sqlite_refresh_shell_command(indexer: &Path, roots: &[PathBuf]) -> String {
    let mut command = format!("{} --rebuild", shell_quote(&indexer.display().to_string()));
    for root in roots {
        command.push(' ');
        command.push_str(&shell_quote(&root.display().to_string()));
    }
    command
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

fn expr_contains_negation(expr: &Expr) -> bool {
    match expr {
        Expr::Not(_) => true,
        Expr::And(parts) | Expr::Or(parts) => parts.iter().any(expr_contains_negation),
        Expr::Term(_) => false,
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn windows_internal_commands_request_no_console_window() {
        assert_eq!(internal_command_creation_flags(true), 0x0800_0000);
        assert_eq!(internal_command_creation_flags(false), 0);
    }

    fn temp_service_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ottrin-search-service-{name}-{}-{:?}.db",
            std::process::id(),
            SystemTime::now()
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
            fs::write(&path, b"stale sqlite sidecar").expect("create sidecar");
            path
        });

        service.clear_storage().expect("clear owned storage");

        assert_eq!(exits.load(Ordering::Acquire), 1);
        assert!(!db_path.exists());
        assert!(sidecars.iter().all(|path| !path.exists()));
        let diagnostics = service.diagnostics();
        assert!(matches!(diagnostics.status, SearchIndexStatus::Unavailable));
        assert_eq!(diagnostics.indexed_items, 0);
        assert!(service.query(query_by_name("anything")).items.is_empty());
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
        assert_eq!(
            effective_roots(&SearchConfig::default()),
            vec![ottrin_core::default_home_dir()]
        );
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
        let root = std::env::temp_dir().join(format!(
            "ottrin-search-handle-owner-{}-{:?}",
            std::process::id(),
            SystemTime::now()
        ));
        fs::create_dir_all(&root).expect("create handle ownership root");
        fs::write(root.join("initial.txt"), b"initial").expect("create initial file");

        let mut service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
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
    fn content_search_uses_active_fts_generation_before_ripgrep_fallback() {
        let _guard = SEARCH_SERVICE_TEST_LOCK.lock().expect("test lock");
        let root = std::env::temp_dir().join(format!(
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
        let root = std::env::temp_dir().join(format!(
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
        let root = std::env::temp_dir().join(format!(
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
        let root = std::env::temp_dir().join(format!(
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
    fn negated_content_search_returns_only_generation_complete_content_absences() {
        use crate::mutation::{IndexMutation, OwnedItem};
        use crate::plan::ScanPlan;

        let root = std::env::temp_dir().join(format!(
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
        fs::write(&complete, "ordinary searchable text").expect("write complete content");
        fs::write(&empty, "").expect("write empty content");
        fs::write(&binary, b"needle\0binary").expect("write binary content");
        fs::write(&oversized, vec![b'x'; 512 * 1024 + 1]).expect("write oversized content");

        let config = SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
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

        let response = service.query(SearchQuery {
            text: "!needle".to_string(),
            scope: SearchScope::Global,
            root_path: None,
            include_hidden_system: false,
            sort: SearchSort::Path,
            limit: 20,
            offset: 0,
            content_search: true,
        });

        assert!(response.error.is_none());
        assert_eq!(
            response.warning.as_deref(),
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

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(&service.store_path);
    }

    #[test]
    fn indexed_content_search_preserves_or_branch_filters() {
        let _guard = SEARCH_SERVICE_TEST_LOCK.lock().expect("test lock");
        let root = std::env::temp_dir().join(format!(
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
        let root = std::env::temp_dir().join(format!(
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
