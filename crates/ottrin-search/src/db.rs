use crate::mutation::{ActivationFence, GenerationSnapshot, IndexMutation, OwnedItem};
use crate::plan::ScanPlan;
use ottrin_core::{EntryKind, SearchResultItem, SearchSort};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
#[cfg(test)]
use std::cell::Cell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(test)]
thread_local! {
    static PATH_FTS_DELETE_COUNT: Cell<usize> = const { Cell::new(0) };
}
use thiserror::Error;

/// Schema version for the on-disk search database.
pub const SEARCH_DB_VERSION: u32 = 9;

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

CREATE INDEX IF NOT EXISTS search_generation_items_by_parent
ON search_generation_items (generation_id, parent_path);

CREATE TABLE IF NOT EXISTS search_store_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    active_generation_id INTEGER NOT NULL,
    FOREIGN KEY (active_generation_id) REFERENCES search_generations(id)
);

"#;

const CONTENT_FTS_SCHEMA: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS search_generation_paths USING fts5(
    generation_id UNINDEXED,
    path UNINDEXED,
    path_text,
    tokenize = 'trigram'
);

-- Every indexed item gets an explicit content-completeness row. An absent FTS
-- row alone is never evidence that a file does not contain a query term.
CREATE TABLE IF NOT EXISTS search_generation_content_state (
    generation_id INTEGER NOT NULL,
    path BLOB NOT NULL,
    state INTEGER NOT NULL,
    content TEXT,
    PRIMARY KEY (generation_id, path)
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
    path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SearchStoreDiagnostics {
    pub(crate) database_bytes: u64,
    pub(crate) wal_bytes: u64,
    pub(crate) freelist_pages: u64,
    pub(crate) freelist_bytes: u64,
}

/// A checkpoint is expensive enough to reserve for whole-generation commits
/// and unusually large incremental batches. Passive mode never blocks readers.
const CHECKPOINT_MUTATION_ITEM_THRESHOLD: usize = 512;
// Reclaim at most 1 MiB with the default 4 KiB page size after a replacement.
// The remaining freelist is reusable by the next generation, avoiding a full
// high-water teardown/rebuild cycle and its corresponding WAL amplification.
const INCREMENTAL_VACUUM_PAGE_BUDGET: u64 = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedContentHit {
    pub(crate) item: SearchResultItem,
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

#[allow(dead_code)]
impl SearchStore {
    pub(crate) fn open(path: PathBuf) -> Result<Self, SearchDbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(&path)?;
        configure_writer_connection(&conn)?;
        let migrated = migrate(&mut conn, &path)?;
        let removed_incomplete = cleanup_incomplete_generations(&mut conn, &path)?;
        if migrated || removed_incomplete {
            let _: (u64, u64, u64) =
                conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?;
        } else {
            checkpoint_wal_passively(&conn)?;
        }
        Ok(Self { conn, path })
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

    /// Validate and load only active-generation metadata. Production startup
    /// must not decode every persisted path merely to publish readiness.
    pub(crate) fn load_count_cancellable(
        &mut self,
        plan: &ScanPlan,
        cancelled: &AtomicBool,
    ) -> Result<Option<usize>, SearchDbError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(SearchDbError::LoadCancelled);
        }
        let Some((generation, stored_identity, completed, stored_count)) = self
            .conn
            .query_row(
                "SELECT g.id, g.plan_identity, g.completed, g.item_count
                 FROM search_store_state s
                 JOIN search_generations g ON g.id = s.active_generation_id
                 WHERE s.singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?
        else {
            return Ok(None);
        };
        if stored_identity != plan.identity() {
            return Ok(None);
        }
        if completed != 1 || stored_count < 0 {
            return Ok(None);
        }
        let actual_count = self.conn.query_row(
            "SELECT COUNT(*) FROM search_generation_items WHERE generation_id = ?1",
            [generation],
            |row| row.get::<_, i64>(0),
        )?;
        if cancelled.load(Ordering::Acquire) {
            return Err(SearchDbError::LoadCancelled);
        }
        if actual_count != stored_count {
            return Ok(None);
        }
        if !generation_content_db_path(&self.path, generation).is_file() {
            return Ok(None);
        }
        Ok(usize::try_from(stored_count).ok())
    }

    pub(crate) fn load_count_read_only(
        path: &Path,
        plan: &ScanPlan,
    ) -> Result<Option<usize>, SearchDbError> {
        if !path.is_file() {
            return Ok(None);
        }
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_millis(250))?;
        let mut store = Self {
            conn,
            path: path.to_path_buf(),
        };
        store.load_count_cancellable(plan, &AtomicBool::new(false))
    }

    pub(crate) fn load_read_only(
        path: &Path,
        plan: &ScanPlan,
    ) -> Result<Option<GenerationSnapshot>, SearchDbError> {
        if !path.is_file() {
            return Ok(None);
        }
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_millis(250))?;
        load_generation(&conn, plan)
    }

    pub(crate) fn diagnostics(&self) -> Result<SearchStoreDiagnostics, SearchDbError> {
        let freelist_pages = self
            .conn
            .query_row("PRAGMA freelist_count", [], |row| row.get::<_, u64>(0))?;
        let page_size = self
            .conn
            .query_row("PRAGMA page_size", [], |row| row.get::<_, u64>(0))?;
        let content_database_bytes = generation_content_db_paths(&self.path)
            .into_iter()
            .filter_map(|path| std::fs::metadata(path).ok())
            .map(|metadata| metadata.len())
            .sum::<u64>();
        Ok(SearchStoreDiagnostics {
            database_bytes: std::fs::metadata(&self.path)
                .map(|metadata| metadata.len())
                .unwrap_or(0)
                .saturating_add(content_database_bytes),
            wal_bytes: std::fs::metadata(sqlite_sidecar_path(&self.path, "-wal"))
                .map(|metadata| metadata.len())
                .unwrap_or(0),
            freelist_pages,
            freelist_bytes: freelist_pages.saturating_mul(page_size),
        })
    }

    pub(crate) fn begin_staged_generation(
        &mut self,
        plan: &ScanPlan,
    ) -> Result<i64, SearchDbError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO search_generations (plan_identity, completed, item_count)
             VALUES (?1, 0, 0)",
            [plan.identity()],
        )?;
        let generation = tx.last_insert_rowid();
        tx.commit()?;
        if let Err(error) = create_generation_content_db(&self.path, generation) {
            self.conn
                .execute("DELETE FROM search_generations WHERE id = ?1", [generation])?;
            return Err(error);
        }
        Ok(generation)
    }

    pub(crate) fn append_staged_generation(
        &mut self,
        plan: &ScanPlan,
        generation: i64,
        items: &[OwnedItem],
    ) -> Result<(), SearchDbError> {
        let content_path = generation_content_db_path(&self.path, generation);
        attach_generation_content_db(&self.conn, &content_path)?;
        let result = (|| {
            let tx = self.conn.transaction()?;
            let (identity, completed): (String, i64) = tx
                .query_row(
                    "SELECT plan_identity, completed FROM search_generations WHERE id = ?1",
                    [generation],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?
                .ok_or_else(|| {
                    SearchDbError::InvalidGeneration("staged generation disappeared".to_string())
                })?;
            if identity != plan.identity() {
                return Err(SearchDbError::PlanIdentityMismatch);
            }
            if completed != 0 {
                return Err(SearchDbError::InvalidGeneration(
                    "cannot append to a completed generation".to_string(),
                ));
            }
            insert_generation_items(&tx, generation, items, false)?;
            insert_generation_content(&tx, generation, items, plan)?;
            tx.commit()?;
            Ok(())
        })();
        detach_generation_content_db(&self.conn);
        result?;
        if items.len() >= CHECKPOINT_MUTATION_ITEM_THRESHOLD {
            checkpoint_wal_passively(&self.conn)?;
        }
        Ok(())
    }

    pub(crate) fn activate_staged_generation(
        &mut self,
        plan: &ScanPlan,
        generation: i64,
        expected_count: usize,
        fence: &ActivationFence,
        expected_epoch: u64,
    ) -> Result<usize, SearchDbError> {
        self.activate_staged_generation_then(
            plan,
            generation,
            expected_count,
            fence,
            expected_epoch,
            |_| {},
        )
    }

    pub(crate) fn activate_staged_generation_then(
        &mut self,
        plan: &ScanPlan,
        generation: i64,
        expected_count: usize,
        fence: &ActivationFence,
        expected_epoch: u64,
        after_commit: impl FnOnce(usize),
    ) -> Result<usize, SearchDbError> {
        validate_generation_content_db(&self.path, generation, expected_count)?;
        let activation_guard = fence.lock();
        if !activation_guard.matches(expected_epoch) {
            return Err(SearchDbError::ActivationCancelled);
        }
        let prior_generation = self
            .conn
            .query_row(
                "SELECT active_generation_id FROM search_store_state WHERE singleton = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let tx = self.conn.transaction()?;
        let (identity, completed): (String, i64) = tx
            .query_row(
                "SELECT plan_identity, completed FROM search_generations WHERE id = ?1",
                [generation],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| {
                SearchDbError::InvalidGeneration("staged generation disappeared".to_string())
            })?;
        if identity != plan.identity() {
            return Err(SearchDbError::PlanIdentityMismatch);
        }
        if completed != 0 {
            return Err(SearchDbError::InvalidGeneration(
                "staged generation was already completed".to_string(),
            ));
        }
        let actual_count: usize = tx.query_row(
            "SELECT COUNT(*) FROM search_generation_items WHERE generation_id = ?1",
            [generation],
            |row| row.get(0),
        )?;
        if actual_count != expected_count {
            return Err(SearchDbError::InvalidGeneration(format!(
                "staged generation expected {expected_count} items but contains {actual_count}"
            )));
        }
        tx.execute(
            "UPDATE search_generations SET completed = 1, item_count = ?1 WHERE id = ?2",
            params![actual_count as i64, generation],
        )?;

        tx.execute(
            "INSERT INTO search_store_state (singleton, active_generation_id) VALUES (1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET active_generation_id = excluded.active_generation_id",
            [generation],
        )?;
        tx.execute(
            "DELETE FROM search_generations WHERE id <> ?1",
            [generation],
        )?;
        tx.commit()?;
        after_commit(actual_count);
        drop(activation_guard);
        if let Some(prior) = prior_generation.filter(|prior| *prior != generation) {
            let _ = remove_sqlite_database(&generation_content_db_path(&self.path, prior));
        }
        checkpoint_wal_passively(&self.conn)?;
        reclaim_freelist_incrementally(&self.conn)?;
        checkpoint_wal_truncate(&self.conn)?;
        release_sqlite_memory(&self.conn)?;
        Ok(actual_count)
    }

    pub(crate) fn discard_staged_generation(
        &mut self,
        generation: i64,
    ) -> Result<(), SearchDbError> {
        let tx = self.conn.transaction()?;
        let discardable = tx.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM search_generations g
                WHERE g.id = ?1
                  AND g.completed = 0
                  AND g.id NOT IN (SELECT active_generation_id FROM search_store_state)
            )",
            [generation],
            |row| row.get::<_, bool>(0),
        )?;
        if !discardable {
            tx.commit()?;
            return Ok(());
        }
        tx.execute("DELETE FROM search_generations WHERE id = ?1", [generation])?;
        tx.commit()?;
        remove_sqlite_database(&generation_content_db_path(&self.path, generation))?;
        checkpoint_wal_passively(&self.conn)?;
        Ok(())
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

    pub(crate) fn commit_delta_guarded_then(
        &mut self,
        plan: &ScanPlan,
        upserts: &[OwnedItem],
        remove_trees: &[PathBuf],
        fence: &ActivationFence,
        expected_epoch: u64,
        after_commit: impl FnOnce(usize),
    ) -> Result<(), SearchDbError> {
        let generation = self
            .conn
            .query_row(
                "SELECT active_generation_id FROM search_store_state WHERE singleton = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or_else(|| {
                SearchDbError::InvalidGeneration(
                    "incremental mutation requires an active generation".to_string(),
                )
            })?;
        attach_generation_content_db(
            &self.conn,
            &generation_content_db_path(&self.path, generation),
        )?;
        let result = (|| {
            let tx = self.conn.transaction()?;
            let (stored_generation, stored_identity, completed, stored_item_count) = tx
                .query_row(
                    "SELECT g.id, g.plan_identity, g.completed, g.item_count
                     FROM search_store_state s
                     JOIN search_generations g ON g.id = s.active_generation_id
                     WHERE s.singleton = 1",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    },
                )
                .optional()?
                .ok_or_else(|| {
                    SearchDbError::InvalidGeneration(
                        "incremental mutation requires an active generation".to_string(),
                    )
                })?;
            if stored_generation != generation {
                return Err(SearchDbError::InvalidGeneration(
                    "active generation changed before incremental mutation".to_string(),
                ));
            }
            if stored_identity != plan.identity() {
                return Err(SearchDbError::PlanIdentityMismatch);
            }
            if completed != 1 {
                return Err(SearchDbError::InvalidGeneration(
                    "incremental mutation requires a completed generation".to_string(),
                ));
            }

            let normalized_upserts = upserts
                .iter()
                .map(|item| (item.item().path.clone(), item.clone()))
                .collect::<BTreeMap<_, _>>()
                .into_values()
                .collect::<Vec<_>>();
            let deleted_count = delete_generation_subtrees(&tx, generation, remove_trees)?;
            let new_upserts = {
                let mut exists = tx.prepare_cached(
                    "SELECT EXISTS(
                        SELECT 1 FROM search_generation_items
                        WHERE generation_id = ?1 AND path = ?2
                     )",
                )?;
                let mut new_upserts = Vec::new();
                for item in &normalized_upserts {
                    let present = exists
                        .query_row(params![generation, encode_path(&item.item().path)], |row| {
                            row.get::<_, bool>(0)
                        })?;
                    if !present {
                        new_upserts.push(item.clone());
                    }
                }
                new_upserts
            };
            let inserted_count = new_upserts.len();
            upsert_generation_item_rows(&tx, generation, &normalized_upserts)?;
            insert_generation_path_rows(&tx, generation, &new_upserts, false)?;
            delete_generation_content(&tx, generation, &normalized_upserts)?;
            insert_generation_content(&tx, generation, &normalized_upserts, plan)?;
            let stored_item_count = usize::try_from(stored_item_count).map_err(|_| {
                SearchDbError::InvalidGeneration(
                    "active generation has a negative item count".to_string(),
                )
            })?;
            let item_count = stored_item_count
                .checked_sub(deleted_count)
                .and_then(|count| count.checked_add(inserted_count))
                .ok_or_else(|| {
                    SearchDbError::InvalidGeneration(
                        "incremental mutation produced an invalid item count".to_string(),
                    )
                })?;
            tx.execute(
                "UPDATE search_generations SET item_count = ?1 WHERE id = ?2",
                params![item_count as i64, generation],
            )?;

            let activation_guard = fence.lock();
            if !activation_guard.matches(expected_epoch) {
                return Err(SearchDbError::ActivationCancelled);
            }
            tx.commit()?;
            after_commit(item_count);
            drop(activation_guard);
            Ok(())
        })();
        detach_generation_content_db(&self.conn);
        result?;
        if upserts.len().saturating_add(remove_trees.len()) >= CHECKPOINT_MUTATION_ITEM_THRESHOLD {
            checkpoint_wal_passively(&self.conn)?;
        }
        Ok(())
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
        let is_full_replacement = matches!(&mutation, IndexMutation::ReplaceAll(_));
        let checkpoint_after_commit = requires_checkpoint_after_commit(&mutation);
        let prior_generation = self
            .conn
            .query_row(
                "SELECT active_generation_id FROM search_store_state WHERE singleton = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let content_generation = if is_full_replacement {
            self.conn.query_row(
                "SELECT COALESCE(MAX(id), 0) + 1 FROM search_generations",
                [],
                |row| row.get::<_, i64>(0),
            )?
        } else {
            prior_generation.ok_or_else(|| {
                SearchDbError::InvalidGeneration(
                    "incremental mutation requires an active matching generation".to_string(),
                )
            })?
        };
        let content_path = generation_content_db_path(&self.path, content_generation);
        if is_full_replacement {
            create_generation_content_db(&self.path, content_generation)?;
        }
        if let Err(error) = attach_generation_content_db(&self.conn, &content_path) {
            if is_full_replacement {
                let _ = remove_sqlite_database(&content_path);
            }
            return Err(error);
        }

        let result = (|| {
            let tx = self.conn.transaction()?;
            let current = if is_full_replacement {
                GenerationSnapshot::new(0, plan.identity().to_string(), Vec::new())
            } else {
                load_generation(&tx, plan)?.ok_or_else(|| {
                    SearchDbError::InvalidGeneration(
                        "incremental mutation requires an active matching generation".to_string(),
                    )
                })?
            };
            let authoritative_content_refresh = incremental_content_refresh_items(&mutation);
            let candidate = current.clone().apply(mutation);
            let generation = if is_full_replacement {
                tx.execute(
                    "INSERT INTO search_generations (id, plan_identity, completed, item_count) VALUES (?1, ?2, 0, ?3)",
                    params![content_generation, plan.identity(), candidate.items().len() as i64],
                )?;
                insert_generation_items(&tx, content_generation, candidate.items(), true)?;
                insert_generation_content(&tx, content_generation, candidate.items(), plan)?;
                tx.execute(
                    "UPDATE search_generations SET completed = 1 WHERE id = ?1",
                    [content_generation],
                )?;
                content_generation
            } else {
                apply_incremental_mutation(
                    &tx,
                    current.generation(),
                    current.items(),
                    candidate.items(),
                    &authoritative_content_refresh,
                    plan,
                )?;
                tx.execute(
                    "UPDATE search_generations SET item_count = ?1 WHERE id = ?2",
                    params![candidate.items().len() as i64, current.generation()],
                )?;
                current.generation()
            };
            before_activation();
            let activation_guard = activation.map(|(fence, expected_epoch)| {
                let guard = fence.lock();
                guard
                    .matches(expected_epoch)
                    .then_some(guard)
                    .ok_or(SearchDbError::ActivationCancelled)
            });
            let _activation_guard = activation_guard.transpose()?;
            if is_full_replacement {
                tx.execute(
                    "INSERT INTO search_store_state (singleton, active_generation_id) VALUES (1, ?1)
                     ON CONFLICT(singleton) DO UPDATE SET active_generation_id = excluded.active_generation_id",
                    [generation],
                )?;
                tx.execute(
                    "DELETE FROM search_generations WHERE id <> ?1",
                    [generation],
                )?;
            }

            let snapshot = load_generation(&tx, plan)?.ok_or_else(|| {
                SearchDbError::InvalidGeneration("active generation disappeared".to_string())
            })?;
            if fail_before_commit {
                return Err(SearchDbError::InjectedFailure);
            }
            tx.commit()?;
            Ok(snapshot)
        })();
        detach_generation_content_db(&self.conn);
        let snapshot = match result {
            Ok(snapshot) => snapshot,
            Err(error) => {
                if is_full_replacement {
                    let _ = remove_sqlite_database(&content_path);
                }
                return Err(error);
            }
        };
        if checkpoint_after_commit {
            checkpoint_wal_passively(&self.conn)?;
        }
        if is_full_replacement {
            reclaim_freelist_incrementally(&self.conn)?;
            checkpoint_wal_truncate(&self.conn)?;
            release_sqlite_memory(&self.conn)?;
            if let Some(prior) = prior_generation.filter(|prior| *prior != content_generation) {
                let _ = remove_sqlite_database(&generation_content_db_path(&self.path, prior));
            }
        }
        after_commit(&snapshot);
        Ok(snapshot)
    }
}

