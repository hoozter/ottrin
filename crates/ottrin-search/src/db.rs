use crate::mutation::{ActivationFence, GenerationSnapshot, IndexMutation, OwnedItem};
use crate::plan::ScanPlan;
use ottrin_core::{EntryKind, SearchResultItem};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use thiserror::Error;

/// Schema version for the on-disk search database.
pub const SEARCH_DB_VERSION: u32 = 6;

const GENERATION_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS search_generations (
    id INTEGER PRIMARY KEY,
    plan_identity TEXT NOT NULL,
    completed INTEGER NOT NULL,
    item_count INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS search_generation_items (
    generation_id INTEGER NOT NULL,
    path BLOB NOT NULL,
    parent_path BLOB NOT NULL,
    root_path BLOB NOT NULL,
    name TEXT NOT NULL,
    kind INTEGER NOT NULL,
    size_bytes INTEGER,
    modified_unix_secs INTEGER,
    is_executable INTEGER NOT NULL,
    symlink_target_is_dir INTEGER,
    PRIMARY KEY (generation_id, path),
    FOREIGN KEY (generation_id) REFERENCES search_generations(id) ON DELETE CASCADE
);

-- Every indexed item gets an explicit content-completeness row.  An absent FTS
-- row alone is never evidence that a file does not contain a query term.
CREATE TABLE IF NOT EXISTS search_generation_content_state (
    generation_id INTEGER NOT NULL,
    path BLOB NOT NULL,
    state INTEGER NOT NULL,
    content TEXT,
    PRIMARY KEY (generation_id, path),
    FOREIGN KEY (generation_id) REFERENCES search_generations(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS search_store_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    active_generation_id INTEGER NOT NULL,
    FOREIGN KEY (active_generation_id) REFERENCES search_generations(id)
);

CREATE VIRTUAL TABLE IF NOT EXISTS search_generation_content USING fts5(
    generation_id UNINDEXED,
    path UNINDEXED,
    content
);
"#;

#[allow(dead_code)] // Slice 2 storage API is intentionally isolated until worker integration.
pub(crate) struct SearchStore {
    conn: Connection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedContentHit {
    pub(crate) path: PathBuf,
    pub(crate) snippet: String,
    pub(crate) content: String,
    /// FTS5 ranks stronger matches with more-negative values. Convert that
    /// ordering into a bounded positive relevance contribution for the UI.
    pub(crate) relevance: i64,
}

pub(crate) struct GenerationContentItem {
    pub(crate) item: SearchResultItem,
    pub(crate) content: Option<String>,
    pub(crate) complete: bool,
}

#[derive(Debug)]
pub(crate) struct SearchGenerationMetadata {
    pub plan_identity: String,
    pub completed: bool,
    pub item_count: usize,
}

#[allow(dead_code)]
impl SearchStore {
    pub(crate) fn open(path: PathBuf) -> Result<Self, SearchDbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        configure_writer_connection(&conn)?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub(crate) fn load(
        &mut self,
        plan: &ScanPlan,
    ) -> Result<Option<GenerationSnapshot>, SearchDbError> {
        load_generation(&self.conn, plan)
    }

    pub(crate) fn load_cancellable(
        &mut self,
        plan: &ScanPlan,
        cancelled: &AtomicBool,
    ) -> Result<Option<GenerationSnapshot>, SearchDbError> {
        load_generation_inner(&self.conn, plan, Some(cancelled))
    }

    pub(crate) fn commit(
        &mut self,
        plan: &ScanPlan,
        mutation: IndexMutation,
    ) -> Result<GenerationSnapshot, SearchDbError> {
        self.commit_inner(plan, mutation, false, None, || {}, |_| {})
    }

    pub(crate) fn commit_guarded(
        &mut self,
        plan: &ScanPlan,
        mutation: IndexMutation,
        fence: &ActivationFence,
        expected_epoch: u64,
    ) -> Result<GenerationSnapshot, SearchDbError> {
        self.commit_guarded_then(plan, mutation, fence, expected_epoch, |_| {})
    }

    pub(crate) fn commit_guarded_then(
        &mut self,
        plan: &ScanPlan,
        mutation: IndexMutation,
        fence: &ActivationFence,
        expected_epoch: u64,
        after_commit: impl FnOnce(&GenerationSnapshot),
    ) -> Result<GenerationSnapshot, SearchDbError> {
        self.commit_inner(
            plan,
            mutation,
            false,
            Some((fence, expected_epoch)),
            || {},
            after_commit,
        )
    }

    #[cfg(test)]
    fn commit_failing_before_commit(
        &mut self,
        plan: &ScanPlan,
        mutation: IndexMutation,
    ) -> Result<GenerationSnapshot, SearchDbError> {
        self.commit_inner(plan, mutation, true, None, || {}, |_| {})
    }

    #[cfg(test)]
    fn commit_guarded_with_before_activation_hook(
        &mut self,
        plan: &ScanPlan,
        mutation: IndexMutation,
        fence: &ActivationFence,
        expected_epoch: u64,
        before_activation: impl FnOnce(),
    ) -> Result<GenerationSnapshot, SearchDbError> {
        self.commit_inner(
            plan,
            mutation,
            false,
            Some((fence, expected_epoch)),
            before_activation,
            |_| {},
        )
    }

    fn commit_inner(
        &mut self,
        plan: &ScanPlan,
        mutation: IndexMutation,
        fail_before_commit: bool,
        activation: Option<(&ActivationFence, u64)>,
        before_activation: impl FnOnce(),
        after_commit: impl FnOnce(&GenerationSnapshot),
    ) -> Result<GenerationSnapshot, SearchDbError> {
        let tx = self.conn.transaction()?;
        let current = if matches!(&mutation, IndexMutation::ReplaceAll(_)) {
            GenerationSnapshot::new(0, plan.identity().to_string(), Vec::new())
        } else {
            load_generation(&tx, plan)?.ok_or_else(|| {
                SearchDbError::InvalidGeneration(
                    "incremental mutation requires an active matching generation".to_string(),
                )
            })?
        };
        let candidate = current.apply(mutation);
        tx.execute(
            "INSERT INTO search_generations (plan_identity, completed, item_count) VALUES (?1, 0, ?2)",
            params![plan.identity(), candidate.items().len() as i64],
        )?;
        let generation = tx.last_insert_rowid();
        insert_generation_items(&tx, generation, candidate.items())?;
        insert_generation_content(&tx, generation, candidate.items())?;
        tx.execute(
            "UPDATE search_generations SET completed = 1 WHERE id = ?1",
            [generation],
        )?;
        before_activation();
        let activation_guard = activation.map(|(fence, expected_epoch)| {
            let guard = fence.lock();
            guard
                .matches(expected_epoch)
                .then_some(guard)
                .ok_or(SearchDbError::ActivationCancelled)
        });
        let _activation_guard = activation_guard.transpose()?;
        tx.execute(
            "INSERT INTO search_store_state (singleton, active_generation_id) VALUES (1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET active_generation_id = excluded.active_generation_id",
            [generation],
        )?;
        tx.execute(
            "DELETE FROM search_generations WHERE id <> ?1",
            [generation],
        )?;
        tx.execute(
            "DELETE FROM search_generation_content WHERE generation_id <> ?1",
            [generation],
        )?;

        let snapshot = load_generation(&tx, plan)?.ok_or_else(|| {
            SearchDbError::InvalidGeneration("active generation disappeared".to_string())
        })?;
        if fail_before_commit {
            return Err(SearchDbError::InjectedFailure);
        }
        tx.commit()?;
        after_commit(&snapshot);
        Ok(snapshot)
    }
}

pub(crate) fn read_active_generation_metadata(
    path: &Path,
) -> Result<Option<SearchGenerationMetadata>, SearchDbError> {
    if !path.exists() {
        return Ok(None);
    }

    let conn = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(250))?;

    let row: Option<(String, i64, i64)> = conn
        .query_row(
            "SELECT g.plan_identity, g.completed, g.item_count
             FROM search_store_state s
             JOIN search_generations g ON g.id = s.active_generation_id
             WHERE s.singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;

    let Some((plan_identity, completed, item_count)) = row else {
        return Ok(None);
    };
    if !(0..=1).contains(&completed) {
        return Err(SearchDbError::InvalidGeneration(
            "active generation has invalid completion state".to_string(),
        ));
    }
    if item_count < 0 {
        return Err(SearchDbError::InvalidGeneration(
            "active generation has invalid item count".to_string(),
        ));
    }
    Ok(Some(SearchGenerationMetadata {
        plan_identity,
        completed: completed == 1,
        item_count: usize::try_from(item_count).unwrap_or(0),
    }))
}

fn configure_writer_connection(conn: &Connection) -> Result<(), SearchDbError> {
    // A negated query intentionally holds a stable read snapshot while it
    // streams. WAL lets the next generation commit alongside that reader;
    // the bounded timeout handles brief writer contention without masking a
    // persistent indexing failure through unbounded retries.
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA busy_timeout = 250;",
    )?;
    Ok(())
}

const MAX_INDEXED_TEXT_BYTES: u64 = 512 * 1024;

fn insert_generation_content(
    tx: &Transaction<'_>,
    generation: i64,
    items: &[OwnedItem],
) -> Result<(), SearchDbError> {
    let mut content_stmt = tx.prepare_cached(
        "INSERT INTO search_generation_content (generation_id, path, content) VALUES (?1, ?2, ?3)",
    )?;
    let mut state_stmt = tx.prepare_cached(
        "INSERT INTO search_generation_content_state (generation_id, path, state, content) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for owned in items {
        let item = owned.item();
        let path = encode_path(&item.path);
        let (state, content) = if !matches!(item.kind, EntryKind::File) {
            (2_i64, None)
        } else if item
            .size_bytes
            .is_none_or(|size| size > MAX_INDEXED_TEXT_BYTES)
        {
            (3, None)
        } else {
            match std::fs::read(&item.path) {
                Err(_) => (4, None),
                Ok(bytes) => match String::from_utf8(bytes) {
                    Err(_) => (5, None),
                    Ok(content) if content.contains('\0') => (5, None),
                    Ok(content) if content.is_empty() => (1, None),
                    Ok(content) => (0, Some(content)),
                },
            }
        };
        state_stmt.execute(params![generation, &path, state, content])?;
        if let Some(content) = content {
            content_stmt.execute(params![generation, path, content])?;
        }
    }
    Ok(())
}

fn insert_generation_items(
    tx: &Transaction<'_>,
    generation: i64,
    items: &[OwnedItem],
) -> Result<(), SearchDbError> {
    let mut stmt = tx.prepare_cached(
        "INSERT INTO search_generation_items
         (generation_id, path, parent_path, root_path, name, kind, size_bytes,
          modified_unix_secs, is_executable, symlink_target_is_dir)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    for owned in items {
        let item = owned.item();
        stmt.execute(params![
            generation,
            encode_path(&item.path),
            encode_path(&item.parent_path),
            encode_path(owned.root()),
            item.name,
            entry_kind_to_i64(item.kind),
            item.size_bytes,
            item.modified_unix_secs,
            i64::from(item.is_executable),
            item.symlink_target_is_dir.map(i64::from),
        ])?;
    }
    Ok(())
}

fn load_generation(
    conn: &Connection,
    plan: &ScanPlan,
) -> Result<Option<GenerationSnapshot>, SearchDbError> {
    load_generation_inner(conn, plan, None)
}

/// Query one ordered page from the completed active FTS generation. The reader
/// opens a separate SQLite connection so UI queries never share the worker's
/// write connection.
pub(crate) fn query_indexed_content_page(
    db_path: &Path,
    plan: &ScanPlan,
    fts_query: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<IndexedContentHit>, SearchDbError> {
    if fts_query.trim().is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(250))?;
    let mut stmt = conn.prepare(
        "SELECT f.path, snippet(search_generation_content, 2, '', '', '…', 20), f.content, rank
         FROM search_generation_content f
         JOIN search_store_state s ON s.singleton = 1 AND s.active_generation_id = f.generation_id
         JOIN search_generations g ON g.id = f.generation_id
         WHERE g.completed = 1 AND g.plan_identity = ?1
           AND search_generation_content MATCH ?2
         ORDER BY rank, f.rowid
         LIMIT ?3 OFFSET ?4",
    )?;
    let rows = stmt.query_map(
        params![plan.identity(), fts_query, limit as i64, offset as i64],
        |row| {
            let raw_snippet: String = row.get(1)?;
            Ok(IndexedContentHit {
                path: decode_path(row.get(0)?)?,
                snippet: content_snippet_line(&raw_snippet, &fts_query_terms(fts_query)),
                content: row.get(2)?,
                relevance: fts_rank_relevance(row.get(3)?),
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Stream the completed active generation from a single read transaction. This
/// is intentionally separate from the worker connection: queries cannot retain
/// the writer and each callback owns at most one item's indexed text.
pub(crate) fn stream_generation_content_items(
    db_path: &Path,
    plan: &ScanPlan,
    cancelled: Option<&AtomicBool>,
    mut visit: impl FnMut(GenerationContentItem),
) -> Result<(), SearchDbError> {
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(SearchDbError::ContentQueryCancelled);
    }
    let mut conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(250))?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)?;
    let mut stmt = tx.prepare(
        "SELECT i.path, i.parent_path, i.root_path, i.name, i.kind, i.size_bytes,
                i.modified_unix_secs, i.is_executable, i.symlink_target_is_dir,
                c.state, c.content
         FROM search_store_state s
         JOIN search_generations g ON g.id = s.active_generation_id
         JOIN search_generation_items i ON i.generation_id = g.id
         LEFT JOIN search_generation_content_state c
           ON c.generation_id = i.generation_id AND c.path = i.path
         WHERE s.singleton = 1 AND g.completed = 1 AND g.plan_identity = ?1
         ORDER BY i.path",
    )?;
    let mut rows = stmt.query([plan.identity()])?;
    while let Some(row) = rows.next()? {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(SearchDbError::ContentQueryCancelled);
        }
        let state: Option<i64> = row.get(9)?;
        let complete = matches!(state, Some(0 | 1));
        let mut item = SearchResultItem {
            path: decode_path(row.get(0)?)?,
            parent_path: decode_path(row.get(1)?)?,
            name: row.get(3)?,
            kind: entry_kind_from_i64(row.get(4)?),
            size_bytes: row.get(5)?,
            modified_unix_secs: row.get(6)?,
            is_executable: row.get::<_, i64>(7)? != 0,
            symlink_target_is_dir: row.get::<_, Option<i64>>(8)?.map(|value| value != 0),
            content_snippet: None,
            name_lower: String::new(),
            path_str: String::new(),
            path_lower: String::new(),
        };
        item.prepare();
        visit(GenerationContentItem {
            item,
            content: row.get(10)?,
            complete,
        });
        // A UI supersession may arrive while the callback evaluates the final
        // streamed row. A post-callback check ensures that request still
        // terminates as cancelled and drops this snapshot transaction.
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(SearchDbError::ContentQueryCancelled);
        }
    }
    drop(rows);
    drop(stmt);
    tx.commit()?;
    Ok(())
}

fn fts_rank_relevance(rank: f64) -> i64 {
    const SCALE: f64 = 1_000_000.0;
    if rank.is_nan() || rank >= 0.0 {
        0
    } else {
        let scaled = (-rank) * SCALE;
        if scaled >= i64::MAX as f64 {
            i64::MAX
        } else {
            scaled as i64
        }
    }
}

fn fts_query_terms(fts_query: &str) -> Vec<String> {
    let quoted = fts_query
        .split('"')
        .enumerate()
        .filter_map(|(index, value)| (index % 2 == 1).then_some(value.replace("\"\"", "\"")))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if quoted.is_empty() {
        fts_query
            .split_whitespace()
            .filter(|term| term.len() >= 2)
            .map(str::to_ascii_lowercase)
            .collect()
    } else {
        quoted
            .into_iter()
            .map(|term| term.to_ascii_lowercase())
            .collect()
    }
}

fn content_snippet_line(content: &str, terms: &[String]) -> String {
    content
        .lines()
        .find(|line| {
            let lowered = line.to_ascii_lowercase();
            terms.iter().all(|term| lowered.contains(term))
        })
        .or_else(|| content.lines().find(|line| !line.trim().is_empty()))
        .unwrap_or(content)
        .trim()
        .to_string()
}

fn load_generation_inner(
    conn: &Connection,
    plan: &ScanPlan,
    cancelled: Option<&AtomicBool>,
) -> Result<Option<GenerationSnapshot>, SearchDbError> {
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(SearchDbError::LoadCancelled);
    }
    let active = conn
        .query_row(
            "SELECT g.id, g.plan_identity, g.completed, g.item_count
             FROM search_store_state s
             JOIN search_generations g ON g.id = s.active_generation_id
             WHERE s.singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((generation, identity, completed, expected_count)): Option<(i64, String, i64, i64)> =
        active
    else {
        return Ok(None);
    };
    if completed != 1 {
        return Err(SearchDbError::InvalidGeneration(
            "active generation is incomplete".to_string(),
        ));
    }
    if identity != plan.identity() {
        return Err(SearchDbError::PlanIdentityMismatch);
    }

    let mut stmt = conn.prepare(
        "SELECT path, parent_path, root_path, name, kind, size_bytes,
                modified_unix_secs, is_executable, symlink_target_is_dir
         FROM search_generation_items WHERE generation_id = ?1 ORDER BY path",
    )?;
    let rows = stmt.query_map([generation], |row| {
        let mut item = SearchResultItem {
            path: decode_path(row.get(0)?)?,
            parent_path: decode_path(row.get(1)?)?,
            name: row.get(3)?,
            kind: entry_kind_from_i64(row.get(4)?),
            size_bytes: row.get(5)?,
            modified_unix_secs: row.get(6)?,
            is_executable: row.get::<_, i64>(7)? != 0,
            symlink_target_is_dir: row.get::<_, Option<i64>>(8)?.map(|value| value != 0),
            content_snippet: None,
            name_lower: String::new(),
            path_str: String::new(),
            path_lower: String::new(),
        };
        item.prepare();
        Ok(OwnedItem::new(decode_path(row.get(2)?)?, item))
    })?;
    let mut items = Vec::with_capacity(usize::try_from(expected_count).unwrap_or(0));
    for row in rows {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(SearchDbError::LoadCancelled);
        }
        items.push(row?);
    }
    if i64::try_from(items.len()).ok() != Some(expected_count) {
        return Err(SearchDbError::InvalidGeneration(format!(
            "active generation expected {expected_count} items but loaded {}",
            items.len()
        )));
    }
    Ok(Some(GenerationSnapshot::new(generation, identity, items)))
}

#[cfg(unix)]
fn encode_path(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn decode_path(bytes: Vec<u8>) -> rusqlite::Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(windows)]
fn encode_path(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
fn decode_path(bytes: Vec<u8>) -> rusqlite::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    if !bytes.len().is_multiple_of(2) {
        return Err(rusqlite::Error::FromSqlConversionFailure(
            bytes.len(),
            rusqlite::types::Type::Blob,
            "path contains an incomplete UTF-16 code unit".into(),
        ));
    }
    let wide = bytes
        .chunks_exact(2)
        .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
        .collect::<Vec<_>>();
    Ok(PathBuf::from(OsString::from_wide(&wide)))
}

#[derive(Debug, Error)]
pub enum SearchDbError {
    #[error("sqlite error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported database version: {0}")]
    UnsupportedVersion(u32),
    #[error("invalid active generation: {0}")]
    InvalidGeneration(String),
    #[error("active generation belongs to a different scan plan")]
    PlanIdentityMismatch,
    #[error("injected failure before commit")]
    InjectedFailure,
    #[error("candidate activation was cancelled")]
    ActivationCancelled,
    #[error("generation load was cancelled")]
    LoadCancelled,
    #[error("content generation query was cancelled")]
    ContentQueryCancelled,
}

pub fn search_db_path_no_create() -> PathBuf {
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
    dir.join("search.db")
}

pub fn search_db_path() -> PathBuf {
    let path = search_db_path_no_create();
    let _ = std::fs::create_dir_all(path.parent().unwrap_or_else(|| std::path::Path::new(".")));
    path
}

fn migrate(conn: &Connection) -> Result<(), SearchDbError> {
    let version: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    match version {
        0..=4 | SEARCH_DB_VERSION => {
            conn.execute_batch(GENERATION_SCHEMA)?;
            conn.execute_batch(
                "DROP TABLE IF EXISTS scan_state;
                 DROP TABLE IF EXISTS roots;
                 DROP TABLE IF EXISTS files;
                 DROP TABLE IF EXISTS meta;",
            )?;
        }
        5 => {
            // v5 had FTS rows without a per-item completeness record. Reusing
            // those generations would make negation unsound, so force the
            // normal rebuild path instead of guessing from absent FTS rows.
            conn.execute_batch(
                "DROP TABLE IF EXISTS search_generation_content;
                 DROP TABLE IF EXISTS search_generation_content_state;
                 DROP TABLE IF EXISTS search_generation_items;
                 DROP TABLE IF EXISTS search_store_state;
                 DROP TABLE IF EXISTS search_generations;",
            )?;
            conn.execute_batch(GENERATION_SCHEMA)?;
        }
        other => return Err(SearchDbError::UnsupportedVersion(other)),
    }
    if version != SEARCH_DB_VERSION {
        conn.execute("PRAGMA user_version = 6", [])?;
    }
    Ok(())
}

fn entry_kind_to_i64(kind: EntryKind) -> i64 {
    match kind {
        EntryKind::File => 0,
        EntryKind::Directory => 1,
        EntryKind::Symlink => 2,
        EntryKind::Other => 3,
    }
}

fn entry_kind_from_i64(value: i64) -> EntryKind {
    match value {
        1 => EntryKind::Directory,
        2 => EntryKind::Symlink,
        3 => EntryKind::Other,
        _ => EntryKind::File,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("ottrin-{name}-{nonce}.db"))
    }

    #[test]
    fn fresh_database_has_no_legacy_persistence_schema() {
        let db_path = temp_db_path("no-legacy-persistence");
        let _store = SearchStore::open(db_path.clone()).expect("open store");
        let conn = Connection::open(&db_path).expect("open database");
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN ('scan_state', 'roots', 'files', 'meta')",
                [],
                |row| row.get(0),
            )
            .expect("inspect schema");
        assert_eq!(table_count, 0);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn fresh_database_has_generation_scoped_fts_content_table() {
        let db_path = temp_db_path("fts-schema");
        let _store = SearchStore::open(db_path.clone()).expect("open store");
        let conn = Connection::open(&db_path).expect("open database");
        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'search_generation_content'",
                [],
                |row| row.get(0),
            )
            .expect("FTS content table");
        assert!(sql.contains("fts5"));
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn v5_database_is_reset_before_completeness_state_is_required() {
        let db_path = temp_db_path("v5-completeness-reset");
        let conn = Connection::open(&db_path).expect("open old database");
        conn.execute_batch(
            "CREATE TABLE search_generations (id INTEGER PRIMARY KEY, plan_identity TEXT NOT NULL, completed INTEGER NOT NULL, item_count INTEGER NOT NULL);
             CREATE TABLE search_generation_items (generation_id INTEGER NOT NULL, path BLOB NOT NULL, parent_path BLOB NOT NULL, root_path BLOB NOT NULL, name TEXT NOT NULL, kind INTEGER NOT NULL, size_bytes INTEGER, modified_unix_secs INTEGER, is_executable INTEGER NOT NULL, symlink_target_is_dir INTEGER, PRIMARY KEY (generation_id, path));
             CREATE TABLE search_store_state (singleton INTEGER PRIMARY KEY, active_generation_id INTEGER NOT NULL);
             CREATE VIRTUAL TABLE search_generation_content USING fts5(generation_id UNINDEXED, path UNINDEXED, content);
             INSERT INTO search_generations VALUES (1, 'old-plan', 1, 1);
             INSERT INTO search_store_state VALUES (1, 1);
             PRAGMA user_version = 5;",
        )
        .expect("seed v5 schema");
        drop(conn);

        let _store = SearchStore::open(db_path.clone()).expect("migrate v5 database");
        let conn = Connection::open(&db_path).expect("reopen migrated database");
        let active_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM search_store_state", [], |row| {
                row.get(0)
            })
            .expect("count active generation");
        let state_table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'search_generation_content_state'",
                [],
                |row| row.get(0),
            )
            .expect("inspect completeness schema");
        assert_eq!(active_count, 0, "v5 rows cannot prove content completeness");
        assert_eq!(state_table_count, 1);
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .expect("schema version"),
            SEARCH_DB_VERSION
        );
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn fts_rank_relevance_is_monotonic_and_saturating() {
        assert!(fts_rank_relevance(-0.01) > fts_rank_relevance(-0.001));
        assert_eq!(fts_rank_relevance(0.0), 0);
        assert_eq!(fts_rank_relevance(f64::NAN), 0);
        assert_eq!(fts_rank_relevance(f64::NEG_INFINITY), i64::MAX);
        assert_eq!(fts_rank_relevance(-f64::MAX), i64::MAX);
    }
}

