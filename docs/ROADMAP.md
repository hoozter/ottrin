# Ottrin roadmap

Last updated: 2026-10-12

Ottrin will complete this roadmap before release. Sequencing may change, but product outcomes are not removed to make an earlier release.

Linux is the primary implementation and daily-driver platform. Windows follows the same product contract in parallel where that does not materially slow Linux, and both ship in the same release. Further macOS implementation and acceptance are deferred until the Linux and Windows roadmap is complete.

The roadmap names outcomes. Linked design and evidence documents define constraints; they do not create extra work by themselves.

## Current Linux delivery status

The saved-view reset was traced to UI tests writing the operator's settings; test profiles are now private, and the runtime restores the saved view. The remaining indexer CPU defect was reproduced with the packaged daemon and the operator's configured roots using a private database copy: ordinary file-change bursts spent about 94% of one core in subtree cleanup. Cleanup now follows indexed parent links instead of scanning the complete generation for each changed path. A bounded database-work regression protects that behavior.

The October 3 native clipboard-event repair is part of the current source. The September 30 AppImage predates that repair and must not be used to evaluate the corrected source. Local builds now put all deliverables from `build-all.sh` in `dist/packages/`; read each artifact's source revision rather than inferring freshness from its version number. Current package acceptance and remaining limits are recorded in `.hermes/coordinator-worker-report.md`, with October 9 baseline evidence in `.hermes/evidence/takeover-20261009/REPORT.md`.

These repairs do not close the full Linux daily-driver or Windows release gates below. Hermes remains paused; the old task board's completion labels are not release acceptance.

## Audited platform status — 2026-10-11

This snapshot reconciles the roadmap with local source, package provenance and retained acceptance receipts. It is not a fresh full-platform test run. `.hermes/coordinator-worker-report.md` is historical: its HEAD and Linux artifact inventory predate the changes below, and its `active` label does not establish a running worker or goal.

- **Linux:** the core browser, file operations, search and Inspector are implemented. Recent delivered changes cover saved-view preservation, indexer performance/service management, theme editing, target alignment, Settings/editor coherence and shared menu/choice-row alignment, repeated/cold AppImage startup, and native window desktop identity. The current stable AppImage is built from clean tracked revision `76f441f`, SHA-256 `4baf41eee3f8ee70d41fea40b2996c4b2389aa1e55d45c4e2fe8aa0e627122b9`. Rapid Shift+Delete now uses the press event’s modifiers: exact-package permanent/Trash confirmations, cancellation and file destinations pass Dark 100% / Light 150%. Refreshed portable/DEB/RPM identities pass; receipt `dist/packages/DELETE_SHORTCUT_ACCEPTANCE.json`. The following Activity receipt remains tied to its preceding `89fa193` hash. Activity now preserves running operations and Undo owners during history trimming and “Clear finished”; exact-package Dark 100% / Light 150% clearing and Copy Undo pass. The refreshed portable, DEB and RPM payload identities pass, with native installation and Windows refresh remaining separate. Receipt: `dist/packages/ACTIVITY_RETENTION_ACCEPTANCE.json`. Inspector and menu receipts below remain tied to their historical hashes. Inspector navigation now reads Previous / Page N of N / Next; exact-package two-page forward/backward behavior and minimum-size Dark/Light acceptance are in `dist/packages/INSPECTOR_NAVIGATION_ACCEPTANCE.json`. The preceding `b8d90a3` menu visual receipt remains historical. Main action menus now omit the unused check column, choice menus use a narrower check column, and view/sort options have complete semantic icons. Seven rendered widget regressions, all 345 regular UI tests and strict Clippy pass; exact-package main/view menu inspection passes Dark 100% and Light 150% with normal exits. Receipt: `dist/packages/MENU_REFINEMENT_ACCEPTANCE.json`. It includes the shared alignment repairs; their focused Dark/Light acceptance remains bound to historical revision `903c553` in `dist/packages/MENU_ALIGNMENT_ACCEPTANCE.json`. It preserves a visible adjacent Miller preview when the window shrinks, without overriding deliberate horizontal scrolling. Focused Dark/Light native Linux resize acceptance is recorded in `dist/packages/MILLER_RESIZE_ACCEPTANCE.json`; native Windows text-preview resize also passes at 100%. The preceding `6e8cef0` package’s shared button wrapping, preview toolbar layout and exact-package local file operations are recorded in `dist/packages/PREVIEW_TOOLBAR_ACCEPTANCE.json`. The earlier Copy/create directory-guard and private mount proof remains in `dist/packages/COPY_DIRECTORY_OWNERSHIP_ACCEPTANCE.json`, bound to that receipt’s historical checksum. The preceding `771eae8` AppImage had SHA-256 `312eb7a0907e3895aee45ef5c087be315c5a6d94f6f70d44f3a3cff7b237a7c7`. Its rapid path-entry/Select All and local Copy/Move/Cancel/Overwrite/Undo acceptance is in `dist/packages/LOCAL_PATH_ADMISSION_ACCEPTANCE.json`; batch Copy Undo and receipt ownership are now accepted at this scope; safe transaction expiry/reclamation remains open. The preceding `73b913d` AppImage had SHA-256 `9c0c353435ea5fb640616c6ac3caf1c3d44ee7dd42c675f4e792d85de0483efb`. Historical exact-package Xvfb checks on that `73b913d` artifact at 1100 × 690 and 800 × 600 verify startup file selection reveals its full adjacent preview, ArrowUp preserves column positions, and both launches exit cleanly; receipt `dist/packages/MILLER_STARTUP_ACCEPTANCE.json`. `dist/packages/` also contains refreshed portable, DEB and RPM candidates from clean package revision `6e8cef08a2fc8f812bc515b875205230440b7166`, reusing the accepted `6e8cef0` release binaries; all three now contain the local-path admission, shortcut, batch Copy Undo and mount-safe directory-guard repairs. Portable PDFium placement is corrected and an exact archived-app PDF preview passes. Portable/DEB whole-file identity and RPM code/read-only-data identity after standard stripping are verified. All three extracted candidates pass startup selection/full-preview and ArrowUp checks at 1100 × 690 and 800 × 600 in private Xvfb/DBus, with clean exits and cleanup; this does not complete clean-distribution installation/upgrade acceptance. Checksums, source provenance and exact scope are recorded in `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json`. Fresh Ubuntu 24.04.5 VM DEB installation, regular-user text-preview launch, same-version reinstall/uninstall, preference preservation and real systemd user-service install/status/restart/removal now pass; all five installed executables match accepted release binaries. Receipt: `dist/packages/DEB_INSTALL_ACCEPTANCE.json`. Full desktop integration, cross-version upgrade and fresh RPM installation remain open. Earlier receipt results apply to their recorded artifacts, not automatically to the newest package. The desktop-identity receipt verifies private KWin Wayland protocol, not a visual host-taskbar acceptance.
- **Linux remaining before the new deferred outcome:** clean-desktop package installation/launch; the consolidated daily-driver workflow including cross-volume conflicts, interruption/recovery and editor recovery; real removable-volume lifecycle; supported installed-LibreOffice conversion/fallback; audible representative media; the packaged helper/polkit permission matrix; and an explicit assistive-integration decision. Existing Xvfb and focused native checks narrow these tasks but do not close them. Accessibility is currently disabled after an upstream lifecycle defect.
- **Windows:** native ordinary-user evidence exists for text saving, many Move/recovery cases, per-user scheduled indexing, Office conversion before the unfinished containment changes, and a generated video thumbnail. This is substantial implementation progress; focused package checks below do not close full Windows release acceptance. Cross-volume Move is enabled in source after native transaction and guest power-cut checks; its public workflow/package acceptance remains open. The Office UI provider remains gated. Historical AppContainer diagnostics passed isolation fixtures but timed out on DOCX and Kit initialization; the committed production helper now supersedes that conversion path. See `.hermes/evidence/windows-20261010/office-appcontainer-private-temp-env-results.log`, `office-appcontainer-kit-budget-results.log`, `office-appcontainer-private-temp-docx-results.log` and `office-appcontainer-kit-stack-results.log`. A fresh isolated guest on October 11 passes all ten containment/process fixtures after making fixture executable access self-contained (`9d20378`). The Windows production converter now uses the packaged LibreOfficeKit helper instead of the CLI global pipe denied by AppContainer. Two fresh-profile production DOCX checks pass (7.31s and 9.93s), covering PDFium rendering/text, source preservation and cleanup; in-flight cancellation passes (2.65s). All 17 Linux provider contracts and strict Windows helper Clippy pass. The latest contained nine-format sweep passes DOC/DOCX/ODT but times out on XLS/XLSX/XLSM/ODS/PPTX/ODP. An uncontained ordinary-user Calc control also times out; a message-pump experiment returns a load failure. Both threading experiments are rejected, with production code unchanged. Macro suppression is unaccepted because its positive control did not execute. All-format/fidelity/fallback and exact-package acceptance remain open, so the Office UI provider stays gated. Current native evidence is in `.hermes/evidence/windows-20261011/REPORT.md`.
- **Windows remaining before the new deferred outcome:** repair and accept Office containment/conversion/fallback; accept enabled cross-volume Move through the public workflow; construct current portable/MSI/installer candidates and accept editing, Move, installation/upgrade/rollback/uninstall; validate native/Mesa graphics, representative media, UAC, storage attachment/network-drive states and the consolidated daily-driver workflow.
- **Both platforms:** the final exact-candidate workspace/CI, package-integrity, production-signing and downloaded-artifact checks remain open. The required picker/folder integration below adds a deliberate later milestone. No percentage or calendar estimate is claimed: the open outcomes vary greatly in size and several need native/device acceptance. macOS remains deferred.

## How to use this roadmap

- Work on the first dependency-ready outcome in the order below.
- Prefer the smallest user-visible or release-enabling slice.
- During implementation, run focused tests for the behavior changed. Run broad workspace, package and platform matrices only at an integration milestone or against the exact release candidate.
- Complete source outcomes before physical QA. Batch desktop, device and exact-artifact acceptance near the end. Run an earlier physical check only when runtime evidence is required to choose or unblock the implementation; unavailable hardware does not justify synthetic QA infrastructure.
- Research is allowed only when a named product decision blocks the next implementation slice. Answer that decision with bounded evidence, record it, and stop.
- Historical spikes, possible architectures and provider comparisons are references, not active tasks.
- Do not reopen accepted boundaries without a new product decision. File Move is a required product capability on Linux and Windows; safety semantics are implementation scope, not a feature gate. Ottrin does not silently install renderers, codecs or privileged services.

## Implemented foundation