fn requires_checkpoint_after_commit(mutation: &IndexMutation) -> bool {
    match mutation {
        IndexMutation::ReplaceAll(_) => true,
        IndexMutation::ReplaceRoot { items, .. } => {
            items.len() >= CHECKPOINT_MUTATION_ITEM_THRESHOLD
        }
        IndexMutation::Delta {
            upserts,
            remove_trees,
        } => upserts.len().saturating_add(remove_trees.len()) >= CHECKPOINT_MUTATION_ITEM_THRESHOLD,
    }
}

fn checkpoint_wal_passively(conn: &Connection) -> Result<(), SearchDbError> {
    let _: (u64, u64, u64) = conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    Ok(())
}

fn checkpoint_wal_truncate(conn: &Connection) -> Result<(), SearchDbError> {
    let _: (u64, u64, u64) = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    Ok(())
}

fn reclaim_freelist_incrementally(conn: &Connection) -> Result<(), SearchDbError> {
    let auto_vacuum = conn.query_row("PRAGMA auto_vacuum", [], |row| row.get::<_, u64>(0))?;
    if auto_vacuum != 2 {
        return Ok(());
    }
    conn.execute_batch(&format!(
        "PRAGMA incremental_vacuum({INCREMENTAL_VACUUM_PAGE_BUDGET});"
    ))?;
    Ok(())
}

fn release_sqlite_memory(conn: &Connection) -> Result<(), SearchDbError> {
    conn.release_memory()?;
    Ok(())
}

fn configure_writer_connection(conn: &Connection) -> Result<(), SearchDbError> {
    // A negated query intentionally holds a stable read snapshot while it
    // streams. WAL lets the next generation commit alongside that reader;
    // the bounded timeout handles brief writer contention without masking a
    // persistent indexing failure through unbounded retries.
    //
    // `auto_vacuum` becomes durable here for empty/fresh databases. SQLite
    // cannot retrofit it onto an existing populated cache without a full
    // VACUUM, which is intentionally not hidden in daemon startup: that can
    // require database-sized temporary storage. Older caches safely reuse
    // freelist pages but need explicit free-space-checked maintenance before
    // incremental vacuum can return those pages to the filesystem.
    conn.execute_batch(
        "PRAGMA auto_vacuum = INCREMENTAL;
         PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA journal_size_limit = 268435456;
         PRAGMA busy_timeout = 250;",
    )?;
    Ok(())
}

const CONTENT_STATE_POLICY_DISABLED: i64 = 6;

fn delete_generation_subtrees(
    tx: &Transaction<'_>,
    generation: i64,
    remove_trees: &[PathBuf],
) -> Result<usize, SearchDbError> {
    const DESCENDANTS: &str = "WITH RECURSIVE descendants(path) AS (
             SELECT path FROM search_generation_items
              WHERE generation_id = ?1 AND (path = ?2 OR parent_path = ?2)
             UNION ALL
             SELECT child.path FROM search_generation_items child
             JOIN descendants parent ON child.parent_path = parent.path
              WHERE child.generation_id = ?1
         ) ";
    let delete_content = format!(
        "{DESCENDANTS}DELETE FROM content_store.search_generation_content
         WHERE generation_id = ?1 AND path IN (SELECT path FROM descendants)"
    );
    let delete_content_state = format!(
        "{DESCENDANTS}DELETE FROM content_store.search_generation_content_state
         WHERE generation_id = ?1 AND path IN (SELECT path FROM descendants)"
    );
    let delete_items = format!(
        "{DESCENDANTS}DELETE FROM search_generation_items
         WHERE generation_id = ?1 AND path IN (SELECT path FROM descendants)"
    );
    let mut deleted_count = 0_usize;
    for removed in remove_trees {
        let path = encode_path(removed);
        tx.execute(&delete_content, params![generation, &path])?;
        tx.execute(&delete_content_state, params![generation, &path])?;
        // The path FTS is a rebuildable accelerator. Deleting by generation/path
        // requires an O(index-size) virtual-table scan, so direct deltas retain
        // stale postings; every query joins authoritative item rows and filters
        // them immediately, while the next full staged generation reclaims them.
        deleted_count =
            deleted_count.saturating_add(tx.execute(&delete_items, params![generation, path])?);
    }
    Ok(deleted_count)
}

/// Filesystem watcher upserts are authoritative even when the scanner's
/// metadata resolution cannot distinguish their old and new contents. Content
/// state must therefore be refreshed from the mutation itself rather than the
/// metadata-level current/candidate diff.
fn incremental_content_refresh_items(mutation: &IndexMutation) -> Vec<OwnedItem> {
    let items = match mutation {
        IndexMutation::ReplaceAll(_) => return Vec::new(),
        IndexMutation::ReplaceRoot { items, .. } => items,
        IndexMutation::Delta { upserts, .. } => upserts,
    };
    items
        .iter()
        .map(|item| (item.item().path.clone(), item.clone()))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect()
}

fn apply_incremental_mutation(
    tx: &Transaction<'_>,
    generation: i64,
    current: &[OwnedItem],
    candidate: &[OwnedItem],
    authoritative_content_refresh: &[OwnedItem],
    plan: &ScanPlan,
) -> Result<(), SearchDbError> {
    let current_by_path = current
        .iter()
        .map(|item| (item.item().path.clone(), item))
        .collect::<BTreeMap<_, _>>();
    let candidate_by_path = candidate
        .iter()
        .map(|item| (item.item().path.clone(), item))
        .collect::<BTreeMap<_, _>>();
    let removed = current_by_path
        .iter()
        .filter_map(|(path, item)| {
            (candidate_by_path.get(path) != Some(item)).then_some((*item).clone())
        })
        .collect::<Vec<_>>();
    let changed = candidate_by_path
        .iter()
        .filter_map(|(path, item)| {
            (current_by_path.get(path) != Some(item)).then_some((*item).clone())
        })
        .collect::<Vec<_>>();
    delete_generation_items(tx, generation, &removed)?;
    insert_generation_items(tx, generation, &changed, true)?;
    delete_generation_content(tx, generation, authoritative_content_refresh)?;
    insert_generation_content(tx, generation, authoritative_content_refresh, plan)?;
    Ok(())
}

fn delete_generation_content(
    tx: &Transaction<'_>,
    generation: i64,
    items: &[OwnedItem],
) -> Result<(), SearchDbError> {
    let mut fts = tx.prepare_cached(
        "DELETE FROM content_store.search_generation_content WHERE generation_id = ?1 AND path = ?2",
    )?;
    let mut state = tx.prepare_cached(
        "DELETE FROM content_store.search_generation_content_state WHERE generation_id = ?1 AND path = ?2",
    )?;
    for item in items {
        let path = encode_path(&item.item().path);
        fts.execute(params![generation, &path])?;
        state.execute(params![generation, path])?;
    }
    Ok(())
}

fn delete_generation_items(
    tx: &Transaction<'_>,
    generation: i64,
    items: &[OwnedItem],
) -> Result<(), SearchDbError> {
    delete_generation_content(tx, generation, items)?;
    let mut paths = tx.prepare_cached(
        "DELETE FROM content_store.search_generation_paths WHERE generation_id = ?1 AND path = ?2",
    )?;
    let mut rows = tx.prepare_cached(
        "DELETE FROM search_generation_items WHERE generation_id = ?1 AND path = ?2",
    )?;
    for item in items {
        let path = encode_path(&item.item().path);
        paths.execute(params![generation, &path])?;
        rows.execute(params![generation, path])?;
    }
    Ok(())
}

