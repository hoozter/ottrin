# Ottrin roadmap

Last updated: 2026-10-15

Ottrin will complete this roadmap before release. Sequencing may change, but product outcomes are not removed to make an earlier release.

Linux is the primary implementation and daily-driver platform. Windows follows the same product contract in parallel where that does not materially slow Linux, and both ship in the same release. Further macOS implementation and acceptance are deferred until the Linux and Windows roadmap is complete.

The roadmap names outcomes. Linked design and evidence documents define constraints; they do not create extra work by themselves.

## Current Linux delivery status

The current local Linux artifacts, exact source revisions, checksums and verification scopes are recorded in `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json`. AppImage, portable, DEB and RPM share stable paths under `dist/packages/`. They include the saved appearance/view repairs, clickable address navigation, complete menu icons and compact action spacing, plus native index-service cleanup. Ubuntu and Fedora package lifecycle evidence is scoped to the exact candidates named in that receipt; a documentation-only repack is distinguished from a new native runtime test. These focused checks do not close the broader release gates below. Older receipts retain their original revision scope.

The saved-view reset was traced to UI tests writing the operator's settings; test profiles are now private, and the runtime restores the saved view. The remaining indexer CPU defect was reproduced with the packaged daemon and the operator's configured roots using a private database copy: ordinary file-change bursts spent about 94% of one core in subtree cleanup. Cleanup now follows indexed parent links instead of scanning the complete generation for each changed path. A bounded database-work regression protects that behavior.

The October 3 native clipboard-event repair is part of the current source. The September 30 AppImage predates that repair and must not be used to evaluate the corrected source. Local builds now put all deliverables from `build-all.sh` in `dist/packages/`; read each artifact's source revision rather than inferring freshness from its version number. Current package acceptance and remaining limits are recorded in `.hermes/coordinator-worker-report.md`, with October 9 baseline evidence in `.hermes/evidence/takeover-20261009/REPORT.md`.

These repairs do not close the full Linux daily-driver or Windows release gates below. Hermes remains paused; the old task board's completion labels are not release acceptance.

## Current Windows local delivery — 2026-10-15

Portable, NSIS and MSI at the stable `dist/` paths use clean source `41027a0`.
Newer coherent candidates from clean `fcf00aa` are staged under
`.hermes/packaging-tmp/windows-current/`. Their exact hashes and native scopes are
in that directory's `CANDIDATE_ACCEPTANCE.json`. The exact NSIS passes fresh
installation/removal, running-daemon replacement, enabled/disabled running/idle
intent preservation, concurrent-setup refusal and corrupt-journal refusal. The
exact MSI passes fresh installation/removal, published-to-candidate upgrade with
a running daemon, and all four repair-intent cases without requiring reboot.
Failed-transaction MSI registration rollback remains rejected, including
independent file-only controls, so these candidates remain unpublished. Broader
current GUI/DPI, login/reboot, Office, NSIS full payload rollback and signing gates
remain separate. The native acceptance described next retains its published
`41027a0` scope.

Current installed NSIS native GUI acceptance verifies readable drive breadcrumbs,
actual drive-root navigation with its full file column visible, exact address
submission to a real trailing-space directory, return navigation, search, and the
same scheduled daemon before/during/after normal window close and subsequent
file watching. Current MSI standard-user installation verifies all 51 installed
files, index registration/query, uninstall and preference/source preservation.
Archive/NSIS payloads match all 51 stage files; MSI embedded data additionally
matches both authoritative installer helper scripts. Current hashes and limits
are in `dist/WINDOWS_PACKAGES_ACCEPTANCE.json` and
`.hermes/evidence/windows-20261011/WINDOWS_COMPLETION_CANDIDATE_ACCEPTANCE.json`.

The preceding `4a0c13b` reboot, cleanup-failure/retry and populated-index CPU/offline
results remain historical evidence for that revision. Current exact reboot/login,
MSI failed-transaction registration rollback, interactive-installer
breadth and physical GPU gates remain open. These are local test builds, not full
release acceptance. The October 11 snapshot below is historical.

## Audited platform status — 2026-10-11

This snapshot reconciles the roadmap with local source, package provenance and retained acceptance receipts. It is not a fresh full-platform test run. `.hermes/coordinator-worker-report.md` is historical: its HEAD and Linux artifact inventory predate the changes below, and its `active` label does not establish a running worker or goal.

- **Linux:** stable AppImage, portable, DEB and RPM are built from clean source `554591d`. AppImage SHA-256: `3758bae8aabda470eb224b7b2889cff6018b480ccbcb2f2fe8dd893e9db5ed55`. Exact-package offline restart freshness, populated/burst/post-restart quiet CPU, session cleanup, Dark 100% / Light 150% Move/menu checks and native/portable payload identities pass. Receipt: `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json`. Personal settings/unit/running daemon remain untouched; the verified launcher uses this package on the next service start. Fresh native install/upgrade, live-home/content-enabled performance, broader graphics/authentication and full daily-driver acceptance remain open.
- **Linux remaining before the new deferred outcome:** clean-desktop package installation/launch; the consolidated daily-driver workflow including cross-volume conflicts, interruption/recovery and editor recovery; real removable-volume lifecycle; supported installed-LibreOffice conversion/fallback; audible representative media; the packaged helper/polkit permission matrix; and an explicit assistive-integration decision. Existing Xvfb and focused native checks narrow these tasks but do not close them. Accessibility is currently disabled after an upstream lifecycle defect.
- **Windows:** current portable/NSIS/MSI packages are built from clean revision `4552483`; all three current artifact hashes match `dist/WINDOWS_PACKAGES_ACCEPTANCE.json`. Exact fresh standard-user silent MSI install/uninstall and NSIS install/reinstall/uninstall are accepted in `dist/WINDOWS_CURRENT_INSTALLER_LIFECYCLE_ACCEPTANCE.json`. Concise task-query error/refusal behavior is accepted in `dist/WINDOWS_TASK_ERROR_NATIVE_ACCEPTANCE.json`. Current failed-transaction MSI registration rollback, cross-version upgrade and interactive installer acceptance remain open. Earlier cleanup failure/retry and GUI receipts retain their original artifact scope. Native ordinary-user evidence exists for text saving, many Move/recovery cases and scheduled indexing including simultaneous two-user coexistence, Office conversion before the unfinished containment changes, and a generated video thumbnail. This is substantial implementation progress; focused package checks below do not close full Windows release acceptance. Cross-volume Move is enabled in source after native transaction and guest power-cut checks; its public workflow/package acceptance remains open. The Office UI provider remains gated. Historical AppContainer diagnostics passed isolation fixtures but timed out on DOCX and Kit initialization; the committed production helper now supersedes that conversion path. See `.hermes/evidence/windows-20261010/office-appcontainer-private-temp-env-results.log`, `office-appcontainer-kit-budget-results.log`, `office-appcontainer-private-temp-docx-results.log` and `office-appcontainer-kit-stack-results.log`. A fresh isolated guest on October 11 passes all ten containment/process fixtures after making fixture executable access self-contained (`9d20378`). The Windows production converter now uses the packaged LibreOfficeKit helper instead of the CLI global pipe denied by AppContainer. Two fresh-profile production DOCX checks pass (7.31s and 9.93s), covering PDFium rendering/text, source preservation and cleanup; in-flight cancellation passes (2.65s). All 17 Linux provider contracts and strict Windows helper Clippy pass. The latest contained nine-format sweep passes DOC/DOCX/ODT but times out on XLS/XLSX/XLSM/ODS/PPTX/ODP. An uncontained ordinary-user Calc control also times out; a message-pump experiment returns a load failure. Both threading experiments are rejected, with production code unchanged. Macro suppression is unaccepted because its positive control did not execute. All-format/fidelity/fallback and exact-package acceptance remain open, so the Office UI provider stays gated. Current native evidence is in `.hermes/evidence/windows-20261011/REPORT.md`.
- **Windows continuous-indexing source:** the shared authenticated daemon now handles UI/CLI queries and writes, configuration, watches and service lifecycle. Native standalone startup/reuse/restart/stop, per-user login-task definition, live task replacement/removal, and short populated-index quiet CPU checks pass within their documented fixture scopes. No-argument indexd launches the persistent daemon; one-shot indexd mode is removed. These changes are newer than the published `4552483` packages. Actual scheduled-daemon execution, GUI search/lifetime and current packaged upgrade/rollback remain required before publishing the replacement candidates.
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
- Session undo for rename, Linux single-entry and batch Copy with Skip, Rename, or Overwrite conflict handling, newly created empty files and folders, newly created symlinks, permission changes, one proven Linux home-Trash entry, and single-entry Linux same-filesystem Move with Skip, Rename, or Overwrite conflict handling. Pathname inverses are guarded by link-entry identity, no-replace publication, storage identity, and in-flight path conflict checks. Copy and Move Overwrite additionally bind the exact retained displaced destination and atomically exchange it back only while both entries retain their recorded identities. Move Overwrite keeps durable recovery evidence until undo cleanup completes and can roll back both entries after interruption immediately following publication. File and folder creation undo is offered only while the exact entry remains empty and unchanged; symlink undo never dereferences the link target. Linux permission undo retains an identity-bound descriptor and restores the prior mode only while that same non-symlink entry still has the mode Ottrin applied. Home-Trash undo binds the exact native payload and refuses an occupied original path; mounted-volume Trash and Windows Recycle Bin entries remain outside this session-undo guarantee because their native metadata does not prove durable source-volume identity. Linux batch Copy Undo retains per-entry receipts and reverses the remaining entries in order; a conflict stops the batch with explicit progress and permits retry without repeating completed inverses. Restored operation grouping and completed filename reuse are covered by the exact-package recovery acceptance above; overlapping operation chains remain open. Multi-entry Move Undo remains unavailable and Activity labels that boundary as `No batch undo`. Quick Inspector image rotation remains an in-place rewrite with no retained original-byte or identity-bound transaction receipt, so Operation History truthfully labels it `Not undoable` rather than offering a lossy counter-rotation. Manual retained-Move recovery is likewise labelled `Not undoable`: it repairs an interrupted transaction but does not create a second inverse receipt.
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
accepted `083fb3f` artifacts. Copy now has durable paired journals and read-only published-tree validation after
restart. Restoring Undo/Activity ownership, interrupted inverse replay and safe
reclamation remain unfinished.
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

### Copy published-tree validation after restart — source verification

Copy journal schema 3 records per-entry filesystem object handles and unique mount
IDs for the staged publication and, before overwrite exchange, the displaced tree.
Native relative paths correspond exactly to the existing content snapshot. Root
device/inode/type evidence remains authoritative in that snapshot; duplicated root
identity fields have been removed. Earlier schemas are preserved as needing review.

Copy and Move share one content sampler. It traverses opened objects, enumerates
directories relative to those handles, reads symlink targets without following
them, and hashes the already opened regular object. File timestamps, directory
timestamps and the final root path/mount binding are checked for concurrent changes.
Copy's observer captures object evidence in that same traversal; recovery compares
each expected object before reading its contents. A substituted FIFO cannot block
the sampler, and a redirected directory cannot take it into an unrelated tree.

Within the recorded boot and mount bindings, read-only discovery now distinguishes
matching publication-intent journals from
Published objects and contents that match their complete evidence. Published data
with missing object evidence, edited/replaced entries, changed descendants or
symlinks, altered overwrite backups, or unexpected candidate staging needs review.
Discovery neither repairs journals nor changes source, publication or backup data.
This is point-in-time validation, not mutation authority.

All 348 regular platform and 351 regular UI tests pass (one ignored in each), with
strict Linux/Windows-target UI Clippy and formatting checks. Fresh-process discovery,
eleven changed-data cases, a file-to-FIFO race and a directory-to-outside-symlink race
pass. Verification owns a private state-home and temporary directory and tears them
down. Evidence: `.hermes/evidence/copy-tree-restart/`. This is source-only; current
Linux packages remain the accepted `76f441f` build. Fresh session guards, restoration
of Undo/Activity ownership, interrupted publication/inverse reconciliation and safe
reclamation remain unfinished. Ownership across an OS reboot is not established.
No restart Undo or automatic deletion is enabled.

### Durable Copy Undo intent and completion — source verification

Session Copy Undo now uses its paired central/local journals. Under the private
transaction's exclusive recovery lock it validates the receipt against the record,
directory identities/mounts/handles, staging ownership/permissions and complete
published/backup data. It persists UndoIntent before withdrawing the publication,
syncs both directories, validates the resulting objects and contents, then persists
Undone. Copy/create inverses share one physical withdrawal implementation; create
and Move-restoration staging retain their existing ownership protocols.

A matching UndoIntent is classified by its data topology: publication still in
place, or withdrawal already complete. An active receipt retry performs at most
the remaining work, without exchanging entries a second time. A matching Undone
receipt can acknowledge completion again. Changed data, mismatched journal pairs,
invalid staging permissions or a competing lock owner are refused without repair.
Copy records continue to retain the withdrawn publication rather than delete it.

Read-only discovery now reports pending-before-withdrawal, pending-after-withdrawal
and matching completed inverses. Actual SIGKILL fixtures exercise four boundaries
for files and directories, with and without overwrite: durable intent, withdrawal,
directory sync and durable completion. All sixteen cases preserve original source
and both applicable versions and leave matching journal pairs with recognized
topologies. Active-receipt retries at all four boundaries and the refusal cases pass.

All 351 regular platform and 351 regular UI tests pass (one ignored in each), with
strict Linux/Windows-target UI Clippy and formatting checks. Evidence:
`.hermes/evidence/copy-undo-journal/`. This is source-only; packages remain unchanged.
New-process guard/receipt restoration and Activity integration remain unfinished;
the crash fixtures establish recognition, not new-process mutation authority.
Journal-pair mismatch reconciliation, automatic safe reclamation, cross-boot
ownership and filesystem object-evidence support beyond the tested scope remain
open. No startup Undo or automatic deletion has been enabled.

### Copy receipt restoration in a new process — source verification

The platform now exposes `restore_retained_copy_undo_receipt(transaction_id)` for
the configured registry. Restoration rejects invalid IDs, mismatched record pairs,
changed directory/object/content evidence, unsafe staging and competing recovery
owners. It opens parent and stage through verified bindings, takes the transaction
lock and establishes fresh watches before validating the data. Normal publication
and restoration share the same watch/guard constructor; discovery, restoration and
Undo share opened-directory validation. Receipt identities are derived from the
authoritative snapshots rather than stored again.

Restoration does not write journals or change the namespace. The returned receipt
can run the durable inverse or finish a matching interrupted inverse; every Undo
revalidates under the lock. PublicationIntent records still need reconciliation,
and completed inverses do not issue another receipt. Boot/mount bindings and
complete filesystem object evidence remain required.

All sixteen SIGKILL cases now exercise new-process admission: twelve pending
cases restore and complete without repeating withdrawal, while the four completed
cases reject a new receipt. Six normal-exit cases cover file, directory and symlink
Copy, with and without overwrite. Restoration preserves journal bytes and retains
no parent/stage directory descriptors. Edited data before or after restoration,
replaced staging, mismatched pairs, invalid/missing IDs and lock contention are
refused while preserving data and records.

All 354 regular platform and 351 regular UI tests pass (one ignored in each), with
strict Linux/Windows-target UI Clippy and formatting checks. Evidence:
`.hermes/evidence/copy-receipt-restart/`. This is source-only; current packages
remain unchanged. Startup Activity ownership and grouped operation history are not
connected yet. Publication-intent reconciliation, pair-mismatch reconciliation,
safe reclamation, cross-boot ownership and broader filesystem acceptance remain
open. No automatic startup inverse or retained-data deletion has been enabled.

### Retained Activity discovery worker — source verification

Retained Move discovery now runs in one background worker instead of blocking
application construction or operation-result handling. A refresh during an active
scan invalidates that response and coalesces further requests into one replacement
scan. Activity shows checking status, disables recovery actions until the current
scan completes, and reports a stopped worker rather than waiting indefinitely.
The worker exits when its owning app disconnects; the UI polls only while work is
pending, without adding an idle refresh loop.

Controlled worker tests block discovery and prove responsive UI polling, execution
on a different thread, stale-result rejection, refresh coalescing, worker exit and
explicit channel-failure handling. Startup and operation-warning tests wait for
and verify the actual asynchronous discovery snapshot. All 354 regular platform
and 353 regular UI tests pass (one ignored in each); strict Linux/Windows-target
UI Clippy and formatting checks are recorded in
`.hermes/evidence/activity-discovery-worker/`.

This prerequisite is source-only; packages remain unchanged. Copy startup Activity
ownership, grouped batch history, interrupted-publication/pair reconciliation,
reclamation and broader filesystem/platform acceptance remain open. No startup
inverse or retained-data deletion is enabled by this change.

### Durable Copy operation grouping and order — source verification

Copy journal schema 4 records one operation ID, the original zero-based item
position and requested item count. A batch allocates its ID before any item is
published; every publication/Undo phase in both journals retains that metadata.
Single-item Copy uses its transaction ID as the operation ID. Skipped items do not
shift later positions, and a batch stopped by failure retains the original count.
Discovery exposes this metadata for startup Activity grouping. Missing records
can indicate Skip, failure or interruption; item count alone never proves batch
completion.

The public single/batch APIs share one receipt/publication implementation with
explicit operation metadata; no thread-local grouping state is used. Invalid IDs,
zero counts, out-of-range positions, missing metadata and earlier schemas are
retained for review and refuse restart receipt admission. No migration or deletion
of earlier retained data is performed.

A new-process test discovers a batch with a skipped middle item and an independent
single Copy, restores receipts and runs inverses in reverse original order. It
checks matching journal pairs, unchanged metadata through Undo, source preservation
and untouched skipped/independent files. Additional tests cover a batch failing
after its first publication and malformed/missing/older metadata refusal without
data or journal mutation. All 357 regular platform and 353 regular UI tests pass
(one ignored in each), with strict Linux/Windows-target UI Clippy and formatting
checks. Evidence: `.hermes/evidence/copy-operation-order/`.

This is source-only; stable packages remain unchanged. Startup Activity still needs
to merge these operation groups, validate duplicate/conflicting group metadata and
own their guarded receipts. Interrupted-publication/pair reconciliation, safe
reclamation, cross-boot ownership and broader filesystem/platform acceptance
remain open. No automatic startup inverse or retained-data deletion is enabled.

### Copy startup Activity ownership — source verification

Startup now discovers Copy groups and restores guarded receipts on a background
worker. Move and Copy scans share one bounded worker implementation: one active
scan per loader, coalesced refresh requests, stale-result rejection and explicit
worker failure. Copy groups merge into normal Activity records. Receipt transaction
IDs and durable ownership prevent duplicate current-session/restored entries; scans
are deferred while operations or inverses are running. Successful session Copy and
Undo do not repeatedly hash all retained history. Uncertain outcomes and superseded
startup snapshots request a fresh scan.

Activity retains completed inverses and review-only groups independently of Undo
receipt availability, protecting them from history trimming and Clear. Missing
previously owned items remain owned and become Needs review. Duplicate item
positions, inconsistent counts, publication-intent records, invalid evidence and
receipt-admission failures offer no inverse. Missing request slots are described as
unknown outcomes. Available groups use the existing batch inverse worker in reverse
original order; already completed inverses are excluded. Conflict checks cover every
batch receipt, including a running restored inverse.

Controlled integration tests run real discovery/restoration off-thread, merge a
partial inverse, execute its remaining Undo through Activity, verify unchanged
sources and completed journals, and preserve ownership after restart, clearing,
trimming and lost evidence. Tests also cover duplicate/inconsistent metadata,
current-session state preservation, deferred merge and later-item conflicts.
All 357 regular platform and 355 regular UI tests pass (one ignored in each),
with strict Linux/Windows-target UI Clippy and formatting checks. Evidence:
`.hermes/evidence/copy-startup-activity/`.

This is source-only; stable packages remain unchanged. Exact-package restart UI
acceptance is required next. Overlapping batch publications need chain validation
before restart Undo can be admitted; current discovery refuses those mismatching
individual snapshots. Publication-intent/pair reconciliation, safe reclamation,
cross-boot ownership and broader filesystem/platform acceptance remain open. No
automatic startup inverse or retained-data deletion has been enabled.

### Copy completion lifecycle — fixes from exact-package restart QA

The `5211d37` AppImage passed real two-file Copy, process restart, restored Activity
Undo, durable journal completion and source preservation. Native/portable payload
identity checks passed. GUI inspection found two remaining defects, so this is an
initial investigation rather than final acceptance. Evidence and cleanup:
`.hermes/evidence/copy-startup-activity/INITIAL_PACKAGE_OBSERVATIONS.json`.

Batch receipt completion used a destination-directory surrogate as an entry path,
which refreshed its parent and left copied files visible after successful Undo.
Receipts now expose their actual publication path, and completion refreshes every
receipt's affected parent once. A real asynchronous listing regression verifies the
visible destination becomes empty after the remaining batch inverse.

Durably completed inverses now validate their owned private copied candidate,
namespace and matching journals. Publication names have been released to later user
work; reusing or editing those entries no longer invalidates completed history.
Evidence is explicitly named `UndoneRetainedObjectsAndContentsMatch`. An active
completed receipt remains a non-mutating idempotent result, while new receipt
admission still refuses an already completed inverse. Interrupted Undo continues
to require the complete withdrawn namespace before finishing its journal. Tests
cover later entry creation/editing with and without overwrite, changed retained
candidate refusal, journal preservation and interruption followed by unrelated
publication, plus Activity restoration after a later batch reuses the same names.

All 359 regular platform and 355 regular UI tests pass (one ignored in each), with
strict Linux/Windows-target UI Clippy and formatting. Source evidence:
`.hermes/evidence/copy-completion-lifecycle/`. Rebuilt exact-package Dark/Light
restart/Undo/history acceptance is still required. Overlapping publication chains,
publication-intent/pair reconciliation, safe reclamation, cross-boot ownership and
broader platform/filesystem acceptance remain open; no automatic deletion or
startup inverse has been enabled.

### Copy restart Activity — rebuilt Linux package acceptance

