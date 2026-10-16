# Ottrin User Guide

## Install

### Linux AppImage

Linux file operations require kernel 6.8 or newer. Local files may be copied or
moved outside Home, including across temporary filesystems. Ottrin records the
actual filesystem reached by each path and refuses queued work if it changes.
Copying a symbolic link keeps the link itself; a destination directory link is
checked against the filesystem it reaches. Unidentified or unavailable discovered
volumes remain refused rather than silently targeting another disk.

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

### Linux native packages

Install the local `.deb` with `sudo apt install ./ottrin_0.1.0_amd64.deb`, or the local `.rpm` with `sudo dnf install ./ottrin-0.1.0-1.x86_64.rpm`.

Removing the DEB or RPM stops native index daemons and removes Ottrin’s native user and Administrator service registrations. Reinstalling refreshes running native services; stopped services stay stopped. Independent portable/AppImage indexers, preferences, saved indexes and your files are retained. If an offline user has a customized service override whose ownership cannot be established, removal stops with an error before changing services. Resolve that override under the owning account before retrying removal. See [ROADMAP.md](ROADMAP.md) for the tested scope and remaining native installation gates.

### Windows Portable

Unzip `Ottrin-*-windows-x64-portable.zip`, then run:

```powershell
.\bin\ottrin.exe
```

### Windows Installer

Run `Ottrin-*-windows-x64-setup.exe` to install under the current user's local application directory. The installer creates Start Menu shortcuts and does not request administrator access. Its uninstaller removes scheduled/background integration and generated search/preview data before removing the application; saved preferences are retained. If background cleanup cannot start or fails, uninstall stops before deleting application files. If application-file deletion cannot complete, Installed Apps registration is retained so uninstall can be retried. The per-user `Ottrin-*-windows-x64.msi` provides the same application-owned background setup and cleanup ordering. Recorded MSI/NSIS candidates have scoped ordinary-user native installation, upgrade, indexing and uninstall checks. Failed-transaction MSI rollback currently fails native tests: Windows can lose product registration even while Ottrin's files remain, and a later repair or uninstall can fail. MSI acceptance is blocked; the broader installer matrix remains incomplete. [ROADMAP.md](ROADMAP.md) identifies exact artifacts and acceptance limits.

### macOS

The package source creates `Ottrin-*-macos-*.zip`. After an accepted package is
published, unzip it and move `Ottrin.app` to Applications. No current macOS
artifact is accepted: native launch, Finder integration, quarantine/Gatekeeper,
and signing still require macOS release-candidate evidence.

## Basic Navigation

| Shortcut | Action |
| --- | --- |
| Arrow keys | Move through files and columns |
| Enter | Open selected folder or file |
| Backspace | Go to parent folder |
| Ctrl+L | Select the full address for replacement; Enter opens it |
| Ctrl+F | Open search |
| Space | Open Quick Inspector |
| Delete | Move selected item to Trash |
| Shift+Delete | Permanently delete selected item |
| Ctrl+C / Ctrl+X | Copy / cut |
| Ctrl+V | Paste |
| F2 | Rename |
| F5 | Refresh current folder |

## Appearance and themes

Open **Settings → Appearance** to choose a built-in theme or one of **Your themes**.
The connected **Light / Dark / System** control sets the application-wide preference.
System also shows the effective desktop mode. Every theme has a preview generated
from its own colors and styling.

Click a card to apply it. Click its pencil to customize it without changing the saved
selection. A built-in preset opens as **Customize …**; **Create theme** saves an
independent custom theme with both Light and Dark variants and activates it. Built-in
presets stay unchanged. A custom theme opens as **Edit …**; **Save changes** updates
it, and **Save as new** creates a separate copy. Change its name in the editor header
to rename it. The custom card's delete action removes it; deleting the active theme
returns to its source preset while keeping the global mode preference.

In the editor, click the live preview or choose a component from the inspector.
**Theme defaults** contains shared colors, text size, and window/surface geometry;
other contexts show that component's properties. **Shared styling** also exposes relevant
existing theme defaults from the selected component; changing them updates every component
that uses them. There are no separate override copies. The inspector scrolls independently.

