# Ottrin 0.1.0 Release Notes

Ottrin 0.1.0 is a development candidate of the keyboard-first file manager. Final daily-driver and public-release acceptance remains outstanding.

## Current local preview

Linux AppImage/portable/DEB/RPM use runtime `aa241633`; Windows ZIP/NSIS and
unsigned development MSIX17 retain accepted runtime `45830098`. Exact aa source
CI41894809859 passes all four Linux/Windows verification/performance jobs.
The stable AppImage is `dist/packages/Ottrin-0.1.0-x86_64.AppImage`.
`dist/packages/LINUX_PACKAGES_ACCEPTANCE.json` and
`dist/WINDOWS_CANDIDATE_ACCEPTANCE.json` bind the actual delivered hashes/scopes.

Linux administrator Copy now supports **Activity → Load administrator Copy
history** after restarting. This explicit authenticated scan reads canonical
journals without changing files. Undo and confirmed cleanup authenticate again
and freshly validate retained ownership. Protocol2 detects older helpers before
mutations. Current native AppImage checks cover setup/update cancellation,
authenticated helper replacement, protected Copy, normal GUI restart, cancelled
and authenticated history loading, Undo and cleanup. All16 reviewed captures and
independent root checks are recorded in `administrator-copy-status-current/`.
The corrected delivery removes raw cancellation errors and a stale history count.

Current native DEB acceptance covers normal GUI close and two actual LightDM
reboots. Four f8-to-aa AppImage/portable replacement cases preserve enabled/disabled
startup, Grid preference and database identity. The mounted daemon passes private
1000-file watcher/query/restart/two-shutdown checks at0.067% of one core over30seconds
without clients. These bounded software-X11/private-data results do not close
physical graphics, sustained personal workloads or whole-interface acceptance.

The personal managed service is Ready on the matching daemon, with startup choice,
exact preferences hash and database inode preserved. No new personal CPU sample
is claimed. Existing AppImage/portable users can update their system helper in
Settings → Services → Update / reinstall helper with administrator authentication;
this delivery did not silently replace the personal system helper.

The later source checkpoint `af2b0dc5` distinguishes recovered administrator history
from a known completed batch and refreshes its Undo availability while preserving
live receipts. Its502 UI tests and strict native/Windows lint pass. It is pushed
but not yet included in the delivered packages. Windows helper transport also has a source-only checkpoint: shared native pipe I/O
and direct UAC replace temporary request/response files and PowerShell. Ten focused
native checks include Unicode/spaced paths, peer/frame refusal and real UAC cancel/
approve from an ordinary desktop token. This is not a new Windows package or
administrator Copy acceptance. Windows privileged Copy, the full
theme/interface audit, remaining supported integrations, MSI rollback, hardware,
signing/Store and final owner acceptance remain unfinished.

A subsequent native run found that reloading an already-undone Copy showed a
misleading platform error. The correction carries explicit recovery states through
helper protocol3, preserving normal completed-Undo styling and guarded cleanup.
Changed retained contents still require review. Source checks pass; its rebuilt
package/native acceptance is pending. The rejected candidate was retired and did
not replace the usable previews above.

Completed Linux test VMs/volumes, profiles, credentials, mounts, transport files,
consumed staging and superseded packages are removed. Current deliverables and
useful compiler caches remain. Windows artifacts and protected environments are
unchanged. Packaged manuals are build-time snapshots; repository manifests and
roadmap record subsequent acceptance. These are local previews, not a public release.

## Highlights

- Ordinary Column-view startup now reveals the Home folder contents, including
  deep paths, while preserving saved view preferences. The exact packaged Linux
  workflow passes27 checks including verified copy/rename/trash bytes and retained
  recovery journals. These checks supplement, rather than replace, final user review.

- Switching from List or Grid to Column view reveals the current folder and selected
  file preview. Choosing Column view again preserves manual horizontal scrolling.
  Long preview metadata values show an ellipsis when space is tight, with the full
  value available on hover. Long breadcrumb paths scroll within the address area,
  keeping the current folder and view controls visible.

