# GitHub failure notification audit — 2026-11-02

The accessible recent notifications all identify Ottrin CI. Since October31,
49 retained runs include13 failures:11 normal CI and2 isolated installer controls.
There are37 failed runs among88 retained runs across the available history.
Mailbox messages were not accessed, so this does not establish the cause of
hundreds of separate emails.

Every recent failed run was triggered by a push, with one attempt. There is no
scheduled retry loop. Four successive Windows runs repeated the Properties
layout failure while later commits were already queued. Linux failures identified
HTML confinement, portal lifecycle/version, CSS setup and strict-lint defects.
The two diagnostic runs failed respectively on the account-description limit and
hosted Server2025's ordinary MSI policy refusal; that branch is inactive.

Those normal-CI defect scopes subsequently pass in all three jobs of
[run41374248948](https://github.com/hoozter/ottrin/actions/runs/41374248948)
and [run41380442981](https://github.com/hoozter/ottrin/actions/runs/41380442981).
The former verifies the current delivered Linux source. At this observation,
[run41394641644](https://github.com/hoozter/ottrin/actions/runs/41394641644)
passes Linux/performance while Windows is still running Cargo tests.
The separately reproduced native MSIX startup crash remains open.

The CI workflow now groups by workflow/ref and cancels superseded runs, following
[GitHub's concurrency mechanism](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency).
The platform matrix, test steps and failure conditions are unchanged. Existing
receipt-only path exclusions remain narrow; no account notification settings are
changed. Required CI failures must be investigated before piling more independent
feature pushes onto the failing source, and reported with the exact failing gate.

Evidence consists of run/job metadata, compact failure extracts and the current
run snapshot. No fixtures, package extractions or test processes were allocated
by this audit. Publication and hosted validation of the workflow follow-up must
be recorded after its commit, rather than inferred from this local check.