Click the preview tab or navigation bar to select **Navigation & tabs** together.
Their toolbar color is shared; active tabs blend it with the title-bar background,
use muted text and the accent underline, and share **Surface corners** with inputs
and expanded location cards. **Window corners** affects application windows, including Settings,
the editor and Inspector. **Button corners** affects buttons and collapsed location icons.
The file canvas and row fills are square and have no independent radius setting;
component outlines identify editing targets and do not add a frame to the real view.
The editor's Light/Dark selector edits the two variants separately and does not change
the global appearance preference.

Each row's reset restores its opening value. **Reset component** restores the
component-specific controls above it, leaving **Shared styling** unchanged, in the
current variant; **More → Reset theme to starting values** restores both variants.
**More** also imports and exports the complete paired theme in the v2 JSON format.
Save or discard an existing changed draft before importing another theme.

**Cancel** discards the preview and restores the saved appearance. Closing a changed
editor keeps its draft available through **Continue editing** on the Appearance page.
Closing Ottrin with unsaved edits offers explicit save, discard, or keep-editing choices.

## Search

Ottrin starts with Settings closed. Open Settings → Search to review the setup card for its three independent layers: broad system filenames, rich native metadata, and optional file content. The card shows the active choices and does not enable one layer as a side effect of another. Existing search scopes are preserved; completing the review hides the setup card.

Ottrin's primary search source is its own SQLite index. Configure indexed locations in Settings -> Search. For most users, the best setup is a focused set of home, project, document, and download folders. Attached drives remain browseable without being indexed: use a drive's Locations context menu to enable or disable indexed search explicitly. Ottrin remembers that choice by volume identity and restores it when the volume returns, while an unavailable volume is left out of the active index.

Search freshness uses three mechanisms:

| Mechanism | Works while Ottrin is closed? | Purpose |
| --- | --- | --- |
| Saved SQLite index | Yes | Gives instant results from the last completed scan |
| File watcher | Yes on Linux when the per-user daemon is active | Applies direct changes independently of GUI windows |
| Periodic reconciliation | Yes | Runs a bounded full refresh (every four hours by default) as a safety net for missed or ambiguous watcher events |

On Linux, click the index icon in the status bar or open **Settings → Services** to manage background indexing. Green status dots show when the administrator helper is ready and the indexer is running. Automatic startup at sign-in is shown separately: indexing already works when Ottrin is open even when automatic startup is off. **Enable automatic startup** turns it on; **Update / reinstall service** connects it to the current app version. **Start / restart**, **Stop indexing**, **Uninstall service**, and **Refresh status** control its lifecycle. Stopping or uninstalling keeps your saved index and preferences. Opening Ottrin again starts an index for that session. The enabled user service stays running after Ottrin closes and starts at sign-in. Opening another AppImage mount reuses a daemon with identical executable contents. A changed daemon build uses the same orderly service upgrade path; cold startup allows up to 30 seconds for readiness. Service installation first asks an existing session daemon to finish and release its database before starting the managed service; failures are reported.

On Linux, global filename/path search can merge the native live index with an enabled system locate/plocate database. System filename search remains useful when you intentionally remove every native indexed folder; a new or reset configuration still indexes Home by default. Exact duplicate paths appear once with native metadata preferred. A system result may remain path-only when its metadata is inaccessible; discovery does not grant permission to read, preview, copy, or delete protected files. Search settings show the system database age and reuse an existing distribution `updatedb` timer or cron job without duplicating it. When the database is stale, **Update system filename database now** uses Ottrin's fixed-operation privileged helper. If no maintenance exists, **Keep plocate database fresh** can install an Ottrin-owned daily job and can remove that job again; OS/package-owned maintenance is never removed by Ottrin.

In an intentional Unix Administrator session, the application and Quick Inspector titles, the main title bar, and privileged text-save surfaces are visibly labelled **Administrator**. A new profile starts with no native index roots selected and browsing remains available without an index. Search Settings offers an explicit temporary session index for the currently selected locations. That index runs inside the Ottrin session, always keeps content extraction off, installs no service, and removes only its process-scoped runtime database when the session ends. Persistent Administrator indexing is a separate confirmed advanced action: it publishes metadata-only configuration into protected root-owned namespaces and installs the separately named `ottrin-administrator-index.service`. Status, restart, removal, retained-data, and explicit-purge actions never reuse or mutate the ordinary user service, configuration, cache, database, lock, or socket.

