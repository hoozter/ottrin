# Ottrin autonomy state

Updated: 2026-08-26T03:08:55+02:00
Status: paused
Model route: balanced
Repository: /home/campbell/Projects/Ottrin
Branch: main
HEAD: d399451682411a28e5f93db0c24aa76af3670ae1
Tracked dirty files: .github/workflows/release.yml, .gitignore, .hermes/autonomy-state.md, .hermes/video-thumbnail-policy-approved.md, Cargo.lock, README.md, build-all.sh, crates/ottrin-app/Cargo.toml, crates/ottrin-app/src/main.rs, crates/ottrin-core/Cargo.toml, crates/ottrin-core/src/lib.rs, crates/ottrin-platform/Cargo.toml, crates/ottrin-platform/src/bin/ottrin-priv-helper.rs, crates/ottrin-platform/src/copy_transaction.rs, crates/ottrin-platform/src/lib.rs, crates/ottrin-platform/src/move_recovery.rs, crates/ottrin-platform/src/move_transaction.rs, crates/ottrin-preview/Cargo.toml, crates/ottrin-preview/src/affinity_visual.rs, crates/ottrin-preview/src/lib.rs, crates/ottrin-preview/src/video_thumbnail.rs, crates/ottrin-preview/tests/video_thumbnail_contract.rs, crates/ottrin-search/Cargo.toml, crates/ottrin-search/src/daemon.rs, crates/ottrin-search/src/db.rs, crates/ottrin-search/src/lib.rs, crates/ottrin-search/src/plan.rs, crates/ottrin-search/src/scanner.rs, crates/ottrin-search/src/worker.rs, crates/ottrin-ui/Cargo.toml, crates/ottrin-ui/src/config.rs, crates/ottrin-ui/src/lib.rs, crates/ottrin-ui/src/preview_render.rs, crates/ottrin-ui/src/thumbnail_cache.rs, docs/DEVELOPMENT.md, docs/MOVE_RECOVERY_DESIGN.md, docs/RELEASE_CHECKLIST.md, docs/RELEASE_NOTES.md, docs/ROADMAP.md, docs/USER_GUIDE.md, docs/VIDEO_THUMBNAIL_SPIKE.md, packaging/linux/bundle-runtime-libs.sh, packaging/linux/org.ottrin.filesystem.policy, packaging/linux/package-appimage.sh, packaging/linux/package-portable.sh, packaging/tests/verify-linux-index-daemon-contract.sh, packaging/tests/verify-release-checklist-provenance.sh, packaging/tests/verify-release-preflight-contract-coverage.sh, packaging/tests/verify-release-preflight-shell-coverage.sh, packaging/tests/verify-release-provenance.sh, packaging/tests/verify-release-workflow-dependencies.sh, packaging/tests/verify-windows-installer-index-lifecycle.sh, packaging/verify-release-artifacts.sh, scripts/release-preflight.sh
## Authority
The roadmap and release checklist make Windows Move the first remaining release outcome. Windows cross-volume directory trees containing symbolic links for Skip and Rename are now source-complete, while non-symbolic reparse points remain rejected. The completed top-level symbolic-link Overwrite and nested-link slices must not be replayed. Native Windows crash, ACL, sharing-mode, and packaged acceptance remain separate final gates.

## Just completed
EPS now reaches the existing bounded, non-networked Ghostscript renderer from the UI preview worker and Quick Inspector. The renderer remains optional and host-supplied; its established metadata/external-open fallback remains authoritative on unavailable or failed capability. Quick Inspector treats EPS as a visual preview with Fit/100%/zoom controls rather than silently routing it to metadata only.

The typed Excel grid no longer claims that a bounded blank column was omitted when it is visibly rendered between populated columns. Sparse-range safety limits remain authoritative: the UI suppresses only a short-gap omission that its own grid expands into visible blanks; genuinely hidden ranges remain disclosed.

## Evidence actually run
- RED: `cargo test -p ottrin-ui eps_quick_inspector_uses_the_isolated_visual_preview_path -- --nocapture` failed because EPS was excluded from the Quick Inspector visual path.
- GREEN: the same focused test passed after routing EPS through the preview worker and visual inspector path.
- EPS isolation: `cargo test -p ottrin-preview --test eps_visual -- --nocapture` passed against installed Ghostscript, bubblewrap, and systemd-user isolation.
- Regressions: `cargo test -p ottrin-ui quick_inspector -- --nocapture` passed 17 tests; three existing FIEMAP-dependent text-edit fixtures reported their host skip. `cargo test -p ottrin-preview eps -- --nocapture` passed 8 EPS-selected tests across unit/integration targets.
- Build and source gates: `cargo build -p ottrin-app`, `cargo fmt --all --check`, and `git diff --check` passed.
- Real rendered verification: a disposable Xvfb session with isolated state/cache/config paths and no restored desktop variables launched `target/debug/ottrin-app` directly into `Ottrin Quick Inspector — fixture.eps`. The active window rendered the EPS artwork, exposed Fit/100%/zoom controls, retained metadata, had empty app stderr, and closed on Escape. The transient window-only PNG was SHA-256 `be9474ca1e8b39be3bf168dc63fea45ab28d647f1411d1a42b4f9d6b0036b16d` (720x560).
- Excel RED/GREEN: `cargo test -p ottrin-ui spreadsheet_grid_does_not_report_a_visible_blank_column_as_omitted -- --nocapture` initially failed on the contradictory omission notice, then passed after the presentation correction. `cargo test -p ottrin-ui spreadsheet_grid -- --nocapture` passed both spreadsheet-grid tests, and `cargo test -p ottrin-preview xlsx -- --nocapture` passed 9 typed-XLSX safety/extraction tests.
- Excel real rendered verification: a disposable Xvfb Quick Inspector opened a generated A/C worksheet with the visible A, B, and C headers and Name/Score data. Before the correction it displayed duplicate `Columns B–B omitted` notices; after, it displayed no false omission notice, had empty app stderr, and closed on Escape. Fixed window-only PNG SHA-256: `41c3794c293b4e9355494d774ad574e765790c22d53ea32e3064d7b78d39531b`.

