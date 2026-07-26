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
| Ctrl+C / Ctrl+X | Copy / cut (Move is disabled in the current hardening build) |
| Ctrl+V | Paste |
| F2 | Rename |
| F5 | Refresh current folder |

## Search

Ottrin's primary search source is its own SQLite index. Configure indexed locations in Settings -> Search. For most users, the best setup is a focused set of home, project, document, and download folders.

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

Press Space on a selected item to open Quick Inspector. Ottrin previews common text and image formats directly and renders a bounded first PDF page when its packaged PDFium runtime is available; it also shows extracted PDF text and metadata. DOCX content, XLSX/XLSM bounded worksheet tables, PPTX slide text, and ZIP listings are available as text previews.

Damaged, encrypted, unsupported, or unrenderable PDFs fall back to extracted text and metadata without exposing raw PDF bytes to the image renderer. Legacy binary Office formats such as `.doc`, `.xls`, and `.ppt` currently show metadata and can be opened in their associated desktop application.

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

View Options in the navigation bar controls the active view: Column/List/Grid mode, image thumbnails in List/Grid, hidden files, sorting, and List columns. View mode is remembered as the global default for future launches. Thumbnail toggles are saved for the current folder so an image-heavy folder can show thumbnails without forcing every folder into thumbnail mode. Use Global View Settings from that menu to change saved defaults in Settings. Image thumbnails currently cover common web/desktop formats such as PNG, JPEG, GIF, BMP, TIFF, WebP, ICO, AVIF, and SVG.

The SQLite search index is safe to rebuild from Settings -> Search.

Portable packages keep binaries and helper files together, but user configuration remains in the normal platform config directory. Removing a portable folder does not remove user configuration or cache data.

Ottrin also provides generated-data cleanup:

```sh
ottrin --uninstall
ottrin --uninstall --purge-config
ottrin-index --remove-schedule
```

## Current Limitations

- Windows builds pass CI and archive inspection, but real Windows desktop runtime smoke testing is still pending.
- Windows UAC/helper behavior still needs real Windows validation.
- Windows builds default to WGPU with the system graphics driver. On a compatible user-supplied OpenGL driver, the recovery override is `ottrin.exe --renderer opengl` (or `OTTRIN_RENDERER=opengl`). A Mesa/OpenGL workaround was observed on virtual and remote adapters, but it is not part of an Ottrin artifact until its complete third-party notices and `dxil.dll` redistribution evidence are available.
- Linux polkit helper packaging needs real system smoke testing.
- Linux portable and AppImage artifacts have shallow launch smoke coverage; full packaged workflow QA is still pending.
- Quick Inspector covers text/code, images, PDF text/metadata, DOCX content, XLSX/XLSM worksheets, PPTX slide text, ZIP/archive listings, bounded folder summaries, and generic metadata, but the full keyboard/file-type matrix still needs manual QA.
- PDF page navigation, faithful Office layout rendering, text/code/PDF/video document thumbnails, recursive folder sizing/thumbnails, media playback, HEIC/HEIF and camera RAW thumbnails, and text/code edit mode are future work.
- Multi-select is implemented, but still needs manual QA across Miller/List/Grid.
- Tab drag-and-drop is not implemented.
- Operation Activity records session file operations and labels whether entries are restorable or not undoable. Full undo execution is not implemented.
- Context-menu actions, Open With application launching, thumbnail view options, and status messages still need manual QA.
- True FTS5 content indexing is not implemented; content search currently uses ripgrep-backed queries.
- Linux `.deb`/`.rpm`, Windows MSI, macOS packages, and code signing are not complete.