#[cfg(test)]
mod generation_tests {
    use super::*;
    use crate::mutation::{ActivationFence, IndexMutation, OwnedItem};
    use crate::plan::ScanPlan;
    use ottrin_core::SearchConfig;
    use std::sync::{Arc, atomic::AtomicBool};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("ottrin-{name}-{nonce}.db"))
    }

    fn plan(root: &Path) -> ScanPlan {
        ScanPlan::new(&SearchConfig {
            include_roots: vec![root.to_path_buf()],
            include_hidden_system: true,
            ..SearchConfig::default()
        })
        .expect("valid plan")
    }

    fn owned(root: &Path, relative: &str, size: u64) -> OwnedItem {
        let path = root.join(relative);
        let mut item = SearchResultItem {
            parent_path: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path,
            kind: EntryKind::File,
            size_bytes: Some(size),
            modified_unix_secs: Some(1),
            is_executable: false,
            symlink_target_is_dir: None,
            content_snippet: None,
            name_lower: String::new(),
            path_str: String::new(),
            path_lower: String::new(),
        };
        item.prepare();
        OwnedItem::new(root.to_path_buf(), item)
    }

    #[test]
    fn replace_all_removes_stale_rows() {
        let db_path = temp_db_path("generation-replace-all");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &plan,
                IndexMutation::ReplaceAll(vec![
                    owned(&root, "stale.txt", 1),
                    owned(&root, "kept.txt", 2),
                ]),
            )
            .expect("first generation");

        let committed = store
            .commit(
                &plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "kept.txt", 3)]),
            )
            .expect("replacement generation");

        assert_eq!(committed.items(), &[owned(&root, "kept.txt", 3)]);
        assert_eq!(store.load(&plan).expect("load"), Some(committed));
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn wal_allows_generation_commit_while_a_snapshot_reader_is_open() {
        let db_path = temp_db_path("wal-snapshot-writer");
        let root = std::env::temp_dir().join(format!(
            "ottrin-wal-snapshot-reader-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(root.join("first.txt"), "first").expect("write first");
        std::fs::write(root.join("second.txt"), "second").expect("write second");
        let scan_plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open WAL store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "first.txt", 5)]),
            )
            .expect("commit first generation");

        let reader = Connection::open(&db_path).expect("open snapshot reader");
        reader
            .execute_batch("BEGIN; SELECT * FROM search_generation_items;")
            .expect("hold read snapshot");

        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "second.txt", 6)]),
            )
            .expect("WAL writer must commit alongside snapshot reader");
        reader
            .execute_batch("COMMIT")
            .expect("release snapshot reader");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn committed_generation_indexes_bounded_utf8_content_and_replaces_stale_hits() {
        let db_path = temp_db_path("generation-fts-content");
        let root = std::env::temp_dir().join(format!(
            "ottrin-generation-fts-content-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create root");
        let scan_plan = plan(&root);
        let old = root.join("old.txt");
        let new = root.join("new.txt");
        std::fs::write(&old, "retired unique content").expect("write old content");
        std::fs::write(&new, "fresh unique content").expect("write new content");
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "old.txt", 22)]),
            )
            .expect("commit old generation");
        let old_hits =
            query_indexed_content_page(&db_path, &scan_plan, "retired", 10, 0).expect("query old");
        assert_eq!(old_hits.len(), 1);
        assert_eq!(old_hits[0].path, old);
        assert_eq!(old_hits[0].snippet, "retired unique content");
        assert_eq!(old_hits[0].content, "retired unique content");
        assert!(old_hits[0].relevance > 0);
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "new.txt", 20)]),
            )
            .expect("commit new generation");
        assert!(
            query_indexed_content_page(&db_path, &scan_plan, "retired", 10, 0)
                .expect("query stale")
                .is_empty()
        );
        let new_hits =
            query_indexed_content_page(&db_path, &scan_plan, "fresh", 10, 0).expect("query fresh");
        assert_eq!(new_hits.len(), 1);
        assert_eq!(new_hits[0].path, new);
        assert_eq!(new_hits[0].snippet, "fresh unique content");
        assert_eq!(new_hits[0].content, "fresh unique content");
        assert!(new_hits[0].relevance > 0);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn generation_content_skips_binary_and_oversized_files() {
        let db_path = temp_db_path("generation-fts-bounds");
        let root = std::env::temp_dir().join(format!(
            "ottrin-generation-fts-bounds-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create root");
        let scan_plan = plan(&root);
        std::fs::write(root.join("binary.txt"), b"needle\0not text").expect("write binary");
        std::fs::write(
            root.join("oversized.txt"),
            vec![b'x'; MAX_INDEXED_TEXT_BYTES as usize + 1],
        )
        .expect("write oversized content");
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![
                    owned(&root, "binary.txt", 15),
                    owned(&root, "oversized.txt", MAX_INDEXED_TEXT_BYTES + 1),
                ]),
            )
            .expect("commit generation");

        assert!(
            query_indexed_content_page(&db_path, &scan_plan, "needle", 10, 0)
                .expect("query binary")
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn cancelled_content_stream_returns_before_opening_a_reader() {
        let db_path = temp_db_path("generation-content-cancelled-query");
        let root = PathBuf::from("root");
        let scan_plan = plan(&root);
        let cancelled = AtomicBool::new(true);

        assert!(matches!(
            stream_generation_content_items(&db_path, &scan_plan, Some(&cancelled), |_| {}),
            Err(SearchDbError::ContentQueryCancelled)
        ));
    }

    #[test]
    fn cancelled_content_stream_releases_its_snapshot_reader() {
        let db_path = temp_db_path("generation-content-cancel-releases-reader");
        let root = std::env::temp_dir().join(format!(
            "ottrin-content-cancel-reader-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(root.join("item.txt"), "content").expect("write item");
        let scan_plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "item.txt", 7)]),
            )
            .expect("commit generation");
        let cancelled = AtomicBool::new(false);

        assert!(matches!(
            stream_generation_content_items(&db_path, &scan_plan, Some(&cancelled), |_| {
                cancelled.store(true, Ordering::Release);
            }),
            Err(SearchDbError::ContentQueryCancelled)
        ));

        let checkpoint = Connection::open(&db_path).expect("open checkpoint connection");
        let busy: i64 = checkpoint
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
            .expect("checkpoint after cancellation");
        assert_eq!(
            busy, 0,
            "cancelled stream must not retain a snapshot reader"
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn changed_plan_replace_all_publishes_a_new_generation() {
        let db_path = temp_db_path("generation-plan-change");
        let root_a = PathBuf::from("root-a");
        let root_b = PathBuf::from("root-b");
        let plan_a = plan(&root_a);
        let plan_b = plan(&root_b);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &plan_a,
                IndexMutation::ReplaceAll(vec![owned(&root_a, "old.txt", 1)]),
            )
            .expect("commit first plan");

        let replacement = store
            .commit(
                &plan_b,
                IndexMutation::ReplaceAll(vec![owned(&root_b, "new.txt", 2)]),
            )
            .expect("replace generation for changed plan");

        assert_eq!(replacement.plan_identity(), plan_b.identity());
        assert_eq!(replacement.items().len(), 1);
        assert_eq!(replacement.items()[0].item().path, root_b.join("new.txt"));
        assert_eq!(
            store
                .load(&plan_b)
                .expect("load replacement")
                .expect("active replacement"),
            replacement
        );
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn successful_commit_prunes_superseded_generations() {
        let db_path = temp_db_path("generation-pruning");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");

        for revision in 1..=3 {
            store
                .commit(
                    &plan,
                    IndexMutation::ReplaceAll(vec![owned(&root, "current.txt", revision)]),
                )
                .expect("commit generation");
        }

        let generation_count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM search_generations", [], |row| {
                row.get(0)
            })
            .expect("count generations");
        let item_count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM search_generation_items", [], |row| {
                row.get(0)
            })
            .expect("count generation items");
        assert_eq!(generation_count, 1);
        assert_eq!(item_count, 1);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn failure_before_commit_preserves_previous_active_generation() {
        let db_path = temp_db_path("generation-rollback");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        let previous = store
            .commit(
                &plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "old.txt", 1)]),
            )
            .expect("first generation");

        let error = store
            .commit_failing_before_commit(
                &plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "new.txt", 2)]),
            )
            .expect_err("injected failure");

        assert!(matches!(error, SearchDbError::InjectedFailure));
        assert_eq!(store.load(&plan).expect("load"), Some(previous));
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn cancellation_before_activation_rolls_back_real_store_candidate() {
        let db_path = temp_db_path("generation-cancel-before-activation");
        let root = PathBuf::from("root");
        let scan_plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        let previous = store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "old.txt", 1)]),
            )
            .expect("first generation");
        let fence = Arc::new(ActivationFence::new());
        let expected_epoch = fence.capture();
        let commit_fence = Arc::clone(&fence);
        let commit_root = root.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();

        let commit = std::thread::spawn(move || {
            store.commit_guarded_with_before_activation_hook(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![owned(&commit_root, "obsolete.txt", 2)]),
                &commit_fence,
                expected_epoch,
                || {
                    entered_tx.send(()).expect("signal candidate inserted");
                    release_rx.recv().expect("release activation");
                },
            )
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("candidate insertion reached");
        fence.invalidate();
        release_tx.send(()).expect("release candidate");

        assert!(matches!(
            commit.join().expect("commit thread"),
            Err(SearchDbError::ActivationCancelled)
        ));
        let mut reopened = SearchStore::open(db_path.clone()).expect("reopen store");
        let load_plan = plan(&root);
        assert_eq!(reopened.load(&load_plan).expect("load"), Some(previous));
        let generation_count: i64 = reopened
            .conn
            .query_row("SELECT COUNT(*) FROM search_generations", [], |row| {
                row.get(0)
            })
            .expect("count generations");
        assert_eq!(generation_count, 1, "candidate transaction rolled back");
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn load_rejects_incomplete_and_count_mismatched_generations() {
        let db_path = temp_db_path("generation-integrity");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        let snapshot = store
            .commit(
                &plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "one.txt", 1)]),
            )
            .expect("generation");
        let conn = Connection::open(&db_path).expect("open raw connection");
        conn.execute(
            "UPDATE search_generations SET completed = 0 WHERE id = ?1",
            [snapshot.generation()],
        )
        .expect("corrupt completion");
        assert!(matches!(
            store.load(&plan),
            Err(SearchDbError::InvalidGeneration(_))
        ));

        conn.execute(
            "UPDATE search_generations SET completed = 1, item_count = 2 WHERE id = ?1",
            [snapshot.generation()],
        )
        .expect("corrupt count");
        assert!(matches!(
            store.load(&plan),
            Err(SearchDbError::InvalidGeneration(_))
        ));
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn load_rejects_plan_identity_mismatch() {
        let db_path = temp_db_path("generation-plan-mismatch");
        let root = PathBuf::from("root");
        let scan_plan = plan(&root);
        let different_plan = plan(Path::new("different-root"));
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "one.txt", 1)]),
            )
            .expect("generation");

        assert!(matches!(
            store.load(&different_plan),
            Err(SearchDbError::PlanIdentityMismatch)
        ));
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn delta_commit_and_returned_snapshot_are_identical() {
        let db_path = temp_db_path("generation-delta");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &plan,
                IndexMutation::ReplaceAll(vec![
                    owned(&root, "tree/old.txt", 1),
                    owned(&root, "keep.txt", 2),
                ]),
            )
            .expect("first generation");

        let committed = store
            .commit(
                &plan,
                IndexMutation::Delta {
                    upserts: vec![owned(&root, "added.txt", 3), owned(&root, "keep.txt", 4)],
                    remove_trees: vec![root.join("tree")],
                },
            )
            .expect("delta generation");

        assert_eq!(store.load(&plan).expect("load"), Some(committed));
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn recursive_delta_delete_remains_gone_after_database_reopen() {
        let db_path = temp_db_path("generation-recursive-delete-reopen");
        let root = PathBuf::from("root");
        let scan_plan = plan(&root);
        let committed = {
            let mut store = SearchStore::open(db_path.clone()).expect("open store");
            store
                .commit(
                    &scan_plan,
                    IndexMutation::ReplaceAll(vec![
                        owned(&root, "tree/child.txt", 1),
                        owned(&root, "tree/nested/grandchild.txt", 2),
                        owned(&root, "keep.txt", 3),
                    ]),
                )
                .expect("initial generation");
            store
                .commit(
                    &scan_plan,
                    IndexMutation::Delta {
                        upserts: Vec::new(),
                        remove_trees: vec![root.join("tree")],
                    },
                )
                .expect("recursive delete")
        };

        let mut reopened = SearchStore::open(db_path.clone()).expect("reopen store");
        assert_eq!(reopened.load(&scan_plan).expect("load"), Some(committed));
        assert_eq!(
            reopened
                .load(&scan_plan)
                .expect("load")
                .expect("snapshot")
                .items(),
            &[owned(&root, "keep.txt", 3)]
        );
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn replace_root_commit_only_changes_the_named_root() {
        let db_path = temp_db_path("generation-replace-root");
        let parent = PathBuf::from("root");
        let nested = parent.join("nested");
        let scan_plan = plan(&parent);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![
                    owned(&parent, "parent.txt", 1),
                    owned(&nested, "old.txt", 2),
                ]),
            )
            .expect("first generation");

        let committed = store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceRoot {
                    root: nested.clone(),
                    items: vec![owned(&nested, "new.txt", 3)],
                },
            )
            .expect("replace nested root");

        assert_eq!(
            committed.items(),
            &[
                owned(&nested, "new.txt", 3),
                owned(&parent, "parent.txt", 1)
            ]
        );
        assert_eq!(store.load(&scan_plan).expect("load"), Some(committed));
        let _ = std::fs::remove_file(db_path);
    }

    #[cfg(unix)]
    #[test]
    fn generation_path_encoding_round_trips_non_utf8_bytes() {
        use std::os::unix::ffi::OsStringExt;

        let path = PathBuf::from(OsString::from_vec(vec![b'r', b'o', b'o', b't', b'/', 0xff]));
        assert_eq!(decode_path(encode_path(&path)).expect("decode path"), path);
    }

    #[test]
    fn v2_database_is_not_promoted_and_legacy_rows_are_dropped() {
        let db_path = temp_db_path("legacy-v2");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        let conn = Connection::open(&db_path).expect("open legacy database");
        conn.execute_batch(
            "CREATE TABLE files (path TEXT PRIMARY KEY);
             INSERT INTO files(path) VALUES ('legacy.txt');
             PRAGMA user_version = 2;",
        )
        .expect("create v2 schema");
        drop(conn);

        let mut store = SearchStore::open(db_path.clone()).expect("migrate store");
        assert_eq!(store.load(&plan).expect("load"), None);
        let legacy_tables: i64 = Connection::open(&db_path)
            .expect("open migrated database")
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'files'",
                [],
                |row| row.get(0),
            )
            .expect("inspect legacy schema");
        assert_eq!(legacy_tables, 0);
        let _ = std::fs::remove_file(db_path);
    }
}
