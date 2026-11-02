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
The former verifies the current delivered Linux source. The superseded source4d
run41394641644 was canceled after its actual native package failed startup.
Source2ebf [run41400086555](https://github.com/hoozter/ottrin/actions/runs/41400086555)
passes Linux/performance, but the Windows parallel debug functional suite fails
its500k timing check at320ms against250ms. The native MSIX startup repair now
passes its basic package lifecycle; login and cross-version update remain.

The performance follow-up retains500k correctness in debug and moves strict
release100ms enforcement into an exact serial test on both platforms. The first
local Windows release check failed140ms. The final reusable literal matcher
passes at78ms on the same native client and20ms on Linux; all244 Linux search
tests and strict lint on both platforms pass. The100ms budget remains unchanged.

The CI workflow now groups by workflow/ref and cancels superseded runs, following
[GitHub's concurrency mechanism](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency).
The initial concurrency change preserved all checks. The subsequent performance
change adds a Windows release gate. Existing
receipt-only path exclusions remain narrow; no account notification settings are
changed. Required CI failures must be investigated before piling more independent
feature pushes onto the failing source, and reported with the exact failing gate.

Evidence consists of run/job metadata, compact failure extracts and the current
run snapshot. No fixtures, package extractions or test processes were allocated
by this audit. Publication and hosted validation of the workflow follow-up must
be recorded after its commit, rather than inferred from this local check.

The sourcea10792db follow-up completes all four jobs successfully in
[run41413618813](https://github.com/hoozter/ottrin/actions/runs/41413618813).
Native hosted release measurements are16ms Linux and61ms Windows against100ms.
This resolves the observed current CI failure scope; it does not explain unseen
mail messages or accept later source revisions automatically.

The later76bed follow-up run41423617027 exposes a Linux listing-wake test race
(`listing already published at wake: Empty`), while both release performance jobs
pass. A constructor/barrier wake could arrive after the observer bound the shared
context. The actual worker already publishes before wake. A shared production
worker factory lets the test observe only that worker;454 Linux UI tests and
strict Linux/Windows lint pass locally without sleeps, retries or weaker timing
assertions. Publication/hosted confirmation follows the correction commit.
