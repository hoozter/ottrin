# Engineering Conventions

## Logging
1. Use structured logs with clear operation context (`pane`, `path`, `job_id`).
2. Avoid logging full directory contents in release builds.
3. Log user-triggered failures at warning level and internal invariants at error level.

## Error Policy
1. Prefer typed errors in core/platform crates.
2. Convert low-level errors into user-visible messages only at UI boundary.
3. Keep actionable text in UI status lines (what failed + next action).
4. Avoid panics in user flows; return recoverable errors.

## Performance Discipline
1. Never perform filesystem operations on UI thread when avoidable.
2. Use background workers for directory listing and preview loading.
3. Guard UI rendering with sensible caps for very large folders.
4. Track startup time on every launch and regress only with explicit tradeoff.
