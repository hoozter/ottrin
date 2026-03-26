# Privileged Feature QA Matrix

## Scope

Validate integrated privilege management UX and behavior across Linux and Windows.

## Linux Checks

1. `pkexec` and helper available:
- open restricted folder
- click `Retry As Administrator…`
- expect folder entries to load without crash

2. denied file operation retry:
- attempt delete/rename/chmod on root-owned target
- expect command frame error + `Retry As Administrator…`
- click retry and verify operation succeeds

3. helper missing:
- unset helper from PATH and unset `OTTRIN_PRIV_HELPER`
- expect status-bar lock icon warning on hover
- expect retry action to fail with clear misconfiguration message

4. policy misconfigured:
- helper present, policy absent/invalid
- expect authentication failure surfaced as error text (no silent failure)

## Windows Checks

1. UAC allow path:
- trigger denied folder/file op
- click `Retry As Administrator…`
- accept UAC prompt
- verify successful completion and refresh

2. UAC cancel path:
- trigger retry, cancel UAC prompt
- verify clear error surfaced; app remains responsive

3. helper missing:
- no `ottrin-priv-helper.exe` in PATH and no `OTTRIN_PRIV_HELPER`
- expect status-bar warning and clear retry error

## UX Guardrails

1. No global root/admin mode toggle.
2. No persistent shell escalation.
3. Every privileged request must be user-triggered and action-scoped.
4. Unsupported platform must show explicit "not available" messaging.

## Regression Checks

1. Normal non-privileged operations still work unchanged.
2. Column/List/Grid rendering unaffected by privilege retries.
3. App starts without helper installed (degraded mode only, no panic).
