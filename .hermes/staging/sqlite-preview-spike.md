# SQLite preview API and provenance spike

- `rusqlite 0.31.0` is already workspace-resolved through `ottrin-search`.
  Cargo.lock records crates.io source and checksum
  `b838eba278d213a8beaf485bd313fd580ca4505a00d5871caeb1457c55322cae`;
  its crate license is MIT.
- The existing `bundled` feature selects `libsqlite3-sys 0.28.0` (MIT;
  Cargo.lock checksum
  `0c10584274047cb335c23d3e61bcef8e323adae7c5c8c760540f73610177fc3f`)
  and builds the bundled SQLite 3.45.0 amalgamation on Linux and Windows.
  SQLite itself is public domain. No new external binary, process, or runtime
  download is introduced.
- Enabled APIs: `serialize` exposes `Connection::deserialize`; `hooks`
  exposes a progress handler and authorizer; `limits` exposes VM, memory,
  SQL, column, expression, recursion, attachment, and worker limits. The
  bundled feature enables `modern_sqlite`, including defensive and
  trusted-schema connection settings.
- The selected implementation reads one capped snapshot from the source file,
  verifies its header/page arithmetic and rollback-journal mode, copies it to
  SQLite-owned memory, and deserializes it as read-only. SQLite never opens
  the source path, a WAL/journal sidecar, or an attachment. The only query is
  the fixed `sqlite_schema` type/name query, guarded by query-only, defensive,
  trusted-schema-off, disabled triggers/FTS tokenizer, an authorizer, and a
  progress budget; it never evaluates schema SQL, views, triggers, user
  functions, or table rows.
