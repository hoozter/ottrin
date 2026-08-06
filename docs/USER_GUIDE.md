# Ottrin User Guide

## Install

### Linux AppImage

```sh
chmod +x Ottrin-*-x86_64.AppImage
./Ottrin-*-x86_64.AppImage
```

### Linux Portable

```sh
tar -xzf Ottrin-*-linux-x86_64-portable.tar.gz
cd Ottrin-*-linux-x86_64
./bin/ottrin
```

### Windows Portable

Unzip `Ottrin-*-windows-x64-portable.zip`, then run:

```powershell
.\bin\ottrin.exe
```

### Windows Installer

Run `Ottrin-*-windows-x64-setup.exe` to install under the current user's local application directory. The installer creates Start Menu shortcuts and does not request administrator access. Its uninstaller removes scheduled/background integration and generated search/preview data before removing the application; saved preferences are retained. If background cleanup cannot start or fails, uninstall stops before deleting application files. If application-file deletion cannot complete, Installed Apps registration is retained so uninstall can be retried.

Windows MSI and macOS packages are not part of the current v1 release path.

## Basic Navigation

| Shortcut | Action |
| --- | --- |
| Arrow keys | Move through files and columns |
| Enter | Open selected folder or file |
| Backspace | Go to parent folder |
| Ctrl+F | Open search |
| Space | Open Quick Inspector |
| Delete | Move selected item to Trash |
| Shift+Delete | Permanently delete selected item |
| Ctrl+C / Ctrl+X | Copy / cut (same-filesystem Linux Move is supported; cross-filesystem and Windows Move reject without mutation) |
| Ctrl+V | Paste |
| F2 | Rename |
| F5 | Refresh current folder |

## Search

Ottrin's primary search source is its own SQLite index. Configure indexed locations in Settings -> Search. For most users, the best setup is a focused set of home, project, document, and download folders. Attached drives remain browseable without being indexed: use a drive's Locations context menu to enable or disable indexed search explicitly. Ottrin remembers that choice by volume identity and restores it when the volume returns, while an unavailable volume is left out of the active index.

Search freshness uses three mechanisms:

| Mechanism | Works while Ottrin is closed? | Purpose |
| --- | --- | --- |
| Saved SQLite index | Yes | Gives instant results from the last completed scan |
| File watcher | No | Updates results while Ottrin is running |
| Background updater | Yes | Reconciles changes made while Ottrin was closed |

System locate/plocate is optional. It can help discover system paths, but it can be stale and does not grant permission to read, preview, copy, or delete protected files.

Built-in generated-folder skips such as `.git`, `node_modules`, `target`, `__pycache__`, and Trash directories keep indexing practical. If you need to search one of those trees, adjust search settings instead of relying on the watcher.

## Trash

Open the Trash section in the smart panel to review trashed items, restore items, permanently delete selected trash entries, or empty Trash. Empty Trash asks for confirmation and runs in the background.

Permanent deletion cannot be undone from Ottrin.

## Quick Inspector

Press Space on a selected item to open Quick Inspector. Ottrin previews common text and image formats directly and renders bounded multi-page PDF visuals with Previous/Next/page selection when its packaged PDFium runtime is available; it also shows extracted PDF text and metadata. It renders bounded DOCX/XLSX/XLSM content-preview visuals and ODS worksheet previews alongside their extracted text and metadata. On Unix, PPTX receives a bounded, non-authoritative first-slide text card alongside its slide text and metadata; this is not a faithful PowerPoint layout and does not render native fonts, shapes, charts, media, animations, transitions, speaker notes, themes, or exact positioning. PPTX visual admission deliberately requires canonical matching local and central ZIP metadata, so files repacked by some third-party ZIP tools may retain text and metadata without a visual card. On non-Unix platforms, PPTX keeps its text and metadata fallback until a reparse-safe admission protocol exists.

Damaged, encrypted, unsupported, or unrenderable PDFs fall back to extracted text and metadata without exposing raw PDF bytes to the image renderer. Legacy binary Office formats such as `.doc`, `.xls`, and `.ppt` currently show metadata and can be opened in their associated desktop application.

## Legacy Retained Move Records

Ottrin contains dormant/test-only journal and recovery machinery from an earlier
Move design. Legacy retained records can have source-level discovery and
manual-review surfaces, but the accepted Linux same-filesystem native-rename
path does not create or depend on them. No packaged or runtime recovery
capability is claimed. Cross-filesystem and Windows Move reject without mutation.

## Privileged Operations

Normal user-owned files do not need elevated setup. If an operation is denied, Ottrin can retry through a privileged helper when that helper is available.

Linux portable helper setup:

```sh
sudo install -m 0755 ./bin/ottrin-priv-helper /usr/libexec/ottrin-priv-helper
sudo install -m 0644 ./packaging/linux/org.ottrin.filesystem.policy /usr/share/polkit-1/actions/
```

Windows portable packages bundle `ottrin-priv-helper.exe` beside the app and use the Windows UAC flow when needed.

## Cleanup And Upgrades

Ottrin stores user configuration in the platform config directory:

| Platform | Config |
| --- | --- |
| Linux | `${XDG_CONFIG_HOME:-~/.config}/ottrin/config.json` |
| Windows | `%APPDATA%\Ottrin\config.json` |

Thumbnail cache files are disposable. If previews look stale after an upgrade, clear the cache from Settings -> Cache.

View Options in the navigation bar controls the active view: Column/List/Grid mode, image thumbnails in List/Grid, hidden files, sorting, and List columns. View mode is remembered as the global default for future launches. Thumbnail toggles are saved for the current folder so an image-heavy folder can show thumbnails without forcing every folder into thumbnail mode. Use Global View Settings from that menu to change saved defaults in Settings. Generated thumbnail cards cover supported text/code, PDF, Office, font, SQLite, mesh, camera-RAW, and bounded Affinity previews; folders use a bounded direct-child collage. Common web/desktop images such as PNG, JPEG, GIF, BMP, TIFF, WebP, ICO, and safely rasterized SVG appear directly. Unsupported, malformed, or oversized inputs fall back to metadata rather than raw document bytes.

The SQLite search index is safe to rebuild from Settings -> Search.

Portable packages keep binaries and helper files together, but user configuration remains in the normal platform config directory. Removing a portable folder does not remove user configuration or cache data.

Ottrin also provides generated-data cleanup:

```sh
ottrin --uninstall
ottrin --uninstall --purge-config
ottrin-index --remove-schedule
```

## Current Limitations

- Packaging workflows and historical local artifact checks exist, but no rebuilt exact-current-commit Windows artifact, accepted CI run, or real Windows desktop runtime smoke is claimed.
- Windows UAC/helper behavior still needs real Windows validation.
- Windows GUI startup failures write `%LOCALAPPDATA%\\Ottrin\\startup.log` and
  show the local log location in a native error dialog. Wine launch evidence is
  limited: corrected probes reject any newly appended startup failure and prove
  the OpenGL path loaded Ottrin's app-local Mesa loader; real Windows
  native-driver validation remains required.
- Windows packaging is designed to use WGPU when Automatic finds no compatibility signal and to include a checksum-pinned Mesa 26.1.3 OpenGL compatibility runtime with app-local `opengl32.dll`, its `libgallium_wgl.dll` driver bundle, EGL/GLES libraries, and DXIL. Automatic selection, persisted preferences, diagnostic override behavior, notices, and DLL provenance still require acceptance against the exact rebuilt artifacts.
- Linux polkit helper packaging needs real system smoke testing.
- Historical Linux portable/AppImage artifacts had shallow launch smoke coverage; the exact current candidate has no accepted rebuilt-artifact evidence and full packaged workflow QA remains pending.
- Quick Inspector covers text/code, images, PDF text/metadata, DOCX content, XLSX/XLSM/ODS worksheets, Unix-only bounded first-slide PPTX text cards and slide text, ZIP/archive listings, bounded folder summaries, and generic metadata, but the full keyboard/file-type matrix still needs manual QA.
- PPTX visual cards fail closed when local and central ZIP extra metadata is noncanonical or differs; affected files still retain slide text and metadata and can be opened normally.
- Faithful Office layout rendering, video thumbnails, media playback, and HEIC/HEIF thumbnails are future work. Quick Inspector shows a bounded recursive folder-size summary; unreadable, excessively deep, or very large trees are explicitly marked as incomplete. Editing is limited to complete direct UTF-8 text on supported Linux filesystems; unsupported or metadata-sensitive sources remain read-only.
- Multi-select is implemented, but still needs manual QA across Miller/List/Grid.
- Tab drag-and-drop is implemented and test-covered; packaged/manual desktop acceptance is pending.
- Operation Activity records session file operations and labels whether entries are restorable or not undoable. Full undo execution is not implemented.
- Context-menu actions, Open With application launching, thumbnail view options, and status messages still need manual QA.
- Indexed FTS5 content search supports negated content terms. A negated content query uses a completed indexed generation and excludes entries whose content could not be indexed, rather than guessing that missing content is absent. Filename, path, extension, and type exclusions keep the established positive-content fallback behavior. Ripgrep is never used to form a negated content complement.
- Linux `.deb`/`.rpm`, Windows MSI, macOS packages, and code signing are not complete. Any current Windows candidate remains unaccepted until the exact unsigned artifacts are rebuilt and tested; signing and SmartScreen reputation remain release blockers.
