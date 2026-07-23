# Ottrin v1 Release Checklist

Every item must be checked against the exact artifact intended for release. A source-tree test does not count as packaged desktop validation.

## Automated Gates

- [x] `cargo fmt --all --check`
- [x] `cargo check --workspace --all-targets`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Linux and Windows CI are green for the release commit.
- [x] Release artifact checksums verify.
- [x] Archive inspection confirms the app, indexer, privileged helper, policy/assets, and user documentation are present.

Local automated gates and artifact inspection last passed on 2026-07-23. CI and
the clean-machine/manual gates below remain required against the eventual
release commit and artifacts.

### Local Preflight Evidence (2026-07-23)

- The workspace test run passed 132 tests.
- The strict artifact verifier accepted one version-matched Linux portable
  archive, AppImage, Windows portable archive, and NSIS installer; all four
  checksums passed.
- AppImage payload inspection confirmed the app, indexer, privileged helper,
  desktop entry, AppStream metadata, and polkit policy.
- The freshly packaged Linux portable executable and AppImage each remained
  running until an eight-second smoke-test timeout, with no startup error.
- The Windows portable app inspected as an x86-64 PE using Windows system DLLs;
  the installer inspected as an NSIS executable.
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
- [ ] Copy, overwrite, rename, multi-select, Trash, permanent delete, and partial-batch warnings pass; Move rejects without mutation.
- [ ] Tandem View pane-to-pane copy passes, and Move reports unsupported without mutation.
- [ ] Search initial index, restart, watcher update, rebuild, scope changes, content search, and result actions pass.
- [ ] Quick Inspector passes text, image, PDF text/metadata, DOCX, XLSX/XLSM, PPTX, archive, folder, and generic files.
- [ ] Privileged helper allow, cancel, missing-helper, and misconfigured-policy paths pass.

## Windows Artifact QA

- [ ] Portable zip launches on a real Windows desktop.
- [ ] Installer performs a clean per-user install, launches Ottrin, appears in Installed Apps, and creates working Start Menu shortcuts.
- [ ] Uninstall removes the app, shortcuts, scheduled/background integration, generated search data, and preview cache while retaining saved preferences.
- [ ] Navigation, file operations, Recycle Bin, multi-select, Tandem View, and status messages pass.
- [ ] Search indexing, restart, watcher updates, and background updater scheduling/removal pass.
- [ ] Quick Inspector passes the Windows file-type matrix, including PDF text/metadata and malformed/encrypted PDF fallback paths.
- [ ] UAC allow, cancel, helper-missing, and denied-operation paths pass.
- [ ] Uninstall cleanup commands remove scheduled/background integration without deleting user files; cleanup launch/failure stops before application deletion, and incomplete file deletion retains Installed Apps registration for retry.

## Release Review

- [ ] `README.md`, `USER_GUIDE.md`, `ROADMAP.md`, and `RELEASE_NOTES.md` match observed behavior.
- [ ] Known limitations are complete and plainly worded.
- [ ] Version, artifact names, checksums, and generated release notes agree.
- [ ] No release-blocking item is waived without a written rationale.
