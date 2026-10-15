# Ottrin Linux hands-on QA checklist

Use this checklist for a release candidate built from one exact commit. It complements
[`RELEASE_CHECKLIST.md`](RELEASE_CHECKLIST.md); it does not replace automated preflight,
artifact verification, or platform-specific Windows QA.

## 1. Candidate card

Record these before testing. Do not mix artifacts from different commits.

- [ ] Version: `________________`
- [ ] Commit: `________________`
- [ ] Build host/distribution: `________________`
- [ ] Desktop/session (for example KDE Plasma/Wayland): `________________`
- [ ] GPU/driver: `________________`
- [ ] Portable archive path: `________________`
- [ ] Portable SHA-256: `________________`
- [ ] AppImage path: `________________`
- [ ] AppImage SHA-256: `________________`
- [ ] DEB path and SHA-256, if tested: `________________`
- [ ] RPM path and SHA-256, if tested: `________________`
- [ ] Evidence directory under `.hermes/qa/`: `________________`

Build and verify from the repository root:

```bash
./build-all.sh --linux-only
./scripts/release-preflight.sh
sha256sum dist/Ottrin-*-linux-x86_64-portable.tar.gz \
  dist/Ottrin-*-x86_64.AppImage \
  dist/ottrin_*_*.deb 2>/dev/null
```

- [ ] Automated preflight passed without skipped required gates.
- [ ] Artifact names, checksums, bundled runtime files, and provenance checks passed.
- [ ] `git status --short --branch` and the exact tested commit were recorded.

## 2. Clean-environment setup

Test with disposable data, not a real library.

```bash
export XDG_CONFIG_HOME="$(mktemp -d)"
export XDG_CACHE_HOME="$(mktemp -d)"
export XDG_DATA_HOME="$(mktemp -d)"
mkdir -p /tmp/ottrin-qa/{library,other-volume}
```

Populate `library` with representative files:

- [ ] folders, empty folders, hidden files, and Unicode names;
- [ ] Markdown and plain text;
- [ ] source/code files;
- [ ] images including animated GIF/WebP if available;
- [ ] PDF;
- [ ] one legacy DOC and its equivalent DOCX, plus XLS/XLSX and PPTX;
- [ ] audio and video supported by the host GStreamer installation;
- [ ] a large file and a directory tree large enough to exercise indexing;
- [ ] read-only, broken-symlink, and permission-denied cases where safe.

For move/copy testing, use a second mounted filesystem when available. A second
directory on the same filesystem is not cross-volume evidence.

## 3. Launch each Linux artifact

### Portable archive

- [ ] Extract to a new directory; do not launch from a prior extraction.
- [ ] Read the artifact-local `RUNNING.md`.
- [ ] Launch using the documented entry point.
- [ ] Confirm the app opens without a terminal-only dependency or missing-library error.
- [ ] Capture stdout/stderr and the application log.

### AppImage

- [ ] Mark executable and launch normally.
- [ ] If FUSE is unavailable, test the documented extraction fallback separately.
- [ ] Confirm bundled documentation and privileged-helper policy are present.
- [ ] Capture stdout/stderr and the application log.

### Native package

- [ ] Install the DEB or RPM through the native package manager.
- [ ] Launch from the desktop application menu.
- [ ] Launch from a terminal and capture diagnostics.
- [ ] Confirm icon, application name, and uninstall behavior.

For every artifact:

- [ ] No crash, panic, endless spinner, blank window, or unexpected root prompt.
- [ ] Logs identify the tested executable/artifact and expected runtime provenance.
- [ ] Closing the final window terminates owned background processes cleanly.

## 4. Navigation and file operations

- [ ] Open a folder by double-click and Enter.
- [ ] Navigate backward, forward, upward, and by typed path.
- [ ] Selection behaves correctly with mouse, Ctrl, Shift, keyboard, and empty-space click.
- [ ] Context actions are consistent between mouse and keyboard selection.
- [ ] Rename a file and folder, including Unicode and case-only names where supported.
- [ ] Create a folder.
- [ ] Copy and move files and directories on the same filesystem.
- [ ] Copy and move across filesystems when a real second filesystem is available.
- [ ] Test destination-exists conflicts for file/file, directory/directory, and cross-kind cases.
- [ ] Cancel an in-progress copy/move and verify source/destination outcomes are coherent.
- [ ] Trigger an I/O or permission failure and verify the error is actionable and retry-safe.
- [ ] Delete/trash behavior matches the visible action and does not bypass confirmation policy.
- [ ] Refresh reflects external create, rename, modify, and delete operations.

**Safety verdict:** no silent overwrite, partial success reported as full success, lost source,
stale duplicate, or replay of an already committed operation.

## 5. Search and indexing

- [ ] Initial indexing reaches a truthful ready/failed state.
- [ ] Filename search finds expected files and ranks useful matches first.
- [ ] Content search finds text in supported formats.
- [ ] New, modified, renamed, moved, and deleted files are reflected without restarting.
- [ ] Excluded/hidden paths follow configured policy.
- [ ] Restart loads or rebuilds the index coherently.
- [ ] Search during rebuild remains responsive and does not expose partial generations.
- [ ] A failed or cancelled refresh remains retryable and does not erase the last good index.

