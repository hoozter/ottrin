# Arthur · Ottrin V1 State

Status: blocked on physical QA and product-scope decisions after all current dependency-ready local work
Repository: `/home/campbell/.hermes/worktrees/Ottrin-arthur`
Branch: `arthur/ui-followups`
Accepted base: `78bc42df7a96960261a0bad88fd8e9109da796fc`

## Independently accepted source range

- `0a142d6 Validate creation names at platform boundary`
  - Root cause: `DefaultPlatform` accepted `FileCommand::CreateFile` and `CreateFolder` names with path separators, relying solely on UI preflight. A helper/direct command could therefore create below the requested parent.
  - Correction: one platform validation source is used by both UI preflight and actual execution; file and folder creation now reject non-component names before mutation. The existing exclusive create/create-dir primitives retain no-overwrite behavior.
  - RED observed: `cargo test -p ottrin-platform creation_commands_reject_names_that_escape_the_requested_parent` failed because `nested/created.txt` was created.
  - GREEN/final source evidence: focused platform/UI tests and `./scripts/release-preflight.sh --source-only` passed (format, workspace check/tests, strict Clippy, shell/package contracts, AppStream/XML). ShellCheck and actionlint are not installed, so their optional checks were skipped.
- `8c706ad Serialize packaged PDFium tests`
  - Root cause: the four ignored packaged-PDFium integration probes ran concurrently despite PDFium's process-global runtime, producing SIGSEGV in the batch run.
  - Correction: the test-only probes share a process mutex. The exact four-test ignored batch now passes against the freshly packaged PDFium runtime.

## Corrected source boundary

The earlier exact-diff review found these required corrections; they are now
implemented in new commit `d2ab158 Harden creation and PDFium corrections`:

1. The Windows reserved-device predicate rejects documented superscript forms `COM¹`, `COM²`, `COM³`, `LPT¹`, `LPT²`, and `LPT³`, including extensions, with portable predicate coverage and Windows-boundary creation coverage.
2. Trust-boundary behavioral coverage makes `../external` CreateFile and CreateFolder attempts from an isolated requested parent and proves an external sibling remains empty.
3. `PACKAGED_PDFIUM_TEST_LOCK` now recovers deliberately from poison via a named helper, with regression coverage.

Do not amend the immutable reviewed commits. An independent current-source review
accepted the exact range `78bc42d..e063d0b`, including `0a142d6`, `8c706ad`,
`6dfd058`, `d2ab158`, and `e063d0b`. Artifacts built before `d2ab158` remain
superseded and are not evidence for the corrected source.

## Completed post-V1 slice

- `6dfd058 Summarize recursive folder size`
  - Quick Inspector folder metadata now reports recursive regular-file size.
    The scan is bounded to 16,384 entries and 64 levels, never follows
    symlinks, and marks its value as an incomplete lower bound on unreadable,
    too-deep, or capped trees.
  - RED observed: the nested-folder fixture reported only `Known direct file
    size: 3 B` and omitted the nested two-byte file.
  - GREEN evidence: `cargo fmt --check`; `CARGO_TARGET_DIR=/tmp/ottrin-preview-folder-size-red cargo test -p ottrin-preview` (139 unit tests passed, 4 packaged-PDFium tests ignored); and `CARGO_TARGET_DIR=/tmp/ottrin-preview-folder-size-clippy cargo clippy -p ottrin-preview --all-targets -- -D warnings` passed.

## Superseded artifact evidence

Built from `0a142d6` before the test-only `8c706ad` commit:
- `dist/Ottrin-0.1.0-linux-x86_64-portable.tar.gz`
  SHA-256 `e2a55d3423190c51ccb3a988de176ab0487b60ff2d0242dec14435a1fd7f3603`
- `dist/Ottrin-0.1.0-x86_64.AppImage`
  SHA-256 `16d014bea4fa471ea07af8c3ade5f13cd49141d9533956ae5ab668147ad48346`

These pre-correction artifacts are retained only as historical evidence and are
not release candidates. Final-current artifacts must embed the commit containing
this state and the corrected release documentation.

## Remaining V1 boundary

Implementation/source gates for Storage Locations and first-class Create workflows are present and covered, and the user-file mutation boundary has independent source acceptance. Remaining release blockers require physical Linux desktop workflow and polkit smoke, attachment/removal behavior, real Windows drive/share and package/UAC testing, CI runs, signing/publication decisions, and release-owner artifact review. No source scope is currently authorized to replace those gates.

## Evidence for `d2ab158`

- Focused parent-traversal non-mutation and superscript-name predicate tests passed.
- `cargo fmt --all --check`, host and Windows-target `cargo check` for platform/preview/UI, packaged PDFium `--include-ignored --test-threads=4` (6/6), `cargo test --workspace`, strict workspace Clippy, and `git diff --check` passed.

## Reconciled correction boundary

- The corrections named by the standing goal are already committed at
  `d2ab158 Harden creation and PDFium corrections`; the live worktree contains
  no uncommitted source diff in `crates/ottrin-platform/src/lib.rs` or
  `crates/ottrin-preview/tests/pdf_visual.rs`.
- Current-HEAD focused verification passed: `cargo fmt --all --check`; the
  external-sibling non-mutation test; the portable superscript-device predicate
  test; the poison-recovering PDFium test-lock test; Windows-target checks for
  `ottrin-platform` and `ottrin-preview`; and `git diff --check`.
- `d2ab158` is included in the independently accepted source range. No reviewed
  history was amended.

## Documentation correction

- `e063d0b Correct thumbnail limitations` brings `docs/USER_GUIDE.md` into line with the existing generated thumbnail surface: text/code, PDF, Office, font, SQLite, mesh, camera-RAW, bounded Affinity, and folder-collage thumbnails are present; video thumbnails, media playback, HEIC/HEIF thumbnails, and faithful Office layout remain future work.
- The final range `0a142d6..e063d0b` passed independent exact-diff review. Because documentation entering an artifact changed, pre-correction artifacts are not evidence for final HEAD.

## Current roadmap scan and blockers

All source-level V1 work listed in `docs/ROADMAP.md` is implemented; the remaining V1 items require physical Linux/Windows/polkit/removable-media QA, CI, release-owner review, signing, or artifact publication.

The remaining deferred candidates each require a real product/architecture decision before source changes: faithful/advanced Office scope; video thumbnails (decoder/backend, sandboxing, licensing and packaged-runtime contract); native non-Linux Open With discovery; media playback UX/backend; fanotify service/package security model; generalized undo safety contract; and new native package/signing targets. No current source item is dependency-ready without selecting one of those contracts.

## Exact next action

Rebuild and verify all final-current artifacts after committing this corrected
release claim. Then select and authorize a concrete deferred product contract
(recommended first decision: video-thumbnail decoder/backend, packaging and
safety model) or provide the unavailable physical QA. V1 physical/CI/release
gates remain blockers for release acceptance.