Built-in generated-folder skips such as `.git`, `node_modules`, `target`, `__pycache__`, and Trash directories keep indexing practical. If you need to search one of those trees, adjust search settings instead of relying on the watcher.

Content search follows its own privacy policy. Its selected roots are independent from the native metadata locations, and content indexing is off by default. Saved extraction and background watching also cover content-only folders, including folders excluded from native metadata. Content exclusions govern those bodies separately. Adding a content folder does not add its filenames to global native filename search. The service watches the combined active locations; disabling content or excluding a whole content root releases its content-only watches. When content indexing is disabled, Ottrin does not read file bodies for either saved-index or live fallback search. When it is enabled, only explicitly included roots are eligible; excluded file bodies and descendants are not searched by live fallback and remain out of saved-index results. Live fallback can still enumerate excluded directories while ripgrep applies those exclusions. Search Settings separately selects supported text-document and source-code/structured-text groups and exposes the maximum file size admitted for extraction (512 KiB by default). Unselected types and larger files are skipped without reading their bodies in both saved-index and live fallback paths. Resetting Search settings restores both sets of choices to their defaults, including Home for native metadata and a disabled content policy with no content roots.

## Command Line

Installed packages expose one `ottrin` command:

```sh
ottrin search "summer photos"
ottrin -s "summer photos"

ottrin index                  # update/reconcile the configured roots
ottrin index status           # read the completed index without starting a worker
ottrin index rebuild          # build and atomically publish a fresh replacement
ottrin index service-status   # inspect the persistent background service
ottrin index service-install  # install or upgrade the per-user service
ottrin index service-restart  # explicitly restart the installed service
ottrin index service-remove   # stop and remove the installed service

ottrin preview report.pdf
ottrin report.pdf              # open a file directly in standalone Quick Inspector
ottrin -p report.pdf
ottrin ~/Documents             # open the main window in this directory
ottrin --select report.pdf     # open its parent directory with the file selected
```

Search results show **Previous** and **Next** when another page is available, along with the visible range. The configured result limit controls the page size. Changing the query, scope, hidden-file option, content mode or sort starts at the first page. Results refresh when the saved index changes; if the current final page disappears, Ottrin returns to the last nonempty page.

`search` prints one matching path per line. Use `--limit 25 --offset 50` to request the third page of 25 results; offsets start at zero. Positive literal content queries count and sort all saved/live content matches before returning the page, rather than sampling the first matches from each source. Filename-only fuzzy suggestions are not added to content expressions. System filename searches and searches before Ottrin’s index is ready may still be incomplete; their completeness reporting remains unfinished. Use `ottrin search "invoice" --content` to also search the configured content folders; content search must be enabled in Settings → Search. `--json` works with this option, and an unavailable or failed provider returns an explicit error and a nonzero command exit. Current-folder scope uses the terminal's working directory. Linux and Windows packages include a pinned live-content provider and its notices, so a separate ripgrep installation is unnecessary. Add `--json` to search or the update/status/rebuild index commands for structured output. The `index service-*` commands expose the background-index lifecycle available in **Settings → Services**. Linux uses a persistent per-user daemon; Current local Windows builds use a persistent per-user daemon with an optional Task Scheduler login trigger. Closing the file-manager window keeps that daemon running. `ottrin-indexd` starts the daemon without arguments or with `--index-daemon`; use `ottrin index update` for a requested reconciliation and `ottrin index rebuild` for a rebuild. Windows service setup/restart waits for daemon readiness, and Stop/removal first releases the daemon and any owned running task before changing registration. Current installed-package checks cover daemon continuity through GUI close and subsequent file watching; exact reboot/login, upgrade and wider platform acceptance remain separately scoped in the roadmap. `index status` is read-only; watcher health is reported as unavailable when it cannot be proven from the persisted snapshot. Update and rebuild wait for the exact requested operation and return a nonzero exit code on failure. A failed or cancelled rebuild preserves the prior completed index. `preview`, `-p`, and a direct file argument open the same full Quick Inspector used by Space in the file manager and report missing paths or unavailable renderers explicitly. A standalone Inspector loads only the file's immediate parent as navigation context, so the arrow keys can move through visible siblings without recursively scanning or requiring that folder to be indexed. Its **Show in Ottrin** action reveals the current file in the main window. A direct directory argument opens that directory in the main window; `--select FILE` opens the parent and selects the file.