Record approximate library size, initial indexing time, steady-state memory, and any CPU
or I/O spike that materially harms desktop use.

## 6. Preview and rendering

Open previews with Space and the preview action/shortcut exposed by the UI.

- [ ] Images render with correct orientation, transparency, zoom, and animation behavior.
- [ ] PDF opens, pages render, and zoom/navigation remain responsive.
- [ ] DOCX uses the native structured renderer by default and preserves readable structure.
- [ ] With LibreOffice absent or disabled, legacy DOC uses Ottrin's native visual renderer
  rather than an extraction card; compare its structure with the equivalent DOCX and record
  fidelity differences rather than treating them as automatically equivalent.
- [ ] With LibreOffice absent or disabled, XLSX uses a readable light worksheet surface under
  Ottrin's dark application theme: white/light cells, dark text, visible gridlines and stronger
  row/column headers.
- [ ] With LibreOffice absent or disabled, legacy XLS falls back truthfully instead of claiming
  a native visual renderer that Ottrin does not provide.
- [ ] With installed LibreOffice enabled, DOC/DOCX, XLS/XLSX and PPTX use the installed-provider
  path where supported, identify that source honestly, and retain page/sheet navigation.
- [ ] Disabling LibreOffice immediately restores the available Ottrin-native renderer; removing
  or breaking LibreOffice produces an actionable provider failure and truthful native or
  text/metadata fallback, never an empty preview.
- [ ] XLSX and PPTX native views do not pretend unsupported fidelity is exact.
- [ ] Replacing an Office source while conversion/rendering is in progress never publishes a
  page from the old generation under the new file.
- [ ] Markdown renders correctly and transitions into the flagship editor without losing data.
- [ ] Plain text opens read-only; only **Edit text** enables editing. Arrow keys navigate files and Space closes the Inspector before edit mode.
- [ ] Code opens with syntax highlighting, line numbers, and search.
- [ ] Audio/video preview either plays with host-supported codecs or reports missing support
  clearly; it never hangs silently.
- [ ] Unsupported, malformed, empty, and very large files fail safely.
- [ ] Rapidly switching selected files never shows stale content under the wrong filename.

## 7. Editing and external-change safety

- [ ] Edit and save Markdown.
- [ ] Edit and save plain text.
- [ ] Edit and save a safely text-equivalent/code file.
- [ ] Reopen each saved file and verify exact persisted content.
- [ ] Modify the source externally after opening it; saving from Ottrin must detect the change
  and clearly warn before any overwrite.
- [ ] Exercise cancel, reload/keep-external, and explicit overwrite choices where offered.
- [ ] Failed saves leave the original recoverable and report the actual failure.
- [ ] Line endings, Unicode, and trailing newline behavior are not silently corrupted.

## 8. Settings, persistence, and desktop behavior

- [ ] Change relevant view, preview, search, and theme settings.
- [ ] Restart and confirm intended settings persist once, without duplicate state.
- [ ] Window size/state restores sensibly across normal and maximized launches.
- [ ] Light/dark theme, scaling, keyboard focus, and text contrast remain usable.
- [ ] Open-with/file-association flows identify Ottrin correctly where installed.
- [ ] Multiple-window or second-instance behavior follows the product contract.
- [ ] Sleep/resume and removable-volume disappearance do not strand the app or corrupt state.

## 9. Bounded stress and recovery

- [ ] Browse and search repeatedly while indexing and previewing files.
- [ ] Rapidly change selection during slow preview generation.
- [ ] Close the app during active indexing/preview work, then relaunch.
- [ ] Terminate the process once during disposable-state indexing, then relaunch and verify
  the persisted state is either last-known-good or explicitly rebuilt—not half-activated.
- [ ] Repeat one representative copy/move after interruption and confirm no duplicate replay.
- [ ] Observe memory after repeated previews; it should return toward a stable baseline.

Do not perform destructive interruption tests on personal files or the only copy of data.

## 10. Result and evidence

- [ ] Portable: **PASS / FAIL / BLOCKED**
- [ ] AppImage: **PASS / FAIL / BLOCKED**
- [ ] DEB: **PASS / FAIL / NOT TESTED**
- [ ] RPM: **PASS / FAIL / NOT TESTED**
- [ ] Same-filesystem operations: **PASS / FAIL**
- [ ] Cross-filesystem operations: **PASS / FAIL / NOT AVAILABLE**
- [ ] Preview/editing: **PASS / FAIL**
- [ ] Search/indexing: **PASS / FAIL**
- [ ] Desktop integration: **PASS / FAIL / PARTIAL**

For each failure, record:

- exact artifact and checksum;
- distribution, desktop/session, GPU/driver, and filesystem types;
- minimal reproduction steps and expected/actual result;
- timestamped app log and terminal output;
- smallest safe sample file or directory topology needed to reproduce;
- screenshot or short recording only when it adds evidence.

**Final Linux verdict:** `PASS / FAIL / BLOCKED`

**Tester:** `________________`  **Date:** `________________`

A Linux release candidate passes only when required artifact types launch, core navigation,
search, preview, editing, and same-filesystem safety pass; cross-filesystem testing is either
passed on a real second filesystem or explicitly remains a release limitation. Automated
checks are not substitutes for the hands-on verdict.