The exact `123e703` AppImage passes two-file Copy, normal process restart,
restored batch Undo and another restart in Dark 100% and Light 150%. The visible
destination refreshes to zero items. A subsequent batch reuses the released names;
an in-place edit makes only that new batch Needs review while the older completed
batch remains Copy undone. Retained owners cannot be cleared. Source, edited copy,
other copy, unrelated fixture and all paired journals are preserved. Each mode exits
normally four times; its private processes, bus, display and profile are removed.
Portable, DEB and RPM payload identities match the AppImage. Receipt:
`dist/packages/COPY_RESTART_ACTIVITY_ACCEPTANCE.json`. This accepts the tested
Linux same-filesystem restart workflow; the recovery/platform limits above remain
open. The narrow Miller empty-state caption clips beside Activity and needs repair.

### Empty-folder layout — clipped Miller columns

Native Copy restart acceptance exposed a clipped empty-folder caption when the
Activity panel leaves only part of a Miller column visible. Empty-state text and
actions now lay out within the visible column rectangle, without changing saved
column widths or horizontal scrolling. Captions wrap inside that width. A rendered
regression covers left/right clipping, full columns, Dark/Light and enlarged text.
All 355 regular UI tests pass (one ignored), with strict Linux and Windows-target
UI Clippy. The run also exposed competing background discovery in ordinary UI test
fixtures: those fixtures now use an empty worker, while recovery integration tests
explicitly inject real discovery. Their journal, inverse and preservation assertions
remain intact and pass in the concurrent suite. Exact-package layout acceptance is
pending; the latest delivered `123e703` package retains its recorded scope above.

### Empty-folder layout — exact-package follow-up

The `665c358` package repeats two-file Copy, normal restart and restored Undo in
Dark 100% and Light 150%; destination counts become zero, source bytes and paired
journals are preserved, and all private processes/profile are cleaned. Refreshed
portable/DEB/RPM payload identities pass. Actual screenshots still show the caption
clipped beside Activity, contradicting the helper-only rendered acceptance. Layout
acceptance remains open. Evidence:
`.hermes/evidence/copy-completion-lifecycle/EMPTY_LAYOUT_INITIAL_PACKAGE.json`.
A follow-up is testing the production Miller viewport and its inherited clipping.

The follow-up explicitly constrains Miller's inherited clip to its file viewport.
A production-renderer regression restores a partly visible current column beside a
panel and checks that its entire empty caption stays inside the pane and wraps.
All 356 regular UI tests pass (one ignored) with private XDG state, plus strict
Linux/Windows-target UI Clippy and formatting. The earlier two verification runs
had inadvertently used the operator's state directory; 22 journals tied to their
exact disposable fixture paths were removed after confirming their sources,
publications and transactions were absent. Registry/markers were preserved.
Evidence: `.hermes/evidence/copy-completion-lifecycle/TEST_JOURNAL_CLEANUP.json`.
A rebuilt exact-package layout check is still required; it is not yet accepted.

Activity operation identity now belongs to the history record instead of its disposable Undo receipt. Successful or refused text-edit Undo can release the editor session while preserving its text-edit classification; the rename fallback is removed. The save/Undo regression also inspects rendered Activity text. All 356 regular UI tests, the non-skipped native text save/Undo fixture, strict Linux/Windows-target UI Clippy and formatting pass. The correction was packaged and accepted in the preceding `c27403f` Linux package. Exact-package Dark 100% / Light 150% native text save/Undo, restored batch Copy Undo and caption layout acceptance pass. Native/portable payload identities pass; receipts are linked in the current delivery status above. Windows native GUI and broader editor/filesystem acceptance remain separate.


### Overlapping Copy dependencies — packaged checkpoint

Read-only Linux discovery now recognizes an older publication retained in a newer
Copy's backup. It requires matching central/local records, namespace bindings,
complete filesystem object evidence and unchanged content; dependencies must end
in a fully validated current publication. Ordering comes from retained identity,
not timestamps or transaction IDs. Ambiguous owners, cycles, changed data and
missing records remain review cases. Discovery grants no Undo authority: an older
receipt is refused until its newer overwrite is undone and the publication is
freshly revalidated.

Activity retains the blocker identities internally and describes the affected
file to the user. Successful Undo requests a single background refresh only when
it unblocks another owned Copy. A real two-operation UI regression proves the
predecessor becomes available without a restart. Three-overwrite native fixtures
cover files, directories and symlinks; publication/backup edits, journal mismatch
and missing-owner controls preserve data and refuse admission.

Overlaps within one batch still need lazy reverse-order receipt admission; this
checkpoint does not close that requirement. Interrupted publication and journal
pair reconciliation, safe reclamation, cross-boot ownership and Windows Copy
remain open. The current `2003969` package contains this checkpoint. Exact-package Dark 100% / Light 150% fresh-process restoration and both Undo actions pass, including automatic predecessor admission, unchanged sources, paired completed journals, visible empty caption and normal exits. Fixture publications were produced by the source-built platform API, not the packaged GUI. Native/portable payload identity checks pass; native installation/authentication remains separate. Receipt: `dist/packages/COPY_DEPENDENCY_ACCEPTANCE.json`.

Source verification for this checkpoint passes 361 regular platform tests and 357 regular UI tests (one ignored in each), strict Linux/Windows-target platform/UI Clippy, formatting, and all four disposable-runner contracts. The runner now uses short private temporary paths compatible with Unix sockets; alias/directory Copy fixtures bind private journal roots rather than shared app state.


### Restored overlapping Copy batches — packaged checkpoint

Restored Activity now admits an opaque reverse-order plan for one Copy operation,
including overlapping entries. The plan binds the original validated records and
obtains a fresh receipt only after a newer inverse has restored the next
publication. Matching transaction IDs alone grant no authority. Original journal
fields are checked again under the inverse transaction lock; only permitted Undo
phase progression is accepted. A refused inverse retains its progress and its
original in-flight receipt for retry without adopting changed evidence.

Activity uses one batch worker for live receipts and restored plans. It rejects
stale grouping snapshots, external blockers, inconsistent ordering and changed
data. Native platform fixtures cover three overlapping files, directories and
symlinks, read-only admission, original source preservation, completed journals,
and paired-journal edits before restoration and after receipt admission. The real
Activity worker regression completes all three inverses in one batch; existing
partial/completed ownership and external-operation refresh tests remain covered.

Source verification passes 363 regular platform tests and 358 regular UI tests
(one ignored in each), strict Linux/Windows-target platform/UI Clippy, formatting
and diff checks. The exact `9622e61` AppImage passes Dark 100% / Light 150% fresh-process
restoration, one-click batch Undo, unchanged sources and paired journals, zero-item
listing/visible empty caption, and completed history after normal restart. Fixture
publications were source-produced; exact GUI publication is a separate scope.
Receipt: `dist/packages/COPY_BATCH_PLAN_ACCEPTANCE.json`. Windows native Copy, interrupted
publication reconciliation, safe reclamation and cross-boot ownership remain open.


### Recovery performance and owned GUI cleanup follow-ups

Six independent 2 MiB publications demonstrate whole-registry plan-admission read
amplification: one plan reads 12,611,475 characters and six plans read 75,668,306
(`/proc/self/io`, including journal/metadata reads). Plan restoration must validate
only its captured operation records while retaining the original-evidence and
external-blocker refusal guarantees. Evidence:
`.hermes/evidence/copy-batch-plan/READ_AMPLIFICATION.json`.

The exact batch restart also exposes a Miller viewport issue: the destination
breadcrumb/footer are correct, but its column starts outside the visible viewport
while ancestor columns are shown. Target-column reveal needs a focused fix.

The menu GUI harness left eight private index daemons because teardown searched
command lines for profile paths carried only in HOME/XDG environment variables.
Exact uid and profile ownership were verified before terminating those daemons
and one private bus; the regular user daemon was preserved. Earlier command-line
cleanup claims are corrected by `.hermes/evidence/menu-alignment/ORPHAN_CLEANUP.json`.
The harness now verifies environment identity and cleans private processes before
removing the profile. Fresh Dark/Light success and cancellation with a live private
daemon both pass; no test processes/profile remain.

The Light batch harness was repeated under an explicit Python supervisor after a
unified-session exit 143 observation. The supervised harness exits zero, both
applications exit zero, all three inverse/restart checks pass and owned cleanup
is empty. Receipt: `.hermes/evidence/copy-batch-plan/gui/Light-SUPERVISOR_EXIT.json`.


### Scoped Copy restoration and startup directory reveal — source checkpoint

Plan admission now consumes private observations retained by read-only discovery.
The observation binds original records, evidence and complete observed operation
membership. Admission revalidates only those records and their content/namespace;
it no longer scans or hashes unrelated Copy publications. Public metadata edits,
missing members, changed paired journals and captured external blockers refuse
admission. Original-record comparison under the inverse lock remains unchanged.

The same six-operation 2 MiB native fixture now reads 2,101,764 characters for one
plan and 12,610,061 for all six, versus 12,611,475 and 75,668,306 previously: an
83.34% reduction in plan-admission reads. These counters include journal/metadata
reads, not just file contents. `.hermes/evidence/copy-batch-plan/SCOPED_READS.json`
records the fixture and library identity. A thread-specific read-byte regression
proves an unrelated 4 MiB publication is not reread, avoiding timing thresholds.

Directory startup now requests the existing one-shot Miller reveal after navigation,
as file startup already does. The rendered deep-directory fixture proves its empty
caption is visible and the request is consumed. Ordinary later scrolling retains
its existing policy. Native `a242cdc` inspection exposed a remaining root cause:
the expanded Smart Panel covered file-view space that navigation treated as
visible. The layout now reserves the whole panel for all file views and the
caption-only overlay compensation is removed. The startup regression now opens
a 420-pixel Activity panel after the initial reveal and requires the destination
caption to remain fully visible beside it. Layout coverage includes collapsed/expanded panels in
all three views and narrow windows. Native `31db85a` confirms initial startup
visibility but exposes a second transition: opening Activity after the initial
reveal hides a directory column. Resize preservation now includes a previously
visible current directory as well as a file preview; deliberate horizontal
scrolling remains respected. The rendered regression exercises actual panel
opening and subsequent manual scrolling/another expansion. Rebuilt-package
Dark 100% / Light 150% acceptance now passes in `b2d2702`, including the panel
opening transition, full caption visibility, restored reverse batch Undo,
completed restart and current main/View Options menu visuals. The receipt is
`dist/packages/COPY_SCOPED_VIEWPORT_ACCEPTANCE.json`. Both app launches exit
normally in each run; all private processes, displays, buses and profiles are
removed, and the task-owned compiled fixture is removed.

All 365 regular platform tests and 359 regular UI tests pass (one ignored each),
as do strict Linux/Windows-target platform/UI Clippy, formatting and diff checks.
The scoped Copy source checkpoint has been packaged as `a242cdc`; Dark 100%
restored batch Undo and completed restart pass with unchanged sources and paired
journals. The subsequent viewport repair passes all 360 regular UI tests (one
ignored), and exact-package Dark/Light GUI checks now pass. Other roadmap
requirements and platform acceptance gates remain open.

### Index listener idle readiness — source checkpoint

An isolated empty-root daemon still woke its main thread approximately 100 times
per second: 595 voluntary context switches over six seconds, 0.5% of one CPU core
and up to 10.19 ms ping latency in the reproducible baseline. The listener now
waits for OS socket readiness. Incoming connections wake it immediately; a
100 ms timeout bounds shutdown requested by client workers. Interrupted waits
retry and actual readiness errors retain the existing explicit shutdown/error
path. All 213 regular search tests pass (two ignored), all 360 regular UI tests
pass (one ignored), and strict Linux/Windows-target search/UI Clippy passes.
The source-built daemon records 60 wakeups over six seconds, 0.167% of one core,
0.358 ms maximum ping latency and a clean 113.832 ms shutdown. These short,
empty-root measurements are in `.hermes/evidence/index-idle-current/`; they do not
establish CPU usage while watching a real, changing home directory. Exact-package
empty-root checks pass with the same daemon hash: 60 wakeups, no measured CPU
ticks, 0.308 ms maximum ping latency and clean 113.964 ms shutdown. The installed
systemd service is now running that exact binary after graceful upgrade, with
unchanged preferences/unit and the same database inode; the existing main app
was preserved. Its home-directory watch is active. Startup work was still
running at the upgrade checkpoint; a subsequent observation reports Ready with
158,825 entries and an active watch. Over twelve seconds with the existing main
GUI, CPU measures 0.5% of one core while the snapshot revision advances from 143
to 146. This records a changing home session, not strict quiet idle. Evidence:
`.hermes/evidence/index-idle-current/INSTALLED_UPGRADE.json` and
`INSTALLED_SAMPLE.json`. Sustained quiet-idle and heavy-change acceptance remain
open.

### Windows background index process identity — native portable acceptance

The Windows scheduled refresh now targets the dedicated `ottrin-indexd.exe`
with `--index-update`, replacing the main GUI executable as its task action.
The dedicated process uses the Windows GUI PE subsystem to avoid console flashes;
portable builders include it, installer manifests require it, and the release
verifier requires its presence. This preserves the current Windows periodic
refresh contract; continuous Windows daemon/IPC parity is not established.

The CLI and dedicated refresh share one owned single-update implementation. It
disables filesystem watching for this bounded run, commits the requested update
and releases the worker/writer lock on completion or failure. Two consecutive
updates against the same private database prove committed entries, source-byte
preservation and writer reuse. All 214 regular search tests pass (two ignored).
Strict Linux and Windows-target app/search Clippy pass. A newly exposed listener
capacity race also now reads a bounded rejection after a failed request write,
while refusing successful operation replies after failed writes; the real
capacity regression and complete search suite pass.

The exact portable archive from clean revision `8602748` now passes native
limited-user Windows checks: the process is `ottrin-indexd`, has no main window,
two consecutive refreshes commit 2,000 then 2,001 fixture entries, and the public
service installer registers that executable with `--index-update`. An actual
Scheduler run under the ordinary interactive token commits all 2,001 entries.
Source bytes are preserved; the original task, preferences and cache are restored,
and the private fixture is removed. Receipt collection initially failed at the
provenance path and then at PowerShell metadata serialization; the corrected full
rerun passes. Receipt: `dist/WINDOWS_INDEX_IDENTITY_ACCEPTANCE.json`.

Portable, NSIS and MSI artifacts are refreshed at their stable paths from that
same clean revision. Archive SHA-256 is
`a9652ec7a0a0c4f20c7ae89414d93434c9d417b1200f7229fec001344ce72368`;
NSIS is `205a329534ef39bece57f4a2fbc7a07fac389bb24f8e4ffd86dd359456d90a40`;
MSI is `de9755ccabd44b0ef1a396218de662639d4830bd3c7ec924b55894cd96c13750`.
This refresh supersedes the older installer artifacts; their prior lifecycle
receipts do not establish acceptance of these new bytes. The exact refreshed MSI now passes install and uninstall under a fresh ordinary
standard user using an interactive, medium-integrity token. All 51 payload entries
match their hashes, both Start menu shortcuts and icon bytes match, the installed
index task targets `ottrin-indexd.exe --index-update`, and uninstall removes
payload files, task, registration, menu folder and cached product icon while
preserving preferences. Product state transitions from 5 to -1. Receipt:
`dist/WINDOWS_CURRENT_MSI_LIFECYCLE.json`. The original installation receipt's
file counter reports one JSON array; the hash-verification loop checks every
entry, and the corrected uninstall receipt counts 51. Both batch-logon controls
fail before package actions with Windows Installer 1601 / access denied, whether
the service is initially stopped or started; this is separate from the passing
interactive standard-user workflow.

The exact refreshed NSIS also passes fresh standard-user silent install,
same-version reinstall and uninstall. All 51 payload hashes, Installed Apps
registration/icon, Start menu targets and the dedicated index task match. The
managed preview-cache file survives installation/reinstall and is removed by
uninstall; preferences and a user-data control remain unchanged. An unknown file
in the cache root is preserved deliberately. The first new cleanup fixture used
that unknown-file location and failed; the corrected full three-phase run uses
the authoritative `preview_cache` path and passes. Receipt:
`dist/WINDOWS_CURRENT_NSIS_LIFECYCLE.json`. This proves same-version reinstall,
not cross-version upgrade. Cross-version upgrade, failure recovery/rollback,
interactive installer UI, installed GUI workflows and continuous Windows
daemon/IPC parity remain open.


### Cleanup failure reporting — Linux and Windows CLI acceptance

Thumbnail-cache cleanup previously ignored read, metadata and removal failures,
allowing Settings and CLI uninstall to report success while managed files remained.
It now returns a path-specific error through both Settings cleanup actions and
CLI uninstall. The existing NSIS failure branch therefore receives a nonzero
cleanup exit instead of deleting the application after a hidden thumbnail-cache
failure. Search fallback-file cleanup likewise uses its authoritative removal
helper so inspection errors are propagated rather than skipped. Automatic
zero-budget cache pruning logs cleanup failures.

Private-directory regressions cover a missing cache, a blocked directory entry,
preserved nested data, retry after removing the obstruction, an unreadable cache
location, and removal of a symlink without changing its target. No recursive
cache-root deletion is introduced. All 363 regular UI tests and 214 regular search tests pass (one and two ignored,
respectively), including the ownership regression; strict Linux/Windows-target
app/search checks and Linux UI-test Clippy pass.

The clean `ad364d4` AppImage passes actual CLI failure/retry acceptance: a blocked
entry returns exit 1 with its path, nested data/source/preferences are unchanged,
and removal of the obstruction permits exit 0 and managed cache deletion. All
private profiles and commands are cleaned. Receipt:
`dist/packages/CACHE_CLEANUP_ACCEPTANCE.json`. Linux portable, DEB and RPM are
refreshed from that same revision. The production native-verifier block confirms
executable/PDFium identity, embedded revision, GLIBC and polkit metadata, with all
portable entry requirements present. Receipt:
`.hermes/evidence/index-idle-current/NATIVE_PACKAGE_IDENTITY.json`.
Windows portable, NSIS and MSI are now refreshed from clean `3c7e9e6`.
The exact portable passes a native limited-token interactive CLI test with a
thumbnail held open without delete sharing: cleanup returns 1 and identifies
its path, preserves the locked file/preferences/user source, then returns 0 and
removes the managed thumbnail after release. All test commands and the private
profile exit/are removed; the outer task, compiler/extracted payload and guest
copies are removed, with the pre-existing installation checksum unchanged.
Receipt: `dist/WINDOWS_CACHE_CLEANUP_ACCEPTANCE.json`.

The refreshed MSI/NSIS are constructed; their new artifact hashes supersede the
preceding lifecycle receipts. Retained NSIS installation after cleanup failure,
retry and current installed lifecycle acceptance remain required.
Rendered Settings error feedback and full current-package GUI/platform acceptance
remain separate from this CLI gate.


### Silent NSIS early-failure retention and retry — native acceptance

Clean `4e43998` gives every scripted installer message a silent-mode default and
sets a nonzero error level before all uninstall failure aborts. Interactive
warnings retain their messages. The rebuilt stable portable and NSIS artifacts
carry that source revision. Exact NSIS SHA-256 is
`7fa0b6c18e911eb01ac0230e44fae0a8fc3d3df123c99fe29cf1ea6a9f034fce`.

Native fresh standard-user silent installation verifies all 51 payload hashes,
index task, registration and shortcuts. Three early uninstall failure controls
pass: a delete-locked thumbnail, missing cleanup executable, and a present
executable Windows cannot launch. Each returns 1 without a waiting dialog;
after test-controlled restoration all payload hashes, shortcuts and Installed
Apps registration are intact. The restored app responds to `--help`. Release
of the cache lock permits normal silent uninstall, which removes payload/task/
registration/shortcuts and managed cache while preserving preferences and user
data. Failure status is measured using NSIS's in-place `/S _?=INSTALLDIR`
execution; the final retry uses normal `/S` and verifies final state. Receipt:
`dist/NSIS_FAILURE_RETRY_ACCEPTANCE.json`. The initial single-control run passed;
the full rerun includes all three controls. The disposable account/profile/tasks,
batch-logon right and fixtures are removed, and the original guest installation
checksum is unchanged. Recursive payload-deletion failure,
cross-version upgrades, MSI rollback and interactive installer UI remain open.


### NSIS locked-payload retry — native acceptance

A native negative control of the preceding `4e43998` installer confirms a dead
end: locking `LICENSE` during recursive removal returns 1 and retains Installed
Apps registration, but deletes both `ottrin.exe` and `Uninstall.exe`. The isolated
fixture required reinstallation for teardown; that repair is not acceptance.
Baseline: `.hermes/evidence/index-idle-current/NSIS_PAYLOAD_FAILURE_BASELINE.json`.

Clean `ba13c7a` removes other payload files first while reserving the cleanup
executable and uninstaller. DELETE-access handles and read-only checks reject
unremovable retry executables before payload deletion; handles are closed on
success and failure. The exact native standard-user silent run passes locked
cleanup-executable, locked-uninstaller and read-only-cleanup-executable controls:
all 51 payload hashes remain intact. A locked `LICENSE` then reaches actual
partial payload deletion and leaves precisely the locked file plus both retry
executables. The cleanup executable responds to `--help`, registration remains,
and normal `/S` retry after lock release completes without reinstalling.
Preferences and user-data controls are preserved. The disposable account/profile,
tasks, batch-logon right and fixtures are removed; the original guest installation
checksum is unchanged. Receipt:
`dist/NSIS_PAYLOAD_RETRY_ACCEPTANCE.json`. Exact NSIS SHA-256 is
`cbebd9d3e9a25fa04a2ae01d5e4d49fcf4614fe3e87cb0e9bfbe0ec906ca3934`.

Failure status uses an identical copied uninstaller worker outside the payload
with `/S _?=INSTALLDIR`, preventing self-locking from confounding the file-lock
control. This proves cleanup/retry capability; full GUI usability after partial
payload removal is not claimed. Sudden interruption, cross-version upgrades,
MSI rollback, other filesystems and interactive UI remain separate gates.


### Windows multi-user index task collision — confirmed native failure

The exact clean `ba13c7a` portable archive is tested with two fresh local standard
accounts and the public `index service-install` command. Account A succeeds;
account B fails with `0x80070005 / E_ACCESSDENIED` at `GetTask('Ottrin Index')`.
The shared global task name blocks B before the existing ownership check can run.
A's task XML and security descriptor remain byte-for-byte unchanged across B's
failed attempt. The first comparison observed delayed persistence of the test's
own disable operation; a fresh COM read after that state settled and a repeated
B attempt prove preservation. This is a failure receipt, not acceptance:
`.hermes/evidence/index-idle-current/WINDOWS_MULTIUSER_TASK_FAILURE.json`.

