# Ottrin v1 Release Checklist

Every item must be checked against the exact artifact intended for release. A source-tree test does not count as packaged desktop validation.

Current scope is the Linux/Windows release defined in the [roadmap](ROADMAP.md).
macOS construction, acceptance and signing are deferred; the future macOS checks
below do not block this release. For current source/package progress and the
unfinished QA handoff, use the [audited checkpoint](ROADMAP.md#current-checkpoint-and-next-work--audited-2026-10-24).
Unchecked final gates do not mean no scoped implementation or QA has occurred.

## UI coherence gate

- [ ] Shared theme roles and text/icon alignment meet [UI consistency requirements](DEVELOPMENT.md#ui-consistency-requirements) across main window, menus/submenus, Settings, theme editor, Inspector, and dialogs; checked, unchecked, iconless, and disabled rows retain common columns in Light/Dark at 100%/150% scaling and enlarged text.

- [ ] Inspector previews remain read-only until **Edit text**; arrows and Space work on VCF/plain text before editing, source bytes remain unchanged, auxiliary title bars match the theme, and non-PDF → PDF transitions stay stable at scrollbar overflow boundaries.

## Automated Gates

- [ ] `cargo fmt --all --check`
- [ ] `cargo check --workspace --all-targets`
- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Linux and Windows CI are green for the release commit.
- [ ] Exact release artifact checksums verify after rebuilding from the current release commit.
- [ ] Archive inspection confirms the exact rebuilt app, indexer, privileged helper, policy/assets, and user documentation are present.
- [ ] Live-content search works in exact installed and standalone artifacts without a preinstalled ripgrep. Required provider binaries/notices are delivered or installed as declared dependencies; missing/damaged providers produce an actionable error instead of empty content matches.
- [ ] Native DEB/RPM checksums, source revision, helper policy and executable code match the release AppImage; generated dependencies include the payload-derived minimum libc and administrator authentication runtime.
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
- [ ] Copy, overwrite, rename, multi-select, Trash, permanent delete, and partial-batch warnings pass; Linux same-filesystem and cross-filesystem Move support Skip/Rename/Overwrite/Cancel with explicit complete/partial/recovery outcomes.
- [ ] Tandem View pane-to-pane copy and Move pass, including cross-filesystem directory/symlink handling and `No batch undo` Activity reporting.
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
- [ ] Installer rollback is verified after the installation script and background setup actually execute, including version upgrade and uninstall failures: prior payload, product/component registration, preferences, generated data and complete index-task configuration (including disabled state) are restored. A failure injected before execution does not satisfy this gate.
- [ ] Navigation, file operations, Recycle Bin, multi-select, Tandem View, and status messages pass; same-volume and cross-volume Windows Move satisfy the roadmap contract and fail closed for every still-unsupported conflict or entry-kind boundary.
- [ ] Search indexing, GUI watcher updates, concurrent GUI/CLI access to the saved index, and Task Scheduler installation, upgrade, restart and removal pass.
- [ ] Quick Inspector passes the Windows file-type matrix, including rendered PDF pages plus malformed/encrypted PDF text/metadata fallback paths.
- [ ] If the graphics compatibility payload is shipped, native-driver and bundled-OpenGL launch paths each pass on their intended physical/virtual or remote adapters, including Automatic detection, the persisted renderer preference, and `--renderer opengl` / `OTTRIN_RENDERER=opengl` diagnostic selection.
- [ ] UAC allow, cancel, helper-missing, and denied-operation paths pass.
- [ ] Uninstall cleanup commands remove scheduled/background integration without deleting user files; cleanup launch/failure stops before application deletion, and incomplete file deletion retains Installed Apps registration for retry.


The staged `fcf00aa` Windows MSI passes exact native running-daemon upgrade and
all four login-enabled/disabled and running/idle repair cases. Its failed-uninstall
rollback still damages product registration; normal repair/retry does not recover
that installation. Independent file-only per-user controls reproduce registration
rollback failures after actual file execution. This does not establish a safe
compiler/runtime fix and does not waive the rollback gate. Evidence and exact
scope: `.hermes/evidence/windows-msi-rollback-current/ACCEPTANCE.json`.


## Release Review

- [ ] `README.md`, `USER_GUIDE.md`, `ROADMAP.md`, and `RELEASE_NOTES.md` match observed behavior.
- [ ] Known limitations are complete and plainly worded.
- [ ] Version, artifact names, checksums, and generated release notes agree for the exact release artifacts.
- [ ] No release-blocking item is waived without a written rationale.
- [ ] Windows artifacts are Authenticode-signed with an approved production
  identity and verified after download. Current rebuilt artifacts are unsigned;
  SmartScreen reputation warnings remain a release blocker and must not be
  bypassed or claimed as trusted.
- [ ] Linux portable and AppImage artifacts have armored detached signatures;
  the publish job verifies the exact downloaded files against the approved
  primary-key fingerprint before creating the release.

Deferred macOS release requirement:

- [ ] The macOS app ZIP and DMG are Developer ID signed, notarized, and stapled;
  the publish dependency verifies the exact downloaded checksums, team
  identifier, code signatures, notarization tickets, and Gatekeeper assessment.

The Linux/Windows tag workflow requires these GitHub Actions settings:

- secret `LINUX_SIGNING_PRIVATE_KEY` and variable
  `LINUX_SIGNING_KEY_FINGERPRINT` (the full 40-hex primary-key fingerprint),
- secrets `WINDOWS_SIGNING_CERTIFICATE_BASE64` and
  `WINDOWS_SIGNING_CERTIFICATE_PASSWORD`, plus variable
  `WINDOWS_SIGNING_CERTIFICATE_THUMBPRINT` (the 40-hex certificate thumbprint).

Future macOS signing, when that platform is resumed, requires:

- secrets `MACOS_SIGNING_CERTIFICATE_BASE64`,
  `MACOS_SIGNING_CERTIFICATE_PASSWORD`, `MACOS_NOTARIZATION_APPLE_ID`, and
  `MACOS_NOTARIZATION_PASSWORD`, plus variables `MACOS_SIGNING_IDENTITY`
  (the complete `Developer ID Application: ...` identity) and `MACOS_TEAM_ID`
  (the ten-character Apple developer team identifier).

Provisioning and approving those production identities remains an external
release gate. Private keys and certificate bundles must remain in GitHub
Actions secrets, never in the repository or release artifacts.
