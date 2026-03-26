# Ottrin Build Diagnosis

**Status:** 7 errors in `ottrin-search` crate

## Root Causes & Fixes

### Issue 1: PathBuf ↔ SQLite serialization
**Problem:** `PathBuf` doesn't implement `FromSql`/`ToSql` traits for rusqlite.

**Fix:** Convert `PathBuf` to/from `String` in `crates/ottrin-search/src/db.rs`
- Line 73: `path: row.get::<_, String>(0)?.into(),`
- Line 74: `parent_path: row.get::<_, String>(1)?.into(),`
- Lines 123, 170, 198: `.as_str()` when passing to params

### Issue 2: Borrow checker — closure captures
**Problem:** `db_writer` and `new_items` moved into closure at line 265, then borrowed again later.

**Fix:** Use `Arc<Mutex<T>>` or move ownership handling in `crates/ottrin-search/src/lib.rs`
- Wrap `db_writer` in `Arc<Mutex<>>` before closure
- Move `new_items` collection outside closure scope or use `Arc<Mutex<Vec>>`

## Severity
**Low** — all fixable with type conversions and closure scope adjustments.