The task installer, status/run/delete command arguments and MSI transaction
helper all currently address that shared name. They must use one authoritative
identity scoped to the Windows user, including rollback and retirement of any
superseded managed registration without touching another user's task. Until
that coherent implementation and a simultaneous two-user native retest pass,
Windows background-service coexistence remains incomplete. This does not depend
on user testing or new permissions. The index data/profile paths remain separate;
this failure concerns Scheduler registration and management.

The product registration and both actor registrations, disposable accounts/profiles,
temporary batch-logon rights, command processes and extracted guest fixtures are
removed. The pre-existing guest installation checksum remains unchanged. The
failure is confirmed and remains unfixed; the next work is the authoritative
user-scoped task identity and simultaneous native two-user acceptance.


### Windows user-scoped task identity — source checkpoint

One shared `windows-index-identity.ps1` defines the SID-based task name for the
application's embedded setup/management code and the installed MSI transaction
helper. Status/run/delete use the same identity (successful resolution is cached
per application process; failures can retry). Setup retains current-user task
permissions/disabled state and retires the old global registration only after
checking its owner, action and logon type. A failed retirement restores prior
registrations rather than leaving two writers scheduled. The MSI snapshot schema
is updated to capture scoped and superseded owned registrations together; rollback
checks current ownership before restoration or removal. No management operation
falls back to the global name.

All 214 existing regular search tests pass (two ignored), along with the added
scoped-operation regression; strict Linux/Windows-target app/search checks and
MSI source contract pass. Source completion does not close the native failure.
Current deliverables predate the change. A clean rebuild, simultaneous two-user
setup/management, retirement of an owned old task, preservation of a foreign
old task, and native transaction rollback acceptance remain required.


### Windows scoped task identity — native coexistence and helper rollback acceptance

Clean `b2f7fc0` portable passes the public setup/status/remove commands under two
fresh standard accounts. Owned global-task retirement preserves disabled state
and permissions. The native transaction helper restores the prior global XML/
security and removes the newly created scoped task on rollback. Account B then
installs, queries, rolls back, removes and reinstalls its own scoped registration
beside A's scoped and foreign global registrations. Both A definitions and
security descriptors remain byte-for-byte unchanged. Both scoped registrations
coexist; receipt `dist/WINDOWS_SCOPED_TASK_ACCEPTANCE.json`.

WiX initially rejects two unversioned files in one auto-GUID component. The
`e937db7` correction gives the identity script a separate component. Portable,
NSIS and MSI are refreshed at stable paths, with runtime binary hashes identical
to the native-tested build. Full current installer lifecycle, interactive-token
Run, packaged MSI rollback and continuous Windows daemon parity remain separate
from this public-management/native-helper gate.

Both disposable accounts/profiles, three owned product registrations, actor tasks,
temporary batch-logon rights and compiler/extracted guest fixtures are removed;
the pre-existing guest installation checksum is unchanged. Runtime main/indexer
hashes in the refreshed packages match the exact native-tested binaries.


### Refreshed scoped MSI — exact installed lifecycle acceptance

The exact clean `e937db7` MSI, SHA-256
`fd600666e72a0cce60b9a233dcea742f802f2de1dfd6f0bdfa7d6e0a65a528b8`,
passes native install/uninstall under a fresh standard account with an interactive
secondary-logon token. All 51 portable payload entries match; both installed
`index-task-transaction.ps1` and `windows-index-identity.ps1` match their source
bytes. Shortcuts/working directory, shortcut/Installed Apps icon bytes, per-user
SID task target/arguments and installed product state 5 pass. Uninstall removes
payload/scripts, scoped task, menu folder, registration/marker and cached product
icon, transitions product state to -1 and preserves the saved preferences.
Receipt: `dist/MSI_SCOPED_LIFECYCLE_ACCEPTANCE.json`.

An additional scheduled-run probe calls public `index service-restart` from the
same secondary-logon account. The task does not execute (last result `0x41303`,
last-run time unset); the bounded probe expires without an indexed fixture.
The account has no primary signed-in desktop session. This is retained as an
unsuccessful context probe, not product index acceptance or a proven daemon bug.
A real signed-in-session run remains required; packaged MSI failure/rollback,
cross-version upgrade and interactive installer UI are also still open.

The disposable account/profile, encrypted temporary credential, actor tasks,
product task and extracted guest fixtures are removed. The pre-existing guest
installation checksum remains unchanged. The failed secondary-logon Run probe
is retained separately within the lifecycle receipt and does not close its gate.


### MSI failed-uninstall recovery — source repair and native helper verification

The current `e937db7` MSI fails the locked-preview-cache uninstall probe: exit
1603 retains all 53 installed files but deletes the user's scoped index task and
leaves Windows Installer product state 1 (advertised). A normal retry then fails
with error 2753 because MainExecutable is no longer marked for installation.
Baseline: `.hermes/evidence/index-idle-current/MSI_UNINSTALL_FAILURE_BASELINE.json`.

Uninstall now snapshots task state immediately after InstallInitialize and pairs
cleanup with rollback before destructive standard actions. Both installation and
uninstall commit from a transaction-owned copy of the same helper and identity
script, so successful uninstall can remove its installed scripts before commit.
Rollback and commit remove their transaction scripts; failed restoration retains
its snapshot for diagnosis. The existing MSI source contract passes. A native
standard-user helper probe restores task XML/security and verifies rollback and
commit cleanup using only the detached transaction scripts. This is helper
acceptance, not revised MSI package acceptance. The revised MSI still needs an
exact-package locked-cache failure/retry test and ordinary install/uninstall
checks before publication. Existing deliverables retain their prior provenance.


### MSI uninstall rollback — exact revised-package result

The clean `1f29a94` MSI (SHA-256
`792a51b4187dc4cd797b94b2fdd654a15eeb0989e3c2c10fac77affff2433962`)
passes fresh standard-user installation, including all 51 portable payload hashes,
both installed transaction/identity scripts, scoped task, shortcuts, and icons.
The locked-cache uninstall failure now restores the task XML and security and
removes its transaction snapshot/scripts. All payload hashes remain intact.
However, the full recovery gate still fails: product state becomes 1 (advertised),
and a subsequent ordinary uninstall fails with error 2753/1603. This remains an
open registration/rollback defect; task recovery alone does not close it.
Windows Installer logs rollback errors 3 and 5. The failed test and causal logs
are retained in `.hermes/evidence/msi-rollback-current/`; the compact receipt is
`dist/MSI_UNINSTALL_ROLLBACK_ACCEPTANCE.json`. Current Windows artifact pointers
record the exact scope and failing gate. Linux artifacts are unchanged.


### Windows index task inspection — distinguish unavailable from absent

Windows service status and restart/uninstall preflight no longer interpret every
failed `schtasks` query as a missing registration. One shared scoped-task lookup
returns absence only for ERROR_FILE_NOT_FOUND and propagates access denial or
other scheduler failures. Setup, MSI snapshots, and status use that lookup.
Status inspects authenticated task ownership and Settings XML through Scheduler
COM, with explicit UTF-8 output, instead of parsing localized command failures.
Management stops before Run/Delete when inspection fails. Source tests cover
absence, invalid XML, query errors, and prevention of subsequent mutations; the
full search suite passes (216 regular tests, two ignored). Native PowerShell 5.1
checks real Scheduler absence and injected COM missing/denied/failure branches.
This is source/helper acceptance, not full current packaged-app acceptance.
Continuous Windows daemon/IPC and MSI registration rollback remain open.


### Windows index task inspection — exact portable native acceptance

The clean `95f7d39` portable archive (SHA-256
`5644a55586b88352b6aa1211acecad46b73252ad6178f78ab3c15df34af50e90`)
passes service status/setup/remove checks under a fresh non-administrator user.
An actual task DACL denies query access: status correctly remains sourced from
Task Scheduler with an unavailable diagnostic; restart and removal fail without
changing task XML or last-run time. The full original security descriptor is
restored, normal removal succeeds, status becomes absent, and preferences remain
unchanged. `dist/WINDOWS_TASK_QUERY_NATIVE_ACCEPTANCE.json` records exact binary
identity, medium-integrity token, cases, and limits. Error details still include
a verbose PowerShell stack trace; concise user-facing error formatting remains
open. This does not prove native Settings rendering or continuous daemon/IPC.
The refreshed MSI is built from the same clean producer, SHA-256
`a80b577de64cf636d005ddbd7bda27db787566bff13dca49bdd23bdc148d486f`.
Its exact installation lifecycle has not been repeated. MSI registration
rollback remains an open gate, independently of task query acceptance.


### Windows task query error display — exact native correction

The clean `4552483` Windows portable archive, SHA-256
`182b866abc927d6e294a1a87269fa74133ca803afe059df5a55329fedff67653`,
passes the same fresh standard-user status/setup/remove and real task-ACL denial
checks. The denied status, restart, and removal preserve the underlying error
message and failure behavior without PowerShell source lines, stack metadata, or
CLIXML. Task XML/last-run time are unchanged by denied management; the original
security is restored before normal removal, and preferences are unchanged.
Sixteen focused Windows search tests and strict Windows all-target Clippy pass.
`dist/WINDOWS_TASK_ERROR_NATIVE_ACCEPTANCE.json` binds the native checks to the
exact executable and medium-integrity account. Portable, NSIS, and MSI packages
are refreshed from the same clean source. Native Settings rendering, current
installer lifecycle, continuous daemon/IPC, and MSI registration rollback remain
separate open gates.


### Current Windows installer lifecycle — exact native acceptance

Both installers from clean `4552483` now pass under a fresh non-administrator
medium-integrity secondary interactive account. MSI SHA-256
`42dca996e8429163a7a53a33e27eac11a6249d0cb6e6de6911abca931903f60c`
passes silent installation/uninstall, all 51 portable payload hashes and both
installed helper hashes, scoped indexd task, shortcut/icon registration, complete
removal of product registration/payload/task/transaction files, and preference
preservation. NSIS SHA-256
`a4217d3184e63bedaab1cf6d0e5002c214acdbadd2cafbd6f5d3f3f13cf02077`
passes silent install, same-version reinstall and uninstall, all 51 payload
hashes, scoped indexd task, Installed Apps/shortcut registration/removal,
generated-cache cleanup, and preference/user-source preservation. The account is
reused sequentially only after MSI removal.
`dist/WINDOWS_CURRENT_INSTALLER_LIFECYCLE_ACCEPTANCE.json` retains exact receipts
and token scope. Native GUI/Installed Apps interaction, cross-version upgrades,
continuous Windows daemon/IPC, and failed-transaction MSI registration rollback
remain open; ordinary lifecycle success does not close those gates.


### Continuous Windows indexing — shared wire contract foundation

The index daemon request/response schema and bounded frame codec now have one
transport-independent implementation in `crates/ottrin-search/src/daemon_protocol.rs`.
The existing Unix client/server use it directly; Windows test builds compile the
same contract without suppressing dead-code validation or creating a second
protocol. The Windows production daemon is still the one-shot updater. Native
Windows execution of the compiled test binary passes six protocol tests: bounded
round trips, operation identity, oversized input/output rejection, fragmented
Unicode responses, and malformed/truncated requests. Fourteen Linux daemon and
protocol tests, including live filesystem watch and snapshot restart, plus strict
Linux/Windows all-target Clippy pass. This refactor is a foundation only. Secure
Windows transport, peer authentication, persistent daemon lifecycle, remote UI
search/configuration, startup/update/stop integration, idle CPU checks and native
end-to-end acceptance remain required. Published packages are unchanged.


### Continuous Windows indexing — native process identity foundation

`IndexDaemonIdentity` obtains the current user SID from the Windows process token,
derives a per-user pipe endpoint name, and rejects a peer PID whose token belongs
to another user or cannot be inspected. Process/token handles are owned and
released on all result paths; the temporary SID allocation is freed. It does not
authenticate executable provenance or a connected pipe: transport callers must
obtain peer PIDs from kernel pipe metadata. Four native Windows tests pass,
including current-process acceptance, invalid-PID rejection, a live owned peer
running as another account, and no handle-count growth across 100 inspections.
Strict Linux/Windows all-target Clippy pass. Native test executable SHA-256:
`2454e75712375b2f21ea99843629dafc9b4598e8e96ade5544b92a0f4f6be3cc`.
This public Windows transport component is not wired into a running daemon.
Pipe ACLs, bounded I/O/cancellation, connected-peer metadata, service/server/client
lifecycle, UI integration and end-to-end/idle acceptance remain required.
Published application packages remain unchanged.


### Continuous Windows indexing — native authenticated connection layer

Windows named-pipe listener/stream components now use per-user protected ACLs,
remote-client rejection, exclusive first-instance binding, and PID/SID checks of
the actual connected peer before command data is sent. Read/write/connect waits
have deadlines; pending operations are canceled and drained before buffers/event
handles are released. Accepted/rejected/disconnected instances retain namespace
ownership while their replacement is created. An abandoned-client regression
initially failed: DisconnectNamedPipe did not make the instance immediately
connectable. Replacing that instance before closing the old handle fixes it.
Five regular native tests pass: round trip, collision refusal, accept timeout
retry, read cancellation/recovery, buffered reply after close and abandoned-peer
recovery (round-trip/collision share one test). Three coordinated native probes
pass owner-server teardown, foreign-user ACL denial, and counterfeit-server
PID/SID rejection with zero request bytes received. The early cross-user harness
missed its stop deadline; its controller was fixed and final probes repeated.
Receipt: `.hermes/evidence/windows-20261011/WINDOWS_PIPE_TRANSPORT_ACCEPTANCE.json`.
This is transport acceptance only. Shared command dispatch, persistent Windows
daemon/server/client lifecycle, UI integration, scheduling/update/shutdown and
end-to-end idle CPU acceptance remain open. Published packages are unchanged.


### Continuous Windows indexing — persistent daemon core

The authenticated Windows pipe now serves the existing persistent SearchService
worker through one shared client API and command dispatcher with Unix. Windows
`ottrin-indexd --index-daemon` enters that server; active clients are bounded by
the pipe instance capacity, rejected peers do not terminate the listener, and
shutdown stops watchers and releases the writer lock before relinquishing the
endpoint. The scheduled task and GUI still use their preceding paths.

A native Windows integration test passes ping/peer PID, reconcile/query, watched
file changes, rebuild, replacement folder configuration, storage clearing, an
expected error after clearing, shutdown and subsequent writer-lock acquisition.
It links the production library/storage path but runs the server on a thread
inside the test process with the guest SYSTEM token. The initial library-test
flow failed because its test-only storage disables staged generations; the flow
was moved to an integration test rather than adapting production code to that
fixture. Receipt: `.hermes/evidence/windows-20261011/WINDOWS_DAEMON_CORE_ACCEPTANCE.json`.
Five Linux daemon regressions and strict Linux/Windows app/search all-target
Clippy pass. This does not establish standalone daemon lifetime, standard-user
startup, scheduler/update integration, UI routing, packaged end-to-end behavior
or idle CPU acceptance. Published packages are unchanged.


### Continuous Windows indexing — captured process upgrade lifecycle

The Windows client can inspect the authenticated server executable and prepare
an upgrade while waiting up to 30 seconds for that captured process object to
exit. The process handle is opened and its token checked while the authenticated
pipe is live; preparation is sent on that same connection. Query-only peer
authentication retains its limited rights; synchronization rights are requested
only for captured lifetime/executable inspection.

Native integration now runs the daemon in an owned subprocess, verifies its
distinct kernel-reported PID and executable, repeats indexing/watcher/config/clear
checks, and verifies process exit, endpoint removal and writer-lock reacquisition
after preparation. Both parent and child exit normally. Fourteen Linux
daemon/protocol tests and strict Linux/Windows app/search all-target Clippy pass.
Receipt: `.hermes/evidence/windows-20261011/WINDOWS_DAEMON_PROCESS_ACCEPTANCE.json`.
The actor links production storage but is an integration-test executable under
the guest SYSTEM token; it does not establish packaged standalone startup,
standard-user scheduler behavior, UI routing or idle CPU acceptance. Windows
GUI and scheduling integration remain next; published packages are unchanged.


### Continuous Windows indexing — application routing and login task source

Windows production UI now uses the shared remote search client for queries,
diagnostics, configuration, rebuild and clearing; closing the GUI does not own
or stop the daemon. Application startup and command-line index writers use the
package-owned standalone daemon, reusing equal executable content or preparing
a different running version before replacement. Status/search use the daemon
when connected and retain read-only persisted snapshots when it is absent.
Windows session Start/Stop now use the daemon. Scheduler install changes from
periodic one-shot updates to a per-user login trigger for `--index-daemon`, with
no execution-time limit or battery stop. Refresh timing belongs to the worker.
Inspection errors precede task-management runtime changes; disabled state and
permissions are preserved. The obsolete scheduler repetition-limit code/tests
are removed.

The current standalone indexd passes native cold startup, reuse of the same PID,
reconcile/query, client disconnection, graceful upgrade shutdown and restart
with retained index using a private profile under SYSTEM. Native registration
under the primary signed-in user's least-privilege interactive token passes
login-trigger/action/runtime/battery checks and disabled-state/security retention
on replacement. The fixture substitutes QA task names and disables retirement
of the historical global registration; it does not run the daemon at a real
login. Its initial assertion did not account for Task Scheduler account-name
normalization; the corrected check resolves SIDs. Controller task completion and
expected missing-task exit status were also corrected. Final actor/controller
exit normally and both QA registrations are removed. 218 regular Linux search
tests pass (two ignored), along with strict Linux/Windows all-target Clippy.
Receipt: `.hermes/evidence/windows-20261011/WINDOWS_UI_DAEMON_SOURCE_ACCEPTANCE.json`.
Native GUI search/lifetime, actual scheduled execution/login cycle, update
management, current installers and idle CPU acceptance remain open. Published
packages are unchanged and retain their earlier provenance.


### Continuous Windows indexing — service readiness and quiet populated fixture

Windows service install/restart now waits for the package-owned daemon to respond
instead of reporting success solely from task registration/Run. Installation
applies the caller's current search configuration after readiness; disabled
registration retains its state and a previously running session. Explicit session
Start/restart replaces the process, while Stop preserves index/login setup.
Settings exposes Stop on Windows and uses a nonblocking pipe-presence check to
distinguish stopped indexing from an unavailable live endpoint. Presence does
not substitute for command peer authentication. Rejected server peers now return
PermissionDenied so the daemon's rejection path can keep serving other clients.

Native standalone/session testing under SYSTEM passes with a private populated
1,001-file index and active watch: cold startup, reuse, public restart with a
changed PID, endpoint presence, public stop, endpoint disappearance, restart,
retained query result and cleanup. GetProcessTimes measures 0.046875 CPU seconds
over 12.000 seconds of request-free quiet time (0.3906% of one CPU core), below
the fixture's 1% limit. The preceding build before the peer-classification fix
measured the same percentage; this is a scoped measurement, not evidence that
that fix changed performance. 218 regular Linux search tests pass (two ignored),
as do strict Linux/Windows all-target Clippy checks. Receipt:
`.hermes/evidence/windows-20261011/WINDOWS_SERVICE_READY_IDLE_ACCEPTANCE.json`.

Actual scheduled execution/install/upgrade/removal readiness, native GUI status,
longer idle intervals, large trees and current packaged acceptance remain open.
No new coordinated foreign-peer probe is claimed for the error classification.
Published packages remain unchanged.


### Continuous Windows indexing — running task replacement/removal

Task registration replacement now stops and waits for the prior owned task
instances before writing a new definition. Shared Stop-OttrinIndexTask validates
owner/logon mode, requests scheduler stop and waits up to ten seconds for both
instances and running state to clear; timeout retains the registration and is
reported. Public Stop, task restart/removal and rollback task restoration use
that same helper after graceful daemon preparation where available. This also
handles a previous one-shot task that has no daemon endpoint. NSIS setup failure
text now names the background index service and directs users to Services
settings.

A native least-privilege actor in the primary signed-in user's session starts
three real scheduled sleeping workers and captures their process handles.
Replacement through the production setup script, explicit shared Stop preserving
XML/security, and rollback deletion each stop the captured worker. Actor/task
cleanup succeeds. The fixture substitutes QA task names and a harmless PowerShell
target for file-existence validation; it does not run the new --index-daemon
action or touch the existing installation/index. Receipt:
`.hermes/evidence/windows-20261011/WINDOWS_RUNNING_TASK_STOP_ACCEPTANCE.json`.
Fifteen focused Windows scheduling regressions on Linux, strict Linux/Windows
all-target Clippy, and MSI/NSIS lifecycle source contracts pass. Actual scheduled
index daemon execution, data preservation during upgrade/rollback, descendant
process teardown and exact current installers remain open. Published packages
are unchanged.


### Continuous Windows indexing — one authoritative daemon entry mode

Windows indexd now starts the persistent daemon without arguments as well as
with --index-daemon. Its superseded one-shot path and --index-update option are
removed; requested updates use the application's shared-client index update
command. Historical argument recognition remains only in owned obsolete task
retirement. The user guide distinguishes continuous-indexing source behavior
from the still-published periodic-task packages. The overview now points to
verified 4552483 artifacts and separates current source progress from release
acceptance; all three published file hashes match their current manifest.

Native current-indexd testing under SYSTEM starts a no-argument child, verifies
its daemon PID, reuses it through the application ensure API, repeats public
session restart/stop and retained queries, and proves removed --index-update
returns exit code 2. The private populated active-watch index contains 1,001
files. A twelve-second request-free sample measures 0.015625 CPU seconds, or
0.1302% of one core, below the fixture's 1% ceiling. No performance improvement
over preceding builds is inferred from this short sample. Twenty-two Linux
application binary tests and strict Linux/Windows all-target Clippy pass.
Receipt: `.hermes/evidence/windows-20261011/WINDOWS_DEFAULT_DAEMON_ACCEPTANCE.json`.
Owned test processes/profile/staging are removed. Actual scheduled daemon
execution, native GUI and replacement-package acceptance remain open; published
packages are unchanged.


### Current Windows candidate — native GUI rejection and path-identity repair

