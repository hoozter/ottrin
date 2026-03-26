use ottrin_core::{EntryKind, SearchResultItem};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

/// Schema version for the on-disk search database.
pub const SEARCH_DB_VERSION: u32 = 2;

/// Draft schema for the SQLite-backed index (metadata + scan state).
/// Content indexing will be added via FTS5 in a later step.
pub const SEARCH_DB_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS roots (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    fs_id TEXT,
    last_scan_unix_secs INTEGER,
    last_scan_completed_unix_secs INTEGER
);

CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    parent_path TEXT,
    name TEXT,
    kind INTEGER,
    size_bytes INTEGER,
    modified_unix_secs INTEGER,
    is_executable INTEGER,
    symlink_target_is_dir INTEGER
);

CREATE TABLE IF NOT EXISTS scan_state (
    id INTEGER PRIMARY KEY,
    root_id INTEGER NOT NULL,
    cursor TEXT,
    last_progress_unix_secs INTEGER,
    FOREIGN KEY(root_id) REFERENCES roots(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS scan_state_root_id_idx ON scan_state(root_id);
"#;

#[derive(Debug, Clone)]
pub struct SearchDb {
    pub path: PathBuf,
}

impl SearchDb {
    pub fn open(path: PathBuf) -> Result<Self, SearchDbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        migrate(&conn)?;
        Ok(Self { path })
    }

    pub fn load_items(&self) -> Result<Vec<SearchResultItem>, SearchDbError> {
        let conn = Connection::open(&self.path)?;
        migrate(&conn)?;
        let mut stmt = conn.prepare(
            "SELECT path, parent_path, name, kind, size_bytes, modified_unix_secs, is_executable, symlink_target_is_dir FROM files",
        )?;
        let rows = stmt.query_map([], |row| {
            let kind: i64 = row.get(3)?;
            Ok(SearchResultItem {
                path: row.get::<_, String>(0)?.into(),
                parent_path: row.get::<_, String>(1)?.into(),
                name: row.get(2)?,
                kind: entry_kind_from_i64(kind),
                size_bytes: row.get(4)?,
                modified_unix_secs: row.get(5)?,
                is_executable: row.get::<_, i64>(6)? != 0,
                symlink_target_is_dir: match row.get::<_, Option<i64>>(7)? {
                    Some(v) => Some(v != 0),
                    None => None,
                },
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn scan_cursors(&self) -> Result<HashMap<PathBuf, PathBuf>, SearchDbError> {
        let conn = Connection::open(&self.path)?;
        migrate(&conn)?;
        let mut stmt = conn.prepare(
            "SELECT roots.path, scan_state.cursor FROM scan_state JOIN roots ON roots.id = scan_state.root_id WHERE scan_state.cursor IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |row| {
            let root_path: String = row.get(0)?;
            let cursor: String = row.get(1)?;
            Ok((PathBuf::from(root_path), PathBuf::from(cursor)))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (root, cursor) = row?;
            out.insert(root, cursor);
        }
        Ok(out)
    }

    pub fn write_snapshot(&self, items: &[SearchResultItem]) -> Result<(), SearchDbError> {
        let mut conn = Connection::open(&self.path)?;
        migrate(&conn)?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM files", [])?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO files (path, parent_path, name, kind, size_bytes, modified_unix_secs, is_executable, symlink_target_is_dir)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for item in items {
                stmt.execute(params![
                    item.path.to_string_lossy().to_string(),
                    item.parent_path.to_string_lossy().to_string(),
                    item.name,
                    entry_kind_to_i64(item.kind),
                    item.size_bytes,
                    item.modified_unix_secs,
                    if item.is_executable { 1 } else { 0 },
                    item.symlink_target_is_dir.map(|v| if v { 1 } else { 0 }),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

pub struct SearchDbWriter {
    conn: Connection,
}

impl SearchDbWriter {
    pub fn open(path: PathBuf) -> Result<Self, SearchDbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn clear_files(&mut self) -> Result<(), SearchDbError> {
        self.conn.execute("DELETE FROM files", [])?;
        Ok(())
    }

    pub fn insert_batch(&mut self, items: &[SearchResultItem]) -> Result<(), SearchDbError> {
        if items.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO files (path, parent_path, name, kind, size_bytes, modified_unix_secs, is_executable, symlink_target_is_dir)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for item in items {
                stmt.execute(params![
                    item.path.to_string_lossy().to_string(),
                    item.parent_path.to_string_lossy().to_string(),
                    item.name,
                    entry_kind_to_i64(item.kind),
                    item.size_bytes,
                    item.modified_unix_secs,
                    if item.is_executable { 1 } else { 0 },
                    item.symlink_target_is_dir.map(|v| if v { 1 } else { 0 }),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_item(&mut self, item: &SearchResultItem) -> Result<(), SearchDbError> {
        self.conn.execute(
            "INSERT INTO files (path, parent_path, name, kind, size_bytes, modified_unix_secs, is_executable, symlink_target_is_dir)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(path) DO UPDATE SET
                parent_path=excluded.parent_path,
                name=excluded.name,
                kind=excluded.kind,
                size_bytes=excluded.size_bytes,
                modified_unix_secs=excluded.modified_unix_secs,
                is_executable=excluded.is_executable,
                symlink_target_is_dir=excluded.symlink_target_is_dir",
            params![
                item.path.to_string_lossy().to_string(),
                item.parent_path.to_string_lossy().to_string(),
                item.name,
                entry_kind_to_i64(item.kind),
                item.size_bytes,
                item.modified_unix_secs,
                if item.is_executable { 1 } else { 0 },
                item.symlink_target_is_dir.map(|v| if v { 1 } else { 0 }),
            ],
        )?;
        Ok(())
    }

    pub fn remove_path_tree(&mut self, path: &PathBuf) -> Result<(), SearchDbError> {
        let path_str = path.to_string_lossy();
        let prefix = format!("{}/%", path_str.trim_end_matches('/'));
        self.conn.execute(
            "DELETE FROM files WHERE path = ?1 OR path LIKE ?2",
            params![path_str.as_ref(), prefix],
        )?;
        Ok(())
    }

    pub fn set_scan_cursor(&mut self, root: &PathBuf, cursor: Option<&PathBuf>) -> Result<(), SearchDbError> {
        let root_id = self.root_id(root)?;
        let cursor_text = cursor.map(|p| p.to_string_lossy().to_string());
        let now = current_unix_secs();
        self.conn.execute(
            "INSERT INTO scan_state (root_id, cursor, last_progress_unix_secs)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(root_id) DO UPDATE SET
                cursor=excluded.cursor,
                last_progress_unix_secs=excluded.last_progress_unix_secs",
            params![root_id, cursor_text, now],
        )?;
        Ok(())
    }

    pub fn clear_scan_cursors(&mut self) -> Result<(), SearchDbError> {
        self.conn.execute("UPDATE scan_state SET cursor = NULL", [])?;
        Ok(())
    }

    fn root_id(&mut self, root: &PathBuf) -> Result<i64, SearchDbError> {
        let root_text = root.to_string_lossy().to_string();
        self.conn
            .execute(
                "INSERT INTO roots (path) VALUES (?1) ON CONFLICT(path) DO NOTHING",
                params![root_text],
            )?;
        let id: i64 = self
            .conn
            .query_row("SELECT id FROM roots WHERE path = ?1", params![root_text], |row| row.get(0))?;
        Ok(id)
    }
}

#[derive(Debug, Error)]
pub enum SearchDbError {
    #[error("sqlite error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported database version: {0}")]
    UnsupportedVersion(u32),
}

pub fn search_db_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    let cache_root = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| ottrin_core::default_home_dir().join("AppData").join("Local"));
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
        0 => {
            conn.execute_batch(SEARCH_DB_SCHEMA)?;
            conn.execute("PRAGMA user_version = 2", [])?;
            Ok(())
        }
        1 => {
            conn.execute("CREATE UNIQUE INDEX IF NOT EXISTS scan_state_root_id_idx ON scan_state(root_id)", [])?;
            conn.execute("PRAGMA user_version = 2", [])?;
            Ok(())
        }
        SEARCH_DB_VERSION => Ok(()),
        other => Err(SearchDbError::UnsupportedVersion(other)),
    }
}

fn current_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