- Miller, List and Grid views; tabs, bookmarks, path navigation and keyboard navigation.
- Tandem View, multi-select, context menus, Smart Panel sections and view options.
- Copy, rename, Trash, permanent delete and validated New File/New Folder flows.
- Linux same-filesystem and cross-filesystem Move with explicit Skip/Rename/Overwrite/Cancel handling. Cross-filesystem files, directories, and symlinks use destination-local staging, complete-tree verification, durable publication evidence, guarded recursive source cleanup, restart recovery, honest batch/Activity accounting, and evidence-bound single-entry Undo. Native packaged interruption and removable-volume acceptance remains in final Linux QA.
- Windows same-volume Skip/Rename Move is source-complete through identity-pinned source and destination handles plus `NtSetInformationFile(FileRenameInformation)` no-replace publication. Occupied regular-file Overwrite uses a current-user-private, same-volume transaction plus paired durable journals, identity-pinned source/target/retained evidence, `ReplaceFileW` displaced-file retention, and topology-driven restart recovery before later Moves. Cross-volume transaction code covers unoccupied, Rename, and Overwrite regular-file and top-level symbolic-link targets, plus Skip, Rename and Overwrite directory trees containing symbolic links, through a current-user-private destination stage, paired durable journals, complete manifest/content verification, identity-pinned publication, retained displaced-target evidence where applicable, and source identity/content revalidation. Non-symbolic reparse points remain rejected. Native acceptance has exposed and repaired stream enumeration and handle-lifetime defects. The Windows public cross-volume route and retained recovery are enabled in source after native transaction and guest power-cut evidence. Their public GUI and exact-package acceptance remain pending.
- Session undo for rename, Linux single-entry and batch Copy with Skip, Rename, or Overwrite conflict handling, newly created empty files and folders, newly created symlinks, permission changes, one proven Linux home-Trash entry, and single-entry Linux same-filesystem Move with Skip, Rename, or Overwrite conflict handling. Pathname inverses are guarded by link-entry identity, no-replace publication, storage identity, and in-flight path conflict checks. Copy and Move Overwrite additionally bind the exact retained displaced destination and atomically exchange it back only while both entries retain their recorded identities. Move Overwrite keeps durable recovery evidence until undo cleanup completes and can roll back both entries after interruption immediately following publication. File and folder creation undo is offered only while the exact entry remains empty and unchanged; symlink undo never dereferences the link target. Linux permission undo retains an identity-bound descriptor and restores the prior mode only while that same non-symlink entry still has the mode Ottrin applied. Home-Trash undo binds the exact native payload and refuses an occupied original path; mounted-volume Trash and Windows Recycle Bin entries remain outside this session-undo guarantee because their native metadata does not prove durable source-volume identity. Multi-entry Copy and Move undo remains unavailable: independent pathname inverses cannot be committed atomically, so a conflict on a later inverse could leave a partially undone batch that no retry can safely reconstruct. Operation History now labels this boundary explicitly as `No batch undo` instead of implying that a batch receipt exists. Quick Inspector image rotation remains an in-place rewrite with no retained original-byte or identity-bound transaction receipt, so Operation History truthfully labels it `Not undoable` rather than offering a lossy counter-rotation. Manual retained-Move recovery is likewise labelled `Not undoable`: it repairs an interrupted transaction but does not create a second inverse receipt.
- Linux and Windows Trash backends.
- SQLite search, Linux `ottrin-indexd`, watcher updates, scoped content snippets and result actions.
- Linux global filename/path search merges native metadata with system `plocate` results, works with zero native roots, and deduplicates by exact path with native metadata preferred.
- Linux system filename maintenance detects and reuses existing distribution timers or cron jobs. Stale databases can be updated through the fixed-operation privileged helper; Ottrin offers its own removable daily job only when no existing maintenance is detected.
- Native metadata roots and optional content-extraction roots are separate persisted user selections. Content indexing is off by default, reads only explicitly selected content roots, and has independent exclusions.
- Quick Inspector for text, images, multi-page PDF, spreadsheets, archives, folders and generic metadata.
- Source-complete shared preview sizing: whole-image fit without automatic upscaling, explicit Fit page/Fit width/100% controls, effective fit percentages and manual zoom preserved on resize; continuous PDF pages in Fit width; adjustable flowing-text/DOCX sizing; readable spreadsheet rows with explicit Overview. Focused source and harness gates pass; [landing evidence and remaining acceptance boundaries](PREVIEW_SIZING_EVIDENCE.md) distinguish inherited packaged Inspector evidence from pending editing, accessibility and platform acceptance.
- Native structured DOCX preview with headings, runs, alignment, lists, page-break cues, tables and bounded inline raster images. Unsupported documents retain a labelled extracted fallback.
- Linux installed-LibreOffice provider pipeline in source: bounded discovery, probing, isolation, validated PDF rendering, provider labels, page navigation, stale-response rejection, persisted enable/disable control and native fallback.
- Windows Job Object Office process boundary in source: atomic job assignment, kill-on-close descendant ownership, aggregate/process memory and active-process limits, explicit standard-I/O handle inheritance, sanitized private environment, timeout/cancellation cleanup and private profile/output path translation. The current AppContainer layer admits a read-only private input snapshot and no network capabilities; ten native process/containment fixtures and bounded installed-provider DOCX checks pass; the Windows provider remains disabled pending remaining containment/UI acceptance.
- Direct UTF-8 editing on Linux for complete admitted plain-text, text-equivalent and code files up to 128 KiB. Ordinary ext4 extent-backed files are admitted; metadata-sensitive and binary inputs remain read-only. Code uses syntax colors, aligned unwrapped source lines with line numbers, and Unicode-safe in-editor search. Save conflicts retain the buffer and provide in-app copy/reload/discard actions. Linux publication uses atomic exchange with retained displaced entries and independent durable drafts. Post-exchange conflicts retain the editor and identify recovery data; restoration never replaces an occupied name. The retention semantics and limits are documented in [USER_GUIDE.md](USER_GUIDE.md).
- Linux and Windows native Open With discovery. Choices retain opaque native handler identities and are re-queried before launch; unsupported platforms retain the manual picker.
- Folder thumbnail collages, folder-size metadata and generated text/source cards.
- Source-complete Windows Media Foundation video thumbnails with truthful metadata fallback.
- Source-complete Linux media playback through an optional system-GStreamer helper: the GUI remains launchable without GStreamer, admitted descriptors cross the helper boundary, and portable/AppImage verification rejects bundled framework, plugin and codec payloads.
- Compact and expanded Locations rail with stable identities and no automatic external-volume indexing.
- Theme presets and custom themes.
- Linux portable/AppImage and Windows portable/installer packaging scripts.
- Native Linux `.deb` and `.rpm` packaging from one `/usr` staging source of truth. Native packages use distribution runtime dependencies rather than bundling the portable library closure; locally built archives have been inspected for payload, executable modes, dynamic resolution, and dependency metadata.
- Source tests, formatting and strict Clippy gates.
- Cohesive first-run Search setup for the three independent layers: system filenames, native metadata and optional content. The completed choice is persisted without overwriting existing search scopes.
- Packaged Linux `ottrin-indexd` lifecycle with install, status, restart, rebuild, configuration propagation, graceful upgrade migration and removal, verified under an isolated per-user systemd manager with concurrent clients, watcher reconciliation and persisted-state recovery.
- Source-complete Windows background index lifecycle using the current user’s Task Scheduler: install/upgrade replaces the configured periodic task after final payload publication, status uses locale-independent task XML, restart requires an existing task, and removal refuses to mask scheduler cleanup failure. Installer ordering and the Windows cross-build are verified. Ordinary-user native Task Scheduler acceptance now covers install, four-to-five-hour upgrade, status, actual scheduled indexing with searchable fixture results, removal and absent-task restart refusal. Exact installer integration remains in final Windows QA.
- Per-user Windows MSI candidate built from the same provenance-checked portable stage as the NSIS candidate. It schedules application-owned background setup after file publication and blocks uninstall before `RemoveFiles` when application-owned integration/data cleanup fails. WiX source/package contracts, native x64 construction and exact-package silent install/uninstall are verified. Recorded QA derivatives pass a version upgrade and early injected failure without losing the prior installation. Later custom-action rollback, fresh non-administrator interactive installation and Installed Apps acceptance remain in final Windows QA.
- Source-complete macOS app-bundle ZIP and DMG with one staging layout for the GUI, index daemon, bundle metadata, icon, release documentation, and source provenance. The release workflow uses an ephemeral keychain to Developer ID sign the app, notarizes and staples the app and DMG, publishes fresh checksums, and verifies downloaded identities, tickets, and Gatekeeper assessment. Native construction, launch, Finder behavior, and production-identity acceptance remain pending on macOS.
- Source-complete Unix Administrator profile with visibly identified application and save surfaces, index-free direct file use, process-scoped temporary indexing, and a separately confirmed persistent root service isolated in protected root-owned configuration, state, database, lock and socket namespaces.
- Source-complete storage locations: Linux mounted/removable roots and Windows fixed, removable, optical and mapped-network drives are discovered with stable identities; attachment/removal refresh retains unavailable locations without redirecting operations; direct browsing remains independent from explicit indexed-volume opt-in.
- Source-complete release signing: the tag workflow imports an approved Linux OpenPGP identity, creates armored detached signatures for the portable tarball and AppImage, signs Windows payload binaries before packaging and the final portable/MSI/installer artifacts afterward, and Developer ID signs, notarizes, and staples the macOS app and DMG. It recomputes checksums after signing and verifies downloaded Linux signatures, Windows Authenticode identities, and macOS team identities/tickets/Gatekeeper assessment before publication. Production identities and exact-artifact acceptance remain in final platform QA.

## Remaining work

### 1. Finish previews and editing

