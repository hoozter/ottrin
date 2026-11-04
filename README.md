# Ottrin

**Navigate with clarity.**

A fast, keyboard-first file manager built in Rust with [egui](https://github.com/emilk/egui). Miller columns, blazing-fast indexed search, rich file previews, and a full theme system — no Electron, no web views.

[![CI](https://github.com/hoozter/ottrin/actions/workflows/ci.yml/badge.svg)](https://github.com/hoozter/ottrin/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/hoozter/ottrin)](https://github.com/hoozter/ottrin/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.94.0-orange.svg)](https://www.rust-lang.org)

---

## Release Artifacts

Public releases will be available from [GitHub Releases](https://github.com/hoozter/ottrin/releases).
The current 0.1.0 is a locally tested development candidate; no public release is
claimed. The latest usable local packages and exact source/hashes/tested scopes are
recorded in `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json` and
`dist/WINDOWS_CANDIDATE_ACCEPTANCE.json`. See the [roadmap](docs/ROADMAP.md)
for current delivery and the [release checklist](docs/RELEASE_CHECKLIST.md) for gates.

Local Linux formats are AppImage, portable tarball, DEB and RPM. Windows has portable
ZIP, a per-user NSIS installer and an unsigned development MSIX for local testing.
The MSI remains held: native failed-transaction registration rollback can lose
registration while application files remain. Its normal install/update/removal
receipts do not waive that failure. Store submission waits for the owner's later
account/certification work; a paid signing certificate is not required for this plan.
macOS construction and acceptance are deferred outside the Linux/Windows goal.

The stable Linux AppImage is `dist/packages/Ottrin-0.1.0-x86_64.AppImage`.
Windows candidates are at `dist/windows-candidate/`. Verified replacements use
these stable paths; superseded packages and owned temporary QA resources are
retired after consumers. The current Windows 58-file payload is retained with its
candidate for exact identity checks; no versioned package folders accumulate.
Generated `SOURCE_REVISION.txt` identifies package source. Packaged documentation
is a build-time snapshot; repository manifests/documents record later acceptance.

All seven Linux/Windows preview packages use runtime `45830098`, including
unsigned development MSIX0.1.0.17. CI41809847269 passes all four exact-source jobs for
the shared-widget UI delivery. Earlier061 AppImage evidence verifies gray Stopped in Services,
nine service states/five startup toggles, real helper setup/cancel/reinstall and
normal GUI close/reopen with the managed PID retained. Three real same-version
DEB replacement states preserve source, preferences and the startup choice. These
receipts keep their original artifact scopes; the Linux458 daemon is byte-identical.
The earlier061 Linux1000-file daemon probe measures0.067% of one core over30seconds.
Windows458 passes nine settings cases/eight preview actions in Native/OpenGL,
fresh MSIX17 runtime/login/removal and16-to17 updates with startup enabled and
disabled. All58 payload files match; earlier full Services matrices retain their
original source scope. These are bounded
software fixtures, not sustained, physical or whole-interface acceptance.

The Linux458 packages share live toolbar/breadcrumb/tab, window-control widgets
and Miller file rows with the editor; 492 UI tests and strict Linux/Windows UI lint
pass. File-row painting/hit bounds match, and measured filename elision reserves
directory-chevron space. Exact AppImage Nord Dark150/Light100 preview checks pass.
Windows458/MSIX17 are delivered at their stable paths; consumed staging and
superseded packages are removed. Source50a breadcrumb classification is tested
but not packaged. The personal Linux daemon now matches the delivered binary
with startup, preferences and saved-index identity preserved.
Full theme/role consistency, positive local-seat protected-file elevation,
login/reboot, MSI rollback, physical coverage and final daily-driver acceptance
remain open. See the roadmap and stable manifests for exact evidence and limits.

### Linux

File-operation provenance requires Linux 6.8 or newer. Ottrin uses unique mount
identities to reject a filesystem that changes between selection and execution.

```sh
chmod +x Ottrin-*.AppImage
./Ottrin-*.AppImage
```

For optional administrator file operations and the offline HTML sandbox, open
Settings → Services. Setup stages the complete bundled integration and opens
your administrator permission prompt. Terminal users of the portable archive
can invoke the same installer:

```sh
sudo ./libexec/ottrin/ottrin-priv-helper --install
```

### Windows

Run the per-user installer, or unzip the portable package and run:

```powershell
.\bin\ottrin.exe
```

Normal user-owned file operations do not require elevation. Denied operations use the bundled helper through the Windows UAC flow.

Windows packaging is designed to use WGPU when Automatic finds no compatibility
signal and to select a checksum-pinned Mesa 26.1.3 OpenGL compatibility runtime
for detected remote sessions and recognized virtual/problem adapters. Settings
offers Automatic, Native, and OpenGL compatibility, while
`ottrin.exe --renderer opengl` (or `OTTRIN_RENDERER=opengl`) is the diagnostic
override. Exact release artifacts must still verify the app-local
`opengl32.dll`, `libgallium_wgl.dll`, EGL/GLES, DXIL, and complete notices before
this compatibility path is accepted.

### macOS

Deferred until the Linux/Windows roadmap and release acceptance are complete.
Existing packaging source does not imply a current accepted macOS build, and
macOS signing/launch checks do not block Linux/Windows work.

---

## Features

- **Miller columns** — browse parent, current, and preview pane side-by-side
- **Smooth column navigation** — horizontal auto-scroll keeps focus visible while preserving path context
- **Resizable columns** — drag separators; widths persist while you navigate
- **List and Grid views** — switch instantly with a single click
- **Smart sidebar** — info, drop folder, and search panels in a compact, toggleable activity strip
- **Bookmarks bar** — one-click shortcuts to your most-used folders (right-click to add/remove)
- **Integrated command frame** — run `mkdir`, `touch`, `mv`, `cp`, `rm`, `chmod`, `ln -s` and more without leaving the app; opens on any keypress
- **Keyboard first** — arrow keys, Enter, Backspace, and Tab navigate the Miller view
- **Tabs** — multiple locations open at once, full navigation history per tab
- **Theme system** — built-in presets (Ottrin, Breeze, Adwaita, Windows 11, Solarized, Nord, G33k) with live editing and custom save/load
- **Semantic file styling** — file and folder types are classified and styled consistently across views
- **Drop Folder workflow** — quick copy destination with recents and pinned entries
- **Integrated privilege management** — denied operations can retry via helper flow
- **Global search** — indexed search with scope switching and per-result actions
- **Preview system** — text and images in-app, bounded multi-page PDF previews with Previous/Next/page selection, bounded SVG visuals admitted by source, XML, path-work, semantic reuse, filter-work, dimension, and pixel limits, bounded XLSX/XLSM and ODS structured worksheet previews, a bounded native DOCX structure view, and a clearly labeled PPTX extracted-text fallback, plus rich archive/media metadata (Space bar). The DOCX view preserves paragraphs, styled runs, alignment cues, and bordered tables without claiming exact pagination or Office fidelity; PDF and Office previews retain extracted-text and metadata fallbacks. On Linux, EPS can use separately installed Ghostscript through bounded non-networked isolation; Ottrin does not bundle it and retains metadata/external-open fallback when unavailable.
- **Coherent command line** — `ottrin search`/`-s`, `ottrin index` (`update`, read-only `status`, atomic `rebuild`), and full Quick Inspector `ottrin preview`/`-p`

---

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| Arrow keys | Navigate between files |
| Enter | Open directory / launch file |
| Backspace | Go to parent directory |
| Ctrl+F | Open search panel |
| Space | Preview file |
| Esc | Close preview / command frame |
| F5 | Refresh current directory |
| Ctrl+C / Ctrl+X | Copy / Cut |
| Ctrl+V | Paste |
| Delete | Move to trash |
| Shift+Delete | Permanently delete |
| F2 | Rename |
| Any printable key | Open command frame |

---

## Build from Source

Requires Rust 1.94.0. CI and release workflows pin this exact toolchain for Linux, Windows, and macOS builds.

```sh
git clone https://github.com/hoozter/ottrin.git
cd ottrin
cargo run -p ottrin-app
```

Build local release packages into `dist/packages/`:

```sh
./build-all.sh
```

Linux dependencies:

```sh
sudo apt-get install -y \
    libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libssl-dev libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev
```

---

## Workspace

| Crate | Purpose |
|-------|---------|
| `ottrin-app` | Native entrypoint (eframe) |
| `ottrin-ui` | egui application shell and all views |
| `ottrin-core` | Domain models, commands, settings |
| `ottrin-platform` | OS integrations (trash, shell actions, privileged helper flow) |
| `ottrin-search` | Indexed search, watcher sync, and fallback querying |
| `ottrin-preview` | Preview request/response pipeline |

---

## Status

v0.1.0 is in release hardening. Linux AppImage, portable, `.deb`, and `.rpm` package sources plus Windows portable/installer sources are the current release path. See [docs/USER_GUIDE.md](docs/USER_GUIDE.md), [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md), and [docs/ROADMAP.md](docs/ROADMAP.md).

## License

MIT

## Author

Made by [hoozter](https://hoozter.com) — [github.com/hoozter/ottrin](https://github.com/hoozter/ottrin)