## Trash

Open the Trash section in the smart panel to review trashed items, restore items, permanently delete selected trash entries, or empty Trash. Empty Trash asks for confirmation and runs in the background.

Permanent deletion cannot be undone from Ottrin.

## Quick Inspector

Press Space on a selected item to open Quick Inspector. Ottrin previews common text and image formats directly and renders bounded multi-page PDF visuals with Previous/Next/page selection when its packaged PDFium runtime is available; it also shows extracted PDF text and metadata as a fallback when visual rendering is unavailable. On Windows, supported video files of at most 512 MiB may show one bounded still decoded by the Media Foundation codecs already installed with the operating system; unsupported, malformed, cancelled, oversized, or otherwise over-limit files remain metadata-only. XLSX/XLSM and ODS use bounded structured worksheet previews. Admitted DOCX packages use a bounded native structure view that preserves paragraphs, heading/style and alignment cues, bold/italic runs, source page-break cues, and bordered tables. It is labeled as native structured content and does not claim exact pagination, typography, images, or complex Office layout. Malformed, encrypted, oversized, or unsupported DOCX files and all PPTX files retain clearly labeled, left-aligned extracted-text/metadata fallbacks.

On Linux and supported local Windows NTFS files, complete admitted UTF-8 plain-text, text-equivalent and code files open read-only. Choose **Edit text** to enter the editor; preview loading and navigation never enter edit mode automatically. The limit is 128 KiB; binary controls, symbolic or hard links, sparse files, and metadata that cannot be preserved remain read-only. Code shows syntax colors, line numbers and a Find field; long source lines scroll horizontally. Use **Save** or Ctrl+S to save. A source conflict keeps your buffer and offers **Copy my edits** and **Reload from disk…**; reloading or closing with unsaved edits requires an in-app discard choice. **Keep editing** preserves your buffer. Reload takes a fresh source snapshot, so you can apply your copied edits to the current file.

Linux saves compare source identity, metadata and contents after staging, then atomically exchange the draft with the source. The displaced entry is validated after exchange; a detected race produces a persistent **Save needs attention** recovery conflict, not a completed save or an Undo receipt. Your buffer stays in the editor. **Copy recovery details** copies the exact recovery directory and explanation; **Copy my edits** copies the current buffer. Save stays disabled until you explicitly reload. Reload and closing a dirty editor use Ottrin's own discard dialog.

Once staging completes, a changed save retains a private `.ottrin-text-edit-<random>` directory beside the source: `edits` is an independent, synced copy of the draft, and after exchange `candidate` is the actual displaced entry, including a racing external version. Before exchange, `candidate` is the staged draft. Recovery can restore the displaced entry only into a vacant source name, without replacing anything; an occupied name is left alone because even an identity check followed by rollback would introduce another overwrite race. Inspect the current source and both recovery entries before merging. Recovery names are resolved from the pinned directory, so a parent rename during save does not redirect publication.

Recovery directories are intentionally retained even after successful saves and across application restarts. A writer may still have the displaced inode open and write to it after Ottrin validates it. There is no automatic pruning: only remove a recovery directory manually after closing other writers and checking that its versions are no longer needed. This uses additional disk space on every changed save. Ordinary permissions and ownership are preserved for the published file; unsupported ownership, metadata, filesystem operations or platforms fail closed. macOS remains read-only. Windows native public-session and source-built desktop checks pass; exact portable-package acceptance remains pending.

Windows uses a different retained-publication sequence: it holds a source handle that denies write/delete sharing during save, prepares and flushes an independent draft, moves the original to `candidate`, then publishes the edited file without replacing an occupied name. The source name is briefly vacant; this is not an atomic exchange. Recovery restores only a vacant name, retains every version and requires explicit reload after uncertainty. Windows refuses non-NTFS sources, object IDs, extended attributes, alternate streams, any present SACL (including audit rules and security resource attributes), and other unsupported file attributes before staging. These restrictions also apply to metadata added after opening. See the [Windows text-save boundary](WINDOWS_TEXT_EDITING.md) for the native evidence and limits.