- Copy and Move check destination names in the background, keeping the window responsive during large or slow checks. Cancel remains available while checking, and a cancelled result cannot start a later transfer.

- Copy and Move share a themed conflict dialog with Keep both, Replace, Skip, and Cancel choices. Copy no longer silently chooses renamed duplicates when a destination name already exists. Replacement retains the existing guarded transaction and Undo behavior.

- Renaming a selected file keeps the new filename selected in List, Grid, and Column views, so subsequent Copy/Cut actions use the renamed file without requiring another click.

- Windows text editing preserves already exact file permissions instead of reapplying inheritance. This repairs safe-save refusals in private folders while retaining exact metadata checks.

- Positive literal content queries count and sort their complete saved/live match set before paging, deduplicate overlapping sources and use a path tie-breaker for stable pages. Query failures replace stale rows with current partial results and an explicit error. Search exposes Previous/Next and visible ranges; the CLI accepts `--limit` and `--offset`. Superseded content queries interrupt the live provider and release their saved-generation reader. Bounded system-filename/fallback completeness remains a separate gate.

- Saved content indexing and file watching now cover selected folders independently of filename-index roots, including configurations with no metadata roots. Content-only files stay out of global native filename results, and disabling content releases its watches. Missing content-only folders no longer prevent healthy filename indexing; watcher status reports the failure and retries automatically. This update rebuilds the derived search cache once; saved preferences and source files are preserved.

- Linux and Windows packages include the pinned live-content search provider. Provider failures are reported with repair/access guidance instead of silently returning empty matches. `ottrin search QUERY --content [--json]` uses the configured content policy, including content folders outside the native metadata roots and terminal current-folder scope.

- Windows cross-volume Replace batches now move their nonconflicting files and
  folders using non-replacing transactions. Existing conflicting destinations
  retain the full identity-bound replacement/recovery checks. The current internal
  installer passes twenty directory/mixed-batch GUI cases across NTFS volumes.

- Windows indexing now watches configured roots recursively instead of holding
  descendant directory handles that prevent ordinary populated-folder moves.
  Excluded events are filtered before waking the worker, and configuration
  replacement releases the full previous watch set.

- Quick Inspector opens text read-only and requires **Edit text** before accepting edits. Arrow keys navigate files and Space closes a read-only Inspector. Its title bar now uses the same edge-aligned layout as Settings. Width-fitted PDF previews reserve a stable scrollbar gutter to prevent repeated page resizing at the overflow boundary.

- Linux windows now identify as `org.ottrin.Ottrin`, matching the packaged desktop entry so Wayland desktops can resolve the Ottrin icon. All auxiliary windows share this identity; X11 desktop integration declares `StartupWMClass=ottrin`. Exact packaged verification is recorded in `dist/packages/DESKTOP_ICON_ACCEPTANCE.json`.

- Linux AppImage startup reuses an identical running index daemon across separate mounts. A changed daemon uses the orderly service upgrade path, avoiding a shutdown/start race. Cold startup allows 30 seconds for readiness. The enabled background service remains running after the window closes. Verified repeated and cold launches for packaged revision `756ee9405a906c444d7a54a0e1268a81f89d3946` (`022d0ee3b31215ac23362d65af76295d9835e196fb6edda1b4c4e05e5d438baa`); current receipt: `dist/packages/INDEX_ACCEPTANCE.json`.