## Current outcome
Legacy `.doc` files now enter the existing optional installed-LibreOffice visual Office provider when it is available. `.doc` has an explicit typed format and sandbox input mapping (`/input/source.doc`); the accepted DOCX path and the truthful catdoc/metadata/external-open fallback remain unchanged.

## Evidence actually run
- RED: `cargo test -p ottrin-preview --test office_provider_contract production_provider_format_mapping_accepts_only_the_bounded_office_contract -- --exact` failed because `.doc` mapped to `None`; the same test passed after the provider change.
- Provider contract: `cargo test -p ottrin-preview --test office_provider_contract` passed all 14 tests, including installed-LibreOffice capability detection, sandboxing, timeout, cancellation, output validation, fallback, and legacy-doc extraction.
- Focused regression: `cargo test -p ottrin-preview office_provider` passed across all relevant targets.
- Source gates: `cargo fmt --all --check`, `cargo build -p ottrin-app`, and `git diff --check` passed.
- Real rendered verification was unavailable: `DISPLAY=:0` and a Wayland socket exist, but no `soffice` or `libreoffice` executable is installed. `catdoc` is installed, so the established bounded text fallback remains available.

## Remaining gate
Visual LibreOffice-backed `.doc` rendering still needs a representative real document on a machine where the user has independently installed and enabled LibreOffice. No dependency was installed or bundled during this work.

## Media boundary revalidated
Linux media playback is already source-complete through the optional `ottrin-media-playback` helper linked to the user's system GStreamer. The GUI remains launchable without GStreamer, packages reject bundled framework/plugin/codec payloads, and unavailable capability falls back truthfully with **Open externally** retained.

Focused source evidence passed on this machine: two `ottrin-preview` media-playback tests, the `ottrin-ui` Linux audio/video routing test, `verify-linux-system-media-boundary.sh`, and `verify-linux-native-package-contract.sh`. No `gst-launch-1.0`, `gst-inspect-1.0`, or `gstreamer-1.0` pkg-config capability is installed here, so enabled physical playback and exact-package acceptance remain external release gates; no dependency was installed or bundled.

## HTML and theme boundary revalidated
The accepted offline HTML renderer passed its sanitizer and bounded browser-isolation evidence against this host's installed Google Chrome and bubblewrap runtime. Focused tests covered a real bounded PNG render, local relative stylesheet rendering, disabled script execution, active/external-content removal, traversal rejection, and truthful UI fallback.

Existing syntax highlighting and theme-editor save/import semantics also passed their focused UI tests. No source defect was reproduced and no compensating implementation was added.

## Current outcome
Windows cross-volume directory snapshots now classify symbolic links as link entries rather than traversing them, bind each raw UTF-16 target and link kind into durable manifest evidence, recreate and verify links in destination-local staging, and revalidate source and published trees before identity-pinned cleanup. Skip and Rename directory publication use this path; non-symbolic reparse points remain fail-closed.

## Evidence actually run
- RED: `cargo test -p ottrin-platform windows_directory_manifest_accepts_and_binds_nested_symbolic_links -- --exact` failed because directory manifest evidence had no symbolic-link-kind field. The focused test passed after binding the kind bit into every manifest entry and aggregate digest.
- `cargo test -p ottrin-platform move_transaction::windows_source_tests -- --nocapture` passed all 42 focused source-contract tests, including nested-link manifest binding, snapshot target/kind revalidation, child-first manifest cleanup, and non-symbolic reparse rejection.
- `cargo check -p ottrin-platform --target x86_64-pc-windows-gnu` passed.
- `cargo fmt --all --check` and focused `git diff --check` passed.

## Remaining gate
Native Windows crash, ACL, sharing-mode, second-volume, and packaged acceptance cannot be exercised on this Linux host. The user-facing cross-volume route remains fail-closed until those gates pass.

## First exact next action
On a representative native Windows machine with a real second volume, run the cross-volume directory Skip/Rename matrix with file and directory symbolic links, interruption recovery, hostile ACLs, and sharing conflicts; enable the route only if the durable journal and cleanup evidence survive every case.

## Accessibility and privileged-action boundaries revalidated
Ottrin's accessibility source contract passed the focused UI accessibility tests and a current `ottrin-app` build. The Unix AccessKit/AT-SPI lifecycle is still owned upstream by egui/winit, so native assistive-technology behavior remains a representative desktop QA gate rather than a reason to add a parallel accessibility stack.

The Linux privileged-helper implementation and package boundary also passed focused helper and native-package tests. The release-provenance test had drifted behind the authoritative Move and signing wording in `docs/RELEASE_CHECKLIST.md` and `docs/ROADMAP.md`; its exact contract strings were corrected after reproducing the failure. `verify-release-checklist-provenance.sh`, `verify-linux-native-package-contract.sh`, and `git diff --check` now pass. Interactive polkit allow/cancel behavior remains a packaged Linux desktop QA gate.

## Milestone Report
Report ID: pdf-initial-zoom-20260825
Completed: stable initial width-fit PDF Quick Inspector presentation across cached and refined rasters
Evidence: focused RED/GREEN tests, PDF/Quick Inspector regressions, current app build, formatting/diff gates, and a real window-only KWin/Xvfb render
Safety status: enabled; renderer/cache bounds and multi-page/non-PDF behavior preserved
Next milestone: EPS rendering, active under the coordinator queue
