# Config Versioning and Migration

## Versioning model
1. Add `schema_version` to persisted config.
2. Use monotonically increasing integer versions.

## Migration flow
1. Read raw config.
2. Detect schema version.
3. Apply migration steps sequentially to current version.
4. Persist migrated config atomically.

## Rules
1. Migrations must be deterministic and idempotent.
2. Unknown fields should be ignored when possible.
3. On migration failure, backup original config and start from defaults.