fn insert_generation_content(
    tx: &Transaction<'_>,
    generation: i64,
    items: &[OwnedItem],
    plan: &ScanPlan,
) -> Result<(), SearchDbError> {
    let mut content_stmt = tx.prepare_cached(
        "INSERT INTO content_store.search_generation_content (generation_id, path, content) VALUES (?1, ?2, ?3)",
    )?;
    let mut state_stmt = tx.prepare_cached(
        "INSERT INTO content_store.search_generation_content_state (generation_id, path, state, content) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for owned in items {
        let item = owned.item();
        let path = encode_path(&item.path);
        let (state, content) = if !plan.indexes_content_for(&item.path) {
            (CONTENT_STATE_POLICY_DISABLED, None)
        } else if !matches!(item.kind, EntryKind::File) {
            (2_i64, None)
        } else if item
            .size_bytes
            .is_none_or(|size| size > plan.content_max_file_size_bytes())
        {
            (3, None)
        } else {
            let mut bytes = Vec::new();
            let read = std::fs::File::open(&item.path).and_then(|file| {
                file.take(plan.content_max_file_size_bytes().saturating_add(1))
                    .read_to_end(&mut bytes)
            });
            match read {
                Err(_) => (4, None),
                Ok(_) if bytes.len() as u64 > plan.content_max_file_size_bytes() => (3, None),
                Ok(_) => match String::from_utf8(bytes) {
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

fn upsert_generation_item_rows(
    tx: &Transaction<'_>,
    generation: i64,
    items: &[OwnedItem],
) -> Result<(), SearchDbError> {
    let mut stmt = tx.prepare_cached(
        "INSERT INTO search_generation_items
         (generation_id, path, parent_path, root_path, name, kind, size_bytes,
          modified_unix_secs, is_executable, symlink_target_is_dir)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(generation_id, path) DO UPDATE SET
            parent_path = excluded.parent_path,
            root_path = excluded.root_path,
            name = excluded.name,
            kind = excluded.kind,
            size_bytes = excluded.size_bytes,
            modified_unix_secs = excluded.modified_unix_secs,
            is_executable = excluded.is_executable,
            symlink_target_is_dir = excluded.symlink_target_is_dir",
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

fn insert_generation_path_rows(
    tx: &Transaction<'_>,
    generation: i64,
    items: &[OwnedItem],
    replace_existing_paths: bool,
) -> Result<(), SearchDbError> {
    let mut path_delete = tx.prepare_cached(
        "DELETE FROM content_store.search_generation_paths WHERE generation_id = ?1 AND path = ?2",
    )?;
    let mut path_insert = tx.prepare_cached(
        "INSERT INTO content_store.search_generation_paths (generation_id, path, path_text) VALUES (?1, ?2, ?3)",
    )?;
    for owned in items {
        let item = owned.item();
        let encoded_path = encode_path(&item.path);
        if replace_existing_paths {
            #[cfg(test)]
            PATH_FTS_DELETE_COUNT.with(|count| count.set(count.get() + 1));
            path_delete.execute(params![generation, &encoded_path])?;
        }
        path_insert.execute(params![
            generation,
            encoded_path,
            item.path.to_string_lossy()
        ])?;
    }
    Ok(())
}

fn insert_generation_items(
    tx: &Transaction<'_>,
    generation: i64,
    items: &[OwnedItem],
    replace_existing_paths: bool,
) -> Result<(), SearchDbError> {
    upsert_generation_item_rows(tx, generation, items)?;
    insert_generation_path_rows(tx, generation, items, replace_existing_paths)
}

fn cleanup_incomplete_generations(
    conn: &mut Connection,
    db_path: &Path,
) -> Result<bool, SearchDbError> {
    let tx = conn.transaction()?;
    let removed_generations = tx.execute(
        "DELETE FROM search_generations
         WHERE completed = 0
           AND id NOT IN (SELECT active_generation_id FROM search_store_state)",
        [],
    )?;
    let retained_content_paths = {
        let mut stmt = tx.prepare("SELECT id FROM search_generations")?;
        stmt.query_map([], |row| row.get::<_, i64>(0))?
            .map(|generation| generation.map(|id| generation_content_db_path(db_path, id)))
            .collect::<Result<Vec<_>, _>>()?
    };
    tx.commit()?;
    let mut removed_content = false;
    for content_path in generation_content_db_paths(db_path) {
        if !retained_content_paths.contains(&content_path) {
            remove_sqlite_database(&content_path)?;
            removed_content = true;
        }
    }
    Ok(removed_generations != 0 || removed_content)
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
    query_indexed_content_page_then(db_path, plan, fts_query, limit, offset, || {})
}

fn query_indexed_content_page_then(
    db_path: &Path,
    plan: &ScanPlan,
    fts_query: &str,
    limit: usize,
    offset: usize,
    after_content_attach: impl FnOnce(),
) -> Result<Vec<IndexedContentHit>, SearchDbError> {
    if fts_query.trim().is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let mut conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(250))?;
    let tx = conn.transaction()?;
    let Some(generation) = tx
        .query_row(
            "SELECT active_generation_id FROM search_store_state WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
    else {
        return Ok(Vec::new());
    };
    attach_generation_content_db(&tx, &generation_content_db_path(db_path, generation))?;
    after_content_attach();
    let mut stmt = tx.prepare(
        "SELECT i.path, i.name, i.kind, i.size_bytes, i.modified_unix_secs,
                i.is_executable, i.symlink_target_is_dir,
                snippet(search_generation_content, 2, '', '', '…', 20),
                f.content, rank
         FROM content_store.search_generation_content f
         JOIN search_store_state s ON s.singleton = 1 AND s.active_generation_id = f.generation_id
         JOIN search_generations g ON g.id = f.generation_id
         JOIN search_generation_items i
           ON i.generation_id = f.generation_id AND i.path = f.path
         WHERE g.completed = 1 AND g.plan_identity = ?1
           AND search_generation_content MATCH ?2
         ORDER BY rank, f.rowid
         LIMIT ?3 OFFSET ?4",
    )?;
    let rows = stmt.query_map(
        params![plan.identity(), fts_query, limit as i64, offset as i64],
        |row| {
            let raw_snippet: String = row.get(7)?;
            Ok(IndexedContentHit {
                item: SearchResultItem {
                    path: decode_path(row.get(0)?)?,
                    parent_path: PathBuf::new(),
                    name: row.get(1)?,
                    kind: entry_kind_from_i64(row.get(2)?),
                    size_bytes: row.get(3)?,
                    modified_unix_secs: row.get(4)?,
                    is_executable: row.get::<_, i64>(5)? != 0,
                    symlink_target_is_dir: row.get::<_, Option<i64>>(6)?.map(|value| value != 0),
                    content_snippet: None,
                    name_lower: String::new(),
                    path_str: String::new(),
                    path_lower: String::new(),
                },
                snippet: content_snippet_line(&raw_snippet, &fts_query_terms(fts_query)),
                content: row.get(8)?,
                relevance: fts_rank_relevance(row.get(9)?),
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Execute the overwhelmingly common single-literal query inside SQLite. This
/// keeps both decoded rows and Rust heap usage bounded to the requested page.
pub(crate) struct SimpleTermPage<'a> {
    pub(crate) term: &'a str,
    pub(crate) include_hidden: bool,
    pub(crate) sort: SearchSort,
    pub(crate) limit: usize,
    pub(crate) offset: usize,
}

pub(crate) fn query_simple_term_page(
    path: &Path,
    plan: &ScanPlan,
    request: SimpleTermPage<'_>,
) -> Result<(Vec<SearchResultItem>, usize), SearchDbError> {
    query_simple_term_page_then(path, plan, request, || {})
}

/// Check exact canonical paths against the completed active generation in one
/// read snapshot. This lets bounded result pages deduplicate peer search sources
/// without loading or scanning the complete native generation.
pub(crate) fn active_generation_contains_paths(
    path: &Path,
    plan: &ScanPlan,
    candidates: &[PathBuf],
) -> Result<Vec<bool>, SearchDbError> {
    if candidates.is_empty() || !path.is_file() {
        return Ok(vec![false; candidates.len()]);
    }
    let mut conn = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(250))?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)?;
    let generation = tx
        .query_row(
            "SELECT g.id
             FROM search_store_state s
             JOIN search_generations g ON g.id = s.active_generation_id
             WHERE s.singleton = 1 AND g.completed = 1 AND g.plan_identity = ?1",
            [plan.identity()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(generation) = generation else {
        return Ok(vec![false; candidates.len()]);
    };
    let mut statement = tx.prepare(
        "SELECT EXISTS(
             SELECT 1 FROM search_generation_items
             WHERE generation_id = ?1 AND path = ?2
         )",
    )?;
    let mut present = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        present.push(
            statement.query_row(params![generation, encode_path(candidate)], |row| {
                row.get::<_, bool>(0)
            })?,
        );
    }
    drop(statement);
    tx.commit()?;
    Ok(present)
}

fn query_simple_term_page_then(
    path: &Path,
    plan: &ScanPlan,
    request: SimpleTermPage<'_>,
    after_path_attach: impl FnOnce(),
) -> Result<(Vec<SearchResultItem>, usize), SearchDbError> {
    let SimpleTermPage {
        term,
        include_hidden,
        sort,
        limit,
        offset,
    } = request;
    if !path.is_file() {
        return Ok((Vec::new(), 0));
    }
    let mut conn = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(250))?;
    let tx = conn.transaction()?;
    let Some(generation) = tx
        .query_row(
            "SELECT active_generation_id FROM search_store_state WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
    else {
        return Ok((Vec::new(), 0));
    };
    attach_generation_content_db(&tx, &generation_content_db_path(path, generation))?;
    after_path_attach();
    let term = term.to_ascii_lowercase();
    let match_query = format!("\"{}\"", term.replace('"', "\"\""));
    let hidden = i64::from(include_hidden);
    let total = tx.query_row(
        "SELECT COUNT(*)
         FROM content_store.search_generation_paths f
         JOIN search_store_state s ON s.singleton = 1 AND s.active_generation_id = f.generation_id
         JOIN search_generations g ON g.id = f.generation_id
         JOIN search_generation_items i
           ON i.generation_id = f.generation_id AND i.path = f.path
         WHERE g.completed = 1 AND g.plan_identity = ?1
           AND search_generation_paths MATCH ?2
           AND (?3 = 1 OR substr(i.name, 1, 1) <> '.')",
        params![plan.identity(), match_query, hidden],
        |row| row.get::<_, usize>(0),
    )?;
    if total == 0 || limit == 0 {
        return Ok((Vec::new(), total));
    }
    let order = match sort {
        SearchSort::Relevance => "score DESC, i.name ASC",
        SearchSort::Name => "lower(i.name) ASC, i.name ASC",
        SearchSort::Path => "CAST(i.path AS TEXT) ASC",
        SearchSort::Modified => "i.modified_unix_secs DESC, i.name ASC",
        SearchSort::Size => "i.size_bytes DESC, i.name ASC",
    };
    let sql = format!(
        "SELECT i.path, i.name, i.kind, i.size_bytes, i.modified_unix_secs,
                i.is_executable, i.symlink_target_is_dir,
                CASE
                  WHEN lower(i.name) = ?6 THEN 10000
                  WHEN substr(lower(i.name), 1, length(?6)) = ?6 THEN 7500
                  WHEN instr(lower(i.name), ?6) > 0 THEN 5000
                  ELSE 2500
                END AS score
         FROM content_store.search_generation_paths f
         JOIN search_store_state s ON s.singleton = 1 AND s.active_generation_id = f.generation_id
         JOIN search_generations g ON g.id = f.generation_id
         JOIN search_generation_items i
           ON i.generation_id = f.generation_id AND i.path = f.path
         WHERE g.completed = 1 AND g.plan_identity = ?1
           AND search_generation_paths MATCH ?2
           AND (?3 = 1 OR substr(i.name, 1, 1) <> '.')
         ORDER BY {order}
         LIMIT ?4 OFFSET ?5"
    );
    let mut stmt = tx.prepare(&sql)?;
    let rows = stmt.query_map(
        params![
            plan.identity(),
            match_query,
            hidden,
            limit as i64,
            offset as i64,
            term
        ],
        |row| {
            Ok(SearchResultItem {
                path: decode_path(row.get(0)?)?,
                parent_path: PathBuf::new(),
                name: row.get(1)?,
                kind: entry_kind_from_i64(row.get(2)?),
                size_bytes: row.get(3)?,
                modified_unix_secs: row.get(4)?,
                is_executable: row.get::<_, i64>(5)? != 0,
                symlink_target_is_dir: row.get::<_, Option<i64>>(6)?.map(|value| value != 0),
                content_snippet: None,
                name_lower: String::new(),
                path_str: String::new(),
                path_lower: String::new(),
            })
        },
    )?;
    let items = rows.collect::<Result<Vec<_>, _>>()?;
    Ok((items, total))
}

/// Stream only item metadata from the completed active generation. The caller
/// owns at most one decoded item at a time, so filename/path queries do not
/// require a resident mirror of the SQLite generation.
pub(crate) fn stream_generation_items(
    db_path: &Path,
    plan: &ScanPlan,
    cancelled: Option<&AtomicBool>,
    mut visit: impl FnMut(SearchResultItem),
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
        "SELECT i.path, i.name, i.kind, i.size_bytes, i.modified_unix_secs,
                i.is_executable, i.symlink_target_is_dir
         FROM search_store_state s
         JOIN search_generations g ON g.id = s.active_generation_id
         JOIN search_generation_items i ON i.generation_id = g.id
         WHERE s.singleton = 1 AND g.completed = 1 AND g.plan_identity = ?1
         ORDER BY i.path",
    )?;
    let mut rows = stmt.query([plan.identity()])?;
    while let Some(row) = rows.next()? {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(SearchDbError::ContentQueryCancelled);
        }
        visit(SearchResultItem {
            path: decode_path(row.get(0)?)?,
            parent_path: PathBuf::new(),
            name: row.get(1)?,
            kind: entry_kind_from_i64(row.get(2)?),
            size_bytes: row.get(3)?,
            modified_unix_secs: row.get(4)?,
            is_executable: row.get::<_, i64>(5)? != 0,
            symlink_target_is_dir: row.get::<_, Option<i64>>(6)?.map(|value| value != 0),
            content_snippet: None,
            name_lower: String::new(),
            path_str: String::new(),
            path_lower: String::new(),
        });
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(SearchDbError::ContentQueryCancelled);
        }
    }
    drop(rows);
    drop(stmt);
    tx.commit()?;
    Ok(())
}

/// Stream the completed active generation from a single read transaction. This
/// is intentionally separate from the worker connection: queries cannot retain
/// the writer and each callback owns at most one item's indexed text.
pub(crate) fn stream_generation_content_items(
    db_path: &Path,
    plan: &ScanPlan,
    cancelled: Option<&AtomicBool>,
    visit: impl FnMut(GenerationContentItem),
) -> Result<(), SearchDbError> {
    stream_generation_content_items_then(db_path, plan, cancelled, visit, || {})
}

fn stream_generation_content_items_then(
    db_path: &Path,
    plan: &ScanPlan,
    cancelled: Option<&AtomicBool>,
    mut visit: impl FnMut(GenerationContentItem),
    after_content_attach: impl FnOnce(),
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
    let Some(generation) = tx
        .query_row(
            "SELECT active_generation_id FROM search_store_state WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
    else {
        return Ok(());
    };
    attach_generation_content_db(&tx, &generation_content_db_path(db_path, generation))?;
    after_content_attach();
    let mut stmt = tx.prepare(
        "SELECT i.path, i.parent_path, i.root_path, i.name, i.kind, i.size_bytes,
                i.modified_unix_secs, i.is_executable, i.symlink_target_is_dir,
                c.state, c.content
         FROM search_store_state s
         JOIN search_generations g ON g.id = s.active_generation_id
         JOIN search_generation_items i ON i.generation_id = g.id
         LEFT JOIN content_store.search_generation_content_state c
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

pub fn search_db_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    let cache_root = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            ottrin_core::default_home_dir()
                .join("AppData")
                .join("Local")
        });
    #[cfg(not(target_os = "windows"))]
    let path = search_db_path_for_unix_user(
        // SAFETY: geteuid has no preconditions and no side effects.
        unsafe { libc::geteuid() },
        std::env::var_os("XDG_CACHE_HOME").as_deref().map(Path::new),
        &ottrin_core::default_home_dir(),
    );
    #[cfg(target_os = "windows")]
    let path = cache_root.join("ottrin").join("search.db");
    let dir = path.parent().expect("search database has a parent");
    let _ = std::fs::create_dir_all(dir);
    path
}

#[cfg(not(target_os = "windows"))]
fn search_db_path_for_unix_user(
    effective_uid: u32,
    xdg_cache_home: Option<&Path>,
    home: &Path,
) -> PathBuf {
    if effective_uid == 0 {
        return PathBuf::from("/root/.cache/ottrin-administrator/search.db");
    }
    xdg_cache_home
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".cache"))
        .join("ottrin/search.db")
}

fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(suffix);
    PathBuf::from(sidecar)
}

pub(crate) fn generation_content_db_path(path: &Path, generation: i64) -> PathBuf {
    let mut content_path = path.as_os_str().to_os_string();
    content_path.push(format!(".content-{generation}.db"));
    PathBuf::from(content_path)
}

pub(crate) fn generation_content_db_paths(path: &Path) -> Vec<PathBuf> {
    let Some(parent) = path.parent() else {
        return Vec::new();
    };
    let Some(file_name) = path.file_name().map(|name| name.to_string_lossy()) else {
        return Vec::new();
    };
    let prefix = format!("{file_name}.content-");
    std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|candidate| {
            candidate
                .file_name()
                .map(|name| name.to_string_lossy())
                .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".db"))
        })
        .collect()
}