- Miller, List, and Grid browsing with tabs, bookmarks, history, and Tandem View.
- Transactional copy publication with race-resistant overwrite, retained recovery data, and explicit partial-batch warnings. Linux same-filesystem Move is enabled for test-covered Skip/Rename/Overwrite/Cancel with explicit dispositions and conflict handling. Session undo now covers rename, single-entry Copy with Skip, Rename, or Overwrite conflict handling, newly created empty files and folders, newly created symlinks, Linux permission changes, one native Linux home-Trash item, and single-entry Linux same-filesystem Move using Skip, Rename, or Overwrite conflict handling with entry identity, storage, and in-flight path checks. Copy and Move Overwrite bind both the published entry and exact retained displaced destination and atomically exchange them back only while both identities still match. Move Overwrite retains a durable journal through undo cleanup, exposes cleanup failure as retryable after pathname restoration, and recovers both entries after interruption immediately following publication. Created-entry undo atomically withdraws the exact entry into private retained staging and is refused after a file contains data, a folder contains an entry, or its pathname names a replacement; symlink undo never dereferences the target. Permission changes reject symlinks and retain an identity-bound descriptor; undo restores the prior mode only while the exact pathname entry remains unchanged and still has the applied mode. Home-Trash undo binds the exact payload and refuses an occupied original path; mounted-volume Trash and Windows Recycle Bin entries do not claim session undo because their native metadata does not prove durable source-volume identity. Batches and other unproven operation types remain non-undoable. Quick Inspector image rotation is explicitly non-undoable because the in-place rewrite retains no original-byte receipt; Ottrin does not claim a lossy counter-rotation as recovery. Linux cross-filesystem Move is implemented with destination-local staging, verification, recovery and guarded source cleanup. Windows same-volume Move is source-complete; its cross-volume route remains disabled pending native acceptance. Exact-package and removable-volume acceptance remain outstanding.
- Indexed search with durable SQLite generations, watcher updates, scoped fallback, content snippets, and Linux global filename/path results merged with the system `plocate` database by exact path while preferring native metadata. First launch presents one persisted setup review for the three independent layers—system filenames, native metadata, and optional content—without overwriting existing scopes or enabling one layer through another. Native metadata locations and optional content-extraction locations are independent persisted choices, with content indexing disabled by default and visible persisted file-group and extraction-size controls enforced by saved and live search; resetting Search settings now restores the complete search policy instead of retaining content roots, exclusions, or indexed-volume choices. System filename search remains available when the user intentionally configures zero native roots, while new and reset configurations index Home by default. Existing distribution `updatedb` maintenance is detected and reused; stale database updates and explicit install/removal of an Ottrin-owned daily job use fixed privileged-helper operations rather than arbitrary elevated shell commands.
- Unix Administrator sessions are visibly identified in application, Quick Inspector, title-bar, and privileged text-save surfaces and can browse, open, and safely edit admitted text files without an index. A new Administrator profile has no implicit native roots. Search Settings can explicitly start a process-scoped temporary metadata index for selected locations only, or—after separate confirmation—install the protected `ottrin-administrator-index.service` with isolated root-owned configuration, state, database, lock, and socket namespaces. Content extraction stays off; status, restart, removal, retained-data, and explicit-purge lifecycle actions do not touch ordinary-user state.
- Quick Inspector for text, images, bounded multi-page PDF visuals with Previous/Next/page selection, light-sheet XLSX/XLSM/ODS structured worksheet previews, a bounded native DOCX structure view, bounded native visual pages for legacy DOC, a clearly labeled PPTX extracted-text fallback, archives, folders, audio metadata, and generic files. The DOCX view preserves paragraphs, run emphasis, style/alignment cues, page-break cues, and bordered tables without claiming exact Office layout; native legacy-DOC pages preserve useful text/table geometry but are not Word-perfect. On Linux, a detected compatible LibreOffice installation is shown in Settings and can be disabled or re-enabled; disabling it selects the native structured/visual or extracted fallback. PDF and Office previews retain extracted-text and metadata fallbacks.
- Linux Inspector audio/video playback uses an optional helper linked to the user's system GStreamer. The GUI itself has no GStreamer loader dependency, packages reject bundled GStreamer/plugin/codec payloads, missing system capability falls back truthfully, and **Open externally** remains available.
- Best-effort embedded Affinity preview for `.afphoto` and `.afdesign`: it uses only a bounded validated embedded PNG and is labeled as potentially different from the saved canvas; `.afpub` receives the same bounded classification and safe metadata fallback, while public visual coverage remains unclaimed pending a legal fixture.
- Linux Trash and Windows Recycle Bin support, plus scoped privileged-helper flows.
- Native Linux and Windows Open With choices retain opaque platform handler identities, are bound to the selected target, and are re-queried before launch. The explicit manual application picker remains available when native discovery has no choices.
- Packaging workflows target Linux portable/AppImage and native `.deb`/`.rpm`, Windows portable plus cleanup-aware per-user installers, with macOS deferred until the Linux/Windows roadmap is complete. The two native Linux formats share one `/usr` staging layout and depend on distribution runtime libraries instead of carrying the portable library closure. The macOS stager binds the GUI, index daemon, bundle metadata, icon, documentation, and source revision into one `.app`; the deferred macOS scripts still require native construction, launch, Finder, quarantine/Gatekeeper, and signing acceptance when that work resumes; exact-current-commit artifacts are not yet accepted.

