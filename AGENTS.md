# Ottrin — project instructions

Read and follow `/home/campbell/.codex/AGENTS.md` first. This file adds only project-specific requirements.

## Product wording

- Describe Ottrin as a desktop file manager with fast local search, a choice of views, and built-in previews. Mention keyboard shortcuts and the command bar as features. Use platform names for specific downloads and releases.
- Call the layout **Column view**. In descriptions, explain that it keeps parent folders visible side by side. Check the website, README, release drafts, and package metadata for consistent wording.

## Linux and Windows delivery

- Treat UI changes as Linux and Windows by default. Before calling a shared UI change delivered, build both candidates from the same clean commit and verify the affected workflow on both platforms. Batch related changes into one candidate per platform. For a Linux-specific change, record why Windows is unaffected. If either platform is blocked, report the pending verification and the revision its current package still uses.
- Build and verify the Linux AppImage, portable tarball, DEB and RPM from the same clean commit and publish them together. If a format is blocked, report its current revision and the blocker before calling the Linux package set current.

## Local AppImage delivery

- Whenever updating the AppImage, also update the user's installed AppImage in the same task. The current desktop launchers directly reference `/home/campbell/Projects/Ottrin/dist/packages/Ottrin-0.1.0-x86_64.AppImage`, so this stable artifact is also the installed target; do not create a second copy. If the launcher target changes, resolve it and atomically update that target after verification. Before handoff, verify the installed launcher target matches the current accepted AppImage checksum.
- Keep desktop launcher window identity consistent with `org.ottrin.Ottrin`. Preserve user launcher customizations and do not restart existing windows, portal or index services merely to update the artifact.

## Project record (.evidence/CURRENT.md)

- Maintain one discoverable project record at `.evidence/CURRENT.md`. This record replaces per-task `.evidence/` receipt accumulation; do not add per-task receipts. Never recreate a retired agent's state directory to hold project evidence.
- Read `.evidence/CURRENT.md` before diagnosing or replacing installed builds or desktop integrations. Unrelated small tasks do not need to read it.
- The record contains, at minimum: the installed artifact path, checksum, and source identity; what was meaningfully verified; known limitations, unresolved issues, and selective support links; and recovery backups with their purpose and deletion condition. It must not make broad promises requiring follow-up.
- Update the record only after a meaningful, accepted build or state change. Do not create evidence for routine edits, discussions, successful ordinary checks, or per-task narrative cleanup receipts. Long-term knowledge belongs in regression tests and maintained documentation, not in this record.
- Add supporting assets under `.evidence/` only when the summary is insufficient and there is an explicit future consumer; prune superseded assets after their replacement is accepted, while preserving needed rollback and user backups. Never retain assets automatically by task name or date.

## Build and QA storage

- Reuse `target/`, `~/.cargo` and `~/.rustup`. Use `scripts/test-workspace.py` for disposable HOME/XDG state; isolated tests must share compiler caches. Keep current deliverables at the stable `dist/` paths and keep `.evidence/CURRENT.md` current as specified above.
- Register teardown when allocating QA resources. On success, failure, timeout or cancellation, stop owned processes/displays/buses/wallet helpers and release mounts before removing owned profiles, runtime extractions and staging. Fix missing teardown in the producer.
- Keep build flags/features consistent. Before substantial builds, note sizes of native and Windows `debug/{deps,incremental}` and the task's staging/evidence roots. Extra target trees or configurations need an explicit purpose and cleanup owner.
- **Before handoff, finish cleanup:** compare those paths with the baseline; remove completed task fixtures and superseded packages/test executables, including obsolete Cargo fingerprints/dep-info. Verify a newer test replacement with matching compiler settings; check references, consumers and hard links. Preserve current binaries, dependencies and useful caches; never blanket `cargo clean`.
- Ottrin development and test builds disable incremental compilation in `.cargo/config.toml`. Use the local Cargo launcher and packaging entry points so the disk reserve is enforced. For any explicitly requested incremental session, use `/home/campbell/.codex/skills/computer-cleanup/scripts/rust_incremental.py` on exact incremental roots: inspect candidates, then use `--apply` once consumers are clear. It retains the newest finalized session per crate/configuration and checks locks. Investigate refusals; do not bypass them.
- Clean owned disposable VMs through libvirt after checking snapshots/backing chains. Preserve the Windows base and explicitly retained `ottrin-copy-cleanup-qa`, `ottrin-index-qa`, and `ottrin-qa-20261011` environments.

When a task actually allocates disposable resources, its handoff states concisely what cleanup was performed and any remaining task-created storage: exact path, purpose, pending consumer or blocker. Routine edits and discussions need no cleanup statement; report only a blocking cleanup issue when one exists. A successful build/test alone does not complete the task; do not leave verified disposable output for a later machine-wide cleanup.