fn create_generation_content_db(path: &Path, generation: i64) -> Result<(), SearchDbError> {
    let content_path = generation_content_db_path(path, generation);
    let conn = Connection::open(&content_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = DELETE;
         PRAGMA synchronous = FULL;
         PRAGMA busy_timeout = 250;",
    )?;
    conn.execute_batch(CONTENT_FTS_SCHEMA)?;
    Ok(())
}

fn validate_generation_content_db(
    path: &Path,
    generation: i64,
    expected_count: usize,
) -> Result<(), SearchDbError> {
    let content_path = generation_content_db_path(path, generation);
    if !content_path.is_file() {
        return Err(SearchDbError::InvalidGeneration(format!(
            "content index is unavailable: {}",
            content_path.display()
        )));
    }
    let conn = Connection::open_with_flags(
        &content_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let (content_state_count, path_count) = conn
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM search_generation_content_state WHERE generation_id = ?1),
                (SELECT COUNT(*) FROM search_generation_paths WHERE generation_id = ?1)",
            [generation],
            |row| Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?)),
        )
        .map_err(|error| {
            SearchDbError::InvalidGeneration(format!(
                "derived index is invalid for generation {generation}: {error}"
            ))
        })?;
    if content_state_count != expected_count || path_count != expected_count {
        return Err(SearchDbError::InvalidGeneration(format!(
            "staged generation expected {expected_count} content states and paths but contains {content_state_count} content states and {path_count} paths"
        )));
    }
    Ok(())
}

fn attach_generation_content_db(
    conn: &Connection,
    content_path: &Path,
) -> Result<(), SearchDbError> {
    if !content_path.is_file() {
        return Err(SearchDbError::InvalidGeneration(format!(
            "content index is unavailable: {}",
            content_path.display()
        )));
    }
    conn.execute(
        "ATTACH DATABASE ?1 AS content_store",
        [content_path.to_string_lossy().as_ref()],
    )?;
    Ok(())
}

fn detach_generation_content_db(conn: &Connection) {
    let _ = conn.execute_batch("DETACH DATABASE content_store");
}

