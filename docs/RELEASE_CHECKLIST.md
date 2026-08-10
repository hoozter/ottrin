# Ottrin v1 Release Checklist

Every item must be checked against the exact artifact intended for release. A source-tree test does not count as packaged desktop validation.

## Automated Gates

- [ ] `cargo fmt --all --check`
- [ ] `cargo check --workspace --all-targets`
- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Linux and Windows CI are green for the release commit.
- [ ] Exact release artifact checksums verify after rebuilding from the current release commit.
- [ ] Archive inspection confirms the exact rebuilt app, indexer, privileged helper, policy/assets, and user documentation are present.
- [ ] Both exact rebuilt Windows artifacts contain the checksum-pinned Mesa 26.1.3/DXC graphics compatibility payload (`opengl32.dll`, `libgallium_wgl.dll`, `libEGL.dll`, `libGLESv2.dll`, and `dxil.dll`) plus complete notices; Automatic selection, persisted preference, explicit overrides, and loaded-DLL provenance pass acceptance.

Historical local gates and artifact inspection do not establish the current
release candidate. Every item above remains unchecked until rerun against the
exact release commit and artifacts.

### Local Preflight Evidence (refreshed 2026-07-31)

- Workspace all-target tests, strict Clippy, and the workspace build passed after
  the bounded PDF and Office visual-preview milestones.
- The checksum-verified packaged Linux PDFium runtime rendered a bounded,
  non-blank first-page PNG; missing runtime remains a fail-closed text/metadata
  fallback.
- The prior strict artifact verification did not require Mesa's app-local
  `opengl32.dll` loader and is invalidated for Windows startup acceptance. The
  corrected verifier requires and hashes that loader in both Windows artifacts.
- A fresh isolated artifact set was rebuilt and reverified on 2026-07-31, including
  the pinned Mesa/DXC payloads and bundled release documentation. This remains
  integrity evidence only; it does not waive the physical acceptance gates.
- AppImage payload inspection confirmed the app, indexer, privileged helper,
  desktop entry, AppStream metadata, and polkit policy.
- The freshly packaged Linux portable executable and AppImage each remained
  running until an eight-second smoke-test timeout, with no startup error.
- The Windows portable app imports `opengl32.dll`; corrected packaging must
  place Mesa's pinned loader beside the executable so Windows does not resolve
  that import to its OpenGL 1.1 system loader on compatibility-path machines.
- The 2026-07-27 Wine launch result was a false positive because wrapper/error-
  dialog liveness was accepted without checking `startup.log` or DLL provenance.
  Corrected preflight checks both and does not treat Wine native WGPU as a real-
  Windows launch pass.
- Windows GUI startup failures now append a durable `startup.log` under
  `%LOCALAPPDATA%\\Ottrin` and show a bounded native error dialog that names
  the log location. This diagnostic is local evidence, not real-Windows QA.
- Isolated uninstall cleanup tests passed both preference-preserving and
  `--purge-config` paths without touching the real user profile.
- Isolated privileged-helper protocol tests passed malformed-request,
  nonexistent-directory, hidden-file filtering, stdio, and file request/response
  paths.

These checks are preflight evidence, not substitutes for CI, clean-desktop,
interactive workflow, polkit-dialog, Windows runtime, UAC, or installer sign-off.

## Linux Artifact QA

- [ ] AppImage launches on a clean supported desktop.
- [ ] Portable tarball launches on a clean supported desktop.
- [ ] Miller/List/Grid navigation, tabs, bookmarks, address navigation, and status messages pass.
- [ ] Copy, overwrite, rename, multi-select, Trash, permanent delete, and partial-batch warnings pass; Linux same-filesystem Move supports Skip/Rename/Overwrite/Cancel with explicit conflict handling and native rename, while cross-filesystem and Windows Move remain unsupported without mutation.
- [ ] Tandem View pane-to-pane copy passes, and cross-filesystem/Windows Move continues to report unsupported without mutation.
- [ ] Search initial index, restart, watcher update, rebuild, scope changes, content search, and result actions pass.
- [ ] Quick Inspector passes text, image, rendered multi-page PDF plus page navigation and text/metadata fallback, DOCX, XLSX/XLSM, PPTX, archive, folder, and generic files.
- [ ] Privileged helper allow, cancel, missing-helper, and misconfigured-policy paths pass.

Legacy retained-record recovery is dormant/test-only and is not an enabled v1
feature or release gate. Reactivating it requires a separate product decision,
implementation scope, and acceptance plan.

## Windows Artifact QA

- [ ] Portable zip launches on a real Windows desktop.
- [ ] Installer performs a clean per-user install, launches Ottrin, appears in Installed Apps, and creates working Start Menu shortcuts.
- [ ] Uninstall removes the app, shortcuts, scheduled/background integration, generated search data, and preview cache while retaining saved preferences.
- [ ] Navigation, file operations, Recycle Bin, multi-select, Tandem View, and status messages pass; cross-filesystem/Windows Move remains unsupported without mutation.
- [ ] Search indexing, daemon restart, watcher updates, concurrent GUI/CLI attach, and persistent background-service installation/removal pass.
- [ ] Quick Inspector passes the Windows file-type matrix, including rendered PDF pages plus malformed/encrypted PDF text/metadata fallback paths.
- [ ] If the graphics compatibility payload is shipped, native-driver and bundled-OpenGL launch paths each pass on their intended physical/virtual or remote adapters, including Automatic detection, the persisted renderer preference, and `--renderer opengl` / `OTTRIN_RENDERER=opengl` diagnostic selection.
- [ ] UAC allow, cancel, helper-missing, and denied-operation paths pass.
- [ ] Uninstall cleanup commands remove scheduled/background integration without deleting user files; cleanup launch/failure stops before application deletion, and incomplete file deletion retains Installed Apps registration for retry.


## Release Review

- [ ] `README.md`, `USER_GUIDE.md`, `ROADMAP.md`, and `RELEASE_NOTES.md` match observed behavior.
- [ ] Known limitations are complete and plainly worded.
- [ ] Version, artifact names, checksums, and generated release notes agree for the exact release artifacts.
- [ ] No release-blocking item is waived without a written rationale.
- [ ] Windows artifacts are Authenticode-signed with an approved production
  identity and verified after download. Current rebuilt artifacts are unsigned;
  SmartScreen reputation warnings remain a release blocker and must not be
  bypassed or claimed as trusted.