The clean 979db8f portable candidate, SHA-256
`c08414ea72e3ff3c79f3221d6fc4a825aa1648d5e8ade8a260d896331516e3d0`,
contains 51 files matching staging and native extraction. Its signed-in
least-privilege/Mesa GUI opens, searches, closes normally and preserves Miller
preference; the daemon indexes a new file after GUI closure. The DPI-aware
screenshot exposes duplicate results for normal/verbatim Windows paths. The
candidate is rejected and not published. Initial fixture alias/focus errors are
corrected before the functional pass and visual rejection.

Windows configured and query roots now share filesystem path resolution,
including exclude/content/active-volume roots and missing-child suffixes.
Unicode/verbatim names are preserved; Unix behavior is unchanged. Native
SYSTEM/private-fixture regression checks pass both root forms in Global and
CurrentFolder returning one file and configured aliases producing one root.
Lifecycle and retained queries pass; short quiet CPU is 0.5208% of one core,
without a performance comparison. All 218 regular Linux search tests (two
ignored) and strict Linux/Windows Clippy pass. Services wording now explains
that closing windows leaves indexing running and sign-in startup is optional;
this label is compile checked. Receipt:
`.hermes/evidence/windows-20261011/WINDOWS_PATH_IDENTITY_ACCEPTANCE.json`.

Discard the rejected candidate after consumers exit, rebuild from the fixed
commit and repeat exact-package GUI search before publication. Scheduled daemon
and installer acceptance remain open. Accepted dist packages are unchanged.


### Fixed Windows candidate — exact native GUI search and daemon lifetime

The clean 0aea0d8 portable candidate, SHA-256
`fcd6dc0440a14e836944383c3f53a36c33285521f9920668719828097992e624`,
passes native signed-in least-privilege/Mesa GUI checks with a private profile.
Ctrl+F search renders exactly one result row; the screenshot is inspected and
the CLI independently confirms one result. The window exits normally, preserves
Miller preference, and leaves the daemon watching: a file created after GUI
closure becomes searchable through the CLI. The owned daemon then exits on
preparation. All 51 archive/native-extracted files match staging.

The same-source NSIS candidate, SHA-256
`344807cbd3b350f81b66a10c26cb4f0ceade99fa61d80fef04a8dc48f5cf12e8`,
contains 51 extracted payload files matching that stage. No current NSIS
lifecycle result is claimed. Both candidates remain at the stable task staging
path `.hermes/packaging-tmp/windows-current` for installer/scheduled-daemon
validation. Receipt:
`.hermes/evidence/windows-20261011/WINDOWS_FIXED_CANDIDATE_GUI_ACCEPTANCE.json`.

This closes the duplicate-row GUI regression and focused post-window daemon
lifetime checks for this candidate. Native graphics, broader theme/scaling, real
login-trigger execution and installer upgrade/rollback remain open. Owned native
GUI/daemon/actor/profile/extraction fixtures are removed; accepted dist downloads
remain unchanged.


### Continuous Windows candidate — real sign-in and NSIS lifecycle

The 0aea0d8 NSIS candidate (SHA-256
`344807cbd3b350f81b66a10c26cb4f0ceade99fa61d80fef04a8dc48f5cf12e8`)
passes fresh silent install and same-version reinstall under a fresh standard
account actually signed into Windows. All 51 installed payload files match
staging; scheduled daemon setup starts it and public reconcile/query/restart
work. The test reboots the guest with the daemon running: the per-user login
trigger starts the installed daemon on the new boot, and its retained index
serves the fixture with an active watch. Public service removal stops it, deletes
the task and retains the database/preferences. Full uninstall subsequently
removes installed files, Installed Apps/Start menu entries and generated index,
while preserving preferences and user source.

The first uninstall probe waited only for the bootstrap launcher and saw files
remaining while its copied worker completed. Independent standard-user final
verification passes after completion; this is retained as a fixture observation,
not a product failure or a claimed worker exit code. Receipt:
`.hermes/evidence/windows-20261011/WINDOWS_SCHEDULED_NSIS_LIFECYCLE_ACCEPTANCE.json`.

Testing uses the disposable `ottrin-index-qa` qcow2 overlay and a fresh signed-in
standard account, so real task startup uses its real profile without changing
the existing guest's index. The base VM remains shut off and backing content
mtime is unchanged; libvirt adjusted access ownership while mode stays 0600.
Actors/processes and script fixtures are cleaned up; the shut-off overlay and
candidates remain needed for MSI/cross-version checks. Do not run the backing VM
while reusing this overlay; verify backing mtime before resuming. MSI, cross-version
upgrade, failed-transaction rollback and installer GUI remain open. Accepted
dist downloads are unchanged.

### Windows NSIS periodic-task migration — native standard-user acceptance

The published 4552483 installer (SHA-256
`a4217d3184e63bedaab1cf6d0e5002c214acdbadd2cafbd6f5d3f3f13cf02077`)
upgrades successfully to the 0aea0d8 continuous-daemon candidate (SHA-256
`344807cbd3b350f81b66a10c26cb4f0ceade99fa61d80fef04a8dc48f5cf12e8`)
under the actual signed-in standard test account. Both package versions are
0.1.0: this verifies a source-revision/task-definition migration, not a numbered
version upgrade. All 51 installed payload files match each respective stage.
The six-hour `--index-update` TimeTrigger becomes a running `--index-daemon`
LogonTrigger belonging to the same user. Public update/query report ready,
active watch and one fixture result. Preference and source-file hashes captured
before installation remain identical after upgrade and full uninstall.

Public service removal deletes the task and retains the database; full uninstall
then removes installed files, generated index, Installed Apps and Start menu
entries. Uninstaller bootstrap exit zero and independently inspected final state
are recorded, not a copied worker exit code. Initial fixture assertions needed
account-name-to-SID normalization and explicit bounded CLI process handling;
the unsuccessful probes are retained alongside passing native verification.
Receipt: `.hermes/evidence/windows-20261011/WINDOWS_NSIS_PERIODIC_UPGRADE_ACCEPTANCE.json`.

Owned actor tasks/processes and guest installers/scripts/results are removed;
the reusable overlay is shut off and backing content mtime remains unchanged.
Its test account/source/preferences and candidate staging remain necessary for
MSI follow-up. Published downloads are unchanged. Upgrade during an actively
writing old refresh, numbered-version upgrade, MSI, rollback and interactive
installer acceptance remain open; this result does not close those gates.

### Continuous Windows MSI candidate — ordinary native lifecycle

The same clean 0aea0d8 stage now has a native WiX 5.0.2 MSI candidate at
`.hermes/packaging-tmp/windows-current/Ottrin-0.1.0-windows-x64.msi`, SHA-256
`cdd87f89da6adfc1edc61539026bc9d87d7e8f1c4c931920a71798afdc22f529`.
The authoritative shell packager runs from a temporary clean checkout of the
candidate revision, retaining exact source/documentation/payload checks. A
QA-only native compiler adapter is restricted to the disposable Windows overlay;
Microsoft runtime downloads are verified against official SHA-512 metadata.

Actual signed-in standard-user silent installation and removal both return zero.
Windows Installer product state is installed (5), then unknown (-1). All 53
installed files match the portable stage plus exact-source transaction/identity
scripts. The running per-user login daemon serves one fixture result with an
active watch. Preferences and source hashes remain unchanged. Uninstall removes
the app directory, index task, generated database and Start menu entry. Native
MSI logs confirm successful install/cleanup actions and final engine returns.
Receipt: `.hermes/evidence/windows-20261011/WINDOWS_MSI_CONTINUOUS_LIFECYCLE_ACCEPTANCE.json`.

Owned actor/processes, guest compiler/payload/archive/scripts, host compiler
archives and temporary source checkout are removed. The overlay is shut off,
backing content mtime unchanged; its owned account/source/preferences and the
candidate MSI remain needed for follow-up. Published downloads are unchanged.
This closes ordinary silent lifecycle only. MSI upgrade/repair, interactive GUI,
failed-transaction rollback and the previously reproduced registration rollback
defect remain open.

### Windows MSI same-version replacement — fixed and native regression verified

The published 4552483 MSI and the original 0aea0d8 candidate both reported
version 0.1.0. A real standard-user upgrade returned zero and replaced all 53
files, but left both ProductCodes registered as installed. This is a product
registration defect, separate from the open failed-transaction rollback defect.
WiX's default equal-version behavior treats these builds as two products.

Revision 85f1395 enables standard `AllowSameVersionUpgrades` for local rebuilds.
A fresh published-to-fixed-candidate upgrade now returns zero, removes the old
registration (state -1), and retains the new installed registration (state 5).
Both old/new 53-file manifests match. The periodic index task becomes the running
login daemon, with a ready live watch and one fixture result. Preference and
source-file hashes remain identical. A native compiler-generated 0.0.9 fixture
is rejected at the newer-version launch condition (1603), preserving current
registration, all 53 installed files, preferences/source and search. Subsequent
removal of the upgraded MSI succeeds and leaves no product/app directory.

Current clean-source candidate hashes at `.hermes/packaging-tmp/windows-current`:
- Portable: `e433275c574d2f8d11b90d4a94da63a2f9fa75743828a0ee7587433396a942fe`.
- NSIS: `59a0bafe0d863594a8869cda0b9a2afcda08abb606e9be88e1ba364af625b997`.
- MSI: `b6f46322965d6e6b3a9624349dc49c2bc7d0fe576c901fcbdbcfc70165351771`.

Receipt: `.hermes/evidence/windows-20261011/WINDOWS_MSI_SAME_VERSION_UPGRADE_ACCEPTANCE.json`.
The unsuccessful registration probe and causal MSI log excerpts are retained.
An initial private ProductCode helper needed null COM-output suppression before
that real regression was identified; its observation is recorded separately.
Same-version builds may replace each other in either source-revision direction;
ordered releases must advance a first-three semantic version field.

Owned actors/processes, native compiler/fixture payloads/installers and host tool
archives are removed; overlay is shut off with backing content mtime unchanged.
The test profile/source/preferences and current candidates remain needed for
rollback/live-worker follow-up. Portable/NSIS were rebuilt coherently but this
new native acceptance is MSI-specific. Public dist downloads are unchanged.
Active-old-writer upgrade, numbered-release upgrade, interactive installer,
MSI repair and failed-transaction rollback remain open.

### Windows failed-uninstall lifecycle — source fixes, registration gate open