The guarantee is retention, not exclusion of other writers: exchange never deletes the displaced entry, and the independent draft survives later changes to the published file. Source identity checks remain mandatory. Exchange itself changes ctime, so post-exchange validation compares inode identity, bytes and all other captured metadata; a ctime-only change cannot be distinguished from exchange. Retaining the prior inode covers that uncertainty. Subsequent external writes are not continuously monitored, and recovery files cannot protect against deliberate removal/modification by the same user, storage failure, or an unverified crash/power-loss scenario. Directory-sync failures remain visible as uncertainty and never become a clean save.

On Linux, Quick Inspector plays admitted local audio and video through GStreamer installed on this system. Ottrin does not include, download, install, or select codec implementations. Playback depends on the media plugins made available by the operating system or user. A missing helper, framework, plugin, or codec does not prevent Ottrin from launching: the Inspector reports the unavailable system capability and retains **Open externally**. Play and Pause are disabled after playback ends or fails; reopen the preview to start a new session.

On Linux, Ottrin detects a compatible installed LibreOffice provider at startup and uses it by default for isolated, page-faithful Office previews. Settings -> Files shows the detected version and executable path. Disable **Use installed LibreOffice for Office previews** there to use Ottrin's native structured or extracted fallback instead; the preference is saved and can be re-enabled without reinstalling or re-detecting the provider.

Damaged, encrypted, unsupported, or unrenderable PDFs fall back to extracted text and metadata without exposing raw PDF bytes to the image renderer. SVG is rendered only after bounded source, XML element/attribute, path-work, dimension, and pixel admission; external references are blocked. Semantic expansion through reuse, embedded content, CSS, patterns, masks, clipping, markers, and animation is not admitted. Filter support is limited to a bounded number of finite drop shadows and applications; rejected SVGs show the admission reason instead of a blank preview. On Linux and Windows, EPS preview uses a separately installed Ghostscript console executable when available. Windows checks versioned Program Files installations and absolute PATH entries. Ottrin neither bundles nor installs it, never executes PostScript in-process or through a shell, and contains each render in a non-networked sandbox with source, output, pixel, memory, process-count, and child lifetime limits; Linux also sets a CPU-time quota. If Ghostscript or the isolation runtime is absent, EPS retains metadata and **Open externally** instead of preventing Ottrin from launching. Legacy binary Office formats such as `.doc`, `.xls`, and `.ppt` currently show metadata and can be opened in their associated desktop application.

## Legacy Retained Move Records

Ottrin contains dormant/test-only journal and recovery machinery from an earlier
Move design. Legacy retained records can have source-level discovery and
manual-review surfaces, but the accepted Linux same-filesystem native-rename
path does not create or depend on them. No packaged or runtime recovery
capability is claimed. Cross-filesystem and Windows Move reject without mutation.

## Privileged Operations

Normal user-owned files do not need elevated setup. If an operation is denied, Ottrin can retry through a privileged helper when that helper is available.

On Linux, click the lock in the status bar or open **Settings → Services**, then choose **Set up administrator helper**. Approve the desktop's administrator authentication prompt. Ottrin installs its bundled helper and authentication policy and refreshes the status immediately. **Update / reinstall helper** repairs setup or installs the helper from your current app version. Cancelling authentication leaves setup pending and shows a clear message. This works for AppImage and portable packages without terminal commands.

Windows portable packages bundle `ottrin-priv-helper.exe` beside the app and use the Windows UAC flow when needed.

## Cleanup And Upgrades

Ottrin stores user configuration in the platform config directory:

| Platform | Config |
| --- | --- |
| Linux | `${XDG_CONFIG_HOME:-~/.config}/ottrin/config.json` |
| Windows | `%APPDATA%\Ottrin\config.json` |

Thumbnail cache files are disposable. If previews look stale after an upgrade, clear the cache from Settings -> Cache.

