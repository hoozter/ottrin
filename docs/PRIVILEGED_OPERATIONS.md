# Privileged Operations (Linux + Windows)

## Goal

Provide **integrated privilege management** for filesystem actions, without exposing a global root/admin shell inside Ottrin.

## Non-Goals

- No persistent "root mode" toggle in the UI.
- No embedded `sudo su` session.
- No shell-string command escalation for file operations.

## Security Model

- Elevation is **per action** (or short-lived per batch), not app-global.
- User always gets an OS-native auth prompt (polkit/UAC).
- Elevated helper process has a **strict allowlist** of supported operations.
- Main UI process stays unprivileged.

## Architecture

1. `ottrin-ui` requests a privileged file op through `ottrin-platform`.
2. `ottrin-platform` serializes a typed request (JSON) to a helper.
3. Helper runs elevated via OS-native mechanism.
4. Helper validates request and executes allowed filesystem op only.
5. Helper returns typed result/error to UI.

## Linux Plan (v1)

- Helper binary: `ottrin-priv-helper` (root only when invoked by polkit).
- Launcher: `pkexec /path/to/ottrin-priv-helper --request <base64-json>`.
- Policy: install polkit action file (`org.ottrin.filesystem.policy`) during packaging.
- Helper allowlist:
  - list directory
  - create/delete/rename/move/copy
  - chmod/chown (optional, explicit)
- Helper denylist:
  - arbitrary shell
  - arbitrary process spawning
  - network operations

## Windows Plan (v1)

- Helper binary: `ottrin-priv-helper.exe`.
- Launcher: `ShellExecuteEx(..., "runas", helper.exe, ...)` (UAC prompt).
- Communication: request blob via arg/file + result via temp file/stdout.
- Same allowlist/denylist as Linux.

## UX Rules

- On `PermissionDenied`, show:
  - reason
  - affected path
  - action button: `Retry As Administrator…`
- Show clear badge while a privileged operation is in progress.
- Never hide elevated actions behind raw shell commands.
- If helper/policy is unavailable, show explicit degraded-mode messaging and keep app functional.

## Delivery Phases

1. Define shared request/response schema (`ottrin-core`).
2. Add `execute_privileged(command)` API in `ottrin-platform`.
3. Implement Linux helper + `pkexec` path.
4. Implement Windows helper + UAC path.
5. Wire UI retry flow for permission errors.
6. Add telemetry/logging for privileged failures (without secrets).

## Marketing Truth

Only claim "integrated privilege management" after phases 1-5 are shipped.

## Current Implementation Notes

- Typed request/response schema is implemented in `ottrin-core`.
- Linux helper + `pkexec` path is wired.
- Windows UAC launcher path is wired.
- UI retries are implemented for restricted directory listing and denied file operations.
- Packaging + signing + policy deployment are still required for production readiness.