## Important Limitations

- Damaged, encrypted, unsupported, or unrenderable PDFs fall back to extracted text and metadata without a visual page.
- Admitted DOCX packages receive a bounded native structured view, not a source-layout page renderer; exact pagination, typography, images, and complex layout remain unsupported. Malformed, encrypted, oversized, or unsupported DOCX packages and all PPTX packages retain clearly labeled extracted-text/metadata fallbacks. Damaged, oversized, unsupported, or unrenderable XLSX/XLSM/ODS packages also fall back to extracted text and metadata. Their visual output is a bounded content preview, not faithful Office layout.
- Legacy DOC receives a bounded Ottrin-native visual page when parsing/rendering succeeds, then falls back truthfully to extracted text/metadata. Legacy XLS can use an installed/enabled LibreOffice provider but has no Ottrin-native structured renderer; legacy PPT remains on the extracted metadata/text boundary.
- Legacy retained Move records have source-level startup/batch discovery and
  manual-review surfaces. They do not back accepted Linux same-filesystem Move,
  and no packaged or runtime recovery capability is claimed.
- Indexed FTS5 negated content search is not an accepted/supported behavior; positive indexed content search remains the supported path.
- Windows packaging is designed to use WGPU when Automatic finds no compatibility signal and to carry the checksum-pinned Mesa 26.1.3 OpenGL compatibility runtime, including app-local `opengl32.dll` and `libgallium_wgl.dll`. Automatic selection, persisted preference, explicit overrides, notices, and loaded-DLL provenance remain release gates for the exact rebuilt portable ZIP and installer.
- Linux and Windows CI, real Windows desktop and UAC/helper, packaged Linux polkit, and full packaged-workflow smoke tests are release blockers until manually signed off against the exact release commit and artifacts. Windows sign-off must cover the portable ZIP and per-user installer on the native WGPU default and Automatic compatibility paths, the persisted preference, and the explicit `--renderer opengl` / `OTTRIN_RENDERER=opengl` recovery path where applicable, including UAC/helper flows.
- Per-user MSI registration rollback is a confirmed native blocker: a failed cleanup can leave application files intact but Windows installation registration damaged. Two file-only control packages reproduce registration failure after actual file execution without Ottrin code. Normal install/upgrade/repair acceptance does not waive this failed-transaction gate. Exact scopes are recorded in the roadmap.
- No release is currently being scoped around these limitations. Full roadmap completion—including faithful Office layout rendering, remaining undo/file-operation work, package targets, and their required acceptance evidence—precedes any release. Linux system-GStreamer playback is source-complete but remains pending exact-package and physical playback acceptance. Tab reordering and List/Grid rubber-band selection have isolated Linux AppImage evidence; physical desktop and native Windows acceptance remain pending.

See [RELEASE_CHECKLIST.md](RELEASE_CHECKLIST.md) for the required sign-off matrix. Reproducible index measurements, methodology, and limitations are recorded in [SEARCH_PERFORMANCE.md](SEARCH_PERFORMANCE.md).

Linux production signing acceptance remains pending until the project OpenPGP
key signs the exact artifacts and downloaded signatures are verified. Direct
Windows ZIP/EXE/MSI downloads will be unsigned with checksums and explicit
publisher/SmartScreen disclosures; no paid signing certificate is required.
A Store-signed MSIX is planned once Windows is ready; no Store acceptance is claimed.