View Options in the navigation bar controls the active view: Column/List/Grid mode, image thumbnails in List/Grid, hidden files, sorting, and List columns. View mode is remembered as the global default for future launches. Thumbnail toggles are saved for the current folder so an image-heavy folder can show thumbnails without forcing every folder into thumbnail mode. Use Global View Settings from that menu to change saved defaults in Settings. Generated thumbnail cards cover supported text/code, PDF, font, SQLite, mesh, camera-RAW, and bounded Affinity previews; folders use a bounded direct-child collage. Windows can additionally show one Media Foundation still for a video format/codec supported by that Windows installation. Ottrin does not bundle, invoke, download, install, or recommend an external video decoder or codec pack. Office visual previews are available only in Quick Inspector. Common web/desktop images such as PNG, JPEG, GIF, BMP, TIFF, WebP, ICO, and safely rasterized SVG appear directly. Unsupported, malformed, or oversized inputs fall back to metadata rather than raw document or media bytes.

The SQLite search index is safe to rebuild from Settings -> Search.

Portable packages keep binaries and helper files together, but user configuration remains in the normal platform config directory. Removing a portable folder does not remove user configuration or cache data.

Ottrin also provides generated-data cleanup:

```sh
ottrin --uninstall
ottrin --uninstall --purge-config
```

## Current Limitations

- Windows packages have native checks for specific recorded builds; [ROADMAP.md](ROADMAP.md) binds their exact hashes and acceptance scopes. These checks do not establish complete Windows release acceptance. MSI registration rollback after a failed transaction remains a confirmed blocker.
- Windows UAC/helper behavior still needs real Windows validation.
- Windows GUI startup failures write `%LOCALAPPDATA%\\Ottrin\\startup.log` and
  show the local log location in a native error dialog. Wine launch evidence is
  limited: corrected probes reject any newly appended startup failure and prove
  the OpenGL path loaded Ottrin's app-local Mesa loader; real Windows
  native-driver validation remains required.