- Native Windows 11 build 26100 fixtures passed on 2026-10-10 as the ordinary desktop user: startup inside the Job Object, output capture/budget, diagnostic cleanup, timeout after parent exit, cancellation of the process tree, per-process and aggregate memory limits, active-process limits and inherited-handle restriction. The run exposed a dangling Job Object attribute buffer causing CreateProcessW error 6; the attribute list now borrows caller-owned value buffers through destruction. Commands are in [DEVELOPMENT.md](DEVELOPMENT.md#windows-office-process-boundary), with RED/GREEN evidence in `.hermes/evidence/windows-20261010/`. A supported LibreOffice 26.8.1 installation is now present in the disposable guest. Ordinary-user production discovery, DOCX conversion through the Job Object, PDFium rendering/text content, provider labeling, private-profile cleanup, source preservation and pre-cancellation pass. Windows discovery was repaired after a native `NotInstalled` regression. Historical pre-AppContainer CLI fixtures passed all nine advertised formats. That evidence does not accept the current contained Kit pipeline: its latest sweep passes only DOC/DOCX/ODT and times out on six spreadsheet/presentation formats. In-flight cancellation passes after LibreOffice acquires its real profile lock. The converter now follows the original Windows input basename after a native filename regression. A longer private-directory layout reproduced a provider crash; shortening the authoritative disposable-profile name fixes it, with all three native acceptance fixtures passing in the original failing layouts. A native fake-credential regression now verifies a private Unicode environment, with all eight process-boundary tests and three installed-provider fixtures passing. Representative fidelity, constrained-token/filesystem isolation, network denial and UI fallback presentation remain pending; the Windows UI provider stays gated until its remaining acceptance is complete.
- The approved Linux editable-text publication contract is source-complete for plain text, text-equivalent/code and Markdown files. Saving retains both the independent draft and the actual displaced inode in a private source-local recovery directory. Final-window content, replacement and permission races are detected after exchange; a recovery conflict keeps the buffer and exact recovery location visible and disables further saves until explicit reload. Restoration only fills a vacant name without replacement; an occupied or failed recovery leaves all retained versions intact. Both affected directories are synced, and uncertainty stays visible. Prior inodes are never automatically pruned, including after clean saves, because late descriptor writers cannot be ruled out. Exchange-induced ctime changes, later external edits, same-user tampering and crash/power-loss limits are stated in the user guide; this is a retention guarantee, not generation-conditional overwrite or writer exclusion. Focused tests cover final-window mutation, inode identity, permissions, late descriptor writes, restoration refusal/failure, durability failure, retention and editor recovery. Workspace and source-built Linux acceptance evidence is recorded in `.hermes/text-publication-20260923/`. The exact AppImage from `cabfc8e4f0a6e4046581e73224eef5b2f9aac944` has packaged-editor byte identity, documentation and isolated headless-entrypoint evidence there; final-window race injection is test-only and has no packaged control, so packaged interactive recovery acceptance remains pending. Native crash/power-loss and physical/Wayland package acceptance remain unclaimed. Windows native text publication and source-built desktop acceptance are recorded below; macOS stays read-only. Additional document formats are not authorized.

- Windows text-save candidate: seventeen native ordinary-user fixtures pass on Windows 11 build 26100, including exact bytes/security preservation for inherited, protected and explicit DACLs, retained originals/drafts, sharing refusal and collision/failure recovery. Unicode names, ancestor-rename refusal, eight process-exit boundaries, and writable-mapping refusal/retry have native evidence; object IDs and all present SACLs are refused before staging. Actual audit-only, mixed audit/resource and scoped-policy fixtures confirm refusal without metadata loss; extended attributes are refused before staging both at admission and after session opening; partial-SACL publication was removed after it dropped an audit rule. Candidate ACL preparation now preserves the automatic-inheritance state through the filesystem security API; exact descriptor validation remains mandatory. Public editing is now enabled for admitted local NTFS files: all 22 native text checks pass, and the source-built desktop Inspector verifies exact saving, retained original/draft, external-change refusal and confirmed reload. Focused exact portable-package Markdown save/conflict/reload acceptance now passes with exact saved bytes, retained Unicode original/draft, external-change refusal, Keep editing, Copy my edits and confirmed reload. [WINDOWS_TEXT_EDITING.md](WINDOWS_TEXT_EDITING.md) records the current archive/hash, limited interactive token, diagnostic OpenGL scope and remaining platform limits.

### 2. Complete file operations and desktop workflows

- **Linux admission repair and acceptance:** Clipboard and queued commands use one fallible admission path with per-path Linux unique mount IDs in addition to discovered-volume checks, correct source/destination symlink semantics and retained unavailable-volume tombstones. Ordinary-user worker/regression tests and a real private-namespace mount replacement pass; Linux and Windows strict Clippy pass. This requires Linux 6.8+. The preceding clean `771eae8` AppImage passes rapid Ctrl+L path entry and two-file Ctrl+A selection, Copy, conflict renaming, reverse batch Copy, Move, Cancel, overwrite retention and overwrite Undo on two distinct local tmpfs filesystems outside Home. Exact byte checks, normal exit and fixture/process cleanup pass. Main command shortcuts use key-event modifiers; address fields receive focus before rendering. Both released-chord/same-frame typing regressions and all 343 regular UI tests pass. Receipt: `dist/packages/LOCAL_PATH_ADMISSION_ACCEPTANCE.json`. Physical removable acceptance remains open. Batch Copy source now retains per-file receipts in Activity, removes the spurious cleanup warning on successful publication and supports reverse-order Undo with retained progress on failure. All 329 platform and 343 regular UI tests pass, including displaced-byte restoration, replacement refusal and retry of only remaining entries. Exact rebuilt-AppImage GUI checks pass two-file cross-filesystem Copy and Undo with source bytes retained, the new action label and no spurious cleanup warning. The full rapid-path/Copy/Move/Cancel/Overwrite/Undo sequence also passes, with normal exit and owned fixture/process cleanup. Receipt: `dist/packages/BATCH_COPY_ACCEPTANCE.json`. Safe expiry/reclamation of session Copy transaction stages remains daily-driver work. The current `4c0961b` source and AppImage use shared nonblocking inotify identity guards and unique mount IDs for Copy/create receipt directories, with short-lived, verified directory-relative handles for the inverse. It refuses replaced parents/stages and redirected destination aliases, including published hard links preserving file inode identity. Long-lived directory handles were rejected because they prevent ordinary unmount. Dedicated private mount acceptance covers ordinary unmount with live receipts, tmpfs replacement, and same-inode bind remount. History eviction/Clear and safe recursive reclamation remain unfinished. All 335 regular platform tests, 343 regular UI tests, strict Linux/Windows cross-target Clippy and the private mount fixture pass. The rebuilt exact AppImage passes rapid path entry, Copy/conflict rename/batch Undo/Move/Cancel/Overwrite/Undo with exact bytes and normal exit; successful Undo messages use normal muted text. Owned apps, daemon, bus, display and fixtures are removed. Receipt: `dist/packages/COPY_DIRECTORY_OWNERSHIP_ACCEPTANCE.json`. Physical removable-device acceptance and native Windows remain open. Evidence and scope: [STORAGE_LOCATIONS_STATUS.md](STORAGE_LOCATIONS_STATUS.md#current-linux-clipboard-path-admission-gap).

- Miller startup selection now requests the same one-shot row/column reveal as explicit navigation, showing the selected file and its adjacent preview rather than leaving the preview off-screen. A rendered deep-path regression reproduces the missing reveal before the fix and passes afterward; actual ArrowUp selection preserves the settled horizontal offset. Existing Miller/pointer navigation tests and strict UI Clippy pass. The private reveal state has one shared implementation for startup and navigation. Native Windows portable acceptance now passes at 1100 × 690 and 800 × 600: selected file/full adjacent preview visible, ArrowUp changes selection without shifting columns, clean exits. Exact portable source `5a6b001895eec2a3c8a426b7cd0ae1acbdf9a2ad`, SHA-256 `f27508c5d85b812a7512585eedf0c0c5c676bb10b619ad869e2da79907861e4d`; receipt `.hermes/evidence/windows-20261011/WINDOWS_MILLER_STARTUP_ACCEPTANCE.json`. Linux AppImage acceptance also passes at both sizes using private Xvfb/DBus and the exact clean-revision `73b913d` package; receipt `dist/packages/MILLER_STARTUP_ACCEPTANCE.json`. Portable/DEB/RPM also contain the fix and pass the same extracted-package layout checks; exact current identities are in `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json`. The current NSIS and MSI installers also contain this fix. Full platform acceptance remains separate. Historical reproduction: `.hermes/evidence/windows-20261011/text-package-start.png`.

- Complete native Windows Move acceptance against the same product contract. Ordinary-user fixtures on Windows 11 build 26100 cover ten same-volume cases and cross-volume file/directory/symlink Move on two NTFS volumes; Skip/Rename/Overwrite with exact displaced retention; raw nested links and empty directories; creation/modification times and hidden/archive/read-only metadata; changed-source/published-entry refusal; adverse sharing and destination ACL refusal/retry; and process exits during staging, publication, source claim and displaced retention. Read-only-source publication recovery survives repeated exits without clearing original source attributes. Read-only destination files survive Skip/Rename; Overwrite safely refuses until destination protection is removed, then ordinary retry retains the exact displaced file. Source cleanup rechecks named streams added after interruption. Journal namespaces are explicitly flushed, with native durability-failure/refusal/retry evidence. Explicit recovery shares the ordinary recovery gate; native checks cover closed-gate refusal, idempotent completed recovery, retained originals/later edits and malformed or misnamed journals. A separate native lock-owning process blocks both explicit recovery and ordinary retry without changing journal or file bytes; killing it releases ownership, after which recovery completes with exact displaced retention and remains idempotent. Strict Windows platform Clippy passes. These fixtures are not power-loss or exact-package acceptance. Directory process-exit acceptance now covers initial journal publication, candidate unlock/publication, before source cleanup, source unlock and source claim. The initial central/local journal gap is repaired only for an empty, identity-bound initial transaction; staged-data and later missing-journal cases remain refused. Native FAT32 boundary checks now prove safe refusal in both directions for Rename/Overwrite with both files unchanged; this does not claim FAT32 Move support or physical removable-device acceptance. Extended attributes are now refused at admission and cleanup, with native exact-retention checks for files, directory roots and nested entries, including attributes added after interrupted publication. Completed non-Overwrite Moves now retain a durable central completion receipt through ordered, flushed teardown; native tests cover deletion failures, process exits and preservation of mismatched or replaced recovery data. Top-level file and directory symlinks now refuse link-owned named streams at admission and cleanup; native checks distinguish those streams from target data and preserve streams added after interrupted publication. The bounded independent review confirms this stream-refusal repair. Source-built Windows GUI acceptance now covers Cut/Paste Overwrite and idempotent same-volume recovery with exact displaced-byte retention. It exposed a storage-root path mismatch, repaired at discovery. Cross-volume recovery now exposes the normal action; recovery revalidates the journal, identities and content before mutation. A fresh network-isolated Windows guest on October 11 passes all 21 current cross-volume native fixtures (15.69s), the FAT32 refusal fixture in both directions (0.05s), and 102 default platform tests (6.60s; 23 ignored), as the limited interactive user; strict Windows all-target platform Clippy passes. Evidence: `.hermes/evidence/windows-20261011/MOVE_ACCEPTANCE.json`. Separate ordinary-user Overwrite fixtures now survive hard guest power cuts at BeforeSourceCleanup for a regular file and SourceClaimed for a directory tree. Both verify published/displaced content and identity, recovery and its idempotent repeat (0.10s and 0.09s). The directory check verifies the claimed source identity, nested payload, empty folder and drained claim. Windows needed one normal restart from Automatic Repair after repeated cuts; fixture state survived. Evidence is `POWER_CUT_ACCEPTANCE.json` and `SOURCE_CLAIM_POWER_CUT_ACCEPTANCE.json` beside that receipt. The temporary Windows acceptance gate and override variables are removed; public Move and retained recovery use the authoritative verified transaction path. A fresh no-override run passes 21 cross-volume cases, FAT32 refusal and 99 default platform tests. The final public-command fixture also passes single-file and batch Move across NTFS volumes (0.69s), with explicit dispositions and no unsupported undo receipt. Strict Windows platform/UI all-target Clippy passes; `PUBLIC_MOVE_ACCEPTANCE.json` records the exact binaries and scope. The clean `9335cff` Windows portable candidate built successfully (SHA-256 `9f3669693a75564542712c7a94381c35104e2d123138554756dd4077d08357b6`); all 49 archive files match its stage. Ordinary-user packaged launch passes with explicit OpenGL, but Cut/Paste to a typed `Q:` destination exposed a lexical mismatch against the discovered verbatim drive root. Both files remain unchanged; this candidate is not accepted for that workflow. The shared storage-root comparison now recognizes ordinary/verbatim drive and UNC prefixes without resolving links or changing the operation path. Its ordinary-user native Windows regression passes valid aliases and missing/replaced/unrelated-volume refusals; strict Windows UI all-target Clippy passes. Evidence: `STORAGE_PREFIX_ACCEPTANCE.json`. The rebuilt clean `56f0d4e` package (SHA-256 `528dc46064877ead4e433ecd955d345802f200229e940acd0001196b5ca9f099`) passes limited-user Cut/Paste/Replace across C:/Q: with source removal, exact published/displaced bytes, retained file identity, paired journals and bundled graphics module paths verified. A VHDX attachment-filter limitation caused the earlier ACL failure; correcting the owned backing-image permissions resolved it without a production ACL change. Fresh native verification passes 22 cross-volume cases including a Modify-only destination and 99 default tests. Evidence: `PREFIX_PACKAGE_ACCEPTANCE.json` and `MODIFY_DESTINATION_ACCEPTANCE.json`. Broader GUI acceptance remains pending. Other power-cut boundaries and physical-device acceptance also remain pending. Non-symbolic reparse points remain rejected. Native commands, detailed evidence and limits are in [DEVELOPMENT.md](DEVELOPMENT.md#windows-move).


Windows index-task status source now reads the task's `Settings/Enabled` value instead of a trigger's earlier `Enabled` flag, which could incorrectly label a disabled task ready. Differing trigger/task flags and malformed settings have rendered-status regressions. Source verification passes 215 search tests (two ignored), strict Linux/Windows search all-target Clippy, formatting and diff checks. The first full run's 500k benchmark hit 270 ms under concurrent compilation; an isolated repeat passed at 121 ms and the full suite passed without compilation. The subsequent task-update repair below includes this status fix in the refreshed portable/NSIS payload; native portable CLI/status acceptance passes. Updated installer lifecycle and MSI refresh pass the subsequent checkpoint below; cross-version upgrade and full rollback remain open. Evidence: `.hermes/evidence/windows-refresh-20261012/TASK_STATUS_SOURCE_VERIFICATION.json`.

The shared Windows background setup now registers the replacement task atomically with its existing enabled state and permissions, verifies task ownership/logon type, and passes the executable path as process data. It replaces the hourly `schtasks /MO` path with a Task Scheduler time trigger and validates the documented 1–744-hour repetition range. The exact embedded script passes native medium-integrity interactive checks for new/enabled tasks, disabled updates at 1/6/24/168/744 hours, unchanged security descriptors, paths with spaces/apostrophes, and mutation-free refusal of 0/745 hours. The initial verifier incorrectly compared an account name returned by Windows with a SID; normalized SID checks pass. All 213 current search tests (two ignored), strict Linux/Windows search all-target Clippy, formatting and diff checks pass. Native script evidence: `.hermes/evidence/windows-task-update-20261012/NATIVE_SCRIPT_ACCEPTANCE.json`. The additional native ownership control refuses a readable other-user task without changing its definition or permissions. Exact portable CLI acceptance now passes setup, disabled update to 24 hours, preserved permissions, correct enabled/disabled status with an enabled trigger, and invalid-interval refusal. The subsequent installer checkpoint below accepts the current same-version lifecycle; cross-version upgrade and full rollback remain open.



The preceding task-CLI checkpoint refreshed Windows portable and NSIS payloads from clean revision `c4cfcc453784d5570d76672775ee896a31979c0d`, including both index-task fixes. Portable SHA-256: `23f11855b1975d0718714694270604ea1be8c520c9ab7c555155f6b54a12cf27`; NSIS: `da50b91901cff2fd69d13b5a812d10b4ef8045cad8a3e5de8f87fbd6a6b93abf`. All 50 portable and extracted NSIS files match the stage. Native exact portable CLI tests pass under the limited medium-integrity interactive desktop account; this account has filtered administrator membership, so fresh non-administrator acceptance is not claimed for this turn. MSI remains the preceding `f18ee49` artifact; updated NSIS install/reinstall/uninstall and MSI rebuild remain next. Receipt: `dist/WINDOWS_INDEX_TASK_ACCEPTANCE.json`. Owned guest tasks, package extraction, binaries, profile and uploads were removed, existing installation checksum preserved, and the owned VM shut down.


The current Windows portable, MSI and NSIS candidates now share clean revision `4a9715f4805895d3196198358c8af4fc17fbcc77`, including the refined menus and Inspector page-navigation layout. Portable SHA-256: `adcf88ea372c44cc85108d3e8ae070b55a8caedae8751496da75e10cd39f9138`; MSI: `d422841b8e562ed89be4ae5054692457cd47de9acaccbcfc0333396fdf7cf181`; NSIS: `b45f279e6325308bad6251c57094aaadebed1906ef5a670c387e8728411735d1`. All 50 portable/NSIS files match staging; the native WiX MSI cabinet matches those files plus the authoritative transaction script. The exact portable passes native medium-integrity interactive Dark/Light checks at 100% scaling: complete menu icons, compact action-menu padding, separate choice checkmarks, two-page PDF Next/Previous and disabled boundaries, Inspector minimum size, unchanged source and clean exits. Current installer lifecycle and Windows 150% scaling were not rerun. Receipt: `dist/WINDOWS_UI_ACCEPTANCE.json`. QA task, processes, profiles, compiler and extracted fixtures were removed; the pre-existing installation checksum is unchanged and the owned guest is shut off. Earlier installer receipts below remain historical.

The preceding portable and MSI used clean revision `8752e92ccda3c29a6648dbc4e3e37d11a8ba5a7a`; NSIS retained its accepted `c4cfcc4` payload with the identical application binary. Portable SHA-256: `dc1a5f387432f03f2b5f58cb762cd8e7e0eecb379fb4433913b8571babb24cac`; MSI: `01348323827bc3979e3ec717b49670835f57a13742e3b9f2a8eea09bc1a7cb18`; NSIS: `da50b91901cff2fd69d13b5a812d10b4ef8045cad8a3e5de8f87fbd6a6b93abf`. Both exact installers pass fresh non-administrator, medium-integrity interactive install, same-version reinstall/repair, payload hashes (50 NSIS / 51 MSI), disabled-index state and permission preservation, configured six-hour refresh, desktop entries, uninstall and preference preservation. The actual MSI Start menu shortcut opens the installed app and exits cleanly with bundled OpenGL. MSI checks use the real disposable account profile: its service action did not consume the earlier private APPDATA fixture. The initial invalid partial-config NSIS fixture and the MSI configuration-path correction are retained separately from passing acceptance. All owned account/profile, credentials, tasks, compiler/extractions and uploads were removed, existing guest installation checksum preserved, and VM shut off. Exact evidence: `dist/CURRENT_INSTALLERS_ACCEPTANCE.json`. Cross-version upgrade, late-failure MSI product rollback, interactive installer UI and broad daily-driver/graphics acceptance remain open.


### 3. Finish packaging source

- Finish native Windows acceptance of the MSI and installer/uninstaller cleanup integration.


The preceding full Windows installer checkpoint used all three artifacts from clean revision `f18ee49ae21669e55f8bb6bd21b9a0886609b15f`: portable SHA-256 `258fb0140cad49d18f0e539a2225afb2e6ed4e7080781e00e64a6063f96addf7`, MSI `61681db3968c3bf319b5c2749f8ec1b2cd3507c9793d4565deb2a967df960d7d`, NSIS `7475fdfc58a30278c1be4abd24f092064ab049faa179e3c7e5fc27a37f862169`. Native WiX compiled the exact MSI. A fresh standard account with a medium-integrity interactive token passes MSI install, all 51 files, x64 summary, index registration, shortcut/icon checks, installed-shortcut window launch/clean exit, and uninstall with product state 5 → -1 and preferences preserved. The current NSIS passes install, all 50 files, registration/shortcut/index checks, same-version reinstall and uninstall with preferences preserved. The first MSI verifier had an incorrect helper-script manifest path; explicit re-verification after correcting the fixture passed, and the original successful native install log and failed fixture are retained. Owned account/profile/credential/tasks/runtime/uploads were removed; the previous guest installation is unchanged. Exact scope and remaining cross-version, rollback, populated-data and interactive UI gates are in `dist/CURRENT_INSTALLERS_ACCEPTANCE.json` and `dist/WINDOWS_PACKAGES_ACCEPTANCE.json`.


The preceding portable and NSIS packages contain the Miller resize repair and were packaged from clean tracked revision `2b3acb25159ec66c988722901f018322435bbb05`. Portable SHA-256: `633c9cbe386536041d3d53f895a17a9833249cfff610b9556d1a2aa8cd961416`; NSIS SHA-256: `bc071d129beb96de792bbb36a99e4376514719876647c9502bcdcd8ee4848aa2`. All 50 portable and extracted NSIS payload files match the current stage. The portable passes medium-integrity interactive launch, actual 1100 × 690 to 800 × 600 resize with selected file and adjacent text preview visible, and clean exit with empty stderr. QA task/profile/uploads were removed; the previous guest installation was preserved and the guest shut down. `dist/packages/MILLER_RESIZE_ACCEPTANCE.json` and `dist/WINDOWS_PACKAGES_ACCEPTANCE.json` record exact scope. At that checkpoint native lifecycle acceptance had not been rerun for its NSIS, and the MSI was still the older `f9b2062` artifact. Historical installer checks below apply only to their recorded hashes.

The earlier clean `0d1ea18` NSIS candidate was built at `dist/Ottrin-0.1.0-windows-x64-setup.exe` (SHA-256 `faa29a8506060ab2d7541f2764ea57d909bde5f70e03c9e73ef85120f4715970`). Silent install and same-version reinstall pass under a fresh non-administrator Windows user: all 49 payload files match, the limited-user periodic index task targets the installed executable, and saved preferences are unchanged. Silent uninstall removes application files, task, shortcuts and registry entries while preserving those preferences. `NSIS_ACCEPTANCE.json` records the exact scope. At that checkpoint, portable and MSI were rebuilt from clean revision `b7f0de608aec454ebcdcb72874a51e755df8fbf3`: portable SHA-256 `a31f2f19e01ef757f95f8d5a0195b9a9734636990eeaa5ce184d7e05ba4979c1`, MSI SHA-256 `82433937a222aaba423a98f228a2eb44c69835487bcb8b6557a634a57efca52e`. The exact MSI passes silent install/uninstall with all 49 installed files matching, index-task registration/removal, product registration/removal and saved preferences retained. `MSI_FINAL_ACCEPTANCE.json` records its medium-integrity interactive token and owned installation/profile scope. True version upgrade/rollback, populated-data cleanup, interactive installer and full installed-app workflows remain open.

Native MSI QA on 2026-10-12 exposed destructive early upgrade removal and an omitted architecture flag. The builders now explicitly select x64, and major upgrades retain the old product until the replacement install script executes. A clean native candidate cycle passes install, an injected immediate upgrade failure with all 49 prior files/product/index-task/preferences preserved, successful 0.1.0-to-0.1.1 upgrade, and uninstall with payload/task removal and preferences retained. `MSI_LIFECYCLE_ACCEPTANCE.json` records the candidate hashes, medium-integrity interactive token and scope. These candidates use the clean `4c4057b` portable payload with revised installer source; they are not the published MSI. The upgrade fixture changes the MSI version, not runtime behavior. The clean stable MSI is separately accepted for silent install/uninstall as recorded above; a fresh non-administrator interactive session, later custom-action failure rollback, populated-data cleanup and Installed Apps workflows remain open. Earlier destructive tests left owned orphan component references; those were recorded and removed before the clean repeat.

A later native failure test on 2026-10-12, after `InstallExecute` and before old-product removal, **fails**: Windows Installer leaves the failed product registered and reports access-denied rollback errors, while background setup re-enables a previously disabled index task. A separate file-only MSI with no Ottrin executable or index custom action reproduces the registration failure (1603, payload rolled back, product state still installed); its explicit cleanup uninstall succeeds. `MSI_LATE_FAILURE.json`, `MSI_REGISTRATION_PROBE.json` and compressed native logs retain the evidence. [WiX issue 8574](https://github.com/wixtoolset/issues/issues/8574) reports similar symptoms; the exact authoring/engine cause is not established here. This is a confirmed open release gate, not covered by the earlier passing pre-execution failure test. Installer-owned index rollback is now implemented: a UTF-8 snapshot records the prior definition and permissions before setup, rollback restores it without running registration triggers, and commit removes the snapshot. A native medium-integrity test with a failure after `InstallExecute` restores the complete disabled task XML and permission entries, preserves the 49 prior payload files/preferences and removes the snapshot. New-task rollback, commit cleanup and stale-snapshot retention also pass focused native tests. `MSI_TASK_ROLLBACK.json` and `MSI_TASK_TRANSACTION_UNIT.json` record this source/candidate evidence; it does not resolve the failed-product registration error or complete populated-data rollback, fresh non-administrator or exact published-MSI acceptance.

A new file-only control rules out filtered-administrator membership as the explanation for the registration rollback failure. Native WiX 5.0.2, Windows build 26100 and `msi.dll` 5.0.26100.9168 reproduce it in a fresh standard account with a medium-integrity interactive token and no administrator membership: injected failure after `InstallExecute` returns 1603, the file rolls back, but product state remains 5. The rollback contains 149 access-denied registry notes, excluding ordinary missing-key diagnostics. Explicit uninstall returns 0 and product state becomes -1. The probe has no Ottrin executable or index custom actions; this does not establish the compiler, engine or authoring cause. The [open upstream report](https://github.com/wixtoolset/issues/issues/8574) describes similar registration failures; similarity is not root-cause proof. `dist/MSI_STANDARD_USER_ROLLBACK_COMPARISON.json` and compact logs under `.hermes/evidence/msi-rollback-20261012/` retain the evidence. Owned account/profile/task/credential/compiler/uploads were removed and the prior guest installation is unchanged. Full MSI rollback remains an open gate; other roadmap work can continue.


The task-transaction portable/MSI checkpoint was built from clean tracked revision `c3bba32532b0a5a3a0c96c52e047b88eec1f3a58`: portable SHA-256 `2864bd4522e67f0b6758789f8705b0471818e3aa4fbc7201a389ed111f35e7cb`, MSI SHA-256 `c53089f1dfd85cebba653e56e318686af110e976bb46b5465eb9dec4e8e33b52`. The MSI adds one installer-owned task-transaction script (50 installed files; portable remains 49). Exact silent install/uninstall passes with complete payload hashes, index task/product registration and removal, preferences retained and no transaction snapshots left. `MSI_TASK_FINAL_ACCEPTANCE.json` binds this medium-integrity interactive acceptance to the exact artifact. A separate native helper test also confirms restoring an enabled registration trigger does not execute it. Full MSI registration/data rollback and fresh non-administrator interactive/Installed Apps acceptance remain open.

The NSIS lifecycle checkpoint used Windows portable and NSIS packages from clean revision `e09d88c8ecf11460338034945ce0b5ee890ed432`. Portable SHA-256: `327a5d8868949fd50dd62139ad290ec457b5e9b63d0646f6b8b6a7bd9d38711f`; NSIS SHA-256: `3389e0128cae0e93c36b76be397481abdd3907ab8df4113941ceac1c56eaf699`. The published archive matches all 49 staged files; its app binary matches the earlier accepted startup-preview binary. Exact NSIS silent install, same-version reinstall and uninstall pass under a fresh standard account with a medium-integrity batch token: full payload hashes, index action/principal, Installed Apps name/version/icon, Start menu shortcut targets, removal of files/task/registration/shortcuts, and unchanged preferences. The owned account, profile, temporary batch-logon right, tasks and fixtures are removed; the existing guest installation is preserved. Receipt: `.hermes/evidence/windows-20261011/CURRENT_NSIS_ACCEPTANCE.json`, also included in `dist/WINDOWS_PACKAGES_ACCEPTANCE.json`. Cross-version upgrade, failure/rollback, populated-data cleanup, interactive installer and full installed-app workflow acceptance remain open.

The historical portable and MSI checkpoint was built from clean revision `fc313932a4993cb3122594257928d676daf3e931`: portable SHA-256 `afda48f12327fb1df2bc9337e97eaa8fc1577c7b2455b767198ac2674b2b66ae`, MSI SHA-256 `f88a875c2b03d5d9b9445d655de023f56c9d9a53b0f4520500b4ef13bc5b3520`. The exact MSI passes silent install/uninstall under a fresh standard account with a medium-integrity **interactive** token: all 50 file hashes, index-task registration/removal, product state 5 → -1, preferences preserved, and no transaction snapshots. Its first batch-token attempt failed before package actions with installer-server access denied; that context failure is retained separately and no permissions were changed. Temporary account/profile, tasks, credential, runtime and fixtures were removed; existing guest installation preserved. Receipt: `.hermes/evidence/windows-20261011/MSI_CURRENT_ACCEPTANCE.json`, also in `dist/WINDOWS_PACKAGES_ACCEPTANCE.json`. MSI authoring currently lacks Start menu app/uninstall entries; add and accept those next. Full rollback/product restoration and the broader release gates remain open.



### 4. Required Ottrin picker and folder integration — deferred

Approved on 2026-10-11 as a must-have, not optional research. Start only after the previously committed implementation, packaging and Linux/Windows platform-acceptance work is complete. Do not divert current repair work to this milestone or start it merely because another acceptance needs a device. After this milestone, run its focused platform acceptance and the final release-candidate gate against the resulting artifacts.

- Build a reusable Ottrin Open/Save/folder picker using the authoritative theme and shared UI components. Use familiar places/bookmarks, breadcrumbs/editable path, search, file views, optional preview and a clear filename/type/action footer. Support caller-specified locations, filters, multiple selection, cancellation, overwrite confirmation and parent-window behavior. Preserve useful picker preferences without overriding the caller's request. Verify theme changes on the actual picker, including fonts, colors, borders and corner geometry.
- Use the picker within Ottrin on Linux and Windows. On Linux, provide a desktop-portal FileChooser backend for compatible applications, with an explicit enable/disable control that preserves other portal providers and restores the prior chooser selection. Windows external integration is limited to cooperating applications; do not claim or offer a universal replacement for other programs' native dialogs.
- Offer optional folder/file-reveal integration: Linux directory handling and the standard FileManager1 interface; Windows Open in Ottrin integration and a clearly scoped, reversible folder-opening preference where supported. Display coverage honestly, preserve previous associations and restore owned changes on disable/uninstall.
- Keep picker launch independent of index readiness. Browse directories directly; use the existing index asynchronously for search when available. Do not add indexing responsibilities or an always-running full GUI to the picker. Start the picker on demand; consider bounded warm retention only if measured cold startup justifies it.
- Accept cold/repeated launch latency, idle CPU, retained memory, cancellation/cleanup, concurrent requests, operation with indexing stopped, theme persistence, and enable/disable restoration on both supported platforms. Record measured results rather than promising unverified speed or universal application coverage.

## Triggered work, not background work

These outcomes remain available, but they start only when their trigger is met. Workers must not choose them merely because another lane is waiting for physical QA.

- **MiniPdf preview pack:** run the bounded Linux/Windows comparison only when deciding whether Ottrin needs a downloadable lightweight pack. The decision needs representative fidelity and failure evidence, not an exhaustive format survey.
- **Portable LibreOffice fidelity pack:** start only after installed-provider acceptance shows a concrete need and provenance, update ownership, licence and removal responsibilities are acceptable.
- **Generic pack manager:** build it only after one pack candidate has passed its product decision. Do not build package infrastructure for hypothetical providers.
- **Renderer, codec or icon replacement research:** start only for a reproduced product gap that the current accepted path cannot solve.
- **Live remote folders and cloud locations:** do not implement these before the current roadmap and release acceptance are complete. The deferred architecture in [`REMOTE_ACCESS_AND_CLOUD_LOCATIONS_DESIGN.md`](REMOTE_ACCESS_AND_CLOUD_LOCATIONS_DESIGN.md) keeps live Ottrin-to-Ottrin access, provider-backed cloud locations, and cloud synchronization as three explicit product boundaries.
- **Advanced image tools:** the repository does not define this outcome. Do not invent features from the label; start only after David chooses a concrete tool and acceptance behavior.
- **Fanotify helper/service:** the current Linux indexer uses its accepted watcher design and no active document defines a fanotify requirement. Reopen this only for a reproduced coverage gap and an accepted replacement or supplemental design.

## Final physical and platform QA

Start this phase after outcomes 1–3 are complete, before starting deferred outcome 4. Keep it as one concentrated acceptance pass against exact candidate artifacts. Fix reproducible defects at their source, then rerun only the affected acceptance path. After outcome 4, accept its integration and recheck any affected existing workflows before the final gate.

### Linux

- Bounded packaged QA on 2026-10-03 verified the then-current AppImage in isolated Xvfb: startup, folder navigation, editable-text Inspector save/reopen, external-change refusal with retained draft, and confirmed reload. Embedded tracked-source provenance matched the checkout before this QA note. This does not close final Linux acceptance: automated Copy/Paste did not produce an accepted conflict flow; clean desktop, real removable storage and installed LibreOffice remain unverified or unavailable. Evidence and the first next acceptance are in `.hermes/evidence/t_fb84c0b6/QA_RECEIPT.md`; Windows remains a separate native gate.
- Launch the AppImage, portable tarball and native packages on a clean supported desktop.
- Run one daily-driver workflow covering navigation, New File/New Folder, file operations, cross-volume Copy, Tandem View, multi-select, search, Quick Inspector, context menus and status messages.
- Attach, browse, index by explicit choice, detach and reconnect real mounted/removable volumes. Check compact/expanded Locations pointer, keyboard, tooltip, ordering, selection and unavailable states.
- Exercise installed LibreOffice with representative DOCX, XLSX and PPTX files. Confirm conversion, cancellation, cleanup, provider labels, page navigation and native fallback. Add fixtures only for defects found or features Ottrin claims to support.
- Native helper smoke verified on Linux: generated silent video, RGBA frames, pause/resume acknowledgement, Stop, end-of-stream, retained-descriptor pathname replacement, malformed input and isolated missing-plugin fallback (`scripts/linux-media-playback-smoke.py`). Source-built Inspector presentation has also been exercised in isolated Xvfb with generated video: decoded frames, Pause/resume, end-of-stream, missing-helper and missing-plugin fallback. Duplicate external-open actions and enabled controls for terminal sessions were corrected. Audible playback, representative user media and clean-package fallback remain pending; no codecs were added.
- Isolated packaged QA on 2026-10-10 verified New File/New Folder, rename, multiselect Copy, Trash, permanent delete, same-filesystem Cut/Paste, Tandem Copy/Move, tab reordering and scoped search with disposable files. List/Grid rubber-band selection from blank space below entries was repaired and verified in the AppImage from `76b50087e0f26a6312976a9def902e0d68409bd3`. Evidence is in `.hermes/evidence/roadmap-20261010/`; these checks do not close physical desktop, removable-volume or full workflow acceptance.
- Check packaged privileged-helper and polkit allow, cancel, missing-helper and policy-error paths.
- Recheck native assistive integration once. If the upstream Unix AT-SPI lifecycle remains unusable, record the observed limitation rather than starting replacement infrastructure during QA.

### Windows

- Launch the portable zip and installed application on real Windows hardware or a representative real Windows VM.
- Check install, Installed Apps registration, shortcuts, launch, upgrade and uninstall cleanup without deleting user files.
- Run the Windows daily-driver workflow: navigation, New File/New Folder, file operations, Recycle Bin, Tandem View, search lifecycle, Quick Inspector, UAC/helper behavior and background integration.
- Attach and remove secondary fixed, removable and optical storage; connect and disconnect a mapped-network drive; check unavailable states and explicit indexing.
- Exercise Office previews through the accepted Windows provider boundary and confirm native/extracted fallback.
- Exercise Media Foundation with representative ordinary, malformed and unsupported media, cancellation, source replacement and the exact package contents. A native ordinary-user fixture now verifies a generated H.264 thumbnail by decoded pixel values, pre-cancellation, malformed-input refusal and admitted-source pathname replacement. Representative format coverage, in-flight cancellation, UI and exact-package checks remain separate.
- Exercise native WGPU and bundled Mesa OpenGL on adapters that need each path. Confirm Automatic selection, saved preference, diagnostic override, startup log and loaded-DLL provenance.
- Exercise same-volume and cross-volume Move with Skip/Rename/Overwrite/Cancel handling, interrupted work, edited-output Undo refusal, removable drives, and explicit partial/recovery outcomes.

### Signing

- Linux and Windows code-signing integration is source-complete; production identities and exact downloaded-artifact verification remain release gates.
- Sign the exact Linux and Windows release artifacts with the approved production identities where required and verify the downloaded signatures.

## Release candidate gate

Run this gate once the implementation outcomes above are source-complete and exact candidate artifacts exist. Earlier focused checks remain useful evidence but do not satisfy release acceptance. The Windows candidate design includes the checksum-pinned Mesa 26.1.3 OpenGL compatibility runtime; exact-artifact verification must cover its payload, renderer preferences, diagnostic override, and loaded-DLL provenance, and no current package acceptance is claimed before those gates pass.

- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace` and strict workspace Clippy pass.
- Linux and Windows CI pass for the release commit.
- Rebuilt artifact checksums verify and archive inspection finds the expected app, indexer, helper, policies, assets and documentation.
- Linux AppImage and portable tarball launch and pass the Linux workflow.
- Windows portable and installer launch and pass the Windows workflow.
- The Windows packages contain the pinned Mesa 26.1.3/DXC payload and notices; Automatic, native and OpenGL paths report the correct loaded DLLs.
- New File/New Folder, Locations, cross-volume Copy and Move, and privileged-helper behavior pass on their target platforms.
- The themed Ottrin picker, supported external integrations, resource/startup checks and reversible preference controls pass outcome 4 acceptance on Linux and Windows.
- Same-volume and cross-volume Move preserve source data on uncertainty and report complete, partial, blocked, and recovery-required outcomes truthfully.
- Known limitations match observed behavior.
- `README.md`, `USER_GUIDE.md`, `ROADMAP.md` and `RELEASE_NOTES.md` agree with the exact artifacts.
- Version, artifact names, checksums and generated release notes agree.
- Required signatures verify after download.
- David reviews the release notes and any remaining known limitations. No blocker is waived without a written decision.

### Windows MSI desktop integration — 2026-10-12

The per-user MSI now defines Ottrin and Uninstall Ottrin Start menu shortcuts, an Installed Apps icon, and uninstall ownership for its menu folder and registration marker. Both Windows portable builders carry `assets/icon.ico`, derived from the authoritative 256-pixel `assets/icon.png` at 16, 24, 32, 48, 64, 128 and 256 pixels. The MSI embeds this small icon resource rather than duplicating the application executable. The exact MSI from clean revision `f9b20627dbeab7ee7ca58648ce35198f3fe44121`, SHA-256 `e8ea82f0da32613fd316d3badc3e352dbc55af3f963d347d3f0c2ee313fe00be`, passes native silent install/uninstall under a fresh standard interactive account. All 51 payload files, both shortcut targets/arguments, shortcut and primary product icon bytes, index registration/removal, component/menu/icon removal, product state 5 → -1 and preference preservation pass. Launching the actual app shortcut opens an Ottrin window and exits cleanly with the bundled OpenGL override after initialization. The first two-second initialization/close probe timed out; a later probe refreshed the window handle after eight seconds and passed, with both results retained. Full UI/native graphics, cross-version upgrade and the separately recorded rollback/product-restoration gate remain open. Receipt: `.hermes/evidence/windows-20261011/MSI_DESKTOP_ACCEPTANCE.json`; current package pointers: `dist/WINDOWS_PACKAGES_ACCEPTANCE.json`. The refreshed portable archive has 50 files, all matching its stage; the application executable remains identical to the earlier accepted startup-preview binary. The disposable account/profile, task, credentials and staging were removed; the pre-existing guest installation checksum is unchanged.

### Preview toolbar wrapping acceptance

The themed-button wrapper now allocates in the caller’s layout while restoring
its style afterward. A failing narrow-pane regression reproduced the nested
scope defect before repair. Shared visual sizing controls group fit actions and
zoom controls into separate wrapping rows, keeping the zoom status on one line.
All 344 regular UI tests and strict Linux/Windows cross-target Clippy pass.

The exact `6e8cef0` AppImage passes text/PDF preview checks at wide/narrow sizes,
Light/Dark, enlarged text, and 150% desktop scaling; inspected controls remain
inside the pane. Copy/conflict rename/batch Undo/Move/Cancel/Overwrite/Undo and
normal exit also pass with exact bytes. Portable, DEB and RPM have matching
current code and pass extracted startup/navigation/exit checks; portable PDF
rendering passes. Owned runtimes, processes and private profiles are removed.
Receipt: `dist/packages/PREVIEW_TOOLBAR_ACCEPTANCE.json`. This does not close
native Windows visuals, full appearance/provider acceptance, installed package
lifecycle gates, physical removable acceptance, or retained-data reclamation.

## Fresh Ubuntu DEB lifecycle acceptance — 2026-10-12

The current `6e8cef0` DEB, SHA-256 `b165dee2e1e8fd89d8aa12c36dd0d7a906ad2cb5551707f95db18247af1b42d4`, passes fresh Ubuntu 24.04.5 VM installation with dependencies, regular-user CLI and text-preview GUI launch/clean exit, same-version reinstall, and removal of package-owned executable/helper/desktop/polkit files. Saved preferences remain byte-identical after reinstall and removal. All five installed executables match the accepted release binaries. Real systemd user-service install (enabled/active), status, restart and removal pass. This replaces the prior namespace-only installation blocker at this scope. Receipt: `dist/packages/DEB_INSTALL_ACCEPTANCE.json`; compact logs and screenshot: `.hermes/evidence/linux-native-vm-20261012/`. Owned VM, disks, seed, image and SSH keys are removed. Full desktop integration, cross-version upgrades, reboot/logout service persistence, fresh RPM installation and helper authentication remain open.

## DEB libc requirement correction — 2026-10-12

The current DEB now declares `libc6 (>= 2.39)`, derived from the highest GLIBC import across all five actual executables. Older systems are rejected by the package manager instead of accepting an app that cannot load. Native package regressions verify generated dependencies and that a malformed helper fails without replacing the previous package. Clean package revision `9b0f6dbe75662003d9f5996042a67ae3f1f6a7bd`, SHA-256 `d04ec65ceebd1328399fb81877a364663ac07c57b023e14917b50d18df64c85c`; all executables are identical to accepted `6e8cef0` binaries, and extracted CLI launch passes. Receipt: `dist/packages/DEB_LIBC_ACCEPTANCE.json`. The preceding VM installation receipt remains bound to its earlier artifact; this metadata-only revision has not been reinstalled in the VM.

## Native authentication runtime and RPM libc admission — 2026-10-12

Clean-system review found that native packages shipped the helper/policy without requiring its authentication runtime. DEB now requires `pkexec` and `polkitd`; RPM requires `polkit` and the `/usr/bin/pkexec` provider. Both native builders share one payload-derived libc requirement; RPM explicitly requires `libc.so.6(GLIBC_2.39)(64bit)` because the local automatic dependency generator omitted versioned ELF requirements. Generated package dependencies are inspected, all five DEB executables match accepted binaries byte-for-byte, RPM code/read-only sections match after standard stripping, and extracted DEB CLI launch passes. Native contract and package replacement/error-preservation regressions pass. Clean package source `221e02b`; current DEB SHA-256 `782a7bf1e57d0ab405d180fa12b7a220abe89731a2465de0ef2210f24b4814cf`, RPM SHA-256 `83ca74287c32734b6015ddfe3e156bc127137ee4e399366bfaa6da75f45c0637`. Receipt: `dist/packages/NATIVE_AUTH_RUNTIME_ACCEPTANCE.json`. These metadata corrections have not been freshly installed; the prior Ubuntu VM lifecycle receipt retains its original artifact checksum. Authentication prompt/cancel/error testing and Copy retained-data reclamation remain open.

## Release verifier native-package coverage — 2026-10-12

The release artifact verifier now requires DEB/RPM checksum entries and inspects both native payloads, source records, exact helper policy and generated libc/polkit dependencies. All five DEB executables must match the release AppImage byte-for-byte; RPM code/read-only sections must match after standard stripping. The portable PDFium checks use its actual corrected `libexec/ottrin/pdfium` layout and the obsolete duplicate check is removed. The shared packaging lifecycle/ABI helper is included in preflight syntax/ShellCheck coverage. Actual production native-check code passes on the current recorded packages and refuses a wrong source revision and a modified executable comparison fixture; all 19 portable archive entry requirements pass. ShellCheck, shell syntax and relevant provenance/coverage contracts pass. Compact receipt: `.hermes/evidence/native-release-gate-20261012/ACCEPTANCE.json`. Full multi-platform release verification remains open pending a coherent exact-release artifact set; this gate does not substitute for native installation/authentication acceptance. Owned extractions and altered fixtures are removed.

## Windows cross-version upgrade acceptance — 2026-10-12

The current NSIS and MSI candidates now pass native upgrades from a QA-only `0.0.9` installer built from historical source `f18ee49` to the published `0.1.0` packages. This fixture is not a claimed historical release. Under a fresh standard interactive Windows account, all 50 NSIS / 51 MSI payload hashes, saved preferences, populated index bytes and search results, disabled indexing and task permissions survive the upgrade. MSI removes the old product registration, registers the current product, updates the uninstall shortcut and refuses downgrade at LaunchConditions without changing the current installation. Final removal preserves preferences. The initial NSIS removal assertion raced its delegated uninstaller; the retained failure and subsequent final-state verification are recorded.

Exact package revisions and checksums, native results, compressed MSI logs and cleanup proof are in `dist/WINDOWS_CROSS_VERSION_ACCEPTANCE.json` and `.hermes/evidence/windows-cross-upgrade-20261012/`. Owned accounts, profiles, tasks, credentials, older-source checkout and test packages are removed; the QA guest is shut off and its existing installation is unchanged. Historical/current builds exposed stale Cargo worktree fingerprints; affected first-party Windows release outputs were cleared and rebuilt from current source, including Office helpers. Published packages remain unchanged. `docs/DEVELOPMENT.md` now requires a task-owned target namespace for historical-source QA builds. Installer late-failure rollback, interactive installer UI and broader Windows daily-driver/graphics acceptance remain open.

## Linux installed Office provider repair — 2026-10-12

Fresh Ubuntu 24.04 native testing exposed two distinct blockers: default AppArmor denies Bubblewrap namespace setup, and Ottrin omitted `/bin` from the sandbox, preventing LibreOffice's `/bin/sh` launcher from executing. The authoritative launcher now mounts `/bin` read-only. The existing Office acceptance contract is shared between Linux and Windows, using their production launchers; superseded Windows-only test names and commands are removed. With the distribution's restrictive Bubblewrap profile explicitly loaded in the disposable guest and global restrictions still enabled, all three native Linux tests pass: DOCX/PDFium rendering and source preservation, in-flight cancellation/cleanup, and all nine advertised formats with expected rendered-text markers. All 17 provider contracts and strict Linux/Windows shared-contract Clippy pass. The refreshed DEB also installs successfully with `pkexec` and `polkitd` present and a clean dpkg audit. Evidence: `.hermes/evidence/linux-office-native-20261012/`.

This accepts the configured installed-provider scope, not Ubuntu's default policy or Inspector UI/package behavior. Administrator-managed Bubblewrap policy setup, native-package PDFium availability, representative visual fidelity and the remaining platform gates stay explicit. Publication of the repaired Linux app and owned-VM cleanup are recorded separately when complete.

### October 12 installed Linux Office and X11 runtime acceptance

The shared native Office contract passes all three installed-provider tests on Ubuntu 24.04.5 with LibreOffice 24.2.7.2: nine advertised formats render text and ink, source bytes are preserved, and cancellation after a real profile lock releases the provider. Read-only `/bin` visibility repairs the actual shell-launcher failure. This acceptance uses an administrator-loaded distribution restrictive Bubblewrap profile; default Ubuntu namespace-policy setup remains open, with the global restriction retained. Native-package PDFium remains open.

Clean-guest startup exposed missing dynamically loaded X11 libraries. One shared runtime declaration now supplies all five required roots to portable bundling and verification; DEB/RPM declare their native dependencies. Positive closure checks and missing-library refusal pass. The exact current AppImage launches the main window and Quick Inspector, visibly renders the DOCX, preserves its source bytes and exits normally. These GUI checks do not establish provider identity or broad visual fidelity. Current DEB installed payload hashes match all five archived executables; actual RPM dependency metadata is verified, without an RPM installation claim. Portable, DEB and RPM stable outputs are refreshed from revision `60f6477`; the prior extracted-package resize receipts remain historical. Exact checksums and limits are in `dist/packages/LINUX_OFFICE_ACCEPTANCE.json`. The disposable guest, volumes, credentials and extraction files are removed.

### October 12 native PDFium delivery

DEB and RPM now contain the same pinned PDFium runtime and licenses as AppImage/portable, using one authoritative Linux packaging helper. Native runtime discovery finds `usr/libexec/ottrin/pdfium/lib` relative to the installation prefix. Download/checksum/extraction failure explicitly stops publication, including when the helper is called conditionally. The native libc requirement includes PDFium; release checks require its library/license payload and compare renderer code identity. All 197 preview tests, seven focused PDF runtime tests, strict Clippy, ShellCheck, native structural/dependency checks, wrong-checksum refusal and preflight coverage pass.

All four stable Linux outputs are refreshed from `b97eb0a`. Actual extracted DEB/RPM and exact AppImage main/Inspector PDF rendering pass in Dark 100% and Light 150%; process maps verify each packaged renderer without overrides, source bytes remain unchanged and exits are normal. DEB first-party executable identity and RPM application/renderer code/read-only-data identity after stripping pass. Owned extracts, displays, profiles and fixtures are removed. This closes missing native renderer delivery at extracted-payload scope; clean distribution installation/upgrade remains a separate gate. Exact identities and scope: `dist/packages/NATIVE_PDFIUM_ACCEPTANCE.json`.

Visual follow-up found during this acceptance: Inspector page navigation reads `Next / of N / Page N / Previous` because of layout ordering. Restore a coherent `Previous / Page N of N / Next` group and verify its narrow-window behavior in the next UI correction.

### October 12 Inspector navigation acceptance

Inspector document navigation now has one left-to-right wrapping implementation, independent of the action row direction. Page number and count remain in one control. The rendered regression starts in a right-to-left parent and passes both themes, text scales 1.0/1.35 and widths 180/320/600 without clipping or reversed controls. An initial height-allocation defect was caught by the continuous-page rendering regression and corrected. All 346 regular UI tests and strict all-target UI Clippy pass.

The exact rebuilt AppImage passes two-page Next/Previous checks in Dark 100% and Light 150%: page counter/content change together and boundary buttons disable correctly. Controls and document remain visible at the 420x320 logical minimum. Artifact renderer mapping, source preservation, normal exits and owned-fixture cleanup pass. Portable/DEB/RPM are also refreshed from `bbbe26a`; this is not native installation or Windows GUI acceptance. Exact checksums and scope: `dist/packages/INSPECTOR_NAVIGATION_ACCEPTANCE.json`. This resolves the navigation-order follow-up recorded above.

### October 12 current native release verification

The production native verification block now passes against the exact `bbbe26a` AppImage/DEB/RPM. It verifies five executables plus PDFium: DEB bytes and RPM code/read-only sections match the AppImage, embedded source notes match their package revision, and libc/polkit metadata is valid. Actual negative fixtures refuse a native package missing PDFium, a substituted renderer, changed executable bytes and a wrong expected source revision. All current portable entry requirements also pass. Private extracted/modified package fixtures are removed. Exact checksums, verifier hash and native source notes: `dist/packages/NATIVE_RELEASE_GATE_ACCEPTANCE.json`. This is production native verifier acceptance, not the full multi-platform release gate or native installation/authentication acceptance.

### Activity history ownership — 2026-10-12

History trimming and Clear previously dropped live operations or the only session
owner of retained Copy transaction data. They now remove only finished records
without Undo receipts. Real overwrite Copy survives 150 later records and clear,
then restores the original destination; all-running history can exceed the limit
without losing results. All 348 regular UI tests and strict Linux/Windows-target
UI Clippy pass. The current exact AppImage passes clear/Copy Undo in Dark 100% and
Light 150%, with source preservation, normal exits and owned teardown. Current
portable/DEB/RPM identity checks pass. Windows package refresh, durable Copy
recovery across restarts and safe transaction reclamation remain open.

The preceding GUI fixture’s burst Shift+Delete showed a Trash confirmation
instead of a permanent-delete confirmation; its clearing acceptance covers Trash
history only. The follow-up source regression and old exact AppImage reproduce
the failure. The handler now reads the Delete press event’s Shift state, so a
released chord still requests permanent deletion and Shift pressed after Delete
cannot change ordinary Trash deletion. All 349 regular UI tests and strict
Linux/Windows-target UI Clippy pass. Exact `66bb75f` AppImage acceptance covers
both confirmations, cancellation without mutation, confirmed permanent deletion
outside Trash, and exact-byte ordinary Trash retention in Dark 100% / Light 150%.
Normal exits and owned teardown pass. Receipt:
`dist/packages/DELETE_SHORTCUT_ACCEPTANCE.json`. Windows package/native shortcut
acceptance remains open.

### Windows current payload and shortcut check — 2026-10-12

Portable, NSIS and MSI were refreshed from clean tracked revision `f428627`
with Activity retention and Delete press-modifier fixes. All 50 portable/NSIS
files and 51 MSI payload members match staging and the index transaction script.
Exact hashes and the failed native check are recorded in
`dist/WINDOWS_SHORTCUT_ACCEPTANCE.json`; current pointers are updated in
`dist/WINDOWS_PACKAGES_ACCEPTANCE.json`. Previous UI/lifecycle receipts remain
tied to their previous artifact hashes.

The ordinary interactive Windows account's Dark 100% check exposed a separate
input-adapter defect: Windows Shift+Delete becomes `Event::Cut` in egui-winit
0.33.3 before a pressed Delete event reaches Ottrin. The screenshot shows Cut,
and confirmed permanent deletion did not occur. Windows permanent-delete
acceptance remains **failed/open**; Light was not reached. Fix the adapter
contract while preserving text-widget clipboard ownership, then rerun native
confirmation, cancellation and destination checks. No current Activity or
installer lifecycle acceptance is claimed. Owned processes, task, payload,
compiler and fixtures were removed; the pre-existing installation's uninstaller
checksum was unchanged. The owned guest was restored to its initial shutoff state.

### Windows Shift+Delete adapter correction — source verification

The pinned egui-winit 0.33.3 adapter now retains the original Windows
Shift+Delete press event immediately after Cut, with event-time modifiers.
Ottrin's file view consumes that paired Cut and requests permanent deletion;
text fields and the Inspector retain clipboard ownership. Ctrl+X is unchanged.
The dependency is patched once via `vendor/egui-winit`, with original licenses
and a documented narrow change; all other dependencies remain registry packages.

The full UI suite passes 350 regular tests with one existing ignored test.
An additional rendered Windows text-field regression then passes alongside the
file regression: selected text alone is cut, rapid release preserves the press
modifiers, file Cut state is not created, and confirmation remains required.
Strict Linux and Windows-target UI Clippy pass. Logs:
`.hermes/evidence/windows-delete-adapter/`. Native exact-package Windows
confirmation/cancellation/destination acceptance and refreshed package delivery
remain open until the corrected build is tested.

### Corrected shortcut packages — exact acceptance, 2026-10-12

All Linux and Windows deliverables are refreshed from clean tracked revision
`083fb3f`. The Windows portable archive matches all 50 staged files; MSI and
NSIS match the same payload (MSI adds the index transaction script). Exact
portable native Windows Dark/Light at 100% verifies permanent versus Trash
confirmation, both cancellations preserving source bytes, permanent deletion
outside the Recycle Bin, unchanged unrelated files and normal exits. Windows
150%, ordinary Trash completion and current installer lifecycle acceptance
remain separate gates. Receipt: `dist/WINDOWS_SHORTCUT_ACCEPTANCE.json`. The
previous failed Windows check is retained under
`.hermes/evidence/windows-shortcut-current/`.

The exact current AppImage passes Dark 100% / Light 150% confirmations,
cancellations, permanent deletion outside Trash and exact-byte ordinary Trash
retention. Portable executable identities and production DEB/RPM payload,
GLIBC, polkit and embedded-source checks pass. Receipt:
`dist/packages/DELETE_SHORTCUT_ACCEPTANCE.json`. These focused checks do not
close clean native installation or full daily-driver acceptance.

Owned GUI processes, displays, buses, profiles, fixtures, package extractions
and compiler runtimes are removed. The owned Windows guest was restored to its
initial shutoff state; the existing installation checksum is unchanged. Copy
Undo ownership across restart and safe retained-transaction reclamation remain
open, as do the other platform and release gates listed above.

### Copy retained-data ownership — publication evidence prerequisite

Linux Copy now captures a content/tree/symlink snapshot of its private staged
candidate before publication and carries that evidence into its Undo receipt.
Copy Undo validates the current publication against that snapshot, preserving
in-place edits rather than withdrawing the edited file or tree. The same
content validator serves cross-filesystem Move Undo. A regression covers files
and directories, plain and overwrite Copy: source, edited publication and any
retained original overwrite backup are unchanged when Undo refuses the edit.

All 336 regular platform tests pass (one existing ignored test), alongside
strict Linux and Windows-target platform Clippy. Source receipt:
`.hermes/evidence/copy-content-evidence/SOURCE_ACCEPTANCE.json`. This step is
source-only; current deliverables remain the accepted `083fb3f` packages.
Durable ownership/journal creation before publication, recovery discovery and
UI ownership after restart, retained-backup evidence, interrupted Undo replay
and safe reclamation are still unfinished. Windows Copy Undo remains
unsupported in the platform implementation; Windows compilation alone does
not close that separate capability gap. No automatic retained-data deletion
has been enabled.

### Copy backup ownership — pre-exchange content evidence

Linux overwrite Copy now captures the original destination's content snapshot
before exchanging it into the retained backup. Undo checks both publication
and backup content after validating the directory bindings. A real descriptor
opened on the original before Copy then writes into the retained file after
the exchange; the file and directory-child regressions verify Undo refusal
without changing the source, replacement or edited backup.

All 337 regular platform tests pass (one ignored); strict Linux/Windows-target
UI Clippy passes. Source receipt:
`.hermes/evidence/copy-content-evidence/backup/SOURCE_ACCEPTANCE.json`. Linux
package/GUI acceptance is pending. Durable journal ownership before publication,
restart discovery, interrupted Undo replay and safe reclamation remain open;
no automatic retained-data deletion has been enabled.

### Copy content protection — current Linux package acceptance

Linux AppImage, portable, DEB and RPM are refreshed from clean tracked revision
`cef00db`. Exact AppImage Dark 100% / Light 150% GUI Copy, same-inode edit and
Activity Undo attempts preserve the edited copy, original source and unrelated
fixture; the screenshots confirm Undo becomes unavailable. Normal exits and
owned teardown pass. Portable executable identity and production native payload,
polkit, GLIBC and source-provenance checks pass. Receipt:
`dist/packages/COPY_CONTENT_ACCEPTANCE.json`. Backup open-writer evidence remains
the real filesystem platform test scope; GUI overwrite acceptance was not run.

Undo now distinguishes changed contents from missing/replaced entries. Copy
content checks follow the corresponding entry identity checks, and Activity
renders its Undo state/reason once rather than duplicating it in the operation
message. All 337 regular platform and 351 regular UI tests pass (one ignored in
each suite), with strict Linux/Windows-target UI Clippy and formatting checks.
Linux AppImage, portable, DEB and RPM are refreshed from clean revision
`76f441f`. Exact AppImage Dark 100% / Light 150% Copy/edit/Undo refusal checks
preserve all fixture bytes; both screenshots show one explicit contents-changed
reason. Normal exits and owned cleanup pass. Production native payload and
portable-path checks pass; installer lifecycle was not rerun. Receipt:
`dist/packages/UNDO_STATUS_ACCEPTANCE.json`. Successful Undo message deduplication
has source-state tests, without a new GUI success-flow claim. Windows packages remain the separate
accepted `083fb3f` artifacts. Durable Copy journal ownership/discovery across
restart, interrupted inverse replay and safe reclamation remain unfinished.
No automatic retained-data deletion has been enabled, and these focused checks
do not close installer lifecycle or full daily-driver acceptance.


### Copy destination-local journal — source verification

Linux Copy now writes a versioned `copy-journal` in its private destination-local
transaction before publication. It records native source/target paths, original
staged content and identity, directory/mount bindings and boot identity. Overwrite
intent records the displaced entry and content before exchange. Both namespace
directories are synced before the journal advances to Published. The shared
atomic journal writer remains authoritative; no separate persistence engine was
added. Move Undo explicitly owns its restoration staging through its existing
protocol, without creating a second Copy-owned record there.

All 339 regular platform tests and 351 regular UI tests pass (one ignored in each),
with strict Linux/Windows-target UI Clippy and formatting checks. The real SIGKILL
fixtures verify durable overwrite evidence immediately before and after exchange;
a journal-write failure refuses publication and preserves both versions. Evidence:
`.hermes/evidence/copy-local-journal/`. This is source-only; current Linux packages
remain the accepted `76f441f` build. Central registration/discovery, validation of
restart authority, Activity ownership restoration, interrupted Undo replay and safe
reclamation remain unfinished. Local records alone do not enable restart Undo or
automatic deletion. Windows Copy Undo remains a separate unsupported capability.


### Copy central registration and read-only restart discovery — source verification

Linux Copy now registers each destination-local transaction in its configured
state-home Copy registry before publication. Central and local records share the
atomic journal writer and advance together; Copy uses a transaction-local lock
so unrelated Copies can publish concurrently. Copy and Move roots have separate
identity markers validated through the same root-opening implementation.

The public discovery facade reads existing records without creating directories,
markers or journals and without repairing or deleting data. It validates schemas,
native paths, content-evidence shape, directory/mount bindings and journal pairs.
Missing, replaced, malformed, mismatched or another-boot evidence remains visible
as needing review. Matching journals alone do not grant restart Undo authority or
prove that the publication/backup contents remain unchanged. Journal and marker
reads reject unsafe ownership/permissions, symlinks and non-regular entries;
substituted FIFOs are rejected without blocking.

All 344 regular platform and 351 regular UI tests pass (one ignored in each), with
strict Linux/Windows-target UI Clippy and formatting checks. Fresh-process,
read-only/no-creation, altered/missing local record, FIFO, registry replacement,
Move/Copy marker coexistence and concurrent Copy checks pass. Verification uses
private state-home data and removes it after execution. Evidence:
`.hermes/evidence/copy-registry/`. This is source-only; current Linux packages
remain the accepted `76f441f` build. Restart mutation authority, Activity ownership
restoration, interrupted inverse replay and safe reclamation remain unfinished.
No restart Undo or automatic retained-data deletion has been enabled. Windows
Copy Undo remains unsupported.

### Copy directory object evidence across restart — source verification

Copy journal schema 2 records identity-only filesystem object handles for its
publication parent and private transaction directory when the filesystem supplies
them. Discovery compares those handles against newly opened objects, alongside
the existing device/inode, boot and unique mount IDs. It also checks the opened
local-journal directory itself. No descriptor is retained between sessions and
no privileged handle-opening operation is used. Unsupported handles remain absent;
other lookup errors are reported. Schema 1 records require review and are preserved,
without an implicit migration or deletion path.

An ordinary-user probe on the project filesystem observed immediate inode reuse
after deletion/recreation, with a distinct object handle. Project-filesystem and
tmpfs probes preserve handles across rename and distinguish replacements and
symlinks. The regression suite checks those behaviors and refuses both altered
generation evidence in otherwise matching journal pairs and malformed empty
handles, without repairing either record or changing published contents.

All 345 regular platform tests pass (one ignored), with strict Linux and
Windows-target UI Clippy and formatting checks. Evidence:
`.hermes/evidence/copy-object-handles/`. This is source-only; Linux packages remain
the accepted `76f441f` build. Directory evidence is one part of restart authority:
published/backup tree object evidence, current-content validation, fresh session
guards, Activity ownership, interrupted inverse replay and safe reclamation remain
unfinished. Matching journals still do not grant restart Undo or deletion authority.
