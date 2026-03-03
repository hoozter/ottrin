# Crash and Error Reporting Strategy

## Runtime behavior
1. Unrecoverable panic: write crash report to disk before process exit.
2. Recoverable operation failures: show actionable status in UI and keep app responsive.

## Crash report contents
1. App version and git commit (if available).
2. OS + architecture.
3. Last user action and command payload (sanitized).
4. Panic message and stack trace.

## Storage paths
- Linux: `${XDG_STATE_HOME:-~/.local/state}/ottrin/crash/`
- Windows: `%LOCALAPPDATA%/Ottrin/crash/`