- Windows packaging is designed to use WGPU when Automatic finds no compatibility signal and includes a checksum-pinned Mesa 26.1.3 OpenGL compatibility runtime with app-local `opengl32.dll`, its `libgallium_wgl.dll` driver bundle, EGL/GLES libraries, and DXIL. Automated payload/provenance checks pass; Automatic selection, persisted preferences, diagnostic override behavior, and DLL loading still require real-Windows acceptance.
- Linux polkit helper packaging needs real system smoke testing.
- Earlier candidates have automated checksum, payload/provenance, Linux portable/AppImage launch, packaged-PDFium, and Wine compatibility-smoke evidence; those results do not certify artifacts from the current source revision. Clean-desktop Linux workflows, polkit dialogs, and real Windows runtime/install/UAC behavior remain pending.
- Quick Inspector covers text/code, images, PDF text/metadata, bounded native DOCX structure, clearly labeled Office extracted fallbacks, XLSX/XLSM/ODS worksheets, ZIP/archive listings, bounded folder summaries, and generic metadata, but the full keyboard/file-type matrix still needs manual QA.
- Installed-LibreOffice page rendering is source-complete on Linux but still requires representative document acceptance. HEIC/HEIF thumbnails and video thumbnails outside the source-complete Windows Media Foundation path remain unavailable. Linux system-GStreamer playback is source-complete but still requires exact-package playback and missing-runtime acceptance during final platform QA. The native DOCX view is intentionally structural rather than page-faithful. The Windows path still requires native and exact-package acceptance before release claims; it promises no codec list beyond the operating system. Its deadline and cancellation checks are cooperative around synchronous native codec calls, not a hard wall-clock guarantee, so hostile-codec timing remains unaccepted until native Windows testing. Quick Inspector shows a bounded recursive folder-size summary; unreadable, excessively deep, or very large trees are explicitly marked as incomplete. Editing is limited to complete direct UTF-8 text on supported Linux filesystems and local Windows NTFS; unsupported or metadata-sensitive sources remain read-only.
- Multi-select is implemented, but still needs manual QA across Miller/List/Grid.
- Tab drag-and-drop is implemented and test-covered; packaged/manual desktop acceptance is pending.
- Operation Activity can undo a completed session rename, a Linux single-entry or batch Copy using Skip, Rename, or Overwrite conflict handling, a newly created empty file or folder, a newly created symlink, a Linux permission change, one item moved to the native Linux home Trash, and a single-entry Linux same-filesystem Move using Skip, Rename, or Overwrite conflict handling. Before an inverse runs, Ottrin checks the entry identity, revalidates relevant attached-storage identity, and blocks overlap with an in-flight operation. Pathname inverses refuse to replace an occupied target. Copy, created-entry, and symlink undo atomically withdraw the exact published entry into a private retained transaction stage rather than deleting by pathname. For Copy or Move Overwrite, retained transaction state binds the exact displaced destination; undo atomically exchanges it back only while both the published entry and displaced entry retain their recorded identities. Move Overwrite keeps durable recovery evidence until cleanup completes; if cleanup fails after both pathnames are restored, Operation Activity keeps Undo available to retry cleanup. An interruption immediately after overwrite publication can be recovered from retained Move activity to restore both original entries. Created-file undo is refused after the file contains data; created-folder undo is refused after the folder contains an entry; all created-entry inverses refuse a pathname replacement, and symlink undo never dereferences its target. Permission changes reject symlinks and retain an identity-bound descriptor; undo restores the prior mode only if the same pathname entry still has the mode Ottrin applied, so replacements and subsequent permission edits are not mutated. Linux home-Trash undo binds the exact native Trash payload and restores it only while the original pathname is free; a replaced or missing payload is left untouched. Mounted-volume Trash, Windows Recycle Bin, Move batches, pre-existing/skipped Copy destinations and other operations remain recorded without this session-undo action. Quick Inspector image rotation rewrites the selected file without retaining its original bytes, so Operation Activity explicitly labels it **Not undoable** and does not offer a lossy counter-rotation. Manual retained-Move recovery is also **Not undoable** because it repairs an interrupted transaction rather than creating another inverse receipt.
- Batch Copy keeps an Undo receipt for each published entry. A later failure keeps the receipts for the completed portion in Activity. Undo runs in reverse order; if it stops, Activity reports the progress and retries only the remaining entries. Transaction stages are retained as owned Undo data, not reported as a cleanup failure merely because a successful copy has an Undo receipt. A replaced published entry is preserved. Linux retained Copy receipts and operation grouping can be restored after an application restart on the same boot when their filesystem evidence remains valid. On Linux, use **Discard Copy undo and clear…** on an Activity entry, or **Clear finished**, to discard retained Copy versions after confirmation. Cleanup runs in the background and leaves current files and original sources in place. It verifies the retained contents and filesystem identity before removing them. Changed or uncertain data remains in Activity with the error and a retry action. An interrupted final cleanup is restored as **Finish Copy cleanup** after reopening Ottrin on the same boot. Copy undo is revoked once cleanup intent is durable; the history entry is removed only after all retained payloads and recovery bookkeeping are reclaimed. Automatic expiry is disabled, and Clear keeps other operations that still own recovery data.
- Context-menu actions, native Linux/Windows Open With application launching, thumbnail view options, and status messages still need manual QA. Open With choices are refreshed before launch; if no native choices are available, the explicit manual application picker remains available.
- Indexed FTS5 content search supports negated content terms only against a completed indexed generation. Files with unknown indexed content (for example binary, oversized, unreadable, or excluded files) are omitted from complement results rather than treated as absent, and Ottrin reports that omission. Negated content search is unavailable while the generation is incomplete. Filename, path, extension, and type exclusions remain available; content-policy exclusions apply to both saved-index and live fallback search.
- Current Linux and Windows local artifacts have scoped native installation, private-display GUI and indexing evidence recorded in [ROADMAP.md](ROADMAP.md); those checks do not establish full daily-driver or release acceptance. Native DEB/RPM cleanup has passed scoped multi-user and session-daemon lifecycle checks on Ubuntu and Fedora; cross-version upgrades and custom runtime/configuration environments remain separate gates. Clean-desktop and physical graphics/workflow gates, the Windows UAC/upgrade/rollback matrix, and production signing remain open. macOS native construction and acceptance are deferred until Linux and Windows are complete.

Linux desktop windows use application ID `org.ottrin.Ottrin`, matching the native desktop entry. Wayland taskbar icons require that entry and the Ottrin icon to be installed in the desktop environment. Native packages provide them; AppImage desktop integration should install `org.ottrin.Ottrin.desktop` with its launch command pointing to the stable AppImage path, plus the `ottrin` icons. The main window, Settings, theme editor and Inspector share this identity.