A fresh native test of the 85f1395 MSI candidate (SHA-256
`b6f46322965d6e6b3a9624349dc49c2bc7d0fe576c901fcbdbcfc70165351771`)
reproduces locked-cache uninstall failure: all 53 payload files and task XML/
security survive, but product state becomes advertised (1), the running daemon
is left stopped, and the generated index was deleted before the cache error.
Standard-user silent repair is then refused with 1602 at `SECUREREPAIR`.
No MSI security-policy or registry-permission workaround is applied. The
[upstream per-user registration report](https://github.com/wixtoolset/issues/issues/8574)
is still open; this is related evidence, not proof of its complete root cause.

Two Ottrin-owned problems are fixed in source. Cleanup now clears the expendable
thumbnail cache first and returns its error before dismantling a usable index/
service. A native source-built CLI reports the actual sharing violation while
retaining the running task, byte-identical database, preferences and source.
Snapshot schema 3 records actual running instances. Restoration leaves identical
live tasks intact, resumes captured running state after task removal, and keeps
idle tasks idle. A disabled-running task is temporarily enabled for demand start,
then disabled again after startup; future login state is preserved.

Native standard-user helper checks pass enabled-running, disabled-running,
enabled-idle and unchanged-running cases, exact XML/security preservation,
unchanged InstanceGuid preservation, transaction artifact cleanup and a live
one-result query. An initial abrupt-stop probe and stale COM XML observation are
retained; the final fixture uses production graceful removal and fresh task
queries. Release Windows app build, strict Windows app/search all-target Clippy,
formatting and MSI source contract pass. Receipt:
`.hermes/evidence/windows-20261011/WINDOWS_RUNNING_ROLLBACK_SOURCE_ACCEPTANCE.json`.

These are source/helper checks, not an updated MSI package or full rollback
acceptance. Rebuild coherent candidates and repeat packaged failure/retry before
publication; product-registration rollback remains open independently. All test
processes and the corrupted task-owned overlay/disk/NVRAM/TPM are removed after
saving evidence. Cleanup discards the disposable environment rather than
claiming MSI recovery. Backing guest is shut off with unchanged content mtime;
published downloads and prior candidate packages remain unchanged. The compact
VM template remains available for fresh subsequent native QA.

### Cleanup/lifetime fixes — coherent Windows candidates rebuilt

Clean revision `160b803e80bd9444c4eecee193a171113c8dc5e9` now has coherent
portable, NSIS and native WiX MSI candidates at the stable task staging path
`.hermes/packaging-tmp/windows-current`. These include the blocked-cache cleanup
guard and schema-3 running-state restoration, alongside the persistent daemon,
path-identity and same-version replacement fixes.

Current candidate SHA-256 values:
- Portable: `07e54ad17514b4593ff06cd46550a638f48d954fde00085178514e298312ebe1`.
- NSIS: `48f6d318a7a1bc702406c84ddea9645aedaaeca2d03c4ad7e61b6f4f577bba2e`.
- MSI: `730d94edf407ee50ed3096a61715cfa6df6cde7e6b6bfc0b0f2904e29cc776a8`.

All 51 portable archive files and 51 embedded NSIS files match staging. The
53-file MSI embedded digest multiset matches the stage plus both exact-source
helper scripts. All authoritative package builds pass against the same clean
source/payload. Receipt:
`.hermes/evidence/windows-20261011/WINDOWS_CLEANUP_FIXED_CANDIDATE_BUILD.json`.

Host extraction/compiler archives and native compiler/payload/script fixtures
are removed. A fresh disposable overlay is shut off and retained for required
native lifecycle follow-up; its backing guest is still shut off with unchanged
content mtime. No test account was created during this compiler-only run.
Published downloads are unchanged. These are package-construction/payload
checks, not new native install/upgrade/rollback/GUI acceptance. Repeat packaged
lifecycle and blocked-cache recovery on these exact candidates; the per-user MSI
registration rollback failure remains open independently.

### Current NSIS candidate — packaged cleanup failure/retry accepted

The clean 160b803 NSIS candidate, SHA-256
`48f6d318a7a1bc702406c84ddea9645aedaaeca2d03c4ad7e61b6f4f577bba2e`,
passes actual signed-in standard-user silent install and same-version reinstall.
All 51 installed files match staging; live update/query report ready and one
fixture. Locked-preview-cache uninstall using `/S _?=INSTALLDIR` returns 1 while
retaining all payload files, Installed Apps registration, the same live scheduled
instance and byte-identical index database. The cleanup guard now has exact
installer acceptance, beyond the preceding standalone source CLI check.

After releasing the lock, ordinary silent retry exits its bootstrap with zero;
final-state inspection verifies removal of app files, task, generated index,
cache entries and Installed Apps/Start menu integration. Preferences and source
hashes remain identical. The empty preview-cache directory remains. Copied
worker exit code is not claimed. Receipt:
`.hermes/evidence/windows-20261011/WINDOWS_NSIS_CLEANUP_RETRY_ACCEPTANCE.json`.

The initial fresh-account run passed but its host export failed before fixture
cleanup; the complete retained receipt comes from a repeat using the same owned
preferences/source, exported and read back before cleanup. An upload interrupted
by planned reboot was also repeated with checksum verification. QA transport
errors now omit full request arguments while retaining action/server cause.
Owned actors/processes and guest installer/script/result fixtures are removed;
overlay is shut off and backing content mtime unchanged. Owned profile/source/
preferences and staged candidates remain needed for subsequent native gates.
Public downloads are unchanged. Current MSI registration rollback, exact-package
login/reboot, interactive installer UI and broader Windows acceptance remain open.

### Current NSIS candidate — real login startup and saved-index restoration

The 160b803 NSIS candidate, SHA-256
`48f6d318a7a1bc702406c84ddea9645aedaaeca2d03c4ad7e61b6f4f577bba2e`,
passes actual standard-user sign-in after a real guest reboot with its scheduled
daemon installed. The new boot is `2026-10-14T23:53:30.5000000+02:00`; the index
task runs automatically at `23:53:42`, before any verification CLI query.
The task is running with one instance and a login trigger. LastTaskResult
267009 (`0x41301`) means running, not an error. The verification phase contains
no demand-start or restart, so it does not manufacture login-trigger evidence.

Read-only query returns the saved fixture; status reports ready with one watched
root. Preferences and source hashes remain unchanged through reboot and removal.
Subsequent ordinary uninstall bootstrap exits zero, and final-state checks find
no app directory, task or generated index. Receipt:
`.hermes/evidence/windows-20261011/WINDOWS_NSIS_CURRENT_LOGIN_ACCEPTANCE.json`.

Owned actor tasks/processes and guest installer/scripts/results are removed;
overlay is shut off with backing content mtime unchanged. The owned account/
source/preferences and current candidates remain needed for other native gates.
Published downloads are unchanged. This closes focused current-package login/
saved-index acceptance, not graphics, full GUI, heavy-index CPU or MSI
failed-transaction registration rollback.

### Current Windows public Move — regular-file integrity and session cleanup fix

The clean 160b803 portable candidate (SHA-256
`07e54ad17514b4593ff06cd46550a638f48d954fde00085178514e298312ebe1`)
passes conflict-free regular-file Move through its native standard-user/Mesa GUI:
select source, Ctrl+X, Ctrl+L to a separate NTFS virtual disk, then Ctrl+V.
Source/destination volume identities differ; the source is removed and the
published destination SHA-256 matches. A settled capture displays the destination
file, and normal GUI closure returns zero. The early two-second capture is
retained separately; this does not establish an acceptable responsiveness bound.

The same workflow exposes a portable cleanup defect: no Task Scheduler
registration caused removal to return before stopping the live session daemon.
Subsequent index deletion failed on its writer lock, with the daemon/data left
behind. Source removal now prepares the authenticated session daemon when task
inspection confirms absence; inspection errors still precede runtime changes.
Linux no-service removal uses its existing daemon preparation path as well.
Native revised Windows CLI cleanup returns zero, stops the captured old package's
session daemon and removes its database. Sixteen focused search tests, strict
Windows app/search and Linux search all-target Clippy, release Windows app build,
formatting and diff checks pass. This fix still needs coherent packaging.

Receipt: `.hermes/evidence/windows-20261011/WINDOWS_PUBLIC_MOVE_SESSION_CLEANUP_ACCEPTANCE.json`.
First-run privacy setup obscured the initial GUI; optional choices were disabled
in the owned account before input. A process-handle fixture correction verifies
normal exit independently. The original portable Finish receipt remains failed
because its cleanup failed; fixed-source cleanup is a separate passing receipt.
Owned actors/processes, private profile/payload/archive and the detached virtual
disk are removed. Overlay is shut off; backing content mtime unchanged. Public
downloads are unchanged. Native Linux session shutdown, overwrite/directory Move,
recovery, responsiveness, GUI breadth and MSI rollback remain open. Windows raw
verbatim prefixes visible in source breadcrumbs/details need display formatting
review before claiming interface polish.

### Event-driven directory and file-operation completion

The Windows public Move observation exposed a missing UI notification path:
ordinary file-operation, Undo and directory-listing workers queued results without
requesting a repaint. Their visible completion depended on unrelated scheduled
refreshes. These workers now share a result publisher that requests an immediate
repaint only after successful result delivery. Both GUI constructors bind the
context; headless callers retain their existing behavior. No additional idle
polling timer is introduced, and dropped result receivers do not trigger paints.

The actual listing worker regression waits for a repaint without input or UI
polling, then confirms the result is already queued. The real Create File/Undo
regression observes immediate notifications and verifies completed Activity and
file removal. Its disposable fixture now tears down on assertion failure too.
All 364 regular UI tests pass (one ignored). Linux and Windows compiler checks
are recorded with the implementation commit. This is source-level notification
acceptance: packaged native response timing and refreshed packages remain open;
existing package receipts retain their original artifact scope.

### Packaged Windows completion notifications and session cleanup

The stable Windows candidate stage is refreshed from clean tracked revision
`43cfa62459601489dca3393af196012aef536793`. Portable SHA-256 is
`d0ea7bbd3853afb6cffb1fe6bd95767848c9813d97790decbf99e55e838a8683`;
NSIS SHA-256 is
`0599eea892a8be8022d3e1d77d7100f7911b15d0a40edbbbc797feef89caf810`.
All 51 portable payload files exactly match the stage; the NSIS embedded payload
contains all 51 corresponding digests. The superseded candidate MSI and its
checksum are removed to avoid mixing revisions in the current candidate folder.
Public downloads remain unchanged; MSI refresh and the new NSIS package's native
installer lifecycle remain open.

The exact portable passes native standard-user/Mesa cross-volume regular-file
Move through Ctrl+X, path navigation and Ctrl+V. Without further input, the first
capture at 647 ms displays the destination file; the source is already removed.
A 2719 ms capture agrees. Destination bytes match the source hash. This is one
observed capture, not a guaranteed latency bound or a comparative benchmark.
Normal GUI closure returns zero, and the same package's portable cleanup succeeds
without leaving its app/index processes or private index files. The QA account's
separate saved preferences and source fixture retain their original hashes.

Receipt: `.hermes/evidence/windows-20261011/WINDOWS_COMPLETION_CANDIDATE_ACCEPTANCE.json`.
Screenshots and phase results are exported before cleanup. Owned actors, private
profile, payload/archive, scripts and detached virtual disk are removed; overlay
is shut off and backing content mtime is unchanged. The first scheduled actor returned one without a launch receipt; retrying with
process-scoped Bypass for the QA script succeeds. No machine/user execution policy
is changed, and the original failure is not attributed to the app.
Overwrite/directory/recovery Move, Linux packaged timing and Windows display-path
polish remain separate gates.

### Refreshed Windows NSIS native lifecycle and coherent MSI candidate

The exact clean-43cfa62 NSIS candidate, SHA-256
`0599eea892a8be8022d3e1d77d7100f7911b15d0a40edbbbc797feef89caf810`,
passes native standard-user silent install/reinstall with all 51 payload hashes,
Ready indexing, an active watch and one fixture result. A locked cache file makes
uninstall return one while preserving complete payload/Installed Apps registration,
the same running scheduled daemon instance and byte-identical database. Releasing
the lock and retrying ordinary uninstall removes app, task, index/cache entries,
Installed Apps and Start menu. Source and preferences retain their prior hashes.
The shared QA actor now requires an explicit expected installer hash; a stale
hardcoded artifact cannot silently satisfy the test.

Receipt: `.hermes/evidence/windows-20261011/WINDOWS_NSIS_COMPLETION_LIFECYCLE_ACCEPTANCE.json`.
The stable candidate MSI is also rebuilt coherently from clean source 43cfa62 with
native WiX 5.0.2 in the owned overlay. SHA-256:
`cca6eca82112f06501b26bdeeab6861fcbb939a57fc6532ffcd656d87ce4a79d`.
Its embedded digest multiset contains all 53 expected payload/helper files. This
is packaging acceptance, not installed-layout or MSI transaction acceptance.
The current candidate receipt aggregates all three artifact hashes:
`.hermes/evidence/windows-20261011/WINDOWS_COMPLETION_CANDIDATE_ACCEPTANCE.json`.

Phase and cleanup receipts are exported and read-back validated before removal.
Owned actors, installer/test staging, guest compiler runtime/archives and the
clean temporary packaging checkout are removed. Overlay is shut off; backing
content mtime unchanged. Useful retained host compiler cache is reused. Public
downloads remain unchanged. Current MSI native lifecycle and product-registration
rollback, exact NSIS login/reboot/installer GUI, Linux package refresh and broader
platform daily-driver gates remain open.

### Historical Linux acceptance scopes

- **Linux:** the core browser, file operations, search and Inspector are implemented. Recent delivered changes cover saved-view preservation, indexer performance/service management, theme editing, target alignment, Settings/editor coherence and shared menu/choice-row alignment, repeated/cold AppImage startup, and native window desktop identity. The current AppImage is built from clean tracked revision `ad364d4`, SHA-256 `e3659a70f17f0352bba5fb8bfaeadf2f6b57dd5edec1b4befed7b983104e1a5a`. Exact-package CLI cleanup failure/retry and source/preference preservation pass; receipt `dist/packages/CACHE_CLEANUP_ACCEPTANCE.json`. DEB/RPM/portable are refreshed and payload identities pass. The following GUI and idle-daemon results remain scoped to preceding `b2d2702` (SHA-256 `e31d09cb36c50aceea716b85d594f3b239852235cee3da1876e8df1ab5fc0d22`); the already running user daemon remains that version until explicitly upgraded. Exact-package Dark 100% / Light 150% three-item restored batch Undo, completed restart, startup destination reveal, visibility after opening Activity and both menu visuals pass. Full destination captions stay visible beside the panel, and complete menu icons retain compact spacing. Native/portable payload identities pass. Receipt: `dist/packages/COPY_SCOPED_VIEWPORT_ACCEPTANCE.json`. The packaged empty-root index daemon records 60 wakeups over six seconds, no measured CPU ticks, 0.308 ms maximum ping latency and clean shutdown. The installed service has been gracefully upgraded to that exact daemon while preserving the main app, configuration, service definition and database inode; The running home index is now Ready with 158,825 entries and an active watch; a 12-second sample with the existing main GUI measured 0.5% of one CPU core while its snapshot advanced. Sustained quiet-idle and heavy-change acceptance remain separate. The preceding accepted batch artifact remains `9622e61`, SHA-256 `b63665e0b6a696476966f78c05398e164abc6bdc4624d7ba889e9446a46daf5f`.  Exact-package Dark 100% / Light 150% restoration and one-click reverse Undo of three overlapping items in one batch pass. Original sources and paired journals are preserved; normal restart shows three completed inverses without an Undo action. Native/portable payload identities pass. Receipt: `dist/packages/COPY_BATCH_PLAN_ACCEPTANCE.json`. These plan-admission and initial Miller target-column follow-ups were subsequently repaired in `a242cdc`; the bounded-read regression and later destination-reveal receipts retain their own exact source/artifact scopes. The following two-operation dependency/menu checks retain their preceding `2003969` artifact scope (`4220b6c435db36986fba6d1d72bc0b8601328e0b17b2689b516b51cb660e3a7a`). Exact-package Dark 100% / Light 150% fresh-process restoration of two source-produced overlapping Copy operations, automatic predecessor admission after newer Undo, both inverses, source preservation, visible empty caption and normal exits pass. Native/portable payload identities pass. Receipt: `dist/packages/COPY_DEPENDENCY_ACCEPTANCE.json`. That AppImage main/View Options menus also pass visual inspection in Dark 100% / Light 150%, including complete semantic icons and compact action-menu leading spacing; receipt `dist/packages/CURRENT_MENU_ACCEPTANCE.json`. Within-batch inverse admission is now covered by the latest package scope above. The following text-edit and batch/caption acceptance remains tied to the preceding `c27403f` artifact (`62e0747da6a40e86917ece64fc29a0b49964e0782465fd0e6edebea4d6c396f1`). Exact-package Dark 100% / Light 150% native Inspector text save, Activity Undo, original-byte restoration and correct completed text-edit labels pass; unrelated files are preserved and both runs exit normally. Receipt: `dist/packages/ACTIVITY_OPERATION_KIND_ACCEPTANCE.json`. Exact-package Dark 100% / Light 150% Copy restart/Undo and empty-caption layout checks pass: both durable inverses complete, the destination shows zero items, sources remain unchanged, and the full caption stays visible beside Activity. Native/portable payload identities pass. Receipt: `dist/packages/EMPTY_CAPTION_ACCEPTANCE.json`. The expanded Smart Panel overlays the file view; empty-state content now uses the uncovered column area. All 356 regular UI tests and Linux/Windows-target strict UI Clippy pass. The disposable Cargo runner passes success, failure, cancellation and native systemd renderer cleanup contracts while preserving unrelated units and operator state; native Windows runner acceptance remains separate. The following full recovery acceptance remains tied to the preceding `123e703` artifact (`a0b0bb28976c8812a9e3bf499ead0f7bc8912d16308fea8cbad7d97659f4efa2`). Exact-package Dark 100% / Light 150% Copy restart, restored batch Undo, visible listing refresh, completed history after filename reuse, and edited-copy refusal pass. Refreshed native/portable payload identities pass. Receipt: `dist/packages/COPY_RESTART_ACTIVITY_ACCEPTANCE.json`. The caption repair is also visible in the latest two-operation dependency checks above. The earlier `76f441f` receipts retain their historical scope. Rapid Shift+Delete now uses the press event’s modifiers: exact-package permanent/Trash confirmations, cancellation and file destinations pass Dark 100% / Light 150%. Refreshed portable/DEB/RPM identities pass; receipt `dist/packages/DELETE_SHORTCUT_ACCEPTANCE.json`. The following Activity receipt remains tied to its preceding `89fa193` hash. Activity now preserves running operations and Undo owners during history trimming and “Clear finished”; exact-package Dark 100% / Light 150% clearing and Copy Undo pass. The refreshed portable, DEB and RPM payload identities pass, with native installation and Windows refresh remaining separate. Receipt: `dist/packages/ACTIVITY_RETENTION_ACCEPTANCE.json`. Inspector and menu receipts below remain tied to their historical hashes. Inspector navigation now reads Previous / Page N of N / Next; exact-package two-page forward/backward behavior and minimum-size Dark/Light acceptance are in `dist/packages/INSPECTOR_NAVIGATION_ACCEPTANCE.json`. The preceding `b8d90a3` menu visual receipt remains historical. Main action menus now omit the unused check column, choice menus use a narrower check column, and view/sort options have complete semantic icons. Seven rendered widget regressions, all 345 regular UI tests and strict Clippy pass; exact-package main/view menu inspection passes Dark 100% and Light 150% with normal exits. Receipt: `dist/packages/MENU_REFINEMENT_ACCEPTANCE.json`. It includes the shared alignment repairs; their focused Dark/Light acceptance remains bound to historical revision `903c553` in `dist/packages/MENU_ALIGNMENT_ACCEPTANCE.json`. It preserves a visible adjacent Miller preview when the window shrinks, without overriding deliberate horizontal scrolling. Focused Dark/Light native Linux resize acceptance is recorded in `dist/packages/MILLER_RESIZE_ACCEPTANCE.json`; native Windows text-preview resize also passes at 100%. The preceding `6e8cef0` package’s shared button wrapping, preview toolbar layout and exact-package local file operations are recorded in `dist/packages/PREVIEW_TOOLBAR_ACCEPTANCE.json`. The earlier Copy/create directory-guard and private mount proof remains in `dist/packages/COPY_DIRECTORY_OWNERSHIP_ACCEPTANCE.json`, bound to that receipt’s historical checksum. The preceding `771eae8` AppImage had SHA-256 `312eb7a0907e3895aee45ef5c087be315c5a6d94f6f70d44f3a3cff7b237a7c7`. Its rapid path-entry/Select All and local Copy/Move/Cancel/Overwrite/Undo acceptance is in `dist/packages/LOCAL_PATH_ADMISSION_ACCEPTANCE.json`; batch Copy Undo and receipt ownership are now accepted at this scope; safe transaction expiry/reclamation remains open. The preceding `73b913d` AppImage had SHA-256 `9c0c353435ea5fb640616c6ac3caf1c3d44ee7dd42c675f4e792d85de0483efb`. Historical exact-package Xvfb checks on that `73b913d` artifact at 1100 × 690 and 800 × 600 verify startup file selection reveals its full adjacent preview, ArrowUp preserves column positions, and both launches exit cleanly; receipt `dist/packages/MILLER_STARTUP_ACCEPTANCE.json`. `dist/packages/` also contains refreshed portable, DEB and RPM candidates from clean package revision `6e8cef08a2fc8f812bc515b875205230440b7166`, reusing the accepted `6e8cef0` release binaries; all three now contain the local-path admission, shortcut, batch Copy Undo and mount-safe directory-guard repairs. Portable PDFium placement is corrected and an exact archived-app PDF preview passes. Portable/DEB whole-file identity and RPM code/read-only-data identity after standard stripping are verified. All three extracted candidates pass startup selection/full-preview and ArrowUp checks at 1100 × 690 and 800 × 600 in private Xvfb/DBus, with clean exits and cleanup; this does not complete clean-distribution installation/upgrade acceptance. Checksums, source provenance and exact scope are recorded in `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json`. Fresh Ubuntu 24.04.5 VM DEB installation, regular-user text-preview launch, same-version reinstall/uninstall, preference preservation and real systemd user-service install/status/restart/removal now pass; all five installed executables match accepted release binaries. Receipt: `dist/packages/DEB_INSTALL_ACCEPTANCE.json`. Full desktop integration, cross-version upgrade and fresh RPM installation remain open. Earlier receipt results apply to their recorded artifacts, not automatically to the newest package. The desktop-identity receipt verifies private KWin Wayland protocol, not a visual host-taskbar acceptance.


### Current Linux completion package and session cleanup

All four stable Linux deliverables are refreshed from clean tracked revision
`af78252bdab900599d40ba8c8d74db29dbda48bc`. The current AppImage SHA-256 is
`dc9496042ce54d4df373de2ce8a25ab121fadc7491a79272116d9056892b25d9`.
The exact package preserves its live private session daemon and database bytes
when cache cleanup fails; retry returns zero, stops that daemon without any login
service registration, removes generated index/cache files and preserves fixture
source/preferences. The private daemon exits normally and its socket is removed.

Packaged public regular-file Move in private Xvfb/DBus passes Dark 100% and Light
150%: the destination is visible at first captures of 647/730 ms, source removed
and bytes preserved. Settled captures at 2762/2894 ms remain correct; the earlier
three-second Cut action message has expired. Both main/View Options menus retain
semantic icons and compact action spacing, and both GUI runs close normally.
These are observed cases, not a latency guarantee or full appearance acceptance.

Portable and DEB five executables/PDFium exactly match AppImage bytes; RPM code
and read-only sections match after standard stripping. Production native package
verification covers embedded clean provenance, GLIBC and polkit metadata;
portable layout and provenance also pass. No fresh native installation/upgrade
acceptance is inferred. Receipts: `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json`,
`CACHE_CLEANUP_ACCEPTANCE.json`, `CURRENT_MENU_ACCEPTANCE.json`, with source
receipts in `.hermes/evidence/linux-completion-current/`.

The readiness fixture initially compared lowercase CLI status with the wire
protocol's `Ready`; the corrected protocol check passes. Initial unsequenced
navigation did not reach the destination and the self-conflict dialog preserved
source. Sequenced navigation with explicit Select All passes. Both failed runs
tear down their private profiles/processes. The workspace connector is unavailable;
existing private Xvfb/DBus runs provide the scoped GUI evidence instead.

All owned displays, buses, portal/wallet helpers, daemon/app processes, temporary
profiles/extractions and packaging checkout/target symlink are removed. Candidates
are moved into stable deliverable paths. Host configuration/service hashes and
running daemon start identity/executable remain unchanged. The installed running
daemon intentionally remains the preceding binary until a deliberate upgrade;
no new live-home CPU/performance claim is made. Wider platform gates remain open.

### Current Linux populated-index idle, burst and persisted restart

Exact af78252 AppImage acceptance now covers 10,000 metadata-only files and 100
directories. Both 30-second quiet samples measure 0.067% of one CPU core, before
and after 500 each rename/delete/modify/create events. Canonical paths/sizes,
query totals and returned result paths/sizes converge in 2,975.613 ms. The active
generation and scan-start marker stay unchanged, proving this burst did not
trigger the earlier full-root fallback. A normal daemon restart restores the
updated index and active watcher in 255.323 ms without replacing its generation
or starting a full scan. Both instances exit normally; profile/socket and all
private processes are removed. The existing live daemon remains untouched.

Runnable gate: `scripts/index-populated-qa.py`. Receipt and runner digest:
`.hermes/evidence/index-populated-current/ACCEPTANCE.json`. Fixture assertions
reject an incomplete app config's empty default root, use the actual nested wire
query response, and distinguish non-persisted scan-start timestamps from a new
scan. The accepted final run includes all correctness and resource assertions.
This closes the controlled current-package populated quiet/burst/restart case;
longer live-home, enabled-content, larger/adversarial, Windows and broad platform
acceptance remain open. See `docs/SEARCH_PERFORMANCE.md` for exact reproduction
and measurement scope.

The new populated-index runner's cancellation contract also passes: SIGTERM at a
live quiet-phase checkpoint exits 143 with no private process/profile and no
passing receipt. Evidence: `index-populated-current/CANCELLATION_ACCEPTANCE.json`.

### Offline index freshness defect — baseline and source repair

Extending the current packaged 10,000-file gate exposes a real restart defect:
with periodic refresh disabled, create/edit/rename/delete while stopped are not
reconciled. The af78252 AppImage returns Ready and establishes watches while its
canonical rows retain two obsolete paths, omit two new paths and retain an old
size. The 15-second freshness deadline rejects this state. Baseline:
`.hermes/evidence/index-populated-current/OFFLINE_BASELINE_FAILURE.json`.
Earlier unchanged-corpus restart timing remains valid for that artifact, but it
does not establish offline freshness.

Source now keeps the saved snapshot queryable during watcher setup and starts a
single background reconciliation after watches are installed. Normal notification
bursts keep their existing incremental path. The real-filesystem regression covers
all four offline changes with refresh disabled; controlled startup regression
proves cached availability before reconciliation and fresh publication afterward.
Steady-state controlled fixtures explicitly complete initial validation before
observing later events; commit/failure/cancellation assertions remain intact.
The shared service-test filename helper now uses portable numeric timestamps.

All 220 regular search tests pass (two explicit storage measurements ignored),
and strict Linux/Windows all-target search Clippy passes. The populated runner
now supports offline mutations and requires startup validation completion; the
previous no-scan restart criterion cannot prove the intended freshness contract.
Refreshed package acceptance and startup/resource measurements still need to be
completed before claiming this source repair is delivered. The personal running
index remains untouched; all failed baseline/test profiles are torn down.


### Delivered Linux offline-freshness repair

Stable Linux AppImage, portable, DEB and RPM are refreshed from clean source
554591d. AppImage SHA-256:
`3758bae8aabda470eb224b7b2889cff6018b480ccbcb2f2fe8dd893e9db5ed55`.
The exact metadata-only 10,000-file package now catches create/edit/rename/delete
while stopped within 508.305 ms of restart, including correct canonical paths,
sizes and returned query results, with periodic refresh disabled. Startup performs
one background validation and replaces the persisted generation when complete;
cached availability is preserved. The earlier no-scan warm-start result belongs
to af78252 and does not establish the intended offline freshness contract.

Quiet CPU over three 30-second samples is 0.033% before the ordinary 2,000-change
burst, 0.100% afterward and 0.067% after offline restart validation. Ordinary
notification updates remain incremental and converge in 3,052.432 ms. Both
private instances exit normally; no profile/socket/process remains. Dark 100% /
Light 150% packaged Move/menu visuals and normal exits pass, as does cleanup
failure/retry/no-unit shutdown. Native/portable payload/provenance/ABI/polkit
comparisons pass. Receipt:
`.hermes/evidence/index-populated-current/OFFLINE_PACKAGE_ACCEPTANCE.json`.

All four candidates replace their stable deliverables; temporary package staging
is removed. The existing personal configuration/service hashes and running daemon
identity/executable remain unchanged. The service's apparent cached executable is
actually a 96-byte shell launcher pointing to the updated public AppImage; no
extra cached-binary mutation is required. Its next start gets the repair. Existing
running code remains the preceding version until that start. Windows source
compile/lint passes, but Windows package refresh/native offline-change, current
native install/upgrade and wider resource/platform acceptance remain open.


### Windows offline index freshness — current candidate acceptance, 2026-10-15

Clean revision `4a0c13b` candidates now contain startup reconciliation of cached
indexes. Native standard-user portable acceptance passes a private 10,000-file
metadata-only index with refresh disabled: offline create/edit/rename/delete
becomes visible after restart in 1.807 seconds, with ready watches and completed
startup validation. Quiet 15-second samples measured 0.208% and 0.312% of one
core before stop and after restart. Session stop preserves the database;
packaged cleanup stops the daemon and removes its private database. No scheduled
background task is installed for this fixture, so login/reboot persistence is
not accepted by it.

All three current candidate formats were rebuilt from that clean source:
portable payload 51 files, NSIS embedded payload 51 matching digests, MSI 53
matching payload/helper digests with multiplicity. Exact hashes and scoped
native results are in
`.hermes/evidence/windows-20261011/WINDOWS_COMPLETION_CANDIDATE_ACCEPTANCE.json`.
The preceding `43cfa62` candidate receipt is retained as
`WINDOWS_43CFA62_CANDIDATE_ACCEPTANCE.json`; its native lifecycle/GUI evidence
does not accept the replacement artifacts. Public Windows downloads remain
unchanged until current native installer acceptance. Current MSI/NSIS lifecycle,
MSI failed-transaction registration recovery, login/reboot and GUI gates remain
open. Owned guest fixtures/compiler copies and the QA task were removed; the
owned VM and both backing guests are shut off, with source/preferences and
backing content mtime preserved.


### Current Windows local packages published after installer verification — 2026-10-15

The `4a0c13b` portable/NSIS/MSI candidates now replace the older files at stable
`dist/` paths. Their SHA-256 values are recorded in the current acceptance receipt
and matching checksum/provenance sidecars. Superseded payload staging and duplicate
candidate archives were removed; the sole current extracted payload is
`dist/windows-payload/Ottrin-0.1.0-windows-x64`.

Exact NSIS standard-user acceptance passes installation, same-version reinstall,
51 installed payload hashes, running index/query, locked-cache uninstall returning
1 while retaining registration, identical running instance, task XML and database
bytes, followed by successful retry and generated-data removal. Exact MSI
standard-user acceptance passes installation, installed ProductState 5, all 53
payload/helper hashes, running per-user login task with expected trigger/action,
fixture query, successful uninstall and ProductState -1. Both preserve source and
preferences. Receipts: `WINDOWS_NSIS_OFFLINE_LIFECYCLE_ACCEPTANCE.json` and
`WINDOWS_MSI_OFFLINE_LIFECYCLE_ACCEPTANCE.json` beside the current receipt. These
supersede the earlier statements that the refreshed files were candidates only;
prior-revision acceptance remains historical. Owned actors and guest fixtures
were removed and the owned guest shut down with backing content mtime unchanged.

MSI failed-transaction registration rollback is still unresolved. Current exact
upgrade/repair, reboot/login persistence, interactive installer and broader GUI
acceptance remain open; publishing a current local test build does not close the
release-candidate gate.


### MSI rollback authoring comparison — 2026-10-15

Read-only inspection of the current MSI and the reference package attached to
[WiX issue 8574](https://github.com/wixtoolset/issues/issues/8574) does not establish
a repair. Both SummaryInformation WordCount values are 10 (compressed payload,
no elevation required). Ottrin's Security value 2 is Microsoft's recommended
read-only setting for installation databases; it is an editor flag, not proof of
registry access. ProductLanguage 0 is valid for a neutral package. The reference
sets EnableUserControl, which broadens public-property control; that is not an
authorized or demonstrated rollback fix and is not copied into Ottrin. Primary
references and compact package/property/action snapshots are retained in
`.hermes/evidence/windows-20261011/MSI_AUTHORING_COMPARISON.json` and
`MSI_SUMMARY_METADATA_COMPARISON.json`. External reference packages were inspected
as data, never executed, and temporary downloads/parser files were removed.

The retained native Ottrin failure log explicitly executes ProductUnregister and
ProductUnpublish before CleanupBeforeUninstall. Source comments and the contract
fixture now distinguish payload-removal ordering from engine-owned registration;
a passing sequencing fixture does not prove registration rollback. The source
contract passes, with no runtime/package change. A newer independent report in the
same upstream issue reproduces failed-uninstall registration restoration on
multiple Windows builds; it is corroboration, not a root-cause determination.

The rollback gate remains open. Any proposed fix must preserve the non-elevated
per-user contract and pass a real failed transaction after execution, original
product/payload/task state restoration, and subsequent repair/removal. A failure
before execution, changing registry ACLs, disabling rollback, or manually deleting
the payload cannot substitute for that acceptance. Continue current reboot/login
and GUI checks, plus other roadmap implementation, while this root cause remains
unresolved.


### Current NSIS real reboot and login-daemon startup — 2026-10-15

The current published NSIS, SHA-256
`78b6de52c91e4cecd4295154168f0723f8d9d21a5886f97dd5d2eea2dc0f558d`
from clean `4a0c13b`, passes actual standard-user sign-in after a real graceful
reboot. New boot: `2026-10-15T02:43:37.5000000+02:00`; scheduled daemon launch:
`02:43:48`. Before any verification CLI call, the per-user login task had one
running instance and no main Ottrin GUI was running. Its indexd SHA-256 matches
the current published payload. A subsequent query returns the saved fixture and
status reports ready watches and completed startup reconciliation. Task result
267009 means running, not failure.

The first reboot had no interactive sign-in because the prior automatic sign-in
had been consumed. That did not exercise the login trigger and was not accepted.
An additional guest-only generated QA credential and one-shot sign-in allowed
the second graceful reboot to test the real trigger. No credential was exported.
Uninstall then removed the app, generated index and task, with exact preference
and source-file hashes preserved. The actor exited zero; owned task/files and
one-shot sign-in/plaintext credential were removed. Receipt and all three phases:
`.hermes/evidence/windows-20261011/WINDOWS_CURRENT_NSIS_REBOOT_ACCEPTANCE.json`.
The owned guest was shut down with backing content mtime unchanged.

This closes the current NSIS installed-daemon reboot/login check, not MSI reboot,
main-window close lifetime, GUI search or whole-home performance. Those checks,
MSI registration rollback and the broader roadmap remain open.


### Installed Windows GUI index lifetime and OS-theme defect — 2026-10-15

The current `4a0c13b` NSIS package passes a standard interactive-user GUI lifetime
check through the installed path. Before opening, while open, after normal GUI
exit and after a subsequent query, the scheduled index daemon retains PID 6392
and the same task InstanceGuid. Ctrl+F search visibly shows one result row and a
separate preview card; the CLI independently confirms one fixture. Closing the
window exits zero. A file created afterward is found by that same daemon in
1,179.069 ms, demonstrating continued watching rather than client-triggered
replacement. Installed GUI and daemon hashes match the published package. Mesa
modules are recorded; native hardware graphics are not accepted. QA preferences
change through normal GUI persistence and are then restored byte-for-byte by the
actor. The source fixture is unchanged. Uninstall removes the app/index/task;
owned actor, guest fixtures, one-shot sign-in and plaintext credential are removed.
The owned VM is shut off with backing content mtime unchanged. Receipt:
`.hermes/evidence/windows-20261011/WINDOWS_INSTALLED_GUI_INDEX_LIFETIME_ACCEPTANCE.json`.

Visual inspection also found default white input/sort controls inside the dark
palette. This is a real failed theme-coherence gate, not accepted by the lifetime
result. A rendered-frame regression reproduces the white `#FFFFFFFF` input
instead of Ottrin's `#313842FF` when an OS Light event arrives after palette setup.
`Context::set_visuals` writes only the active egui style, while egui's default
System preference can later select the untouched style. Ottrin now selects the
style matching its resolved palette before applying colors and spacing. Its own
System-mode resolution still observes raw OS appearance, as the regression checks.

The regression passes for both palettes across OS Light/Dark transitions, including
the real input background shape and control surfaces. All 365 regular UI tests
pass (one ignored), along with strict Linux and Windows UI all-target Clippy,
formatting and diff checks. Source receipt: `THEME_OS_TRANSITION_SOURCE_ACCEPTANCE.json`.
Rebuild and inspect the corrected native packages before claiming this delivered;
the published `4a0c13b` artifacts still contain the visual defect. This checkpoint
closes focused installed-GUI daemon reuse/lifetime, not appearance coherence,
MSI reboot/rollback or the remaining roadmap.


### Palette correction delivered in all current local formats — 2026-10-15

Clean source `d4ef8f9` replaces the preceding Linux and Windows artifacts at their
stable paths. All seven published files were re-hashed after replacement. The
AppImage is `31ede0892909a2bd28d87afd5bd54bcc85de88fcd83c215c4ab5a72e0f98154a`;
all format hashes and sizes are retained in
`.hermes/evidence/windows-20261011/THEME_FIXED_PACKAGE_IDENTITIES.json`.

Native standard-user Windows NSIS GUI captures now show the dark search input
and sort control matching the resolved palette. One visible search result agrees
with the CLI. The same scheduled daemon PID/InstanceGuid survives normal window
open/close and indexes a subsequent file in 615.326 ms. Preferences change through
normal GUI persistence and are restored by the actor; original fixture bytes stay
unchanged. Exact MSI install/index/query/uninstall passes with all 53 payload/helper
hashes. The Windows portable has 51 exact stage matches, NSIS has 51 matching
embedded digests and MSI 53 including helpers. Receipts are
`WINDOWS_THEME_FIXED_GUI_ACCEPTANCE.json` and `WINDOWS_THEME_FIXED_MSI_ACCEPTANCE.json`.
The GUI and MSI harnesses use their `d4ef8f9` templates with only recorded digest/
owned-directory substitutions, identified by executed-script hashes.

Exact Linux AppImage main/View Options captures were inspected in Dark 100% and
Light150. Both Move fixtures completed with exact destination bytes, visible
completion and normal exit. Private Xvfb/message-bus/app/index/profile fixtures
were removed. Portable executable/PDFium bytes match AppImage; the production
native verifier block passes DEB bytes, RPM code/readonly sections, embedded
revision, GLIBC-derived dependencies and polkit bytes. A harness assumption that
the portable launch script was the ELF was corrected to use its authoritative
`libexec/ottrin` layout before acceptance; no package defect was inferred. Linux
receipt: `.hermes/evidence/theme-os-transition-linux/ACCEPTANCE.json`.

All disposable Windows actors, installed app/index/tasks, compiler/payload copies,
one-shot sign-in and plaintext credential were removed. Guest/backing VMs are
shut off with backing content mtime unchanged. Host preferences and index-service
unit retain original hashes. Superseded packages, duplicate staging and task-owned
temporary scripts/extractions were removed; shared caches and current deliverables
remain. Cleanup: `THEME_FIXED_PUBLICATION_CLEANUP.json` beside the Windows receipts.

This closes the delivered OS-appearance/control-palette defect and focused current
GUI lifetime checks. Physical GPU/Wayland, full theme/state/scale acceptance,
current exact reboot/upgrade/repair, MSI registration rollback and the remaining
roadmap are not claimed complete. No new whole-home or CPU benchmark is inferred
from this UI repair; prior performance receipts retain their original hashes.


### Readable Windows path labels and rooted breadcrumbs — source checkpoint

The delivered Windows screenshots exposed internal verbatim prefixes in the
breadcrumb, search Folder column and file Location. Breadcrumb rendering also
split a drive prefix from its root separator. The shared UI `path_display` module
now formats ordinary drive/UNC labels and constructs breadcrumbs from original
`Path::ancestors`. Drive/share roots are one storage breadcrumb; each click target
remains an original `PathBuf`, never a parsed display string. Volume/device
namespaces retain their spelling. Unicode and unusual filename endings are not
trimmed or normalized by display formatting.

Read-only navigation/tooltips, search, Info/Inspector details, Drop Folder labels,
Activity descriptions, diagnostics and folder-read errors now use that helper.
Drop Folder labels derive parent/name from the original path before formatting.
Editable addresses/configuration, clipboard paths, cache keys and command completion
retain original path data. This distinction is now part of the UI development
requirements.

The review also found that address-field Enter trimmed real directory-name
whitespace. A private real-filesystem GUI regression created both `literal target`
and `literal target ` and reproduced navigation to the wrong decoy. Submission now
preserves the entered string, and the regression opens the intended directory.
That real-filesystem case runs on Linux; no Windows filesystem/GUI acceptance is
inferred from it.

Native Windows tests compile and execute the actual std-only UI helper module:
all four pass, covering rooted drive and UNC labels/targets, Unicode, trailing
characters, relative paths and unchanged volume/device namespaces. This is pure
path-format/identity testing under QGA SYSTEM, not file-operation or main-window
acceptance. All 368 regular UI tests pass (one ignored), plus strict Linux and
Windows UI all-target Clippy, formatting and diff checks. Receipts:
`.hermes/evidence/windows-20261011/PATH_DISPLAY_SOURCE_ACCEPTANCE.json` and
`WINDOWS_PATH_DISPLAY_NATIVE_TESTS.json` beside it. Native test fixture/process
are removed; guest/backing VMs are shut off with backing content mtime unchanged.
The failed-address test's private profile was also removed on panic.

Rebuild current Linux/Windows packages and inspect the native breadcrumb, search
and Info labels, including drive-root navigation, before calling this delivered.
Current stable downloads remain the accepted `d4ef8f9` palette repair. The path
source changes do not close any remaining installer, Office or daily-driver gate.


### Packaged breadcrumb click rejection and source repair — 2026-10-15

The first `7907130` packaged Windows GUI attempt renders readable labels but
fails real drive navigation: clicking the `C:\` breadcrumb opens the address
editor at the unchanged fixture directory. The exact rejected NSIS digest is
`7e37ed7478e92680434e6f2d4e27a5e6dc22192e5f1c9171caf52c0583495f5a`.
`PATH_GUI_ROOT_RED.json` and `path-gui-root-red-click.png` in the Windows evidence
directory record the failure. Its app was uninstalled and QA preferences restored;
this candidate was not published or counted as acceptance.

The address frame registered a click region over all breadcrumb buttons after
rendering them. Its hover also overrode their pointing-hand cursor. The edit
interaction now covers only unused space after the last breadcrumb. A rendered
pointer regression independently reproduces the missed navigation, then verifies
parent navigation without editing and blank-space editing with the original path.
Automatic root-tab captions use the shared readable root formatter; explicit user
tab labels remain authoritative.

All 369 regular UI tests pass (one ignored), along with strict Linux and Windows
UI all-target Clippy and formatting/diff checks. Source receipt:
`.hermes/evidence/windows-20261011/BREADCRUMB_POINTER_SOURCE_ACCEPTANCE.json`.
Rebuild the source and rerun the native drive-root and literal-space navigation
checks before publishing. Current downloads remain the preceding accepted palette
build. The rejected candidate's pure label/target tests did not prove clickability;
this real packaged GUI gate is required.


### Address destination visibility — 2026-10-15

The `82d75c1` native packaged drive-click check correctly changed the path, but visual review rejected the resulting file view: the old horizontal offset left almost all of the root column off-screen. This candidate was not published. `.hermes/evidence/windows-20261011/PATH_REVEAL_REJECTED.json` and `path-reveal-red-root.png` record that boundary.

Breadcrumb and address-entry navigation now use the existing one-shot Miller destination reveal. Explicit reveals bring a destination left of the viewport into view as well as revealing destinations to its right. A rendered regression starts deep enough to scroll, clicks the actual ancestor breadcrumb, and checks that the resulting current file column becomes fully visible; it fails before the repair and passes afterward. Ordinary same-directory refresh and deliberate horizontal scrolling keep their existing behavior. All 370 regular UI tests pass (one ignored), with strict Linux/Windows UI all-target Clippy. Packages require a fresh native visual check before publication.


### Address and menu package delivery — 2026-10-15

All seven stable Linux/Windows packages are refreshed from clean `41027a0`. The rejected `7907130` pointer interception and `82d75c1` hidden root-column candidates were replaced without publishing them. Exact installed Windows GUI root/literal-directory/search and continuing-daemon checks pass; the after-close file became searchable in about 632 ms without replacing the daemon. The exact MSI passes ordinary standard-user install, 51 installed-file identities, index registration/query, uninstall and preservation. The first MSI harness incorrectly expected its two embedded transaction scripts to be installed files; the corrected manifest verifies 51 installed files while independent cabinet inspection checks all 53 embedded payload/helper digests.

Exact AppImage main/View menus were inspected in Dark 100% and Light 150%: complete semantic icons, compact action-menu leading padding, aligned labels/checks and matching palettes. Both real Move operations preserve bytes and exit normally. Native Linux package verification and portable/Windows payload identities pass. Source has 370 passing regular UI tests (one ignored) and strict Linux/Windows UI all-target Clippy.

Evidence: `.hermes/evidence/windows-20261011/PATH_FIXED_PACKAGE_IDENTITIES.json`, `PATH_FIXED_VISUAL_REVIEW.json`, `WINDOWS_PATH_FIXED_GUI_ACCEPTANCE.json`, `WINDOWS_PATH_FIXED_MSI_ACCEPTANCE.json`, and `.hermes/evidence/path-display-linux/gui/`. Private Linux profiles/processes and current Windows test tasks, installations, generated databases, compiler/extracted payloads, and autologon credentials were removed. QA preferences/source and operator Linux preferences/unit remain unchanged. The owned guest is shut off; both underlying guests remain shut off and the base disk content timestamp is unchanged. Earlier CPU, reboot, upgrade/rollback, physical GPU and wider appearance acceptance are not inherited by the refreshed artifacts.


### Fresh Fedora RPM lifecycle — direct removal rejected, 2026-10-15

The exact current RPM (`be7a9b9173e350be59f4e79f577e0e8ff8796596a572fadf8a5235f849843a38`, clean `41027a0`) installs through DNF on a fresh signed Fedora 44 Cloud image with dependency resolution. RPM verification and all five installed executable/PDFium loaders pass before GUI tools are added. An ordinary user launches the actual installed GUI in private Xvfb/software OpenGL, sees the selected text preview, and closes normally. The same configured daemon continues through GUI lifetime and detects a subsequent file in 477 ms. Native user-service install/query/restart/removal and same-version RPM reinstall pass; payload checks, preferences and source bytes remain correct. Uninstall after explicit user service removal also passes.

Direct `dnf remove ottrin` with the service still active **fails the required lifecycle outcome**: DNF exits successfully and removes the executable, but `ottrin-indexd` remains running from `/usr/bin/ottrin-indexd (deleted)` and its per-user unit remains active and enabled. The native builders lack package removal hooks; the app-generated user unit lives outside package ownership. This is not accepted as successful native uninstall. The earlier service-first removal check did not cover it.

Next work is shared native package lifecycle cleanup for DEB/RPM: stop/remove only integration owned by the native executable, preserve portable/AppImage integration and all preferences/source files, handle active/offline and multiple users, preserve intended service behavior on reinstall/upgrade, and refuse destructive removal when required cleanup fails. Native package-manager acceptance must exercise those paths directly. No new fixed package is claimed by this checkpoint.

Evidence: `.hermes/evidence/fedora-rpm-current/ACCEPTANCE.json`, `DIRECT_REMOVE_REJECTED.json`, `native/result.json`, actual installed-GUI screenshot, and DNF logs. The test daemon/unit were explicitly stopped and removed after capturing rejection. The disposable Fedora guest/domain, disks, seed and SSH key were removed; the signed base image is retained once in the shared tools cache for the repair's rerun. Existing networks, Windows guests and operator state were preserved. This remains Cloud/private-display scope; clean GNOME/KDE, physical graphics and broad permission/media acceptance remain open.


### Native package cleanup — source and draft RPM checkpoint, 2026-10-15

DEB and RPM now share one native lifecycle helper. Removal preflights standard registrations, delegates user-file operations to each owning UID, checks both generated and effective executable ownership, disables/removes native service integration, and preserves independent portable/AppImage integration and all search/config/source data. Same-version reinstall refreshes running native services without enabling stopped/offline registrations. The protected Administrator service uses the same ownership check. An ambiguous offline override refuses removal before active daemons are stopped. Offline canonical enablement is unlinked only after proving its target; a live user bus is not assumed.

The first native draft exposed the offline `systemctl --no-reload` bus assumption; its failed logs remain retained. The corrected draft passes actual Fedora multi-user same-version reinstall and direct DNF removal with native users, an offline registration, a protected Administrator service and an independent portable daemon. Preferences/source and protected SQLite integrity remain correct. A second attempt failed in fixture setup because it tried to overwrite its running portable daemon; the repeatable fixture now explicitly stops its owned service before copying test binaries.

A separate session-only test reproduced another deleted native daemon without any service registration. `ottrin-indexd --native-package-stop` now uses the current user's existing endpoint, validates peer credentials and executable ownership, retains a pidfd, sends PrepareUpgrade over the same authenticated stream, and waits for that exact process to exit. It never starts an index or signals a PID inferred from a pathname. Native and previously deleted native executables stop; an independent portable session at the same user's endpoint survives removal. The private control mode runs before profile loading. Ordinary startup still requires a valid private runtime.

All 220 regular search tests pass (two ignored), the indexd launch contract passes, and strict Linux/Windows app/search all-target Clippy, formatting and shell syntax pass. The release verifier now checks lifecycle-helper bytes/mode, dependency metadata and actual DEB/RPM maintainer-script guards. DEB control/helper inspection passes; native DPKG execution is not inferred from the Fedora result. Source receipt: `.hermes/evidence/native-lifecycle-current/SOURCE_ACCEPTANCE.json`. Draft artifacts carry modified-tree provenance and are not published. Clean artifacts, native DPKG lifecycle, offline custom configuration roots/nonstandard runtime namespaces and remaining broad lifecycle acceptance still need verification. Current stable packages remain unchanged at this checkpoint.


### Clean native lifecycle artifacts — verified and staged, 2026-10-15

Clean `2350b212e42b637bdee51477555309707bd5b2a3` builds all four Linux formats. Exact RPM SHA-256 `ef1bbff8ffeafeca790441c1c5255cade14e2b6fd34664f842e45b7d1bf35410` passes the native Fedora multi-user same-version reinstall/direct-removal matrix and the session-only native/portable matrix. A controlled unlink/restore of the installed daemon reproduces the deleted-executable case against this exact clean build; the captured native process exits through the authenticated cleanup request. Foreign service/session PIDs remain unchanged. Data preservation, ambiguity refusal and protected SQLite integrity pass. These are scoped native results, not cross-version or arbitrary-environment acceptance.

The four real packages pass payload/code identities and the updated production native verification block, including lifecycle-helper bytes/mode, maintenance-script guards and dependencies. Exact AppImage private GUI Dark 100%/Light 150% launch, menus and Move checks also pass; the inspected menus retain complete icons, compact spacing and matching palettes. Receipt: `.hermes/evidence/native-lifecycle-current/CLEAN_NATIVE_ACCEPTANCE.json`.

This was the clean staged checkpoint. Native Ubuntu execution, guide reconciliation and publication were subsequently completed below; the candidate hashes above retain their exact historical scope.


### Native Ubuntu lifecycle acceptance and guide reconciliation — 2026-10-15

Exact clean `2350b212e42b637bdee51477555309707bd5b2a3` DEB, SHA-256 `e9b1c6e7a9374b03485e8f1c787f2ad02c452ebab2dc9ebd6f83d1ecb2106504`, passes fresh Ubuntu 24.04 native APT/DPKG installation and the multi-user lifecycle matrix. Same-version reinstall restarts both running native users and the Administrator daemon, preserves the offline registration, and leaves the independent portable PID unchanged. An offline drop-in refuses removal before any daemon stops. Direct removal deletes native registrations and preserves portable integration, preferences, source files and protected SQLite integrity. A separate session matrix passes native unregistered-daemon removal, controlled deleted-executable cleanup and foreign portable-session preservation at the same user endpoint. No primary daemon remains after fixture cleanup. DPKG verification output is explicitly checked for discrepancies.

The corrected guide is now delivered in all four Linux formats built from clean `1ac0c8c57249a99c3a06f8d2133bdf9a68034f87`. They replace the stable `dist/packages/` artifacts; current AppImage SHA-256 is `af1801906c80ddc06af86f22c8f9be517aa38b48ac648f75c2a91409680207a6`, DEB `24653789d108ba0e5ed99aac8d98de8a03b6b7d0efb1d9c722516cb0bf2ad07c`. The exact final DEB repeats both managed and session matrices successfully. It also returns enabled/active and Ready with a working watch after an actual guest reboot with explicitly configured lingering; configuration bytes are unchanged. This is not a claim that services start before login without lingering.

The final four payloads pass the production native verifier block, portable six-byte identities, source provenance, lifecycle helper/hooks/dependencies and current guide bytes. Relative to native-tested `2350b21`, source changes are docs/evidence only: Fedora runtime and GUI results retain the preceding candidate’s exact artifact scope, rather than being called fresh tests of the final repackaged documentation. Receipts: `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json`, `.hermes/evidence/deb-lifecycle-current/FINAL_PACKAGE_IDENTITIES.json`, `final-managed/result.json`, `final-session/result.json`, `REBOOT_ACCEPTANCE.json` and `PUBLICATION.json`. Windows outputs remain unchanged at `41027a0`.

This closes native DPKG execution and correct-guide delivery for these hooks. Cross-version upgrade, full desktop integration, arbitrary XDG/runtime namespaces and production signing remain open. The native fixture processes are stopped and removed before the owned Ubuntu VM, disks, seed and SSH key are deleted. Existing Windows guests, host settings and operator service configuration are preserved; cleanup is recorded separately.


### Native daemon filesystem ownership audit — 2026-10-15

A real unprivileged private-mount fixture reproduced a cleanup ownership defect: an independent copied daemon also appears as `/usr/bin/ottrin-indexd` inside its own filesystem. The old internal cleanup stopped it solely because the path text matched. Cleanup now compares the installed file identity in the caller’s view with the entry reached through the connected peer’s `/proc/PEER/root`; a different installed entry is preserved. A private namespace using the actual native entry remains removable, as does a legitimate native daemon retaining its old deleted executable after replacement. The authenticated connection and captured pidfd remain authoritative for the shutdown request and exit observation.

The repeatable actual-binary namespace regression passes all three cases and preserves private source/config bytes; the prior published binary fails only the independent-namespace case. All 220 regular search tests, Linux/Windows app/search strict Clippy, and the native packaging contract pass. The packaging contract now checks dependency membership rather than assuming libc is first, and verifies the lifecycle helper is executable. Evidence: `.hermes/evidence/native-lifecycle-audit/`. This closes the reproduced filesystem-view confusion; arbitrary custom endpoints, offline custom XDG roots and broad namespace races remain separate. All four Linux packages are now published from clean `723ac7c0aa3c83ba932f3948f224da57b841d397`. AppImage SHA-256 is `b0d9d942f7149b1fb7cff105704199fd571f357c80edc4bbc8fbdf27e88a97b9`. Actual extracted AppImage and RPM daemons pass the three ownership cases. Fresh Ubuntu and Fedora guests both pass the exact current packages’ multi-user same-version reinstall, pre-removal ambiguity refusal, direct removal, session-only native/deleted-executable cleanup and portable preservation matrices. Data and protected SQLite integrity remain correct. Current hashes and exact scope are in `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json`; final package/namespace, native and cleanup receipts are under `.hermes/evidence/native-lifecycle-audit/`. Both owned guests, all six disks/seeds, credentials and duplicate package staging were removed. Host configuration and the operator service remain unchanged. Windows packages remain at their separately recorded revision; prior GUI/reboot results retain their exact historical scope.


### Address-selection regression found during desktop acceptance — 2026-10-15

The exact `723ac7c` AppImage’s private-desktop tab workflow reproduced Ctrl+L appending a replacement path to the previous address. The prior same-frame unit assertion only checked that typed text was present, so it accepted this behavior. The regression now requires exact replacement. The editor selects its full character range before rendering on initial focus and on Ctrl/Command+L in an already-open editor, using the same press-event modifier check as other application shortcuts. Ordinary subsequent typing does not reset the selection; Unicode and released-chord cases pass. All 371 regular UI tests pass (one ignored), as do strict Linux/Windows UI all-target Clippy. All four Linux formats are now delivered from clean `a764a0caa97ee975972dcbf40bb351847fcfbc42`. AppImage SHA-256 `1a78543af053e8937d819f66b170ae0a078fb6acd2633ff7176cbc0205498b60`. Exact-package Xvfb/private-DBus/software-GL acceptance passes rendered Dark 100% and actual Light150: replacement navigation without an extra Ctrl+A, repeated Ctrl+L selection, tab reorder in both directions with active identity/moved contents preserved, and select-all/Ctrl-click exclusion in Miller/List/Grid. Each view copies exactly the intended two of three files, with exact destination bytes and all six original fixtures unchanged. Main/View menus retain their icons, compact spacing and palettes; both GUI processes exit normally. The initial incomplete fixture was corrected rather than being accepted as Light: the Dark 100% run rendered System/Dark, and Light150 was selected through the real Settings UI before repeating its checks. This scope and the corrected complete private configuration are recorded explicitly.

Current native/portable payload, source-note, guide, dependency/helper/hook and namespace ownership checks pass. Native daemon/helper bytes remain identical to the preceding native-tested `723ac7c` package; distro lifecycle receipts retain that exact older artifact scope. Current artifacts/scopes: `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json`; GUI and cleanup evidence: `.hermes/evidence/address-selection-current/`. Windows release binaries and GUI PE-header checks are complete for this source, but stable Windows portable/NSIS/MSI outputs still need their refresh and native acceptance. Physical graphics, broader selection/keyboard coverage, and the full appearance/release matrix remain open. A box-shaped List header sort marker in these captures also needs focused font/icon correction before closing typography consistency; source Unicode markers are at the search and file-list header renderers. Evidence: `.hermes/evidence/address-selection-current/`.

Copy retention/reclamation remains unfinished. Current Copy observations are read-only evidence; safe deletion still needs durable cleanup intent and interruption recovery, dependency handling for retained overwrite chains, and ownership/content validation of every deleted entry. Matching journal names or age alone cannot authorize deletion. This investigation did not mutate retained user data.


### Sort-header typography and direction repair — 2026-10-15

The boxed List sort marker was traced to unsupported Unicode triangles in the production text-font stack. Search used the same triangle and always showed ascending, although Size and Modified queries sort descending. File and Search headers now share one painter using installed Material direction icons, independent layout spacing, and reserved marker width so truncated labels cannot hide the active direction. Both application constructors use one font-registration function. A rendered regression checks actual glyph availability, single-line geometry, theme ink and both alignments at 80/240-pixel column widths and normal/enlarged text. All 372 regular UI tests pass (one ignored), along with strict Linux/Windows UI Clippy. Actual packaged GUI and refreshed Windows installer delivery remain required before this source result is called delivered.


### Windows package refresh exposed inherited-handle lifetime defect — 2026-10-15

The Linux `4fb827a` packages are delivered with actual Dark 100%/Light150 menu and List sort-direction acceptance; current hashes and focused scopes are in `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json` and `.hermes/evidence/sort-header-current/DELIVERY.json`. Windows portable/NSIS/MSI were rebuilt coherently from clean `84c05de`; all 51 portable/NSIS files and 53 natively extracted MSI files match their stage/source. They remain unpublished because exact-package runtime acceptance found a new cold CLI defect: `index update --json` exits, but a redirected reader never receives EOF until the persistent daemon exits. A bounded native rerun reproduces the error without manual intervention.

Stable Rust's Windows process launcher inherits unrelated inheritable handles even when its three standard streams are null. Ottrin now creates the persistent daemon with `CreateProcessW` and inheritance disabled, preserving its environment, no-window behavior, readiness deadline and early-exit errors. A native standalone regression proves that a caller-owned inheritable file can be deleted while the daemon stays responsive; ordinary and space/Unicode executable paths pass. The fixed release CLI completes the same cold update with captured output in 1.145 seconds, reuses its singleton daemon, discovers a created file through the watcher and restores both indexed files after process restart. The eight-second quiet VM sample records 62.5 ms CPU and 8.5 MB working set; it is not sustained-idle acceptance. Rebuilt fixed packages and final publication still require their own acceptance. Evidence: `.hermes/evidence/windows-package-current/`. This is separate from the unresolved per-user MSI registration rollback defect. No global policy or registry-permission workaround was applied.


### Ordinary Windows acceptance and startup work-area repair — 2026-10-15

The exact staged `1662557` NSIS passes ordinary interactive-user installation with all 51 payload files matching, live login-daemon indexing, native Dark 100% menus, replacement address entry without extra Ctrl+A, GUI search, normal close with the same daemon PID/InstanceGuid, subsequent watcher discovery, and uninstall with source/preferences preserved. The exact current MSI independently passes fresh ordinary-user install/uninstall: all 53 files match, registration transitions 5 to -1, and its index/task/generated data/app directory are removed while source/preferences remain. These are separate scoped results; they do not make the full combined upgrade run pass.

A real published `41027a0` to `1662557` MSI upgrade with its continuous daemon running returns 3010 (restart required): InstallFiles encounters the old index executable still in use. Pre-replacement daemon quiescence, original task/login-state preservation and rollback-running-intent restoration are the next installer gate. No forced reboot or manual MSI-registry repair is used. This is separate from the known failed-transaction registration rollback defect. Windows public downloads remain unchanged, and staged `1662557` packages are not promoted.

The same native screenshots exposed a real startup placement problem: the 1024×680 main window was cascaded to (156,156), past a 1280×752 work area. Windows main startup now reads the primary work area/DPI in the authoritative initial viewport hook, fits requested/minimum size and centers a fresh window while respecting an explicit position. A source-built GUI now starts at (128,36)–(1152,716), fully inside that work area, and closes/cleans its daemon normally. All 22 Linux app tests and strict Windows app all-target Clippy pass; focused geometry covers 100%, 150% and 200% scaling, bounded and undersized requests. The native probe is 100% only. Packaging this source fix and broader secondary-window/multi-monitor/native-DPI acceptance remain open. A Unix-only root-index-start test also now has its missing Unix test gate, so Windows app test targets compile correctly.

Evidence: `.hermes/evidence/windows-desktop-current/ACCEPTANCE.json`, raw per-phase results, native MSI logs and screenshots. Fixture-only failures (first-login privacy UI, temporary NSIS-uninstaller lifetime, COM null outputs and renderer dummy-window selection) are recorded and corrected without accepting their incomplete runs. The disposable account, password, login overrides, profile, tasks, processes and guest staging are removed and the guest returned to shut off; existing campbell configuration/installation and host settings are preserved in cleanup evidence.


### Pre-replacement Windows index transaction controller — 2026-10-15

The running-daemon MSI upgrade defect is repaired in source: preparation is scheduled after InstallInitialize and before InstallFiles. The MSI embeds its current indexd as a resource controller, so it can snapshot/stop before file replacement and restore/commit after installed files are missing or changed. The obsolete installed transaction/identity script components and copied `.json.scripts` runtime directories are removed. The existing shared identity and transaction implementations are embedded directly; no second scheduler or journal implementation is introduced.

Native source-controller checks against the published `41027a0` baseline pass under an ordinary interactive account: task definition/security and login enablement survive preparation; saved index/source/preferences remain; rollback resumes enabled/disabled running tasks and leaves idle tasks stopped. Commit preserves enabled/disabled running/idle intent using the new task definition. Invalid snapshots are refused without deleting their contents, and a space/Unicode/apostrophe snapshot path works. Explicit indexd argument tests, Windows app/search all-target strict Clippy and the MSI source contract pass. Evidence: `.hermes/evidence/windows-upgrade-current/`. The final executable is bound in `COMMIT_RUNNING_INTENT.json`; earlier controller checks retain their prototype hash. These are controller checks, not yet engine-driven MSI upgrade or failed-transaction registration rollback. Rebuild coherent candidates and repeat the real upgrade before promotion. The owned published MSI remains running in the disposable account as the next upgrade fixture until its recorded cleanup.


### Native MSI upgrade and NSIS replacement gap — 2026-10-15

Exact staged `651d3e9` MSI SHA-256 `a7ba85534614219a1c666d2beb16c74a92dec534fa9e9846430d576fd5828c37` upgrades the published `41027a0` MSI with its continuous daemon running and returns 0, with no reboot requested. All 51 installed payload files match the new stage; the old product registration is removed and the new registration is installed. Task security/login intent, saved SQLite creation identity, source files and preference hashes survive. Disabled-running forced repair resumes the daemon without enabling login; enabled-idle forced repair remains idle. Uninstall returns 0 and removes the product/task/generated index/app directory while preserving source/preferences. Native receipts and correctly decoded MSI logs are under `.hermes/evidence/windows-upgrade-current/`. These results close the reproduced running-daemon replacement gate for this MSI, not failed-transaction product-registration rollback.

The separate NSIS native update test exposed the same replacement defect more severely: it returned 0 while retaining the old daemon executable bytes. Its installer now extracts the current embedded controller independently of installed files, prepares before payload replacement and commits after normal task registration. Failure/cancellation callbacks restore task metadata/runtime intent; failed restoration retains its journal outside NSIS's disposable plugin directory. Copy failures explicitly fail setup. This restores task state, not an entire partially replaced payload; setup errors instruct retry. Native acceptance and exact final package delivery are recorded in the following checkpoint when complete. Public Windows packages remain unchanged pending the remaining installer gates.


### NSIS native transaction acceptance — 2026-10-15

The NSIS prototype with the `651d3e9` payload passes published-NSIS running-daemon upgrade: all 51 payload files match, the daemon PID changes, the saved database creation identity and source/preferences remain, and task login/security stay intact. Reinstall covers enabled-running, disabled-running, enabled-idle and disabled-idle intent. A QA-only Abort injected after successful preparation returns 73; the actual NSIS failure callback restores original XML/security and starts the original task again without changing payload bytes. Invalid journal refusal returns 1, preserving the journal, installed bytes and live daemon. Fresh install/uninstall pass. The final source also passes native named-mutex refusal before any writes and another fresh lifecycle. The first failure fixture incorrectly expected a running task after the preceding disabled-idle case; its rejected result is retained and the corrected explicit-start run passes.

These are source/prototype results with exact setup hashes in `.hermes/evidence/windows-upgrade-current/NSIS_TRANSACTION_ACCEPTANCE.json` and raw per-phase receipts. They do not claim whole-payload rollback or acceptance of a subsequently rebuilt installer. Coherent clean portable/NSIS/MSI candidates must be rebuilt before publication; MSI failed-transaction product-registration rollback remains open. No existing operator installation, configuration or host service was changed.


### Clean Windows transaction candidates — 2026-10-15

All three Windows candidates are rebuilt from clean `fcf00aaee547085517300a5f5d32925c323dd2ae` at `.hermes/packaging-tmp/windows-current/`. Portable SHA-256 is `3ea27c533774afb54ddcf9fabc83fa039cb3008e2f032007d4140bcd0a8636b9`, NSIS `075a537f2852f9b60422fc5a47af4812ec22c6d49395c5df4df401ad49204710`, MSI `f28a7f143edba14fd972781c1fbe4a6c368ec8cc9037d05a5b3d7b84dba15108`. All 51 portable/NSIS payload files and current packaged documentation match the clean stage; the independently embedded NSIS controller matches staged indexd.

The exact new NSIS passes published410 running-daemon upgrade, enabled/disabled running/idle reinstall intent, security/login/database/source/preferences preservation, concurrent-setup refusal and invalid-journal refusal. Its fresh install/uninstall repeats all 51 payload identities and removes the generated index while retaining source/preferences. The earlier separately hashed QA-only Abort installer repeats task-restoration against the new installed payload; it remains a fault-fixture result, not a claim that the exact final setup contains that injected action. Exact new MSI fresh install/uninstall passes with all 51 installed files, live task/index/query, registration transition 5 to -1 and preserved source/preferences.

These candidates remain unpublished. Repeat upgrade/repair against the newest exact MSI and resolve the separate failed-transaction product-registration rollback gate before promotion. Previous651 successful upgrade/repair remains scoped to its recorded artifact. Wider native GUI/DPI, login/reboot, portable-path, Office, cross-version and signing checks remain separate. Exact scopes/hashes are in `.hermes/evidence/windows-upgrade-current/FINAL_ACCEPTANCE.json` and the matching staged acceptance files. The owned account/profile/password/tasks/staging are removed and the VM is shut off; original login values and existing operator installation/configuration are preserved. No Windows security-policy or manual MSI-registry workaround was applied.


### Exact MSI upgrade/repair and independent rollback reproduction — 2026-10-15

The exact clean `fcf00aa` MSI, SHA-256 `f28a7f143edba14fd972781c1fbe4a6c368ec8cc9037d05a5b3d7b84dba15108`, now passes the published410 running-daemon upgrade and all four forced-repair intent cases: enabled-running, disabled-running, enabled-idle and disabled-idle. All return 0 without reboot. All 51 installed files, saved database identity, source/preferences, task permissions and intended login/runtime state pass their checks. The raw upgrade output inherited an NSIS scope label from the common fixture; the executed script, MSI logs, ProductCodes and exact installer hash independently bind the MSI result. This closes the current-artifact normal upgrade/repair gate; it does not close rollback.

A real failed uninstall of that exact MSI reproduces broken registration. Locking an owned preview-cache file makes the application reject cleanup before stopping its daemon. Task XML/security, daemon instance, all payload files and source/preferences remain intact. The Windows Installer rollback then logs access-denied/invalid-owner registration errors, changes product state from installed5 to advertised1, and a normal retry returns1603. An ordinary-user ADDLOCAL repair returns1602 and uninstall still returns1603. No registry/policy/ACL or elevation workaround is used.

Two independently authored file-only per-user controls reproduce registration rollback failure after actual FileCopy/FileRemove operations. Each has one text file, an HKCU registry key path and a standard Type19 error action after InstallExecute; neither runs Ottrin or another custom executable. Failed installation leaves state5 with its file absent. Failed uninstallation restores its file but leaves state1. In the latter case forced repair and uninstall both return0 while the file remains; a fresh reinstall followed by uninstall removes it. This demonstrates why product state and exit0 alone do not prove removal. It does not establish the exact compiler/runtime/environment cause or a safe authoring repair.

[WiX issue8574](https://github.com/wixtoolset/issues/issues/8574) describes related failures. Metadata-only inspection of its attachments shows the reported Advanced Installer comparison uses an immediate failure at6401 before InstallExecute6500, so that comparison does not prove equivalent rollback succeeds. No downloaded third-party MSI was executed. Local controls use pinned native WiX5.0.2 in Windows11 IoT Enterprise LTSC build26100, msi.dll5.0.26100.9168. Initial fixture compilation/export mistakes were corrected before their results were interpreted.

MSI failed-transaction registration remains a rejected release gate. A production failure after index shutdown, full NSIS payload rollback, and the other native acceptance/signing gates retain their separate scopes; no partial release is inferred. README, guide, release notes and checklist now distinguish native construction/normal lifecycle evidence from this observed failure. Full receipts, controls and compact log extracts are under `.hermes/evidence/windows-msi-rollback-current/`; the staged acceptance records reference them. The fresh COW domain, disk, copied NVRAM and new TPM were discarded through libvirt. All backing VMs remain shut off and backing disk/NVRAM modification times are unchanged. Host settings and existing operator files are preserved.


### Copy retained-data cleanup admission — 2026-10-15

The platform now separates cleanup admission from Undo admission. An original-journal-bound `RetainedCopyCleanupPlan` keeps its evidence private and validates complete recorded operation groups, unchanged central/local records, directory/mount/object identity, retained bytes and references from other Copy operations. It permits cleanup planning after legitimate visible-file edits or reuse following completed Undo, while refusing changed backup data, added private-stage entries, incomplete publication/Undo and externally referenced overwrite-chain data. Revalidation never adopts a newer journal. Registry metadata participates in dependency checks; visible-file inspection is not deletion authority. An I/O-counter regression verifies admission and revalidation do not reread a changed 4MiB visible publication. Nested directories, symlink entries/targets, stale journals, replaced stages and batch/dependency selections have focused tests. An ancestor-folder overwrite also retains private transactions belonging to older copies: a differential regression rejects cleanup even after the older visible publication was deleted. The first prototype admitted that unsafe selection; it was corrected before enabling any deletion path. Dependency checks now compare nested generation handles for parent directories, private transactions and live publications; possible ambiguous matches retain data.

All 372 regular platform tests pass (one ignored), including seven new admission regressions. Strict Linux and Windows cross-target platform/UI all-target Clippy pass. This is source-level read-only admission, not implemented disk reclamation. Durable cleanup intent, per-entry recursive deletion with restart/retry progress, final journal/stage removal and Activity Clear/history eviction remain required. No UI cleanup action, automatic expiry or package delivery is claimed by this checkpoint; existing user transactions are untouched. Unknown/corrupt registry records refuse planning because their references cannot safely be excluded. Full lifecycle work remains open and is the next implementation stage.


### Copy retained-payload reclamation — 2026-10-15

The Linux platform now implements durable retained-payload reclamation behind the
original-record-bound cleanup plan. It records intent before claims, revokes
Undo, quarantines and validates each original node, unlinks descendants before
empty directories and persists compact append-only progress. Resume refuses IDs
without prior intent, reconciles canonical/local one-step journal lag and never
adopts changed original evidence. Symlink targets, visible publication edits,
unknown entries, namespace replacements, changed contents and cross-operation
dependencies retain their existing protection. The scoped metadata watch avoids
per-node full-journal/registry reads; active ledger replacement or in-place changes
are refused before unlink.

Focused checks include real child-process SIGKILL at seven durable boundaries,
error-return/reopen recovery, one-sided journals, active metadata/ledger mutation,
root symlinks, replaced ancestors and a 128-file metadata-read bound. Source-only
completion is limited to retained payloads. Private stage/ledger/journal
finalization, Activity background cleanup/retry and history eviction remain the
next work; no UI cleanup, automatic expiry or updated package is claimed. Unowned
pre-intent artifacts and torn writes remain conservative review cases, not
implicitly recoverable deletion authority. Full lifecycle and physical-platform
acceptance remain open.


### Copy group metadata finalization — 2026-10-15

The Linux platform now persists a group finalization manifest after payload
reclamation. It binds complete original records, registry identity and metadata
file content/generation evidence, rejects overlapping ownership, and can discover
or reopen partial finalization without mutation. Matching private ledger/local
journals are unlinked, empty original stages are removed, all canonical journals
are retired only after every stage is absent, and the manifest goes last. A
partial group with one canonical journal already removed remains recoverable.
Unknown children, changed bytes, byte-identical replacement files, reused stage
or parent names, late metadata edits and prematurely missing canonical authority
are refused. File and directory syncs retain explicit ordering; locks are scoped
to workers and explicitly released even if another thread inherited a descriptor.

Private source fixtures exercise in-process interruption/reopen and actual
subprocess SIGKILL at nine finalization boundaries, two-item operation membership,
final metadata-pin races, empty-stage replacement, lost authority and preserved
visible edits/symlink targets. These checks do not establish arbitrary power-loss
or physical-removable acceptance. Activity scheduling, status/retry and Clear/
history eviction integration remain next, followed by exact packaged workflow
acceptance. No automatic cleanup, operator-data mutation or package promotion is
claimed by this platform checkpoint.


### Activity Copy cleanup integration — 2026-10-15

Activity now offers explicit per-entry and bulk Copy cleanup with confirmation,
background progress, retained failure details and retry. It restores interrupted
finalization manifests as actionable history, disables Undo during mutation and
releases ownership/history only after durable finalization. Selected cleanup
observations read retained data without hashing visible publications or unrelated
retained payloads, preserve complete batch membership and cannot authorize Undo.
One-sided pending journals in a partly started batch are validated without writes
and reconciled by the mutation worker. Ordinary history trimming still preserves
owned receipts; automatic expiry remains disabled.

Source fixtures exercise actual asynchronous cleanup, confirmation before
mutation, changed-data refusal and retry, an overwrite chain and restart from
partially removed metadata in disposable processes/profiles. Exact packaged UI
workflow and visual acceptance, large-operation resource profiling and remaining
release/platform gates retain their separate scopes until verified.


### Activity Copy cleanup — verified Linux delivery, 2026-10-15

Clean source `eb10b1b7f42322a0e90f656280df869ebe21d93f` is published at the stable Linux AppImage, portable, DEB and RPM paths. AppImage SHA-256: `291d915dcad494ffcd8af2daf9486c72cdd6bac08330d6c7f7bbc792b0b2ec30`. All398 regular platform tests and374 regular UI tests pass, with one ignored in each suite; Linux and Windows cross-target strict checks pass. Private subprocess cases cover actual background cleanup, changed-data refusal/retry, overwrite-chain ownership and interrupted finalization. A differential I/O regression first reproduced an unnecessary visible-file read in interrupted Undo observation; the early refusal corrects it before delivery.

The exact delivered AppImage passes per-entry/bulk Clear confirmation, cancellation preserving the original backup, changed-data refusal, persistent error/retry, success styling, full selected stage/journal/manifest removal and source/publication preservation in Dark100 and Light150. The same actual package preserves complete menu icons, compact leading padding and separate check columns. The first visual previews exposed duplicated failure details, retry wording and an incorrect success color flag; those previews were corrected, rebuilt and retested before promotion. Normal exits and private fixture/display/daemon teardown are recorded. Current package contents and native namespace ownership checks pass; current daemon bytes differ from the older distro-tested payload, so older package-manager lifecycle receipts remain historical.

This closes the explicit Linux Activity Copy cleanup integration and its focused package workflow. Automatic expiry remains disabled; other owned recovery receipts remain protected. Large-operation resource profiling, broader appearance/desktop/release matrices, arbitrary power-loss boundaries, physical hardware and native Windows gates retain their separate scopes. Operator data/settings/services are unchanged and user testing remains deferred. Evidence: `.hermes/evidence/copy-activity-current/` and `dist/packages/COPY_CLEANUP_ACCEPTANCE.json`. The overall roadmap remains open.


### Copy finalization read scaling — 2026-10-15

A 32-directory overwrite/reclaim fixture reproduced 58,028,663 bytes of metadata
reads during finalization for 555,919 bytes of canonical metadata. The finalizer
now memoizes verified contents and parsed records only within the active worker.
Every repeated check still reopens the named file without following symlinks and
checks its generation, mount and complete inode stamp; every actual unlink keeps
its independent full-content and final pinned-name checks. Inode watches also
invalidate alias writes, including equal-timestamp changes. Cache admission is
limited to known local filesystem types with successful inode watches, 4,096
files and 64 MiB of encoded contents; unknown/remote filesystems and exhausted
watch/cache resources retain full reads. Directory enumeration, operation
membership, external dependency, stage absence and namespace checks remain in
place. Idle plans retain no cache or notification descriptor.

Focused read-volume and alias-write regressions are added. This is a bounded I/O
improvement, not proof of linear CPU/syscall scaling, broad large-operation
acceptance or delivery in an updated package. Current stable packages retain
the preceding Copy Activity receipt until a new exact-artifact acceptance passes.


### Copy namespace validation cost — 2026-10-15

Profiling the same 32-directory finalization fixture identified duplicate named
path metadata checks before the directory was opened and checked again. Copy
namespace validation now opens each current name with O_PATH|O_NOFOLLOW and
reuses the worker's authoritative opened-directory validator. Type, inode,
generation, unique mount and private-stage owner/mode checks remain active.
Mount queries on opened objects use statx AT_EMPTY_PATH directly; named-path
mount queries retain their existing ancestor/dangling-link semantics through the
same unique-mount syscall implementation. No namespace or generation result is
cached. A regression checks opened symlink and followed-target mount identities,
including after unlink/removal, on separate existing mounts.

All402 regular platform tests pass (one ignored), as does strict Linux platform
all-target Clippy. Five untraced runs of the finalization interval measured
329/308/311/316/315 ms before and223/229/225/224/224 ms after (median315 to224 ms).
The complete traced fixture, including setup/payload reclamation, made130,497
statx calls before and97,185 after, with identical name_to_handle_at/fstat counts.
These are bounded local optimized-test-build measurements, not a linear-scaling
or universal performance claim. Whole-group repetition and broader resource
profiling remain open. Exact-package delivery is recorded separately in
`dist/packages/COPY_NAMESPACE_ACCEPTANCE.json` when promoted.


### Copy finalization group scaling — 2026-10-15

The multi-size profile reproduced quadratic finalization growth: 8/32/128 records
used20/229/3330 ms wall time before this change. A worker-scoped namespace guard
now invalidates the full admission proof on registry/metadata/ancestor/stage
changes. It shares the metadata reader's single inotify descriptor, watches
canonical inodes for outside-alias writes, and checks mount namespace identity
and mount-table change events to catch same-inode bind rebinding. Only the exact
worker-owned deletion events are consumed. Every target keeps independent
full-content, generation, mount, name and pinned-file checks; full group
validation remains at admission and phase boundaries. The manifest is rehashed
after watch setup to close that startup interval even with coarse timestamps.

Current-name checks remain authoritative. All lexical ancestors are watched;
symlink ancestors, unknown/remote filesystems, exhausted watch resources or more
than8,192 namespace watches use the same full validator between individual
removals. No directory generation/namespace result is cached across workers.
The notification and mount-table descriptors exist only during finalization.

All408 regular platform tests pass (three ignored). The explicit ignored profile
and private-user/mount-namespace bind test also pass. Targeted regressions refuse
other-stage changes, new/replaced registry records, unselected dependency edits
through outside hard links, another parent's ancestor replacement, and stage
name reuse before any affected canonical journal is retired. Symlink-ancestor
fallback, prior SIGKILL/retry cases and source/publication preservation pass.
Strict Linux platform all-target Clippy passes.

The final optimized-test-build profile measured14/42/149 ms wall time and
10.383/38.090/149.277 ms thread CPU time for8/32/128 records; read calls were
694/2733/10893. The 128-record finalization interval is about22 times faster in
this fixture. Setup, payload reclamation, unrelated-registry scaling, unsupported
notification topologies, arbitrary power loss and physical/native platform gates
retain their separate scopes. Exact-package delivery is recorded in
`dist/packages/COPY_GROUP_SCALING_ACCEPTANCE.json` when promoted.


### Copy payload decoding and metadata invalidation — 2026-10-15

The resource fixture now follows the production cleanup-observation API and
reports copy, observation, planning, payload reclamation, manifest creation and
finalization separately. The retained-data phase was the remaining dominant
cost: 73/440/3961 ms for 8/32/128 overwritten directories with 16 retained leaves.

A shared bounded CopyRecord decoding memo now serves payload validation and
finalization. Every payload registry file is still opened and read through the
existing reader. Only exact byte equality reuses an immutable parsed record;
changed bytes go through the authoritative schema validator. Same-length edits,
invalid schema changes and restored bytes are covered. The memo retains at most
16 MiB of encoded contents and4096 entries, contains no descriptors, and is freed
when the last payload completes. Ownership, directory, mount, dependency and
fresh-byte checks remain in place. Registry reads and watch setup still have
separate larger-group scaling costs; this does not claim linear reclamation.

The review reproduced a payload-monitor gap: an outside hard-link edit of an
unselected dependency journal was detected after the claimed retained object
had already been unlinked. The regression fails before the inode-watch repair
and now preserves that exact object before refusal. Payload monitoring watches
all canonical journal inodes and its local journal, in addition to their parent
directories. Payload and finalization share one mount-namespace invalidation
implementation; a same-inode bind change is refused before unlink. Unknown or
remote notification semantics and failed inode-watch admission force full
scope/pair checks at each monitored boundary; a no-event regression verifies
that fallback still sees changed dependencies.

All411 regular platform tests pass (three gated cases), as does strict Linux
platform all-target Clippy. The explicit private namespace gate now exercises
both payload and metadata finalization. The final local optimized-test-build
profile measured90/413/2173 ms for retained-data reclamation; the128-entry case
is about45 percent faster than3961 ms, with stronger monitoring. Finalization
remains approximately linear in the monitored fixture. Native receipt-unmount,
exact package workflow, broad resource and platform gates retain their recorded
scopes; delivery is recorded in `dist/packages/COPY_PAYLOAD_ACCEPTANCE.json`.


### Windows Office caller-queue hypothesis — fresh native rejection, 2026-10-15

A new ordinary interactive-user control copies each Office fixture into a fresh
owned directory before loading it. This matters because the original retained
spreadsheet corpus has a stale LibreOffice lock: direct-original controls return
a generic load failure rather than testing the previously observed timeout. The
first diagnostic runner also recorded unavailable exit codes; those results are
excluded, and the corrected runner pins the process handle before waiting.

With fresh copies, synchronous Kit converts Writer in14.14 seconds and its PDF
independently contains both expected page markers. Synchronous Calc reaches its
30-second watchdog without a PDF. Keeping the caller thread pumping while a
worker loads fresh XLS and ODP also reaches the watchdog in both cases. This
rejects caller-queue pumping as a repair with valid inputs; it does not establish
a deadlock cause. Native wait-chain capture reports per-thread states without a
dependency cycle. Production code, containment and stable packages are unchanged;
the Windows Office provider remains gated. All-format/fidelity, macro-positive
control and exact-package acceptance remain open. Evidence and precise scope:
`.hermes/evidence/windows-office-loop-current/ACCEPTANCE.json`. Exact probe tasks,
processes, uploads and copied fixture/profile directories were removed; the
original corpus, stale lock and operator installation were preserved.


### NSIS setup/uninstall exclusion — native source regression, 2026-10-15

The NSIS installer acquired its process-lifetime installation mutex, but its
uninstaller did not. Native before/after fixtures reproduce removal despite a
held installer lock. Setup and uninstall now instantiate one shared initializer
that acquires that same mutex before any section or cleanup action. The corrected
uninstaller refuses with exit1 before invoking cleanup; installed file hashes
and registration remain unchanged. Setup also refuses the held lock. Releasing
the holder allows an ordinary uninstall retry to finish with exit0.

A real corrected uninstaller paused in its fixture cleanup also excludes a second
uninstaller and setup. Only one cleanup invocation occurs; the payload stays
unchanged until the first cleanup resumes, then ordinary removal succeeds. All
cases preserve external fixture source/preferences and remove their isolated
registration, shortcut and user-data names. The native fixtures compile the full
NSIS scripts with harmless mock executables and isolated integration names; this
is source behavior acceptance, not exact production payload/service acceptance.
Evidence: `.hermes/evidence/windows-installer-lock-current/ACCEPTANCE.json`.
Owned task, processes, uploads and extracted fixtures were removed and the guest
was shut down. Stable packages remain unchanged. Full NSIS payload rollback,
coherent package refresh, MSI registration rollback and cross-engine coordination
retain their separate open scopes.


### NSIS staged payload replacement — native model checkpoint, 2026-10-15

NSIS now completes extraction and uninstaller construction in a sibling
transaction directory before stopping indexing or changing the installed tree.
Publication retains the complete previous directory and moves the candidate into
place. The payload controller records the user, target, local volume GUID, full
128-bit file identities, owned file hashes, typed installation-registry values
and shortcut bytes. Integration begins only if the captured metadata is still
current. A staging failure does not overwrite concurrent registration changes.

On ordinary setup failure, the new index is stopped before restoring the previous
payload and metadata; the existing index transaction then restores task intent.
Only after that succeeds is the unused candidate reclaimed. Successful index
commit is the point of no return: old managed files are retired through verified
read/delete handles that deny writers and bind deletion to the checked object.
Changed or unowned files remain outside the live installation with a Unicode
notice; failed cleanup retains its record. The index snapshot now lives beside
the payload record. Uninstall refuses a pending payload transaction, and setup
and uninstall do not bypass an unresolved snapshot from the preceding installer.

Eight isolated native model cases cover foreign-directory refusal, a forced
staging failure with a concurrent metadata change, service and commit failures,
a directory lock, stale-record refusal, successful replacement with unowned-file
retention, and pending-transaction uninstall refusal. They check old directory
identity and bytes, typed registry restoration, shortcut bytes, external fixture
source/preferences, and phase ordering. Separate native leaf checks cover wrong
identity, changed bytes, successful exact-object retirement, and an active
writable mapping after its original file handle closes; late writes remain
accessible. The fixture uses harmless mock app/index executables and isolated
integration names. It does not accept the actual scheduled daemon, full package
payload, arbitrary namespace races, restart recovery or power loss.

The source's default NSIS construction also compiles. Native execution exposed
and repaired the NSIS plugin System.dll shadowing the C# framework reference,
and PowerShell's null-to-empty-string binding in atomic journal replacement.
The helper uses the trusted CLR working directory and an explicit null-string
argument. Fixture exit-code, Unicode source-encoding and lock-admission mistakes
were corrected before accepting the final results.

Evidence: `.hermes/evidence/windows-payload-current/ACCEPTANCE.json`. Owned
fixture tasks, processes, integration names, profiles, uploads and staging were
removed; the QA guest is shut off. Stable packages remain unchanged. Next work is restart/commit-boundary recovery and
exact coherent Windows candidate construction with real scheduler, daemon and
installation acceptance. The separate MSI registration rollback and Office
provider gates remain open.

### Windows NSIS durable commit decision checkpoint (2026-10-15)

The controller now has an Activate phase that restores preexisting runtime
intent while retaining the index rollback snapshot. NSIS then records the payload
commit decision, removes the index snapshot, and reclaims the prior payload.
Failures or uncertain process results during the commit write retain the new
installation and recovery records without guessing whether rollback is allowed.
Committed cleanup can resume after individual old leaves have already been
retired. MSI retains its existing single Commit phase.

Eleven isolated native installer cases pass, including failures immediately
before and after the durable decision, activation failure, final snapshot-removal
failure, and cleanup resumed after exact-object retirement of one old leaf.
Two additional real disposable scheduled-task cases confirm stopped/running
intent restoration, unchanged snapshot bytes after Activate, preservation of the
new task definition, final snapshot removal, and missing-snapshot refusal.
The daemon phase-parser test, Windows search-library check/strict Clippy and
default NSIS construction pass. Evidence:
`.hermes/evidence/windows-commit-current/ACCEPTANCE.json`.

This is a source and isolated integration checkpoint. Automatic restart recovery,
full coherent package/daemon acceptance, MSI registration rollback and the Office
provider gates remain open. Stable packages are unchanged. Task-owned fixtures,
integrations, processes and staging were removed; the owned guest was shut down.

### Windows NSIS interrupted-setup recovery checkpoint (2026-10-15)

Setup now validates a retained transaction before creating a new one. An
uncommitted publication restores the original payload and integration metadata,
then the task controller restores scheduler intent. Already restored payloads
avoid another daemon stop. A committed publication retains the new installation
and resumes snapshot removal and old-payload cleanup. The new schema v2 records
staging-directory identity; unknown, torn or foreign records remain refused.

Final transaction-folder retirement precedes deletion of its receipt, so a killed
process can leave a completed receipt or empty folder for the next setup to
reclaim. Unexpected root files retain the journal alongside them. Integration
metadata is recaptured immediately before installation renames. This fixes a
native failure where Windows refreshed shortcut tracking after a rename and the
installer incorrectly rejected that consequence as an external metadata edit.

All 22 native isolated interruption/retry and refusal cases pass on the corrected
source. They check directory identity, typed registry values, shortcut behavior,
preferences, retained unowned files and controller call order. App/index commands
are mocks; this is not combined real-daemon/scheduler/full-package acceptance.
Default NSIS construction and the updated source lifecycle preflight pass. The
Windows app and daemon release cross-build and GUI-subsystem checks also pass;
new coherent installers have not yet been built or promoted. Rejected fixture
assumptions, a bounded timeout and the causal shortcut-guard failure are retained
with the final results at
`.hermes/evidence/windows-recovery-current/ACCEPTANCE.json`.

Owned fixture processes, task, integrations, uploads and staging were removed;
the owned guest was shut down. Stable packages are unchanged. Next work is exact
coherent candidate construction and actual app/daemon/scheduler acceptance.
Pre-journal initialization interruption, arbitrary power loss and namespace races
retain their separate limits. MSI registration rollback and Windows Office
provider acceptance remain open.

### Coherent Windows candidate and real index lifecycle checkpoint (2026-10-15)

Unpublished portable, NSIS and MSI candidates now share clean source
`3ddb1bf65ee514d1ecc69745df66055d2ae515e1` at the stable internal path
`.hermes/packaging-tmp/windows-current/`. All 51 portable and NSIS files match
staging; native administrative MSI extraction matches those same files. Both
installer controllers match the current daemon, and NSIS embeds the exact
payload-recovery script. The preceding generated candidate was atomically
replaced after its receipt was archived.

The exact NSIS passes fresh install/uninstall, upgrade from the published410
build with a running daemon, and all four login/runtime-intent combinations
under a new interactive non-administrator. Checks cover payload hashes, task
permissions, preferences, saved-index behavior and upgrade database creation
identity. The exact MSI passes ordinary-user fresh install/uninstall with all
payload hashes, product registration 5 to -1, task/index/payload removal and
preference/source preservation. Its real daemon observed a new filename in
686 ms; a five-second quiet sample in the two-file fixture measured zero
single-core CPU. This is a small filename-only fixture, not large-index or
content-enabled performance acceptance.

Evidence: `.hermes/evidence/windows-transaction-package-current/ACCEPTANCE.json`.
The disposable account, profile, task, temporary credential, native compiler,
extractions and uploads were removed; prior login values were restored exactly,
the original installation/preferences remained unchanged and the guest is off.
Stable public packages remain unchanged. Next is full-payload interruption and
recovery with the actual daemon/scheduler, plus the still-open MSI registration
rollback gate. Current MSI upgrade/repair, GUI/DPI, reboot/login, Office, signing
and full daily-driver acceptance retain their separate scopes.

### Full-payload NSIS recovery with real indexing checkpoint (2026-10-15)

The coherent `3ddb1bf` NSIS candidate now passes four native forced-termination
and recovery cases with all 51 payload files, the actual daemon and the actual
Task Scheduler controller. A separately hashed QA setup pauses after the old
root moves, after candidate publication, after payload rollback, or after the
commit decision. Each is force-terminated; the exact unmodified candidate
`26888c2439a54e64eb046333cb7541598d4a2142de23805c09346590e9416211`
then retries successfully under a fresh interactive non-administrator.

Observation of the retry's new journal proves the correct original directory
identity was restored before a new uncommitted installation, while the committed
case retains the candidate identity. The retry's real task snapshot verifies
recovered XML, security and running intent. The cases span all four login/runtime
combinations. Binary and QWord registration values are restored before commit;
committed DWORD values remain after it. Payload hashes, preference/source hashes
and saved-index query/creation identity pass. Checkpoint payload hashes also
match the complete current stage, excluding only the QA-generated uninstaller.

Evidence: `.hermes/evidence/windows-full-recovery-current/ACCEPTANCE.json`.
The owned process trees, index task, account, profile, credential, uploads and
private setup outputs were removed; original guest data/login values were
preserved. These are four selected termination boundaries, not arbitrary power
loss, pre-journal initialization, hostile namespace races or the full state/fault
cross-product. Stable public packages are unchanged. MSI registration rollback
is the next installer gate; Office, wider GUI/DPI, reboot/login, signing and
full daily-driver acceptance remain separate.

### Current MSI early-execution hypothesis rejected (2026-10-15)

The exact unpublished `3ddb1bf` MSI (`717890c74af6e259005b37ae057bf5eb273e6bafea58a2c15d9fb139e0e138be`) reproduces the failed-registration rollback gate under a fresh interactive non-administrator. Cleanup refuses a locked owned preview-cache file before touching the daemon. PID/instance, task XML/security, all 51 payload files, preferences, source data and saved-index identity remain intact. Windows Installer nevertheless records 79 skipped registration rollback errors, leaves advertised product state 1, and fails an ordinary uninstall retry with 1603.

A private prototype executes the queued cleanup before `ProcessComponents`, retains transaction rollback, and uses `InstallExecuteAgain` for late upgrade removal. Its log proves `InstallExecute` fails before `ProcessComponents` starts, yet it produces the identical registration failure and retry result. The hypothesis is rejected; production MSI authoring is unchanged. This does not establish the underlying engine/compiler/environment cause. No elevation, ACL/policy change, rollback disabling or manual MSI registry repair was applied. Current exact-MSI upgrade/repair follow-on did not run after corruption.

Evidence: `.hermes/evidence/windows-msi-execution-current/ACCEPTANCE.json`. All three private VM domains, their thin overlay disks, private NVRAM/TPM and the rejected package were removed after guest shutdown and daemon teardown. Original QA machines remain off and preserved. Stable public packages are unchanged; the current candidate receipt now records this exact-artifact rejection. The MSI promotion gate remains open while other actionable roadmap work continues.

### Empty pre-journal NSIS initialization recovery (2026-10-15)

Setup retry now reclaims a journal-free transaction only when it contains no
entries, or only its empty `new` staging directory. It validates directory
identities, refuses reparse entries, and removes directories nonrecursively.
Extraction, daemon preparation and payload publication have not begun at either
of these initialization boundaries. File-containing, partial-journal and
unexpected layouts remain refused with their contents retained.

The native ordinary-user regression passes all 12 cases on Windows 11 IoT
Enterprise LTSC build 26100: four real helper-process terminations (root/new,
fresh/existing), followed by recovery, initialization retry and rollback; and
eight refusal/preservation cases. Original directory identities and file hashes
remain unchanged. A separate comparison reproduced the previous helper's
refusal at all four interruption points. The final tracked regression runs
without a historical source dependency, refuses administrator execution and
terminates whole helper process trees during timeout/cancellation cleanup.
Its exact source and helper hashes match the native uploads.

The NSIS source lifecycle contract and construction with the corrected helper
also pass. Evidence: `.hermes/evidence/windows-initialization-current/ACCEPTANCE.json`.
The private construction output and both disposable native trial environments
were removed; original QA machines remain off and preserved. Public packages
are unchanged. This is helper-level interruption acceptance plus installer
construction, not renewed full-package/daemon/scheduler acceptance. Torn journal
recovery, arbitrary power loss and hostile namespace races remain separate;
MSI registration rollback, Office and wider platform gates remain open.

### Inspector read-only entry and PDF fit stability correction (2026-10-15)

The shipped Linux AppImage reproduced automatic text-editor entry: an admitted
VCF captured arrow keys and Space instead of navigating/closing. Preview-load
completion no longer creates an editor. **Edit text** is the explicit entry
point; existing save/conflict/discard protections remain. Folder sample text
also uses an immutable selectable label rather than an editable throwaway buffer.
The Inspector uses the shared title bar at the window border, with padding in
its body and the shared window surface.

A rendered regression reproduced a width-fitted page alternating between 318
and 325 pixels throughout 180 frames. A real one-page PDF in the shipped package
also produced repeated width/height changes across 100 captures. Conditional
solid-scrollbar allocation changed the fitted page width and therefore whether
it overflowed, creating a feedback loop. Width-fit mode now reserves a stable
vertical gutter, and redundant Inspector visual/Office scroll containers are
removed so the common preview frame owns scrolling. PDF fit defaults use the
file profile, avoiding a different default while asynchronous metadata loads.

All 376 UI library tests pass (one ignored), including explicit edit entry, draft
preservation during refresh, pending-PDF defaults and overflow-boundary settling.
Strict UI all-target Clippy, formatting and diff checks pass. Exact packaged
Linux acceptance follows this source checkpoint; Windows build/native acceptance
of these UI changes remains separate. The newly built `3a3d72d` Windows portable
and NSIS inputs passed all-51-file archive verification but their native installer
actor was not launched before this urgent user correction. Its disposable VM was
shut down and discarded; inputs and handoff are retained at
`.hermes/evidence/windows-init-package-current/`. The full roadmap goal remains
active, including the independent MSI registration rollback gate.
