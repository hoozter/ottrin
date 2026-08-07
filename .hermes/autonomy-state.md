# Arthur · Ottrin V1 State

Status: native-only video-thumbnail Phase 1 is authorized: Windows Media Foundation first, with no bundled or external decoder/runtime; all other current dependency-ready local work remains exhausted
Repository: `/home/campbell/.hermes/worktrees/Ottrin-arthur`
Branch: `arthur/ui-followups`
Accepted base: `5f2d929 Keep folder summary boundary warning-free`

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

## Artifact evidence

Built from `0a142d6` before the test-only `8c706ad` commit:
- `dist/Ottrin-0.1.0-linux-x86_64-portable.tar.gz`
  SHA-256 `e2a55d3423190c51ccb3a988de176ab0487b60ff2d0242dec14435a1fd7f3603`
- `dist/Ottrin-0.1.0-x86_64.AppImage`
  SHA-256 `16d014bea4fa471ea07af8c3ade5f13cd49141d9533956ae5ab668147ad48346`

Those pre-correction checksums are historical only. Final-current Linux portable,
Linux AppImage, Windows portable, and Windows NSIS candidates have since passed
the strict artifact verifier, Linux Xvfb launch smoke, packaged-PDFium probes,
and Wine app-local OpenGL compatibility smoke. The verifier must show each
artifact's `SOURCE_REVISION.txt` equals live HEAD and embedded release documents
match the worktree; exact current hashes are recorded with the kanban evidence.

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

## Video-thumbnail native-only Phase 1

- `.hermes/video-thumbnail-policy-approved.md` supersedes the earlier
  FFmpeg-first spike. Phase 1 uses only OS-supplied decoding: Windows Media
  Foundation is the first backend, with no bundled, downloaded, recommended,
  or invoked FFmpeg, GStreamer, codec/plugin payload, codec pack, external
  decoder, or additional codec implementation.
- The common contract permits one bounded still frame, no playback/audio,
  network, sidecars, plugins, executable lookup, or path reopen. It requires
  no-follow regular-file/handle admission, cancellation/resource/dimension/
  output limits, generation-safe cache admission, and deterministic unavailable
  results for unsupported or malformed media.
- Linux and other platforms remain metadata-only until a separately reviewed
  OS-native backend exists. Test doubles and Linux success cannot establish
  Windows acceptance. Native Windows acceptance still requires tests for
  reparse/file identity, cancellation, malformed input, resource bounds,
  capability reporting, cache invalidation, unsupported codecs, and packages
  containing no decoder/runtime/codec payload or installer.

The remaining deferred candidates each require a real product/architecture decision before source changes: faithful/advanced Office scope; native non-Linux Open With discovery; media playback UX/backend; fanotify service/package security model; generalized undo safety contract; and new native package/signing targets. No current source item is dependency-ready without selecting one of those contracts.

## Exact next action

This safety-and-policy baseline does not implement or stub Media Foundation.
The next separately authorized source slice is a Windows Media Foundation backend
under the approved native-only contract, followed by required native
Windows/package acceptance. V1 physical/CI/release gates remain blockers for
release acceptance.