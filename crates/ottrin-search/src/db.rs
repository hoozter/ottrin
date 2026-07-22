use crate::mutation::{ActivationFence, GenerationSnapshot, IndexMutation, OwnedItem};
use crate::plan::ScanPlan;
use ottrin_core::{EntryKind, SearchResultItem};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Schema version for the on-disk search database.
pub const SEARCH_DB_VERSION: u32 = 4;

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

CREATE TABLE IF NOT EXISTS search_store_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    active_generation_id INTEGER NOT NULL,
    FOREIGN KEY (active_generation_id) REFERENCES search_generations(id)
);
"#;

#[allow(dead_code)] // Slice 2 storage API is intentionally isolated until worker integration.
pub(crate) struct SearchStore {
    conn: Connection,
}

#[allow(dead_code)]
impl SearchStore {
    pub(crate) fn open(path: PathBuf) -> Result<Self, SearchDbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub(crate) fn load(
        &mut self,
        plan: &ScanPlan,
    ) -> Result<Option<GenerationSnapshot>, SearchDbError> {
        load_generation(&self.conn, plan)
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
    let items = rows.collect::<Result<Vec<_>, _>>()?;
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
    let cache_root = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| ottrin_core::default_home_dir().join(".cache"));
    let dir = cache_root.join("ottrin");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("search.db")
}

fn migrate(conn: &Connection) -> Result<(), SearchDbError> {
    let version: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    match version {
        0..=3 | SEARCH_DB_VERSION => {
            conn.execute_batch(GENERATION_SCHEMA)?;
            conn.execute_batch(
                "DROP TABLE IF EXISTS scan_state;
                 DROP TABLE IF EXISTS roots;
                 DROP TABLE IF EXISTS files;
                 DROP TABLE IF EXISTS meta;",
            )?;
        }
        other => return Err(SearchDbError::UnsupportedVersion(other)),
    }
    if version != SEARCH_DB_VERSION {
        conn.execute("PRAGMA user_version = 4", [])?;
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
}

#[cfg(test)]
mod generation_tests {
    use super::*;
    use crate::mutation::{ActivationFence, IndexMutation, OwnedItem};
    use crate::plan::ScanPlan;
    use ottrin_core::SearchConfig;
    use std::sync::Arc;
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
