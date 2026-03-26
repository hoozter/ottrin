# Ottrin Build Fixes

## Errors to fix

### 1. PathBuf serialization (db.rs)
**Lines 73, 74, 123, 170, 198**

rusqlite doesn't serialize/deserialize `PathBuf` directly. Convert to/from `String`:

**Reads (lines 73-74):**
```rust
path: row.get::<_, String>(0)?.into(),
parent_path: row.get::<_, String>(1)?.into(),
```

**Writes (lines 123, 170, 198):**
```rust
params![
    item.path.to_string_lossy().to_string(),
    item.parent_path.to_string_lossy().to_string(),
    // ... rest
]
```

### 2. Closure borrow issue (lib.rs, lines 248-309)
**Problem:** `db_writer` and `new_items` moved into closure at line 265, then borrowed at lines 293 & 309.

**Solution:** Wrap in `Arc<Mutex<>>` or move the collection outside the closure.

**Recommended fix:**
```rust
// Before the closure, wrap db_writer
let mut db_writer = SearchDbWriter::open(search_db_path()).ok();
let db_writer = Arc::new(Mutex::new(db_writer));

// In the closure:
move |batch| {
    if let Ok(mut writer) = db_writer.lock() {
        if let Some(ref mut w) = *writer {
            let _ = w.insert_batch(&batch);
            // ... rest
        }
    }
    // ... collect new_items outside closure or use Arc<Mutex<Vec>>
}

// After closure completes, use db_writer
if let Ok(mut writer) = db_writer.lock() {
    if let Some(ref mut w) = *writer {
        let _ = w.clear_scan_cursors();
    }
}
```

## Instructions for Arthur

1. Apply the PathBuf fixes to `crates/ottrin-search/src/db.rs` (straightforward string conversions)
2. Apply the closure Arc<Mutex> fix to `crates/ottrin-search/src/lib.rs`
3. Run: `cargo build --release` from the Ottrin root
4. Fix any new errors immediately (keep iterating until clean build)
5. Test build success
6. Report success with commit message and lines changed

**DO NOT** report that you're doing this work — only report back when the build is clean and tested.