fn remove_sqlite_database(path: &Path) -> Result<(), SearchDbError> {
    for candidate in [
        path.to_path_buf(),
        sqlite_sidecar_path(path, "-wal"),
        sqlite_sidecar_path(path, "-shm"),
        sqlite_sidecar_path(path, "-journal"),
    ] {
        match std::fs::remove_file(candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn prepare_v7_content_database(
    conn: &Connection,
    db_path: &Path,
) -> Result<Option<i64>, SearchDbError> {
    let Some((generation, completed, expected_count)) = conn
        .query_row(
            "SELECT g.id, g.completed, g.item_count
             FROM search_store_state s
             JOIN search_generations g ON g.id = s.active_generation_id
             WHERE s.singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let actual_count = conn.query_row(
        "SELECT COUNT(*) FROM search_generation_items WHERE generation_id = ?1",
        [generation],
        |row| row.get::<_, i64>(0),
    )?;
    if completed != 1 || expected_count < 0 || actual_count != expected_count {
        return Ok(None);
    }

    let content_path = generation_content_db_path(db_path, generation);
    remove_sqlite_database(&content_path)?;
    create_generation_content_db(db_path, generation)?;
    let mut content = Connection::open(&content_path)?;
    content.execute_batch("PRAGMA synchronous = FULL; PRAGMA busy_timeout = 250;")?;
    content.execute(
        "ATTACH DATABASE ?1 AS legacy_store",
        [db_path.to_string_lossy().as_ref()],
    )?;
    let copy_result = (|| {
        let tx = content.transaction()?;
        tx.execute(
            "INSERT INTO search_generation_paths (generation_id, path, path_text)
             SELECT generation_id, path, path_text
             FROM legacy_store.search_generation_paths
             WHERE generation_id = ?1",
            [generation],
        )?;
        tx.execute(
            "INSERT INTO search_generation_content_state (generation_id, path, state, content)
             SELECT generation_id, path, state, content
             FROM legacy_store.search_generation_content_state
             WHERE generation_id = ?1",
            [generation],
        )?;
        tx.execute(
            "INSERT INTO search_generation_content (generation_id, path, content)
             SELECT generation_id, path, content
             FROM legacy_store.search_generation_content
             WHERE generation_id = ?1",
            [generation],
        )?;
        tx.commit()?;
        Ok::<(), SearchDbError>(())
    })();
    let _ = content.execute_batch("DETACH DATABASE legacy_store");
    if let Err(error) = copy_result {
        drop(content);
        let _ = remove_sqlite_database(&content_path);
        return Err(error);
    }
    drop(content);
    validate_generation_content_db(db_path, generation, expected_count as usize)?;
    Ok(Some(generation))
}

fn prepare_v8_path_database(
    conn: &Connection,
    db_path: &Path,
) -> Result<Option<i64>, SearchDbError> {
    let Some((generation, completed, expected_count)) = conn
        .query_row(
            "SELECT g.id, g.completed, g.item_count
             FROM search_store_state s
             JOIN search_generations g ON g.id = s.active_generation_id
             WHERE s.singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let actual_count = conn.query_row(
        "SELECT COUNT(*) FROM search_generation_items WHERE generation_id = ?1",
        [generation],
        |row| row.get::<_, i64>(0),
    )?;
    if completed != 1 || expected_count < 0 || actual_count != expected_count {
        return Ok(None);
    }

    let content_path = generation_content_db_path(db_path, generation);
    if !content_path.is_file() {
        return Ok(None);
    }
    let mut content = Connection::open(&content_path)?;
    content.execute_batch("PRAGMA synchronous = FULL; PRAGMA busy_timeout = 250;")?;
    // Rebuilding this derived table is restart-safe while user_version remains
    // v8: old readers continue using the untouched canonical path FTS.
    content.execute_batch("DROP TABLE IF EXISTS search_generation_paths;")?;
    content.execute_batch(CONTENT_FTS_SCHEMA)?;
    content.execute(
        "ATTACH DATABASE ?1 AS legacy_store",
        [db_path.to_string_lossy().as_ref()],
    )?;
    let copy_result = (|| {
        let tx = content.transaction()?;
        {
            let mut canonical_paths = tx.prepare(
                "SELECT path
                 FROM legacy_store.search_generation_items
                 WHERE generation_id = ?1",
            )?;
            let mut paths = canonical_paths.query([generation])?;
            let mut insert_path = tx.prepare(
                "INSERT INTO search_generation_paths (generation_id, path, path_text)
                 VALUES (?1, ?2, ?3)",
            )?;
            while let Some(row) = paths.next()? {
                let encoded_path = row.get::<_, Vec<u8>>(0)?;
                let path_text = decode_path(encoded_path.clone())?
                    .to_string_lossy()
                    .into_owned();
                insert_path.execute(params![generation, encoded_path, path_text])?;
            }
        }
        tx.commit()?;
        Ok::<(), SearchDbError>(())
    })();
    let _ = content.execute_batch("DETACH DATABASE legacy_store");
    copy_result?;
    drop(content);
    validate_generation_content_db(db_path, generation, expected_count as usize)?;
    Ok(Some(generation))
}

fn migrate(conn: &mut Connection, db_path: &Path) -> Result<bool, SearchDbError> {
    let version: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == SEARCH_DB_VERSION {
        conn.execute_batch(GENERATION_SCHEMA)?;
        return Ok(false);
    }

    // A v7 active generation remains canonical and searchable until its derived
    // content database is complete and durable. Publishing v8 then changes only
    // the content-storage location; metadata, paths, and active authority remain.
    let prepared_v7_generation = if version == 7 {
        prepare_v7_content_database(conn, db_path)?
    } else {
        None
    };
    let prepared_v8_generation = if version == 8 {
        prepare_v8_path_database(conn, db_path)?
    } else {
        None
    };
    let tx = conn.transaction()?;
    match version {
        0..=4 => {
            tx.execute_batch(GENERATION_SCHEMA)?;
            tx.execute_batch(
                "DROP TABLE IF EXISTS scan_state;
                 DROP TABLE IF EXISTS roots;
                 DROP TABLE IF EXISTS files;
                 DROP TABLE IF EXISTS meta;",
            )?;
        }
        5..=6 => {
            // Old generations either lack completeness records or own their FTS
            // inside search.db. Force the normal rebuild path rather than mix
            // incompatible storage authorities.
            tx.execute_batch(
                "DROP TABLE IF EXISTS search_generation_content;
                 DROP TABLE IF EXISTS search_generation_paths;
                 DROP TABLE IF EXISTS search_generation_content_state;
                 DROP TABLE IF EXISTS search_generation_items;
                 DROP TABLE IF EXISTS search_store_state;
                 DROP TABLE IF EXISTS search_generations;",
            )?;
            tx.execute_batch(GENERATION_SCHEMA)?;
        }
        7 if prepared_v7_generation.is_some() => {
            tx.execute_batch(
                "DROP TABLE IF EXISTS search_generation_content;
                 DROP TABLE IF EXISTS search_generation_content_state;
                 DROP TABLE IF EXISTS search_generation_paths;",
            )?;
            tx.execute_batch(GENERATION_SCHEMA)?;
        }
        7 => {
            tx.execute_batch(
                "DROP TABLE IF EXISTS search_generation_content;
                 DROP TABLE IF EXISTS search_generation_paths;
                 DROP TABLE IF EXISTS search_generation_content_state;
                 DROP TABLE IF EXISTS search_generation_items;
                 DROP TABLE IF EXISTS search_store_state;
                 DROP TABLE IF EXISTS search_generations;",
            )?;
            tx.execute_batch(GENERATION_SCHEMA)?;
        }
        8 if prepared_v8_generation.is_some() => {
            tx.execute_batch("DROP TABLE IF EXISTS search_generation_paths;")?;
            tx.execute_batch(GENERATION_SCHEMA)?;
        }
        8 => {
            tx.execute_batch(
                "DROP TABLE IF EXISTS search_generation_paths;
                 DROP TABLE IF EXISTS search_generation_items;
                 DROP TABLE IF EXISTS search_store_state;
                 DROP TABLE IF EXISTS search_generations;",
            )?;
            tx.execute_batch(GENERATION_SCHEMA)?;
        }
        other => return Err(SearchDbError::UnsupportedVersion(other)),
    }
    tx.execute("PRAGMA user_version = 9", [])?;
    tx.commit()?;
    Ok(true)
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

    #[cfg(unix)]
    #[test]
    fn administrator_database_path_ignores_preserved_user_environment() {
        assert_eq!(
            search_db_path_for_unix_user(
                0,
                Some(Path::new("/home/alice/.cache")),
                Path::new("/home/alice"),
            ),
            PathBuf::from("/root/.cache/ottrin-administrator/search.db")
        );
    }

    fn temp_db_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("ottrin-{name}-{nonce}.db"))
    }

    #[test]
    fn populated_legacy_cache_does_not_claim_incremental_vacuum_without_vacuum() {
        let db_path = temp_db_path("legacy-auto-vacuum-policy");
        let conn = Connection::open(&db_path).expect("open database");
        conn.execute("CREATE TABLE legacy_row (id INTEGER PRIMARY KEY)", [])
            .expect("create populated schema");
        conn.execute("INSERT INTO legacy_row VALUES (1)", [])
            .expect("insert row");
        configure_writer_connection(&conn).expect("configure writer");
        assert_eq!(
            conn.query_row("PRAGMA auto_vacuum", [], |row| row.get::<_, u32>(0))
                .expect("auto vacuum mode"),
            0,
            "SQLite requires an explicit full VACUUM to retrofit incremental mode"
        );
        let _ = std::fs::remove_file(db_path);
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
    fn path_and_content_fts_are_stored_in_a_disposable_generation_database() {
        let db_path = temp_db_path("separate-fts-schema");
        let root = db_path.with_extension("root");
        std::fs::create_dir_all(&root).expect("create content root");
        let plan = ScanPlan::new(&ottrin_core::SearchConfig {
            include_roots: vec![root.clone()],
            include_hidden_system: true,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..ottrin_core::SearchConfig::default()
        })
        .expect("valid plan");
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        let generation = store
            .begin_staged_generation(&plan)
            .expect("begin staged generation");
        let conn = Connection::open(&db_path).expect("open database");
        let canonical_fts_count: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE name IN ('search_generation_paths', 'search_generation_content')",
                [],
                |row| row.get(0),
            )
            .expect("inspect canonical schema");
        assert_eq!(canonical_fts_count, 0);

        let content_path = generation_content_db_path(&db_path, generation);
        let content = Connection::open(&content_path).expect("open generation content database");
        let sql: String = content
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'search_generation_content'",
                [],
                |row| row.get(0),
            )
            .expect("generation FTS content table");
        assert!(sql.contains("fts5"));
        let path_sql: String = content
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'search_generation_paths'",
                [],
                |row| row.get(0),
            )
            .expect("generation FTS path table");
        assert!(path_sql.contains("fts5"));

        drop(content);
        drop(conn);
        drop(store);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(content_path);
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
        assert_eq!(
            state_table_count, 0,
            "completeness state is derived content and must not remain in canonical metadata"
        );
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .expect("schema version"),
            SEARCH_DB_VERSION
        );
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn v6_canonical_content_layout_is_reset_once_for_separate_fts_rebuild() {
        let db_path = temp_db_path("v6-separate-fts-reset");
        let conn = Connection::open(&db_path).expect("open old database");
        conn.execute_batch(GENERATION_SCHEMA)
            .expect("seed generation schema");
        conn.execute_batch(
            "INSERT INTO search_generations VALUES (1, 'plan', 1, 1);
             INSERT INTO search_generation_items
               (generation_id, path, parent_path, root_path, name, kind,
                size_bytes, modified_unix_secs, is_executable, symlink_target_is_dir)
             VALUES (1, X'2F726F6F742F6E6565646C652E747874', X'2F726F6F74', X'2F726F6F74',
                     'needle.txt', 0, 1, 1, 0, NULL);
             INSERT INTO search_store_state VALUES (1, 1);
             PRAGMA user_version = 6;",
        )
        .expect("seed v6 database");
        drop(conn);

        drop(SearchStore::open(db_path.clone()).expect("migrate v6 database"));
        drop(SearchStore::open(db_path.clone()).expect("reopen v7 database"));
        let conn = Connection::open(&db_path).expect("inspect migrated database");
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .expect("schema version"),
            SEARCH_DB_VERSION
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'search_generation_paths'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("canonical path table count"),
            0,
            "derived path FTS must not be recreated in canonical metadata"
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM search_store_state", [], |row| {
                row.get::<_, usize>(0)
            })
            .expect("active generation count"),
            0
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
    use ottrin_core::{ContentFileGroup, SearchConfig};
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
            content_indexing_enabled: true,
            content_include_roots: vec![root.to_path_buf()],
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

    fn sqlite_cache_bytes(conn: &Connection) -> usize {
        let mut current = 0;
        let mut highwater = 0;
        // SAFETY: the handle is borrowed only for this synchronous status call;
        // SQLite does not retain it or mutate the connection through this API.
        let result = unsafe {
            rusqlite::ffi::sqlite3_db_status(
                conn.handle(),
                rusqlite::ffi::SQLITE_DBSTATUS_CACHE_USED,
                &mut current,
                &mut highwater,
                0,
            )
        };
        assert_eq!(result, rusqlite::ffi::SQLITE_OK, "read SQLite cache use");
        usize::try_from(current).expect("non-negative SQLite cache use")
    }

    #[test]
    fn diagnostics_include_generation_content_database_storage() {
        let db_path = temp_db_path("generation-storage-diagnostics");
        let root = db_path.with_extension("root");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(root.join("indexed.txt"), "indexed content").expect("write content");
        let scan_plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "indexed.txt", 15)]),
            )
            .expect("commit generation");
        let content_paths = generation_content_db_paths(&db_path);
        assert_eq!(content_paths.len(), 1);
        let expected_bytes = std::fs::metadata(&db_path)
            .expect("canonical metadata")
            .len()
            + std::fs::metadata(&content_paths[0])
                .expect("content metadata")
                .len();

        assert_eq!(
            store
                .diagnostics()
                .expect("storage diagnostics")
                .database_bytes,
            expected_bytes
        );

        drop(store);
        let _ = std::fs::remove_dir_all(root);
        for content_path in content_paths {
            let _ = remove_sqlite_database(&content_path);
        }
        let _ = remove_sqlite_database(&db_path);
    }

    #[test]
    fn v7_migration_preserves_active_filename_path_and_content_queries_across_reopen() {
        let db_path = temp_db_path("v7-online-migration");
        let root = db_path.with_extension("root");
        let scan_plan = plan(&root);
        let legacy_item = owned(&root, "path-only-parent/legacy-needle.txt", 19);
        let item = legacy_item.item();
        let generation = 7_i64;
        let conn = Connection::open(&db_path).expect("open v7 database");
        conn.execute_batch(GENERATION_SCHEMA)
            .expect("seed canonical generation schema");
        conn.execute_batch(CONTENT_FTS_SCHEMA)
            .expect("seed canonical content schema");
        conn.execute(
            "INSERT INTO search_generations (id, plan_identity, completed, item_count)
             VALUES (?1, ?2, 1, 1)",
            params![generation, scan_plan.identity()],
        )
        .expect("seed completed generation");
        conn.execute(
            "INSERT INTO search_generation_items
             (generation_id, path, parent_path, root_path, name, kind, size_bytes,
              modified_unix_secs, is_executable, symlink_target_is_dir)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                generation,
                encode_path(&item.path),
                encode_path(&item.parent_path),
                encode_path(legacy_item.root()),
                item.name,
                entry_kind_to_i64(item.kind),
                item.size_bytes,
                item.modified_unix_secs,
                i64::from(item.is_executable),
                item.symlink_target_is_dir.map(i64::from),
            ],
        )
        .expect("seed canonical item");
        conn.execute(
            "INSERT INTO search_generation_paths (generation_id, path, path_text)
             VALUES (?1, ?2, ?3)",
            params![
                generation,
                encode_path(&item.path),
                item.path.to_string_lossy()
            ],
        )
        .expect("seed path accelerator");
        conn.execute(
            "INSERT INTO search_generation_content_state (generation_id, path, state, content)
             VALUES (?1, ?2, 0, 'legacy body needle')",
            params![generation, encode_path(&item.path)],
        )
        .expect("seed content completeness");
        conn.execute(
            "INSERT INTO search_generation_content (generation_id, path, content)
             VALUES (?1, ?2, 'legacy body needle')",
            params![generation, encode_path(&item.path)],
        )
        .expect("seed canonical content FTS");
        conn.execute(
            "INSERT INTO search_store_state (singleton, active_generation_id) VALUES (1, ?1)",
            [generation],
        )
        .expect("seed active pointer");
        conn.execute("PRAGMA user_version = 7", [])
            .expect("set v7 schema version");
        drop(conn);

        for reopen in 0..2 {
            let mut store = SearchStore::open(db_path.clone()).expect("open migrated database");
            let active = store
                .load(&scan_plan)
                .expect("load migrated generation")
                .expect("active generation survives migration");
            assert_eq!(active.generation(), generation);
            assert_eq!(active.items(), std::slice::from_ref(&legacy_item));
            assert_eq!(
                store
                    .conn
                    .query_row(
                        "SELECT active_generation_id FROM search_store_state WHERE singleton = 1",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .expect("read active pointer"),
                generation
            );
            drop(store);

            let (paths, total) = query_simple_term_page(
                &db_path,
                &scan_plan,
                SimpleTermPage {
                    term: "legacy-needle",
                    include_hidden: true,
                    sort: SearchSort::Relevance,
                    limit: 10,
                    offset: 0,
                },
            )
            .expect("query migrated path index");
            assert_eq!(total, 1, "reopen {reopen}");
            assert_eq!(paths[0].path, item.path, "reopen {reopen}");
            let (parent_paths, parent_total) = query_simple_term_page(
                &db_path,
                &scan_plan,
                SimpleTermPage {
                    term: "path-only-parent",
                    include_hidden: true,
                    sort: SearchSort::Relevance,
                    limit: 10,
                    offset: 0,
                },
            )
            .expect("query migrated parent path");
            assert_eq!(parent_total, 1, "reopen {reopen}");
            assert_eq!(parent_paths[0].path, item.path, "reopen {reopen}");
            let hits = query_indexed_content_page(&db_path, &scan_plan, "needle", 10, 0)
                .expect("query migrated content index");
            assert_eq!(hits.len(), 1, "reopen {reopen}");
            assert_eq!(hits[0].item.path, item.path, "reopen {reopen}");
        }

        for content_path in generation_content_db_paths(&db_path) {
            let _ = remove_sqlite_database(&content_path);
        }
        let _ = remove_sqlite_database(&db_path);
    }

    #[test]
    fn v8_migration_rebuilds_paths_from_canonical_items_when_accelerator_has_stale_duplicates() {
        let db_path = temp_db_path("v8-online-path-migration");
        let root = db_path.with_extension("root");
        let scan_plan = plan(&root);
        let item = owned(&root, "parent-segment/migration-needle.txt", 1);
        std::fs::create_dir_all(item.item().parent_path.clone()).expect("create content parent");
        std::fs::write(&item.item().path, "preserved body needle").expect("write indexed content");
        let generation = {
            let mut store = SearchStore::open(db_path.clone()).expect("open current store");
            store
                .commit(&scan_plan, IndexMutation::ReplaceAll(vec![item.clone()]))
                .expect("seed active generation")
                .generation()
        };
        let derived_path = generation_content_db_path(&db_path, generation);

        let canonical = Connection::open(&db_path).expect("open canonical database");
        canonical
            .execute_batch(
                "CREATE VIRTUAL TABLE search_generation_paths USING fts5(
                    generation_id UNINDEXED, path UNINDEXED, path_text,
                    tokenize = 'trigram'
                 );",
            )
            .expect("create v8 canonical path FTS");
        canonical
            .execute(
                "ATTACH DATABASE ?1 AS derived_store",
                [derived_path.to_string_lossy().as_ref()],
            )
            .expect("attach current derived database");
        canonical
            .execute(
                "INSERT INTO search_generation_paths (generation_id, path, path_text)
                 SELECT generation_id, path, path_text
                 FROM derived_store.search_generation_paths",
                [],
            )
            .expect("copy path FTS to v8 canonical location");
        canonical
            .execute(
                "INSERT INTO search_generation_paths (generation_id, path, path_text)
                 VALUES (?1, ?2, ?3), (?1, ?4, ?5)",
                params![
                    generation,
                    encode_path(&item.item().path),
                    item.item().path.to_string_lossy(),
                    encode_path(&root.join("retired-parent/stale-needle.txt")),
                    root.join("retired-parent/stale-needle.txt")
                        .to_string_lossy(),
                ],
            )
            .expect("seed duplicate current and stale deleted accelerator rows");
        canonical
            .execute_batch("DETACH DATABASE derived_store; PRAGMA user_version = 8;")
            .expect("finish v8 fixture");
        drop(canonical);
        Connection::open(&derived_path)
            .expect("open derived database")
            .execute_batch("DROP TABLE search_generation_paths;")
            .expect("remove path FTS absent from v8 derived schema");

        for reopen in 0..2 {
            let mut store =
                SearchStore::open(db_path.clone()).expect("migrate or reopen v8 database");
            let active = store
                .load(&scan_plan)
                .expect("load migrated generation")
                .expect("active generation survives migration");
            assert_eq!(active.generation(), generation, "reopen {reopen}");
            assert_eq!(
                active.items(),
                std::slice::from_ref(&item),
                "reopen {reopen}"
            );
        }

        let canonical = Connection::open(&db_path).expect("inspect migrated canonical database");
        assert_eq!(
            canonical
                .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .expect("schema version"),
            SEARCH_DB_VERSION
        );
        assert_eq!(
            canonical
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name = 'search_generation_paths'",
                    [],
                    |row| row.get::<_, usize>(0),
                )
                .expect("canonical path table count"),
            0
        );
        let (results, total) = query_simple_term_page(
            &db_path,
            &scan_plan,
            SimpleTermPage {
                term: "migration-needle",
                include_hidden: true,
                sort: SearchSort::Relevance,
                limit: 10,
                offset: 0,
            },
        )
        .expect("query migrated path FTS");
        assert_eq!(total, 1);
        assert_eq!(results[0].path, item.item().path);
        let (parent_results, parent_total) = query_simple_term_page(
            &db_path,
            &scan_plan,
            SimpleTermPage {
                term: "parent-segment",
                include_hidden: true,
                sort: SearchSort::Relevance,
                limit: 10,
                offset: 0,
            },
        )
        .expect("query migrated parent path");
        assert_eq!(parent_total, 1);
        assert_eq!(parent_results[0].path, item.item().path);
        let (_, stale_total) = query_simple_term_page(
            &db_path,
            &scan_plan,
            SimpleTermPage {
                term: "stale-needle",
                include_hidden: true,
                sort: SearchSort::Relevance,
                limit: 10,
                offset: 0,
            },
        )
        .expect("query retired path posting");
        assert_eq!(stale_total, 0);
        let content_hits = query_indexed_content_page(&db_path, &scan_plan, "preserved", 10, 0)
            .expect("query migrated content FTS");
        assert_eq!(content_hits.len(), 1);
        assert_eq!(content_hits[0].item.path, item.item().path);
        let derived = Connection::open(&derived_path).expect("inspect migrated derived database");
        let canonical_item_count = canonical
            .query_row(
                "SELECT COUNT(*) FROM search_generation_items WHERE generation_id = ?1",
                [generation],
                |row| row.get::<_, usize>(0),
            )
            .expect("count canonical items");
        let derived_path_count = derived
            .query_row("SELECT COUNT(*) FROM search_generation_paths", [], |row| {
                row.get::<_, usize>(0)
            })
            .expect("count rebuilt paths");
        let derived_state_count = derived
            .query_row(
                "SELECT COUNT(*) FROM search_generation_content_state",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("count retained content states");
        assert_eq!(
            (
                canonical_item_count,
                derived_path_count,
                derived_state_count
            ),
            (1, 1, 1),
            "migration must retain exactly one canonical item, path, and content state"
        );

        let _ = std::fs::remove_dir_all(root);
        let _ = remove_sqlite_database(&derived_path);
        let _ = remove_sqlite_database(&db_path);
    }

    #[test]
    fn staged_batches_remain_inactive_until_atomic_activation() {
        let db_path = temp_db_path("generation-staged-batches");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        let original = store
            .commit(
                &plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "old.txt", 1)]),
            )
            .expect("initial generation");

        let staged = store
            .begin_staged_generation(&plan)
            .expect("begin staged generation");
        store
            .append_staged_generation(&plan, staged, &[owned(&root, "new-a.txt", 2)])
            .expect("append first batch");
        assert_eq!(
            store.load(&plan).expect("active generation during staging"),
            Some(original),
            "partial staging must never replace the active generation"
        );
        store
            .append_staged_generation(&plan, staged, &[owned(&root, "new-b.txt", 3)])
            .expect("append second batch");

        let fence = ActivationFence::new();
        let activated_count = store
            .activate_staged_generation(&plan, staged, 2, &fence, fence.capture())
            .expect("activate complete staging");
        assert_eq!(activated_count, 2);
        let active = store
            .load(&plan)
            .expect("load activated generation")
            .expect("active generation");
        assert_eq!(
            active.items(),
            &[owned(&root, "new-a.txt", 2), owned(&root, "new-b.txt", 3)]
        );
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn staged_activation_rejects_a_missing_disposable_content_database() {
        let db_path = temp_db_path("generation-missing-content-store");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        let generation = store
            .begin_staged_generation(&plan)
            .expect("begin staged generation");
        remove_sqlite_database(&generation_content_db_path(&db_path, generation))
            .expect("remove staged content database");
        let fence = ActivationFence::new();

        let result =
            store.activate_staged_generation(&plan, generation, 0, &fence, fence.capture());

        assert!(matches!(result, Err(SearchDbError::InvalidGeneration(_))));
        assert!(store.load(&plan).expect("load canonical state").is_none());
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn staged_append_cost_does_not_scale_with_the_active_path_fts() {
        let db_path = temp_db_path("generation-staged-append-cost");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        let active = (0..500)
            .map(|index| owned(&root, &format!("active-{index:04}.txt"), index))
            .collect::<Vec<_>>();
        store
            .commit(&plan, IndexMutation::ReplaceAll(active))
            .expect("seed active generation");
        let staged = store
            .begin_staged_generation(&plan)
            .expect("begin staged generation");
        PATH_FTS_DELETE_COUNT.with(|count| count.set(0));

        store
            .append_staged_generation(&plan, staged, &[owned(&root, "new.txt", 1000)])
            .expect("append one staged item");

        assert_eq!(
            PATH_FTS_DELETE_COUNT.with(Cell::get),
            0,
            "new staged rows must not run replacement deletes against the unrelated active FTS"
        );
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn staged_activation_holds_the_fence_through_publication() {
        let db_path = temp_db_path("generation-staged-fence");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        let staged = store
            .begin_staged_generation(&plan)
            .expect("begin staged generation");
        store
            .append_staged_generation(&plan, staged, &[owned(&root, "ready.txt", 1)])
            .expect("append batch");
        let fence = Arc::new(ActivationFence::new());
        let expected_epoch = fence.capture();
        let invalidating_fence = Arc::clone(&fence);
        let (attempted_tx, attempted_rx) = std::sync::mpsc::channel();
        let (completed_tx, completed_rx) = std::sync::mpsc::channel();

        let invalidator = Arc::new(std::sync::Mutex::new(None));
        let invalidator_slot = Arc::clone(&invalidator);
        let count = store
            .activate_staged_generation_then(
                &plan,
                staged,
                1,
                &fence,
                expected_epoch,
                move |published_count| {
                    assert_eq!(published_count, 1);
                    let handle = std::thread::spawn(move || {
                        attempted_tx.send(()).expect("signal invalidation attempt");
                        invalidating_fence.invalidate();
                        let _ = completed_tx.send(());
                    });
                    *invalidator_slot.lock().expect("invalidator slot") = Some(handle);
                    attempted_rx.recv().expect("invalidation attempted");
                    assert!(
                        completed_rx
                            .recv_timeout(std::time::Duration::from_millis(25))
                            .is_err(),
                        "activation fence was released before publication completed"
                    );
                },
            )
            .expect("activate staging");
        assert_eq!(count, 1);
        invalidator
            .lock()
            .expect("invalidator slot")
            .take()
            .expect("invalidator")
            .join()
            .expect("join invalidator");
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn discarded_staging_preserves_the_previous_generation() {
        let db_path = temp_db_path("generation-discard-staging");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        let original = store
            .commit(
                &plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "old.txt", 1)]),
            )
            .expect("initial generation");
        let staged = store
            .begin_staged_generation(&plan)
            .expect("begin staged generation");
        store
            .append_staged_generation(&plan, staged, &[owned(&root, "partial.txt", 2)])
            .expect("append partial batch");
        store
            .discard_staged_generation(staged)
            .expect("discard partial staging");
        assert_eq!(store.load(&plan).expect("load active"), Some(original));
        let incomplete: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM search_generations WHERE completed = 0",
                [],
                |row| row.get(0),
            )
            .expect("count incomplete staging");
        assert_eq!(incomplete, 0);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn reopen_removes_crash_left_incomplete_staging() {
        let db_path = temp_db_path("generation-reopen-staging");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        {
            let mut store = SearchStore::open(db_path.clone()).expect("open store");
            store
                .commit(
                    &plan,
                    IndexMutation::ReplaceAll(vec![owned(&root, "old.txt", 1)]),
                )
                .expect("initial generation");
            let staged = store
                .begin_staged_generation(&plan)
                .expect("begin staged generation");
            store
                .append_staged_generation(&plan, staged, &[owned(&root, "partial.txt", 2)])
                .expect("append partial batch");
        }

        let mut reopened = SearchStore::open(db_path.clone()).expect("reopen store");

        assert_eq!(
            reopened
                .load(&plan)
                .expect("load active")
                .expect("active")
                .items()[0]
                .item()
                .name,
            "old.txt"
        );
        let incomplete: i64 = reopened
            .conn
            .query_row(
                "SELECT COUNT(*) FROM search_generations WHERE completed = 0",
                [],
                |row| row.get(0),
            )
            .expect("count incomplete staging");
        assert_eq!(incomplete, 0);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn reopen_removes_orphaned_disposable_content_databases() {
        let db_path = temp_db_path("generation-reopen-orphaned-content");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        let active_generation = {
            let mut store = SearchStore::open(db_path.clone()).expect("open store");
            store
                .commit(
                    &plan,
                    IndexMutation::ReplaceAll(vec![owned(&root, "active.txt", 1)]),
                )
                .expect("commit active generation")
                .generation()
        };
        let orphan_generation = active_generation + 1;
        let active_content = generation_content_db_path(&db_path, active_generation);
        let orphan_content = generation_content_db_path(&db_path, orphan_generation);
        create_generation_content_db(&db_path, orphan_generation)
            .expect("create crash-left orphan content database");
        assert!(active_content.is_file());
        assert!(orphan_content.is_file());

        drop(SearchStore::open(db_path.clone()).expect("reopen store"));

        assert!(
            active_content.is_file(),
            "active content database must remain"
        );
        assert!(
            !orphan_content.exists(),
            "content database without a canonical generation must be removed"
        );
        let _ = std::fs::remove_file(active_content);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn discarding_an_active_completed_generation_is_a_noop() {
        let db_path = temp_db_path("generation-discard-active");
        let root = db_path.with_extension("root");
        let plan = plan(&root);
        let content_path = root.join("active.txt");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(&content_path, "staged active content").expect("write content");
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        let staged = store
            .begin_staged_generation(&plan)
            .expect("begin staged generation");
        store
            .append_staged_generation(&plan, staged, &[owned(&root, "active.txt", 1)])
            .expect("append batch");
        let fence = ActivationFence::new();
        store
            .activate_staged_generation(&plan, staged, 1, &fence, fence.capture())
            .expect("activate staging");

        store
            .discard_staged_generation(staged)
            .expect("idempotent discard");

        assert_eq!(
            store
                .load(&plan)
                .expect("load active")
                .expect("active")
                .items()
                .len(),
            1
        );
        assert_eq!(
            query_indexed_content_page(&db_path, &plan, "staged", 10, 0)
                .expect("query active content")
                .len(),
            1
        );
        let _ = std::fs::remove_file(content_path);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(db_path);
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
    fn trigram_path_query_is_bounded_and_tracks_direct_deltas() {
        let db_path = temp_db_path("trigram-path-delta");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &plan,
                IndexMutation::ReplaceAll(vec![
                    owned(&root, "alpha/needle-one.txt", 1),
                    owned(&root, "needle-folder/two.txt", 2),
                    owned(&root, "unrelated.txt", 3),
                ]),
            )
            .expect("initial generation");

        let (first_page, total) = query_simple_term_page(
            &db_path,
            &plan,
            SimpleTermPage {
                term: "needle",
                include_hidden: true,
                sort: SearchSort::Relevance,
                limit: 1,
                offset: 0,
            },
        )
        .expect("query trigram index");
        assert_eq!(total, 2);
        assert_eq!(first_page.len(), 1, "only the requested page is decoded");

        let fence = ActivationFence::new();
        let expected_epoch = fence.capture();
        PATH_FTS_DELETE_COUNT.with(|count| count.set(0));
        store
            .commit_delta_guarded_then(
                &plan,
                &[
                    owned(&root, "alpha/needle-one.txt", 99),
                    owned(&root, "renamed/result-three.txt", 4),
                ],
                &[root.join("needle-folder")],
                &fence,
                expected_epoch,
                |_| {},
            )
            .expect("direct delta");
        assert_eq!(
            PATH_FTS_DELETE_COUNT.with(Cell::get),
            0,
            "direct upserts must not scan the full path FTS before publication"
        );
        let active_generation: i64 = store
            .conn
            .query_row(
                "SELECT active_generation_id FROM search_store_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .expect("active generation");
        let derived = Connection::open(generation_content_db_path(&db_path, active_generation))
            .expect("open derived generation");
        let physical_fts_rows: usize = derived
            .query_row("SELECT COUNT(*) FROM search_generation_paths", [], |row| {
                row.get(0)
            })
            .expect("count physical path FTS rows");
        assert_eq!(
            physical_fts_rows, 4,
            "direct deletion retains a stale accelerator posting for bounded-query filtering"
        );
        let (_, old_total) = query_simple_term_page(
            &db_path,
            &plan,
            SimpleTermPage {
                term: "needle-folder",
                include_hidden: true,
                sort: SearchSort::Name,
                limit: 10,
                offset: 0,
            },
        )
        .expect("query removed trigram");
        let (new_items, new_total) = query_simple_term_page(
            &db_path,
            &plan,
            SimpleTermPage {
                term: "result-three",
                include_hidden: true,
                sort: SearchSort::Name,
                limit: 10,
                offset: 0,
            },
        )
        .expect("query inserted trigram");
        assert_eq!(old_total, 0);
        assert_eq!(new_total, 1);
        assert_eq!(new_items[0].path, root.join("renamed/result-three.txt"));
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn new_cache_enables_and_applies_bounded_incremental_vacuum() {
        let db_path = temp_db_path("storage-incremental-vacuum");
        let store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .conn
            .execute_batch(
                "CREATE TABLE diagnostic_churn (payload BLOB NOT NULL);
                 INSERT INTO diagnostic_churn VALUES (zeroblob(65536));
                 DELETE FROM diagnostic_churn;",
            )
            .expect("create diagnostic freelist");
        let auto_vacuum: u64 = store
            .conn
            .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
            .expect("read auto vacuum policy");
        let journal_size_limit: i64 = store
            .conn
            .query_row("PRAGMA journal_size_limit", [], |row| row.get(0))
            .expect("read WAL retention limit");
        let before = store
            .diagnostics()
            .expect("read diagnostics before reclaim");

        reclaim_freelist_incrementally(&store.conn).expect("reclaim bounded freelist");

        let after = store.diagnostics().expect("read diagnostics after reclaim");
        assert_eq!(auto_vacuum, 2, "new caches must use incremental vacuum");
        assert_eq!(
            journal_size_limit, 268_435_456,
            "writer connections must bound retained WAL allocation"
        );
        assert!(before.freelist_pages > 0, "fixture must create free pages");
        assert!(
            after.freelist_pages < before.freelist_pages,
            "bounded maintenance must return free pages"
        );
        assert!(after.freelist_bytes < before.freelist_bytes);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn checkpoint_policy_covers_full_and_large_mutations_only() {
        let root = PathBuf::from("root");
        assert!(requires_checkpoint_after_commit(
            &IndexMutation::ReplaceAll(Vec::new())
        ));
        assert!(!requires_checkpoint_after_commit(&IndexMutation::Delta {
            upserts: vec![owned(&root, "small.txt", 1)],
            remove_trees: Vec::new(),
        }));
        assert!(requires_checkpoint_after_commit(&IndexMutation::Delta {
            upserts: (0..CHECKPOINT_MUTATION_ITEM_THRESHOLD)
                .map(|i| owned(&root, &format!("large-{i}.txt"), i as u64))
                .collect(),
            remove_trees: Vec::new(),
        }));
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
    fn content_stream_started_before_replacement_reads_one_complete_generation() {
        let db_path = temp_db_path("generation-content-stream-replacement");
        let root = db_path.with_extension("root");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(root.join("old.txt"), "old body").expect("write old content");
        std::fs::write(root.join("new.txt"), "new body").expect("write new content");
        let scan_plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "old.txt", 1)]),
            )
            .expect("commit old generation");
        let mut streamed = Vec::new();

        stream_generation_content_items_then(
            &db_path,
            &scan_plan,
            None,
            |item| streamed.push((item.item.path, item.content, item.complete)),
            || {
                store
                    .commit(
                        &scan_plan,
                        IndexMutation::ReplaceAll(vec![owned(&root, "new.txt", 2)]),
                    )
                    .expect("replace active generation while reader is open");
            },
        )
        .expect("stream generation selected before replacement");

        assert_eq!(
            streamed,
            vec![(root.join("old.txt"), Some("old body".to_string()), true)]
        );
        let _ = std::fs::remove_dir_all(root);
        for content_path in generation_content_db_paths(&db_path) {
            let _ = remove_sqlite_database(&content_path);
        }
        let _ = remove_sqlite_database(&db_path);
    }

    #[test]
    fn content_query_started_before_replacement_reads_one_complete_generation() {
        let db_path = temp_db_path("generation-fts-reader-replacement");
        let root = db_path.with_extension("root");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(root.join("old.txt"), "shared needle old").expect("write old content");
        std::fs::write(root.join("new.txt"), "shared needle new").expect("write new content");
        let scan_plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "old.txt", 1)]),
            )
            .expect("commit old generation");

        let hits = query_indexed_content_page_then(&db_path, &scan_plan, "needle", 10, 0, || {
            store
                .commit(
                    &scan_plan,
                    IndexMutation::ReplaceAll(vec![owned(&root, "new.txt", 2)]),
                )
                .expect("replace active generation while reader is open");
        })
        .expect("query generation selected before replacement");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].item.path, root.join("old.txt"));
        assert_eq!(
            query_indexed_content_page(&db_path, &scan_plan, "needle", 10, 0)
                .expect("query replacement")[0]
                .item
                .path,
            root.join("new.txt")
        );
        let _ = std::fs::remove_dir_all(root);
        for content_path in generation_content_db_paths(&db_path) {
            let _ = remove_sqlite_database(&content_path);
        }
        let _ = remove_sqlite_database(&db_path);
    }

    #[test]
    fn path_query_started_before_replacement_reads_one_complete_generation() {
        let db_path = temp_db_path("generation-path-reader-replacement");
        let root = PathBuf::from("root");
        let scan_plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "old-shared-needle.txt", 1)]),
            )
            .expect("commit old generation");

        let (items, total) = query_simple_term_page_then(
            &db_path,
            &scan_plan,
            SimpleTermPage {
                term: "shared-needle",
                include_hidden: true,
                sort: SearchSort::Relevance,
                limit: 10,
                offset: 0,
            },
            || {
                store
                    .commit(
                        &scan_plan,
                        IndexMutation::ReplaceAll(vec![owned(&root, "new-shared-needle.txt", 2)]),
                    )
                    .expect("replace active generation while path reader is open");
            },
        )
        .expect("query generation selected before replacement");

        assert_eq!(total, 1);
        assert_eq!(items[0].path, root.join("old-shared-needle.txt"));
        let (replacement, replacement_total) = query_simple_term_page(
            &db_path,
            &scan_plan,
            SimpleTermPage {
                term: "shared-needle",
                include_hidden: true,
                sort: SearchSort::Relevance,
                limit: 10,
                offset: 0,
            },
        )
        .expect("query replacement generation");
        assert_eq!(replacement_total, 1);
        assert_eq!(replacement[0].path, root.join("new-shared-needle.txt"));

        for content_path in generation_content_db_paths(&db_path) {
            let _ = remove_sqlite_database(&content_path);
        }
        let _ = remove_sqlite_database(&db_path);
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
        assert_eq!(old_hits[0].item.path, old);
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
        assert_eq!(new_hits[0].item.path, new);
        assert_eq!(new_hits[0].snippet, "fresh unique content");
        assert_eq!(new_hits[0].content, "fresh unique content");
        assert!(new_hits[0].relevance > 0);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(db_path);
    }

    /// Isolated reproduction for path-FTS replacement amplification. The
    /// fixture disables content indexing and uses equal-sized generations so
    /// aggregate growth can only come from metadata/path-index lifecycle.
    #[test]
    #[ignore = "measurement fixture: run explicitly when evaluating path FTS storage layout"]
    fn equal_sized_path_fts_replacement_keeps_aggregate_storage_bounded() {
        const FILES: usize = 12_000;

        let db_path = temp_db_path("path-fts-replacement-measurement");
        let root = PathBuf::from("root");
        let scan_plan = ScanPlan::new(&SearchConfig {
            include_roots: vec![root.clone()],
            include_hidden_system: true,
            content_indexing_enabled: false,
            ..SearchConfig::default()
        })
        .expect("valid plan");
        let generation = |label: &str| {
            (0..FILES)
                .map(|index| {
                    owned(
                        &root,
                        &format!(
                            "{label}/segment-{index:05}-aaaaaaaa/segment-{index:05}-bbbbbbbb/document-{index:05}-cccccccc.txt"
                        ),
                        index as u64,
                    )
                })
                .collect::<Vec<_>>()
        };
        let mut store = SearchStore::open(db_path.clone()).expect("open isolated store");

        store
            .commit(&scan_plan, IndexMutation::ReplaceAll(generation("retired")))
            .expect("commit initial path generation");
        let initial = store.diagnostics().expect("measure initial storage");
        store
            .commit(&scan_plan, IndexMutation::ReplaceAll(generation("current")))
            .expect("commit replacement path generation");
        let replacement = store.diagnostics().expect("measure replacement storage");
        store
            .commit(&scan_plan, IndexMutation::ReplaceAll(generation("renewed")))
            .expect("commit repeated path generation");
        let repeated = store.diagnostics().expect("measure repeated storage");

        println!(
            "PATH_FTS_REPLACEMENT_MEASUREMENT files={FILES} initial_db_bytes={} replacement_db_bytes={} repeated_db_bytes={} initial_wal_bytes={} replacement_wal_bytes={} repeated_wal_bytes={} freelist_pages={}",
            initial.database_bytes,
            replacement.database_bytes,
            repeated.database_bytes,
            initial.wal_bytes,
            replacement.wal_bytes,
            repeated.wal_bytes,
            repeated.freelist_pages,
        );
        assert!(
            repeated.database_bytes <= replacement.database_bytes.saturating_mul(105) / 100,
            "repeated equal-sized path replacement must reuse the retained high-water allocation: replacement={} repeated={}",
            replacement.database_bytes,
            repeated.database_bytes
        );
        assert!(
            replacement.wal_bytes <= initial.database_bytes / 4
                && repeated.wal_bytes <= initial.database_bytes / 4,
            "canonical WAL must not carry a full disposable path accelerator: {} bytes",
            replacement.wal_bytes.max(repeated.wal_bytes)
        );
        assert_eq!(
            generation_content_db_paths(&db_path).len(),
            1,
            "only the active disposable generation should remain"
        );
        assert!(
            repeated.freelist_pages > 0,
            "bounded maintenance must preserve reusable high-water pages"
        );

        drop(store);
        for content_path in generation_content_db_paths(&db_path) {
            let _ = remove_sqlite_database(&content_path);
        }
        let _ = remove_sqlite_database(&db_path);
    }

    /// Reproducible isolated measurement for the generation-scoped FTS lifecycle.
    /// Run explicitly with `--ignored --nocapture`; it is deliberately excluded
    /// from the ordinary suite because it writes a 24 MiB content fixture twice.
    #[test]
    #[ignore = "measurement fixture: run explicitly when evaluating FTS storage layout"]
    fn full_generation_replacement_reports_fts_write_and_cleanup_cost() {
        const FILES: usize = 96;
        const CONTENT_BYTES: usize = 256 * 1024;

        let db_path = temp_db_path("fts-replacement-measurement");
        let root = std::env::temp_dir().join(format!(
            "ottrin-fts-replacement-measurement-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let scan_plan = plan(&root);
        let items = (0..FILES)
            .map(|index| {
                owned(
                    &root,
                    &format!("document-{index:03}.txt"),
                    CONTENT_BYTES as u64,
                )
            })
            .collect::<Vec<_>>();
        for index in 0..FILES {
            let mut content = format!("retired-{index:03}-").repeat(CONTENT_BYTES / 12 + 1);
            content.truncate(CONTENT_BYTES);
            std::fs::write(root.join(format!("document-{index:03}.txt")), content)
                .expect("write retired fixture content");
        }

        let mut store = SearchStore::open(db_path.clone()).expect("open isolated store");
        let initial_started = std::time::Instant::now();
        store
            .commit(&scan_plan, IndexMutation::ReplaceAll(items.clone()))
            .expect("commit retired generation");
        let initial_elapsed = initial_started.elapsed();
        let initial = store.diagnostics().expect("measure initial storage");

        for index in 0..FILES {
            let mut content = format!("fresh-{index:03}-xx").repeat(CONTENT_BYTES / 12 + 1);
            content.truncate(CONTENT_BYTES);
            std::fs::write(root.join(format!("document-{index:03}.txt")), content)
                .expect("write replacement fixture content");
        }
        let replacement_started = std::time::Instant::now();
        store
            .commit(&scan_plan, IndexMutation::ReplaceAll(items))
            .expect("commit replacement generation");
        let replacement_elapsed = replacement_started.elapsed();
        let replacement = store.diagnostics().expect("measure replacement storage");
        let retired_hits = query_indexed_content_page(&db_path, &scan_plan, "retired", 1, 0)
            .expect("query retired content");
        let fresh_hits = query_indexed_content_page(&db_path, &scan_plan, "fresh", 1, 0)
            .expect("query replacement content");

        println!(
            "FTS_REPLACEMENT_MEASUREMENT files={FILES} payload_bytes={} initial_ms={} replacement_ms={} initial_db_bytes={} replacement_db_bytes={} initial_wal_bytes={} replacement_wal_bytes={} freelist_pages={}",
            FILES * CONTENT_BYTES,
            initial_elapsed.as_millis(),
            replacement_elapsed.as_millis(),
            initial.database_bytes,
            replacement.database_bytes,
            initial.wal_bytes,
            replacement.wal_bytes,
            replacement.freelist_pages,
        );
        assert!(
            retired_hits.is_empty(),
            "obsolete FTS content must not remain queryable"
        );
        assert_eq!(
            fresh_hits.len(),
            1,
            "replacement FTS content must be queryable"
        );
        assert!(
            replacement.freelist_pages <= INCREMENTAL_VACUUM_PAGE_BUDGET,
            "bounded replacement may retain only a small reusable freelist: {} pages",
            replacement.freelist_pages
        );
        assert!(
            replacement.database_bytes <= initial.database_bytes.saturating_mul(105) / 100,
            "replacing equal-sized derived content must not retain the retired generation: initial={} replacement={}",
            initial.database_bytes,
            replacement.database_bytes
        );
        assert!(
            replacement.wal_bytes < (FILES * CONTENT_BYTES) as u64,
            "canonical WAL must not carry the disposable content payload: {} bytes",
            replacement.wal_bytes
        );
        assert_eq!(
            generation_content_db_paths(&db_path).len(),
            1,
            "only the active disposable content generation should remain"
        );

        drop(store);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn metadata_equal_incremental_upserts_refresh_content() {
        let db_path = temp_db_path("delta-authoritative-content-refresh");
        let root = std::env::temp_dir().join(format!(
            "ottrin-delta-authoritative-content-refresh-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("same.txt");
        std::fs::write(&path, "oldcontent").expect("write old same-sized content");
        let scan_plan = plan(&root);
        let item = owned(&root, "same.txt", 10);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        let initial = store
            .commit(&scan_plan, IndexMutation::ReplaceAll(vec![item.clone()]))
            .expect("commit initial generation");

        std::fs::write(&path, "newcontent").expect("write new same-sized content");
        let updated = store
            .commit(
                &scan_plan,
                IndexMutation::Delta {
                    upserts: vec![item.clone()],
                    remove_trees: Vec::new(),
                },
            )
            .expect("commit authoritative delta");

        assert_eq!(updated.generation(), initial.generation());
        assert!(
            query_indexed_content_page(&db_path, &scan_plan, "oldcontent", 10, 0)
                .expect("query stale content")
                .is_empty(),
            "a metadata-equal upsert must remove its stale positive FTS hit"
        );
        assert_eq!(
            query_indexed_content_page(&db_path, &scan_plan, "newcontent", 10, 0)
                .expect("query refreshed content")
                .len(),
            1
        );
        std::fs::write(&path, "oldcontent").expect("restore same-sized content");
        let replaced = store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceRoot {
                    root: root.clone(),
                    items: vec![item],
                },
            )
            .expect("commit authoritative root replacement");
        assert_eq!(replaced.generation(), initial.generation());
        assert!(
            query_indexed_content_page(&db_path, &scan_plan, "newcontent", 10, 0)
                .expect("query superseded replacement content")
                .is_empty(),
            "a metadata-equal root replacement must remove its stale positive FTS hit"
        );
        assert_eq!(
            query_indexed_content_page(&db_path, &scan_plan, "oldcontent", 10, 0)
                .expect("query root replacement content")
                .len(),
            1
        );
        let mut streamed = Vec::new();
        stream_generation_content_items(&db_path, &scan_plan, None, |record| {
            streamed.push((record.item.path, record.content, record.complete));
        })
        .expect("stream content for negated query");
        assert_eq!(
            streamed,
            vec![(path.clone(), Some("oldcontent".to_string()), true)],
            "negated queries must evaluate the refreshed complete content, not stale text"
        );
        let generations: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM search_generations", [], |row| {
                row.get(0)
            })
            .expect("count generations");
        let freelist_pages: i64 = store
            .conn
            .query_row("PRAGMA freelist_count", [], |row| row.get(0))
            .expect("count freelist pages");
        assert_eq!(generations, 1);
        assert_eq!(freelist_pages, 0);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn generation_content_honors_configured_extraction_size_limit() {
        let db_path = temp_db_path("generation-configured-content-limit");
        let root = db_path.with_extension("root");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(root.join("small.txt"), "smallneedle").expect("write small content");
        std::fs::write(root.join("large.txt"), "largeneedle").expect("write large content");
        let scan_plan = ScanPlan::new(&SearchConfig {
            include_roots: vec![root.clone()],
            include_hidden_system: true,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            content_max_file_size_bytes: 11,
            ..SearchConfig::default()
        })
        .expect("valid plan");
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![
                    owned(&root, "small.txt", 11),
                    owned(&root, "large.txt", 12),
                ]),
            )
            .expect("commit generation");

        assert_eq!(
            query_indexed_content_page(&db_path, &scan_plan, "smallneedle", 10, 0)
                .expect("query admitted content")
                .len(),
            1
        );
        assert!(
            query_indexed_content_page(&db_path, &scan_plan, "largeneedle", 10, 0)
                .expect("query excluded content")
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn generation_content_honors_selected_file_groups() {
        let db_path = temp_db_path("generation-selected-content-groups");
        let root = db_path.with_extension("root");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(root.join("notes.txt"), "documentneedle").expect("write document content");
        std::fs::write(root.join("main.rs"), "sourceneedle").expect("write source content");
        let scan_plan = ScanPlan::new(&SearchConfig {
            include_roots: vec![root.clone()],
            include_hidden_system: true,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            content_file_groups: vec![ContentFileGroup::TextDocuments],
            ..SearchConfig::default()
        })
        .expect("valid plan");
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![
                    owned(&root, "notes.txt", 14),
                    owned(&root, "main.rs", 12),
                ]),
            )
            .expect("commit generation");

        assert_eq!(
            query_indexed_content_page(&db_path, &scan_plan, "documentneedle", 10, 0)
                .expect("query selected document group")
                .len(),
            1
        );
        assert!(
            query_indexed_content_page(&db_path, &scan_plan, "sourceneedle", 10, 0)
                .expect("query unselected source group")
                .is_empty()
        );
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
            vec![b'x'; ottrin_core::DEFAULT_CONTENT_MAX_FILE_SIZE_BYTES as usize + 1],
        )
        .expect("write oversized content");
        let mut grown = b"growthneedle ".to_vec();
        grown.resize(
            ottrin_core::DEFAULT_CONTENT_MAX_FILE_SIZE_BYTES as usize + 1,
            b'x',
        );
        std::fs::write(root.join("grown-after-scan.txt"), grown)
            .expect("write content that outgrew scanned metadata");
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![
                    owned(&root, "binary.txt", 15),
                    owned(
                        &root,
                        "oversized.txt",
                        ottrin_core::DEFAULT_CONTENT_MAX_FILE_SIZE_BYTES + 1,
                    ),
                    owned(&root, "grown-after-scan.txt", 1),
                ]),
            )
            .expect("commit generation");

        assert!(
            query_indexed_content_page(&db_path, &scan_plan, "needle", 10, 0)
                .expect("query binary")
                .is_empty()
        );
        assert!(
            query_indexed_content_page(&db_path, &scan_plan, "growthneedle", 10, 0)
                .expect("query file that grew after scan")
                .is_empty(),
            "content reads must enforce their byte cap even when the file grows after metadata scan"
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
    fn completed_generation_replacement_bounds_reclaim_and_reuses_remaining_pages() {
        const OBSOLETE_STORAGE_PAGES: u64 = INCREMENTAL_VACUUM_PAGE_BUDGET * 2 + 1;
        let db_path = temp_db_path("generation-reclaim-obsolete-storage");
        let root = PathBuf::from("root");
        let plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "retired.txt", 1)]),
            )
            .expect("initial generation");
        store
            .conn
            .execute_batch("CREATE TABLE rebuildable_path_fts_fixture (payload BLOB NOT NULL)")
            .expect("create disposable rebuild fixture");
        store
            .conn
            .execute(
                "INSERT INTO rebuildable_path_fts_fixture (payload) VALUES (zeroblob(?1))",
                [(OBSOLETE_STORAGE_PAGES * 4096) as i64],
            )
            .expect("fill disposable rebuild fixture");
        store
            .conn
            .execute("DELETE FROM rebuildable_path_fts_fixture", [])
            .expect("discard obsolete rebuild fixture");
        let before_replacement: u64 = store
            .conn
            .query_row("PRAGMA freelist_count", [], |row| row.get(0))
            .expect("count obsolete pages");
        assert!(
            before_replacement > INCREMENTAL_VACUUM_PAGE_BUDGET,
            "fixture must exceed one maintenance budget"
        );

        store
            .commit(
                &plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "current.txt", 2)]),
            )
            .expect("replacement generation");

        let after_replacement: u64 = store
            .conn
            .query_row("PRAGMA freelist_count", [], |row| row.get(0))
            .expect("count retained reusable pages");
        let reclaimed = before_replacement.saturating_sub(after_replacement);
        assert!(
            reclaimed > 0,
            "maintenance must reclaim some trailing pages"
        );
        assert!(
            reclaimed <= INCREMENTAL_VACUUM_PAGE_BUDGET,
            "one replacement may reclaim at most {INCREMENTAL_VACUUM_PAGE_BUDGET} pages, reclaimed {reclaimed}"
        );
        assert!(
            after_replacement > 0,
            "excess free pages must remain available to the next generation"
        );
        assert_eq!(
            store
                .diagnostics()
                .expect("replacement diagnostics")
                .wal_bytes,
            0,
            "post-replacement WAL checkpoint must truncate retained write traffic"
        );

        let page_count_before_stage: u64 = store
            .conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .expect("page count before staged generation");
        let staged = store
            .begin_staged_generation(&plan)
            .expect("begin next generation");
        let staged_items = (0..512)
            .map(|index| {
                owned(
                    &root,
                    &format!("next/{index:04}-{}.txt", "reusable-path-segment".repeat(8)),
                    index,
                )
            })
            .collect::<Vec<_>>();
        store
            .append_staged_generation(&plan, staged, &staged_items)
            .expect("write next staged generation into reusable pages");
        let page_count_after_stage: u64 = store
            .conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .expect("page count after staged generation");
        let freelist_after_stage: u64 = store
            .conn
            .query_row("PRAGMA freelist_count", [], |row| row.get(0))
            .expect("freelist after staged generation");
        assert_eq!(
            page_count_after_stage, page_count_before_stage,
            "the next staged generation must reuse the retained high-water allocation"
        );
        assert!(
            freelist_after_stage < after_replacement,
            "staging must consume retained free pages"
        );

        store
            .discard_staged_generation(staged)
            .expect("discard staged fixture");
        for content_path in generation_content_db_paths(&db_path) {
            let _ = remove_sqlite_database(&content_path);
        }
        let _ = remove_sqlite_database(&db_path);
    }

    #[test]
    fn staged_generation_activation_releases_sqlite_page_cache() {
        const ITEMS: usize = 8_192;
        let db_path = temp_db_path("generation-activation-cache-release");
        let root = PathBuf::from("root");
        let plan = ScanPlan::new(&SearchConfig {
            include_roots: vec![root.clone()],
            include_hidden_system: true,
            content_indexing_enabled: false,
            ..SearchConfig::default()
        })
        .expect("valid plan");
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "retired.txt", 1)]),
            )
            .expect("initial generation");
        let staged = store
            .begin_staged_generation(&plan)
            .expect("begin staged generation");
        let items = (0..ITEMS)
            .map(|index| {
                owned(
                    &root,
                    &format!(
                        "next/{index:04}-{}.txt",
                        "cache-populating-segment".repeat(8)
                    ),
                    index as u64,
                )
            })
            .collect::<Vec<_>>();
        store
            .append_staged_generation(&plan, staged, &items)
            .expect("populate staged generation");
        let staged_payload_bytes: usize = store
            .conn
            .query_row(
                "SELECT SUM(length(path) + length(parent_path) + length(name))
                 FROM search_generation_items WHERE generation_id = ?1",
                [staged],
                |row| row.get(0),
            )
            .expect("read staged generation through SQLite page cache");
        assert!(staged_payload_bytes >= 256 * 1_024);
        let before_activation = sqlite_cache_bytes(&store.conn);
        assert!(
            before_activation >= 256 * 1_024,
            "fixture must populate a meaningful SQLite page cache, observed {before_activation} bytes"
        );

        let fence = ActivationFence::new();
        store
            .activate_staged_generation(&plan, staged, ITEMS, &fence, fence.capture())
            .expect("activate staged generation");

        let after_activation = sqlite_cache_bytes(&store.conn);
        assert!(
            after_activation <= 64 * 1_024,
            "post-activation lifecycle must release SQLite's replace-time page cache: before={before_activation}, after={after_activation}"
        );
        assert_eq!(
            store
                .load_count_cancellable(&plan, &AtomicBool::new(false))
                .expect("load active count after cache release"),
            Some(ITEMS),
            "cache release must leave the writer connection usable"
        );

        for content_path in generation_content_db_paths(&db_path) {
            let _ = remove_sqlite_database(&content_path);
        }
        let _ = remove_sqlite_database(&db_path);
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
    fn incremental_mutations_keep_the_active_generation_and_storage_bounded() {
        let db_path = temp_db_path("incremental-storage-bounded");
        let root = PathBuf::from("root");
        let nested_root = root.join("nested");
        let scan_plan = ScanPlan::new(&SearchConfig {
            include_roots: vec![root.clone(), nested_root.clone()],
            include_hidden_system: true,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            ..SearchConfig::default()
        })
        .expect("valid plan");
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        let mut initial = (0..255)
            .map(|index| owned(&root, &format!("items/{index}.txt"), index))
            .collect::<Vec<_>>();
        initial.push(owned(&nested_root, "marker.txt", 255));
        let initial = store
            .commit(&scan_plan, IndexMutation::ReplaceAll(initial))
            .expect("initial generation");

        for revision in 0..128 {
            store
                .commit(
                    &scan_plan,
                    IndexMutation::Delta {
                        upserts: vec![owned(&root, "items/0.txt", 10_000 + revision)],
                        remove_trees: Vec::new(),
                    },
                )
                .expect("bounded delta");
        }
        for revision in 0..128 {
            store
                .commit(
                    &scan_plan,
                    IndexMutation::ReplaceRoot {
                        root: nested_root.clone(),
                        items: vec![owned(&nested_root, "marker.txt", 20_000 + revision)],
                    },
                )
                .expect("bounded root replacement");
        }
        let active = store
            .load(&scan_plan)
            .expect("load")
            .expect("active generation");
        let generations: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM search_generations", [], |row| {
                row.get(0)
            })
            .expect("count generations");
        let freelist_pages: i64 = store
            .conn
            .query_row("PRAGMA freelist_count", [], |row| row.get(0))
            .expect("freelist pages");

        assert_eq!(active.generation(), initial.generation());
        assert_eq!(active.items().len(), 256);
        assert_eq!(generations, 1);
        assert!(
            freelist_pages <= 1,
            "bounded FTS delta maintenance may leave at most one reusable page"
        );
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn content_policy_controls_payloads_and_invalidates_prior_generations() {
        let db_path = temp_db_path("content-policy");
        let root = std::env::temp_dir().join(format!(
            "ottrin-content-policy-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let excluded_dir = root.join("excluded");
        std::fs::create_dir_all(&excluded_dir).expect("create content roots");
        std::fs::write(root.join("included.txt"), "included-needle").expect("write included");
        std::fs::write(excluded_dir.join("excluded.txt"), "excluded-needle")
            .expect("write excluded");
        let mut enabled = SearchConfig {
            include_roots: vec![root.clone()],
            include_hidden_system: true,
            content_indexing_enabled: true,
            content_include_roots: vec![root.clone()],
            content_exclude_roots: vec![excluded_dir],
            ..SearchConfig::default()
        };
        let enabled_plan = ScanPlan::new(&enabled).expect("enabled plan");
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &enabled_plan,
                IndexMutation::ReplaceAll(vec![
                    owned(&root, "included.txt", 16),
                    owned(&root, "excluded/excluded.txt", 16),
                ]),
            )
            .expect("commit enabled content");
        assert_eq!(
            query_indexed_content_page(&db_path, &enabled_plan, "\"included-needle\"", 10, 0)
                .expect("query included")
                .len(),
            1
        );
        assert!(
            query_indexed_content_page(&db_path, &enabled_plan, "\"excluded-needle\"", 10, 0)
                .expect("query excluded")
                .is_empty()
        );

        enabled.content_indexing_enabled = false;
        let disabled_plan = ScanPlan::new(&enabled).expect("disabled plan");
        assert_ne!(enabled_plan.identity(), disabled_plan.identity());
        assert!(matches!(
            store.load(&disabled_plan),
            Err(SearchDbError::PlanIdentityMismatch)
        ));
        store
            .commit(
                &disabled_plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "included.txt", 16)]),
            )
            .expect("commit disabled content");
        assert!(
            query_indexed_content_page(&db_path, &disabled_plan, "\"included-needle\"", 10, 0)
                .expect("query disabled")
                .is_empty()
        );
        let generation: i64 = store
            .conn
            .query_row(
                "SELECT active_generation_id FROM search_store_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .expect("active generation");
        let content_conn = Connection::open(generation_content_db_path(&db_path, generation))
            .expect("open disposable content database");
        let content_state_rows: i64 = content_conn
            .query_row(
                "SELECT COUNT(*) FROM search_generation_content_state",
                [],
                |row| row.get(0),
            )
            .expect("count explicit policy state");
        assert_eq!(content_state_rows, 1);
        let _ = std::fs::remove_dir_all(root);
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
    fn direct_delta_updates_only_changed_rows_and_preserves_subtree_semantics() {
        let db_path = temp_db_path("generation-direct-delta");
        let root = PathBuf::from("root");
        let scan_plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![
                    owned(&root, "tree", 0),
                    owned(&root, "tree/child.txt", 1),
                    owned(&root, "tree/nested", 0),
                    owned(&root, "tree/nested/grandchild.txt", 2),
                    owned(&root, "keep.txt", 3),
                ]),
            )
            .expect("initial generation");
        let fence = ActivationFence::new();
        let expected_epoch = fence.capture();
        let mut published_count = None;

        store
            .commit_delta_guarded_then(
                &scan_plan,
                &[owned(&root, "keep.txt", 4), owned(&root, "new.txt", 5)],
                &[root.join("tree"), root.join("keep.txt")],
                &fence,
                expected_epoch,
                |count| published_count = Some(count),
            )
            .expect("direct delta");

        let snapshot = store.load(&scan_plan).expect("load").expect("snapshot");
        assert_eq!(published_count, Some(2));
        assert_eq!(
            snapshot.items(),
            &[owned(&root, "keep.txt", 4), owned(&root, "new.txt", 5)]
        );
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn direct_delta_holds_activation_fence_through_publication() {
        let db_path = temp_db_path("generation-direct-delta-publication-fence");
        let root = PathBuf::from("root");
        let scan_plan = plan(&root);
        let mut store = SearchStore::open(db_path.clone()).expect("open store");
        store
            .commit(
                &scan_plan,
                IndexMutation::ReplaceAll(vec![owned(&root, "old.txt", 1)]),
            )
            .expect("initial generation");
        let fence = Arc::new(ActivationFence::new());
        let expected_epoch = fence.capture();
        let (publication_started_tx, publication_started_rx) = std::sync::mpsc::channel();
        let (invalidation_finished_tx, invalidation_finished_rx) = std::sync::mpsc::channel();
        let invalidating_fence = Arc::clone(&fence);
        let invalidator = std::thread::spawn(move || {
            publication_started_rx
                .recv()
                .expect("publication start signal");
            invalidating_fence.invalidate();
            invalidation_finished_tx
                .send(())
                .expect("invalidation completion signal");
        });

        store
            .commit_delta_guarded_then(
                &scan_plan,
                &[owned(&root, "new.txt", 2)],
                &[root.join("old.txt")],
                &fence,
                expected_epoch,
                |_| {
                    publication_started_tx
                        .send(())
                        .expect("publication start signal");
                    assert!(
                        invalidation_finished_rx
                            .recv_timeout(std::time::Duration::from_millis(100))
                            .is_err(),
                        "activation invalidation must block until publication finishes"
                    );
                },
            )
            .expect("direct delta");
        invalidation_finished_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("invalidation finishes after publication");
        invalidator.join().expect("invalidation thread");
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
