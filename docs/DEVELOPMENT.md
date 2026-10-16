# Ottrin Development

## Workspace

Ottrin is a Rust workspace using egui/eframe.

| Crate | Purpose |
| --- | --- |
| `ottrin-app` | Native eframe entrypoint |
| `ottrin-ui` | egui application shell and UI workflows |
| `ottrin-core` | Domain models, settings, commands, and semantic file classification |
| `ottrin-platform` | OS integrations, file operations, trash, and privileged helper execution |
| `ottrin-search` | Indexed search, filesystem watching, and query fallback logic |
| `ottrin-preview` | Preview classification and loading |

The supported Rust toolchain is pinned in `rust-toolchain.toml`.

Use the workspace `target/` for current builds. Historical-source QA builds must use a task-owned `CARGO_TARGET_DIR` under that directory: Cargo can reuse relative-path fingerprints across worktrees and report an older executable as fresh. Keep that isolation only while its consumers need it, and remove it after QA. Record source revision and executable hashes before packaging; a successful Cargo exit does not prove artifact freshness.

## UI consistency requirements

Every UI change must preserve shared theme roles and consistent text/icon alignment across the main window, Settings, theme editor, Inspector, menus, and dialogs. Reuse the authoritative themed widgets; previews must use production controls where they represent those controls.

Quick Inspector opens every file read-only. Only an explicit **Edit text** action
may create an editor; asynchronous loading, reload of a preview, and selection
navigation must never enable editing. Read-only previews leave arrow navigation
and Space-to-close available. Width-fitted document previews reserve a stable
vertical scrollbar gutter so overflow cannot feed back into page sizing.

Human-readable path labels must use the shared display helpers. Keep navigation, commands, clipboard paths and editable configuration as original paths; never reconstruct an operation path from a shortened display string. Windows drive/share roots are one breadcrumb with the original rooted target. Explicit breadcrumb and address navigation must also reveal the destination file column; changing only the address while its contents remain off-screen is a failed navigation.

Sort headers use the shared header painter and installed Material direction icons, with a reserved marker slot even when the label is truncated. Search markers follow the actual query order: Name/Folder ascending, Size/Modified descending.

Repeated rows must share label starts, icon centers, padding, and vertical centers. Reserve separate fixed columns for selection indicators and icons, including empty columns on unchecked or disabled rows within a choice group. Action-only menus must omit the unused selection column. Use meaningful, consistently sized icons for menu actions and view/sort options; review density, padding and visual balance as well as geometry. Never position icons or checkmarks with whitespace inside text labels. Keep submenu arrows and secondary actions in their own trailing columns. Scale row height and column spacing with text size; avoid clipping or shifting labels when state changes.

Before delivery, inspect affected surfaces in Light and Dark at 100% and 150% desktop scaling and enlarged text. Exercise checked/unchecked, enabled/disabled, hover/focus, and submenu states. Add rendered geometry regressions for shared alignment fixes and verify the exact packaged app. A compilation check alone does not satisfy visual acceptance.

Portable runtime closure includes the window backend libraries loaded with `dlopen`, not only ELF `DT_NEEDED` entries. Keep the shared GUI runtime declaration in `packaging/linux/bundle-runtime-libs.sh` synchronized with window-backend dependency changes, and update native-package dependencies. The verifier must trace those library roots and their transitive dependencies. Verify launch against a fresh supported system; a working development desktop can hide missing dependencies.

## Linux installed Office acceptance

The shared `native_office_boundary_contract` uses the production Linux Bubblewrap launcher and Windows Job Object launcher. On Linux, run the ignored conversion, nine-format corpus and in-flight cancellation checks as an ordinary user with installed LibreOffice, Bubblewrap, a working systemd user manager, and the pinned PDFium runtime:

```sh
OTTRIN_PDFIUM_DIR=/absolute/path/to/pdfium/lib \
LD_LIBRARY_PATH=/absolute/path/to/pdfium/lib \
OTTRIN_NATIVE_OFFICE_FIXTURES=/absolute/path/to/owned/corpus \
cargo test --release -p ottrin-preview --test native_office_boundary_contract -- --ignored --nocapture --test-threads=1
```

The corpus test defines the required filenames and independent rendered-text markers. Macro positive-control acceptance remains Windows-specific and requires its explicit fixture. These native tests prove their provider scope; they do not substitute for Inspector interaction or exact-package acceptance.

Ubuntu 24.04 can deny Bubblewrap namespace setup under its default AppArmor policy. The fresh cloud guest reproduced `net_admin` denial before provider startup. Its configured-provider acceptance uses the distribution's `apparmor-profiles` package and restrictive `bwrap-userns-restrict` extra profile, loaded by the guest administrator; the user-namespace restriction remains enabled. This setup is not installed by Ottrin and must not be mistaken for default-install acceptance. Follow [Ubuntu's documented Bubblewrap policy guidance](https://discourse.ubuntu.com/t/understanding-apparmor-user-namespace-restriction/58007), preserving administrator-owned policy. Do not disable AppArmor or global user-namespace restrictions to make a test pass.

A configured native probe also exposed a missing `/bin` sandbox mount: `/usr/lib/libreoffice/program/soffice` exists but its `/bin/sh` interpreter cannot execute. The launcher now exposes `/bin` read-only alongside `/usr`, with the existing network, filesystem and resource boundaries intact.

## Build And Verify

Run filesystem and UI tests with private XDG state, configuration, cache and data directories, and remove that test profile after all test processes exit. Copy recovery journals must never be written into the operator's state directory. `scripts/test-workspace.py` provides the disposable home and state while retaining Cargo/Rustup caches and the session IPC needed by systemd-isolated renderers. Forward Cargo test options to that runner; native graphical QA uses its own private display harness.

```sh
cargo fmt --check
cargo check --workspace --all-targets
python3 scripts/test-workspace.py --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release
```

Linux development dependencies on Debian/Ubuntu:

```sh
sudo apt-get install -y \
    libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libssl-dev libglib2.0-dev \
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
```

### Linux development playback

`cargo run -p ottrin-app` builds the GUI, but Cargo does not build binaries
belonging to dependency crates. Build the separate playback helper first:

```sh
pkg-config --modversion gstreamer-1.0 gstreamer-app-1.0
cargo build -p ottrin-preview --bin ottrin-media-playback --features system-gstreamer
cargo run -p ottrin-app
```

Rebuild the helper after changing playback code. Keep the same `CARGO_TARGET_DIR`
and profile for both commands (add `--release` to both for release development).
Do not copy an old release helper into `target/debug`. No build script invokes
Cargo. If pkg-config fails, install the development packages listed above;
runtime libraries alone are not enough to compile the helper. Missing helpers
and unsuccessful helper exits appear in the Inspector, with loader diagnostics
also available in the launching terminal. `OTTRIN_MEDIA_HELPER` can explicitly
select a freshly built helper when using a custom layout.

After building the helper, run the non-interactive native playback smoke:

```sh
python3 scripts/linux-media-playback-smoke.py target/debug/ottrin-media-playback
```

This requires Python 3.11+ and `ffmpeg` for generating a tiny silent raw-video
fixture; neither is an end-user playback dependency. The probe prints the exact
helper SHA-256 and checks decoded RGBA pixels, pause/resume acknowledgement,
end-of-stream, Stop, retained-descriptor playback after pathname replacement,
malformed input, and the missing-plugin fallback using an isolated empty plugin
search path. It removes its fixtures and temporary GStreamer registries, without
installing codecs or changing the user's plugin setup. Use a freshly built
helper: its hash identifies the tested bytes but does not prove source freshness.
This is not Inspector visual acceptance, audible playback verification, a codec
coverage claim, or a clean-package test.

Linux native Open With support requires GIO `>= 2.56`. The Linux portable and
AppImage builders bundle the required GIO/GLib runtime closure; this development
package is not an end-user package dependency for those artifacts.

Its `BUNDLED_RUNTIME_MANIFEST.txt` is an **unsigned build record**: its hashes
bind the shipped runtime bytes and copied notices for integrity checks and
diagnosis, but they do not authenticate a final artifact. Someone able to
rewrite an artifact can rewrite this record too. Final-artifact authenticity
requires a future signed artifact or attestation and is not claimed here.

The Linux runtime-closure builder is intentionally Debian-family specific: it
requires `dpkg-query` to record package/version and copyright provenance and
fails closed when that evidence is unavailable. Its supported artifact ABI is
the system dynamic loader, glibc base ABI, and base C++ runtime (`libgcc_s` and
`libstdc++`), including standard glibc split components such as `libresolv`,
`libutil`, and `libanl` when needed. Those components remain host-provided;
Ottrin does not claim broad Linux portability.

GIO uses host desktop-entry and XDG MIME data by design. The artifacts do not
claim to bundle GIO modules or schemas. Packaging has an artifact-level
content-type smoke with empty XDG data, but clean/minimal desktop acceptance is
a separate release gate.

### Windows background indexing

Ordinary-user source-binary acceptance on Windows 11 build 26100 exercised
`index service-install`, `service-status`, an interval upgrade, `service-restart`,
and `service-remove` through the real Task Scheduler. The task changed from four
to five hours, ran the actual `ottrin.exe index update` action with result zero,
and published both files from two isolated configured roots. CLI search returned
those exact paths. Removal left no task; restart then correctly failed. Existing
profile directories were preserved by rename and restored, with generated state
kept only in the disposable fixture (`index-lifecycle-final.log`). This verifies
the per-user source lifecycle, not MSI/NSIS installation or uninstall integration.

Windows uses one recursive native watch per configured index root. Do not open
additional descendant watches: NTFS can deny renaming a populated ancestor while
those handles are open, even when the watcher shares delete access. Linux keeps
its filtered per-directory watch registration. Each replacement scan plan creates
a fresh watcher and drops the entire previous registration set. Backend events
use that watch's immutable scan policy before entering the worker queue; excluded
generated churn is discarded, while overflow/rescan requests retain root recovery.

The native `windows_index_watches_allow_directory_moves_and_follow_new_subtrees`
search test starts a real service, moves existing and newly indexed trees, verifies
their exact new query paths and excluded-content policy, then changes roots and
checks that old registrations are released. Run it on Windows; cross-compilation
alone does not validate native watch handle lifetimes. Configured-root ancestor
moves and junction isolation require separate native acceptance. Microsoft documents
the underlying [directory rename constraints](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information).

### Windows native video thumbnails

The ignored `windows_native_thumbnail_decodes_and_preserves_admitted_source` test
in `video_thumbnail_contract` requires `OTTRIN_NATIVE_VIDEO_FIXTURE` pointing to a
64x48 solid-red H.264 MP4. Generate the fixture with an already available encoder:

```sh
ffmpeg -f lavfi -i color=c=red:s=64x48:r=5 -t 1 -c:v libx264 -pix_fmt yuv420p native-thumbnail-red.mp4
```

Run the integration test on Windows with its installed OS decoder. Ordinary-user
acceptance on Windows 11 build 26100 passed in `native-thumbnail.log`: decoded
32x24 red pixels and opaque alpha, pre-cancellation, malformed-input refusal, and
pathname replacement after admission without reading replacement bytes. It does
not install codecs or prove broad format coverage, in-flight cancellation,
audible playback or exact-package acceptance.

### Windows Office process boundary

Run the native Job Object fixtures on Windows as the ordinary test user:

```powershell
cargo test -p ottrin-preview --test windows_job_native native_job_ -- --test-threads=1
```

The fixtures exercise the actual launcher with disposable child processes and
private temporary files. Each fixture owns a byte-copy of the test executable inside
its private access policy so contained descendants can relaunch it without
changing the build artifact's permissions. Fixture setup owns its temporary root
before fallible operations; its existing cleanup removes partial setup and the
executable copy. The ignored `fixture_root`, `fixture_descendant` and
`fixture_environment_parent` entries are subprocess entrypoints selected by the tests, not independent tests
to invoke manually. They cover output and diagnostic cleanup, timeout after the
parent exits, cancellation, process and memory limits, aggregate descendant
memory, restriction of inherited handles, and environment isolation. They do not certify an installed
LibreOffice conversion, its profile cleanup, or provider fallback presentation.

The environment fixture starts a separate parent containing fake credential,
proxy and Python-runtime overrides, then verifies these do not reach the
renderer, its PATH excludes the parent's override, and profile/temp variables
point to a private Unicode directory. `office-environment-causal-red-results.log`
reproduces the fake credential leak. `office-environment-native-results.log`
passes all eight process-boundary cases (6.80s) and all three installed-provider
fixtures (41.83s), including nine formats and in-flight cancellation.

The launcher builds a sorted, double-NUL-terminated Unicode environment using
the [CreateProcessW environment contract](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw).
It obtains the Windows directory from the OS, limits PATH to the executable and
Windows directories, and maps HOME/USERPROFILE/APPDATA/LOCALAPPDATA/TEMP/TMP to
the private profile. It does not copy parent variables. This addresses environment
inheritance. The current AppContainer admits a guarded read-only input snapshot,
private writable profile/output and no capabilities. Native fixtures verify
private sibling-file denial, input-write denial, network denial and global pipe
denial; installed-provider corpus and fallback acceptance remain separate.

Linux can build the fixture executable with
`cargo test -p ottrin-preview --target x86_64-pc-windows-gnu --test windows_job_native --no-run`.
Cross-compilation is not native acceptance. Keep the Windows Office provider
gated until its remaining installed-provider checks pass.

The ignored `native_installed_office_converts_and_cleans_private_state` fixture
in `native_office_boundary_contract` requires a standard Windows LibreOffice
installation and the packaged PDFium runtime:

```powershell
$env:OTTRIN_PDFIUM_DIR = "C:\path\to\Ottrin\bin\pdfium"
cargo test -p ottrin-preview --test native_office_boundary_contract native_installed_office_converts_and_cleans_private_state -- --ignored --exact --nocapture
```

Ordinary-user acceptance with LibreOffice 26.8.1.1 on Windows 11 build 26100
passed on 2026-10-10 (`installed-office-pdfium.log`). It exercises production
discovery, DOCX conversion through the Job Object, PDFium page rendering and
text content, the provider label, unchanged source bytes, private-profile
cleanup, and pre-cancellation without additional output. Discovery previously
returned `NotInstalled` despite the installed provider (`installed-office-red.log`);
Windows now searches the standard Program Files locations and Windows launch
names. Six focused provider contracts pass on both Linux and native Windows.

Macro suppression requires a working positive control, not merely a macro-free
PDF. The ignored `native_installed_office_suppresses_document_macros` native
fixture takes `OTTRIN_NATIVE_OFFICE_MACRO_FIXTURE` (the owned ODT) and
`OTTRIN_NATIVE_OFFICE_MACRO_CONTROL_PDF` (the PDF exported from that exact ODT
with macros enabled in an ordinary-user control). It requires the control's
`OTTRIN MACRO EXECUTED` marker before checking the production PDF for unchanged
baseline content, absent marker, preserved source and cleaned private state.
The current control has not executed its load event; the test correctly refuses
acceptance. Do not weaken that prerequisite or treat an ignored test as passed.

The Windows converter uses the packaged `ottrin-office-render.exe` helper with
installed LibreOfficeKit rather than the CLI, whose global named pipe is denied
by AppContainer. The helper reads provider version metadata without starting
LibreOffice; conversion uses the stable C ABI, a disposable profile, batch mode
and explicitly disabled macros. It exports `source.pdf` on both platforms.
Each invocation stages helper bytes inside the private sandbox ACL, so portable
builds need no permanent permission changes. Development/native test builds use
`--features windows-office --bin ottrin-office-render` and set
`OTTRIN_OFFICE_HELPER` to that absolute helper path. Packaged builds discover the
helper beside Ottrin. The Windows UI provider remains gated: the earlier CLI
acceptance below predates the AppContainer work and does not accept this path.

The historical pre-AppContainer CLI fixture passed macro-free DOC, DOCX, ODT,
XLS, XLSX, XLSM, ODS, PPTX and ODP (`office-formats-final-results.log`, 27.95s).
The current contained Kit sweep passes DOC/DOCX/ODT and times out on the other
six formats at 30 seconds each (`windows-20261011/helper-formats.log`). An
uncontained Calc control also times out; worker/message-pump and SDK runLoop
controls do not convert it. Production threading remains unchanged. The
historical fixture checks actual
PDFium pixels and expected document text, unchanged source bytes, and private
profile/log cleanup. Spreadsheet thumbnail ink can be antialiased gray (XLS's
darkest pixel was 129); this check detects visible ink rather than requiring
nearly black pixels. Set `OTTRIN_NATIVE_OFFICE_FIXTURES` to the owned fixture
directory containing the filenames and text markers specified in
`native_installed_office_renders_each_advertised_format`.

The in-flight cancellation fixture waits for LibreOffice's actual private-profile
lock before cancelling, then verifies typed cancellation, unchanged source,
profile/log cleanup, released source handles, and directory removal
(`office-formats-green-results.log`). Run the installed-provider fixtures with
`cargo test -p ottrin-preview --test native_office_boundary_contract -- --ignored --nocapture --test-threads=1`.

This bounded acceptance does not enable the Windows UI or certify representative
fidelity, macro security, fallback presentation or installer acceptance.

Longer private-directory layouts reproduced a LibreOffice crash even outside
Ottrin's Job Object (`office-layout-control-results.log`). Length controls failed
with a 150-character profile path and succeeded with 138 characters, independently
of dots in directory names (`office-layout-length-control-results.log`). The
native production contract reproduced the longer-layout crash
(`office-profile-red-results.log`). Ottrin now uses a shorter disposable-profile
name at its authoritative definition, retaining PID/nonce uniqueness. No limits,
containment or provider executable paths changed. All three installed-provider
fixtures pass with the longer baseline directory and original nine-format output
directory names (`office-profile-green-results.log`, 42.20s). Six focused Linux
provider contracts and strict Windows Clippy pass. This closes the reproduced
layout regression, not arbitrary-length provider paths.

An earlier probe
returned unhealthy; a later unchanged native probe succeeded, so no timeout was
relaxed. The earlier test's text extractor could not decode the generated PDF's
Unicode map; the final fixture checks content through the actual PDFium renderer.

### Windows Move

Run same-volume fixtures on a native Windows NTFS volume as the ordinary test
user. Symlink fixtures require permission to create links; the disposable
acceptance guest used [Windows Developer Mode](https://learn.microsoft.com/en-us/windows/advanced-settings/developer-mode).

```powershell
$env:OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT = "$env:TEMP\ottrin-native-move-state"
cargo test -p ottrin-platform same_volume -- --test-threads=1
```

Use a fresh, owned recovery-state directory. Shared `TestDir` fixtures isolate
recovery state per test and restore the previous root on teardown. These tests
check actual NTFS publication, exact source/displaced identities and bytes,
source-substitution refusal, conflict handling and dangling links. Their passing
results do not certify crash/power-loss recovery, adverse ACL/sharing scenarios,
other filesystems, retained-transaction lifecycle or packaged operation.

Journal creation and replacement flush both file contents and the parent namespace
before reporting persistence. `ReplaceFileW` has no supported write-through flag;
see [its native API contract](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew).
A native fixture blocks the directory flush after journal publication and verifies
failure propagation and retry after sharing is restored:

```powershell
cargo test -p ottrin-platform windows_native_move_journal_requires_namespace_flush
```

This verifies issued flushes and error handling, not physical power-loss survival.

Locked-directory verification can be checked independently:

```powershell
cargo test -p ottrin-platform windows_native_locked_directory_snapshot_preserves_exclusion
cargo test -p ottrin-platform windows_native_directory_streams_are_empty_or_rejected
```

Move enumerates streams and directory children through its owned handles using
`GetFileInformationByHandleEx`; it does not reopen an exclusively held directory
by pathname. The snapshot fixture covers multiple buffers, Unicode names and
continued writer/delete exclusion. Windows refuses a directory rename while
descendant files remain open. Move closes those handles at the rename boundary
while retaining the root handle, then revalidates before source removal. For
files, directories and links, the published entry is identity/content verified
and remains locked throughout source cleanup; changed content retains the source
and recovery state.

The temporary acceptance gate was removed after native safety and guest power-cut checks. A fresh no-override native run passed all 21 cross-volume fixtures, FAT32 refusal in both directions and 99 standard platform tests. The final native public-command fixture also passed (0.69s): both `execute_move_with_undo_receipt` and batch `FileCommand::Move` moved exact bytes across independent NTFS volumes, removed the source and returned explicit dispositions without claiming Windows undo. Strict Windows platform/UI all-target Clippy passes. `.hermes/evidence/windows-20261011/PUBLIC_MOVE_ACCEPTANCE.json` binds the separate suite and final public-API binaries. The clean `56f0d4e` portable package also passes limited-user Cut/Paste/Replace from C: to a typed Q: destination: source removal, exact published/displaced bytes, retained file identity, agreeing journal pair, and bundled Mesa/OpenGL/DXC module paths are verified in `PREFIX_PACKAGE_ACCEPTANCE.json`. A fresh native rerun adds Modify-only destination coverage: 22 cross-volume cases and 99 default tests pass; strict Windows platform all-target Clippy passes (`MODIFY_DESTINATION_ACCEPTANCE.json`). Broader public GUI acceptance remains pending.

Cross-volume fixtures are separately ignored by default. They require a real
second Windows volume in `OTTRIN_WINDOWS_CROSS_VOLUME_ROOT` and an isolated
recovery root. They exercise the same transaction route as the public Windows
Move commands; the temporary acceptance gate and its overrides are removed. If using an attached VHDX, grant the test user Full Control on the backing image before attachment. The virtual-disk attachment filter can otherwise deny WRITE_DAC/WRITE_OWNER and replacement even when the inner NTFS ACL grants Full Control. Keep restricted-destination scenarios inside the volume; do not restrict the backing image itself.

```powershell
$env:OTTRIN_WINDOWS_CROSS_VOLUME_ROOT = "Q:\owned-ottrin-fixtures"
cargo test -p ottrin-platform windows_native_cross_volume -- --ignored --test-threads=1
```

Guest hard-power-cut recovery has a separate ignored fixture,
`windows_native_guest_power_cut_recovers_cross_volume_retained_overwrite`.
Run its exact name as the limited interactive user in an owned disposable guest,
with `OTTRIN_NATIVE_MOVE_POWER_CUT_MODE=prepare`,
`OTTRIN_NATIVE_MOVE_POWER_CUT_ROOT` naming a fresh owned NTFS directory, and
`OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT` set to that directory's `move-state`
child. Set `OTTRIN_NATIVE_MOVE_POWER_CUT_BOUNDARY` to `BeforeSourceCleanup`
(regular-file Overwrite) or `SourceClaimed` (directory Overwrite, nested payload
and empty directory). Use the same value for recovery. The second-volume variable above is also required.

Wait for the root's durable `ready` marker **and** verify that the prepare task
and test process are still running. Hard-stop only the owned guest at that point;
do not kill just the process or shut Windows down normally. Preparation has a
90-second deadline and fails if the orchestrator never cuts power. After boot,
reattach the same owned second volume with the same letter/path and run the
exact fixture again with mode `recover`. It verifies original source bytes,
published bytes, displaced bytes and identity, then explicit recovery and its
idempotent repeat. This is virtual guest power-cut evidence, not physical-device
power-loss acceptance. It does not establish physical-device or exact-package acceptance.

The orchestrator owns teardown across both invocations: stop/remove its tasks
and processes, retain compact diagnostic logs, remove both fixture roots as their ordinary-user owner even
on failure or cancellation, detach its volume, and stop the guest. Preparation
intentionally retains those roots across the destroyed process so recovery can
inspect actual journal and file state. Never use user data or the user's VM for
this destructive fixture.

Use a second disposable native volume and an owned fixture directory. The
fixtures check file/directory/symlink Skip/Rename/Overwrite conflicts with exact
displaced-entry retention, nested raw links without traversal, empty directories,
source preservation after mutations at rename handoffs, published-file/link
change refusal, and retry after source or published-file writers close. Native
fixtures also verify destination ACL denial preserves the source and permits retry
after access restoration, plus file/directory creation and modification times,
hidden/archive attributes and payload preservation. An uninterrupted unoccupied-target
Move also preserves a read-only file: the original writable staging handle survives
through publication/flush, followed by exclusive read verification and identity-bound
source deletion without changing its attributes. Identity-only probes request
attribute access so they do not conflict with the content lock. Staged process-exit
recovery also covers read-only files, including a second exit while recovering private
metadata. Recovery verifies the private candidate before making it writable, then
rechecks it and restores source metadata through the reopened writable handle before
publication/flush. The source and published entries are not made writable by this
step. Cross-volume Overwrite of a writable target with a read-only source also
passes: the same verified private-candidate preparation allows `ReplaceFileW`,
then source metadata is restored and flushed through the published handle. The
exact displaced identity, bytes and attributes are retained. A native process exit
immediately after atomic replacement also recovers source metadata before durability
confirmation and source cleanup. Recovery after the published file becomes read-only
verifies and pins both copies, journals reclaim intent, and moves the publication back
to private staging before restoring metadata through a writable handle. A second
process exit after reclaim also recovers; changed source data refuses recovery without
altering the published copy. A source named stream added after interruption also
refuses cleanup: the pinned source stream set is rechecked before deletion, and
ordinary retry succeeds after removal of that added fixture stream. Unexplained
replacement gaps remain blocked. Read-only destination files remain intact under
Skip and Rename; Overwrite refuses without changing either copy, then succeeds
with exact displaced retention after the fixture removes destination protection.
Explicit cross-volume recovery and discovery expose the same verified transaction
route as ordinary Move retry. Native lifecycle evidence covers safe refusal, recovery after protection
removal, repeated completed recovery preserving retained originals and later user
edits and malformed/misnamed journal refusal. Native cross-process exclusion also passes: an independently running
owner holds the actual recovery lock while explicit recovery and ordinary Move
retry both refuse without changing journal/source/target bytes. Killing that owner
releases the Windows share reservation; recovery then completes with exact displaced
identity/data retention and remains idempotent (`recovery-owner.log`). The fixture
uses readiness signaling and bounded polling, and kills only its own child process.
Directory recovery also passes process exits at initial journal publication, candidate
unlock/publication, before source cleanup, source unlock and source claim. The initial
journal case exposed a central intent with no local counterpart. Recovery now completes
only an empty, identity-bound `Prepared` transaction with no candidate; it pins the
transaction directory and creates the local journal without replacing a name. Unexpected
staged data and a local journal missing after publication remain refused. Native evidence
is in `directory-boundaries.log` (RED), `directory-boundaries-pin.log` (GREEN) and
`directory-regression.log` (18 cross-volume, ten same-volume and one namespace-flush
check passing). This is process-death acceptance, not power loss. Filesystem and
packaged cases stay pending.
An owned native FAT32 fixture verifies safe refusal in both NTFS/FAT32 directions
for Rename and Overwrite: the required source/destination identity probe returns
native error 87 before mutation, and both files retain their exact bytes
(`filesystem-boundary.log`). The fixture verifies filesystem names through native
volume handles; this is unsupported-filesystem refusal, not FAT32 Move support or
physical removable-device acceptance.
Extended attributes are refused through a shared handle-based query at admission,
metadata copying, tree snapshots and source cleanup. The native regression exposed
an accepted file whose extended attributes were not copied. It now verifies exact
attribute/data retention for files, directory roots and nested files, both before
Move and after a process exits before source cleanup (`move-ea-red.log`,
`move-ea-final.log`). This is refusal of unsupported metadata; it does not claim
extended-attribute copying or arbitrary concurrent metadata-writer exclusion.
Top-level file and directory symbolic links use the same stream admission and
cleanup guard as nested links. The native regression creates named data on the
reparse object using `FILE_FLAG_OPEN_REPARSE_POINT`, verifies the target has no
named stream, and checks exact link identity, raw target and stream-byte retention.
It covers admission refusal and cleanup-pending refusal after interrupted
publication (`link-stream-red.log`, `link-stream-final.log`). Ordinary dangling-link
Move still passes. This does not claim named-stream copying.
Completed non-Overwrite Moves now persist a central `SourceRemoved` receipt only
after source namespace durability. That state resumes teardown directly and accepts
the matching local `Published` evidence or an already removed local journal. Local
and central journals remain pinned from validation through handle-based deletion;
the verified empty transaction directory is deleted by handle and its parent is
flushed before the central receipt is removed and flushed last. Unknown contents,
mismatched journals and replacement directory identities retain central discovery.
The native teardown fixture covers sharing refusal/retry, process exits before
cleanup and at three terminal boundaries, and journal replacement/truncation refusal
(`teardown-red.log`, `teardown-receipt.log`). This replaces the former mock that
covered only the first journal-removal failure. It does not prove physical power-loss
behavior or discovery of an externally relocated private transaction directory.
Windows storage discovery now canonicalizes available drive roots to the same
verbatim-path form used by navigation. The native regression reproduces and fixes
Cut refusing a selected file because `C:\` did not match `\\?\C:\`
(`storage-root-red.log`, `storage-root-green.log`). A source-built Windows GUI then
completed Cut/Paste with Replace existing item, rediscovered the same-volume
retained journal after restart, and completed Recover now. Ordinary-user checks
confirm exact published and displaced bytes, absent source and unchanged journal
before/after that idempotent recovery (`move-ui-before-recovery.log`,
`move-ui-after-recovery.log`). A separate interrupted cross-volume fixture is
visible without a recovery button while its gate is closed (`move-ui-04.png`).
These checks do not enable cross-volume Move or certify an exact package.
The ACL fixture invokes `icacls.exe` only on its owned directory and restores its temporary denial. Process
exit fixtures recover through ordinary Move after staging, publication, source
claim and displaced directory/link retention. They do not simulate power loss or
certify all conflict, filesystem, ACL, read-only metadata, retained-lifecycle or packaged
cases. Fault hooks exist only in test builds.

## Packaging

End-user packages must run without Cargo.

Build all release artifacts available on the current machine:

```sh
./build-all.sh
```

On Linux this builds the Linux portable tarball and AppImage. It also builds the Windows portable zip when the Windows GNU target is already installed. On macOS it builds the unsigned app-bundle archive. To require the Windows artifact:

```sh
rustup target add x86_64-pc-windows-gnu
./build-all.sh --with-windows
```

Linux portable:

```sh
./packaging/linux/package-portable.sh
```

Linux AppImage:

```sh
./packaging/linux/package-appimage.sh
```

Native Linux packages:

```sh
./packaging/linux/package-deb.sh
./packaging/linux/package-rpm.sh  # requires rpmbuild
```

Both native formats consume `stage-native-package.sh`. Unlike the portable and
AppImage builders, native staging does not bundle a private runtime-library
closure; package metadata declares the system libraries used by the GUI and
optional system-GStreamer playback helper.

macOS app archive (must run on macOS):

```sh
./packaging/macos/package-app.sh
```

`stage-app-bundle.sh` is the single app-layout source of truth. It stages the
GUI, index daemon, `Info.plist`, icon, documentation, and source provenance;
the packager preserves that `.app` layout in a checksum-sidecar ZIP. Native
launch, Finder, quarantine/Gatekeeper, and signing acceptance remain separate
macOS gates.

Windows portable from Linux with the GNU target:

```sh
./packaging/windows/package-portable.sh
```

Windows portable from PowerShell:

```powershell
.\packaging\windows\package-portable.ps1
```

Windows installer from an extracted portable payload (requires NSIS):

```powershell
.\packaging\windows\package-installer.ps1 -Stage .\dist\windows-payload\Ottrin-0.1.0-windows-x64
```

The clean `0d1ea18` NSIS candidate has native standard-user evidence for silent installation, same-version reinstall, exact 49-file payload identity, Task Scheduler registration, and uninstall of files/task/shortcuts/registry with saved preferences preserved. `.hermes/evidence/windows-20261011/NSIS_ACCEPTANCE.json` binds the installer checksum and fresh-account token evidence. This does not accept a true version upgrade, rollback, interactive installer, populated-data cleanup or MSI lifecycle.

Windows MSI from the same extracted portable payload (requires WiX Toolset 5):

```sh
./packaging/windows/package-msi.sh ./dist/windows-payload/Ottrin-0.1.0-windows-x64
```

Linux portable, AppImage, `.deb`, and `.rpm` package sources are available. Windows has portable zip, NSIS per-user installer, and per-user MSI package sources. macOS has an unsigned app-bundle ZIP package source. Native Windows MSI and macOS archive construction plus install/launch/platform acceptance remain pending on their target systems. Exact native package installation and desktop acceptance remain part of the final Linux candidate pass.

## Privileged Helper Model

Privileged operations are per action, never a global root/admin mode.

- The UI requests a typed privileged file operation through `ottrin-platform`.
- The platform crate serializes a request to `ottrin-priv-helper`.
- Linux launches the helper through `pkexec` and a polkit policy.
- Windows launches the helper through the UAC flow.
- The helper validates an allowlist of supported filesystem operations and returns a typed result.

Linux packaging should install:

```sh
sudo install -D -m 0755 ottrin-priv-helper /usr/libexec/ottrin/ottrin-priv-helper
sudo install -m 0644 packaging/linux/org.ottrin.filesystem.policy /usr/share/polkit-1/actions/
```

Runtime helper discovery checks explicit override, environment variable, app directory, standard install paths, and finally `PATH`.

## QA Checklist

### Linux

- Navigate Miller/List/Grid, tabs, bookmarks, and address bar.
- Search first index, restart from DB, rebuild, current-folder search, content search, and result actions.
- Create, rename, copy, trash delete, and permanent delete; verify Linux same-filesystem and cross-filesystem Move with Skip/Rename/Overwrite/Cancel and explicit dispositions. Cross-filesystem Move must stage and verify the complete destination tree before publication, revalidate before guarded source cleanup, retain explicit recovery evidence on uncertainty, and offer Undo only from identity/content-bound single-entry evidence. On Windows, extend the native same-volume Skip/Rename/Overwrite fixture evidence with crash, adverse ACL/sharing, retained-transaction lifecycle and packaged checks. Windows cross-volume Move uses the verified staged-copy route; unsupported filesystems and unsafe metadata remain refused. Accept its public workflow and exact package separately; cross-compilation alone is not acceptance.
- Tandem activation, pinning, side switching, and pane-to-pane copy.
- Quick Inspector for text, image, PDF text/metadata, DOCX, XLSX/XLSM, PPTX, archive, folder, and generic fallback files.
- Privileged helper smoke with helper and policy installed.
- Helper-missing and policy-misconfigured error paths.

### Windows

- Launch the portable zip on a real Windows desktop.
- Navigate, search, run file operations, and validate Trash/Recycling Bin behavior.
- Validate Quick Inspector and Tandem View.
- Validate UAC allow/cancel paths and helper-missing behavior.
- Validate background index updater scheduling/removal.

## Performance Discipline

- Keep filesystem operations off the UI thread.
- Use background workers for directory listing, preview loading, search, trash loading, and long operations.
- Cap expensive UI rendering for very large folders.
- Preserve release-mode search benchmark coverage.
- Avoid logging full directory contents in release builds.

## Release Gate

Before a public v1 release:

- Run `./scripts/release-preflight.sh` for the repeatable, non-publishing local
  gate. Use `--source-only` before artifacts exist, or
  `--require-optional-tools` on a fully provisioned release workstation.
- `cargo fmt --check`, workspace check/test, strict Clippy, and release build pass.
- Linux, Windows, and macOS CI are green.
- Linux AppImage and portable tarball are checksum-verified and smoke-tested from clean artifacts.
- Windows portable zip is launched and smoke-tested on real Windows.
- Linux privileged helper/polkit smoke test passes.
- Windows UAC/helper smoke test passes.
- `README.md`, `docs/USER_GUIDE.md`, `docs/ROADMAP.md`, and `docs/RELEASE_NOTES.md` accurately describe the shipped artifacts and known limitations.

## Theme Source Provenance

Theme presets are derived from upstream palettes where possible:

- Breeze: KDE Breeze color scheme files.
- Adwaita: GNOME libadwaita palette and stylesheet colors.
- Windows 11: Microsoft Fluent UI color tokens.
- Solarized: Ethan Schoonover's Solarized palette.
- Nord: official Nord palette.
- G33k: custom terminal-inspired palette.

When updating a theme preset, keep the mapping deterministic and document any intentional deviation in code comments near the preset definition.

## Linux local-path operation provenance

Navigation discovery and operation admission have different scopes. Local Linux
clipboard and queued operations use the same fallible capture/validation path.
Every path captures its actual `statx` unique mount identity, including Home and
paths under discovered volumes; discovered stable identities are checked as well.
Missing identities never become empty, unprotected operation bindings. Sources
identify the final symlink entry; directory destinations follow the final link.
For new pathnames the nearest existing ancestor is probed again at validation;
dangling followed links and permission failures are refused. This requires Linux
6.8+ (`STATX_MNT_ID_UNIQUE`); no reused-ID fallback is used. Bindings are in-memory
only and never interpreted as persistent volume/index identities.

The mount-replacement fixture must run in an owned private user/mount namespace:

```bash
OTTRIN_QA_PARENT_MOUNT_NAMESPACE="$(readlink /proc/self/ns/mnt)" \
  unshare --user --map-root-user --mount cargo test -p ottrin-ui --lib \
  tests::local_storage_binding_rejects_real_mount_replacement_under_discovered_parent \
  -- --ignored --exact --nocapture
```

Root mapping grants mount capability only within this child namespace; host user
permissions are unchanged. The fixture checks its namespace differs from the
provided parent before attaching anything. It creates and tears down only its
private tmpfs mounts, then checks that replacement is refused despite unchanged
discovered-parent identity. Ordinary-user tests separately exercise the actual
clipboard/paste worker, source-link and destination-link semantics, dangling-link
refusal, changed ancestors and unavailable-volume tombstones. As with existing
volume checks, pathname mutation after the last worker check remains an OS race;
transaction-level source/publication identity checks still apply.

## Batch Copy receipt ownership

Copy batch dispatch has one platform loop, returning a completed-source count,
every identity-bound per-entry Undo receipt, and any failure. A later failure
keeps earlier receipts and explicit recovery paths. The UI owns those receipts
in Activity rather than falling back to the receipt-discarding command route.

Batch Undo runs the existing Copy inverse in reverse order. It tracks a remaining
index while retaining all receipts, including completed ones, so retry never
repeats a completed inverse or drops ownership of its retained transaction data.
Destination-wide overlap guards cover other batch entries and conflict-renamed
outputs. A pathname replacement stops Undo without modifying the replacement;
Activity reports progress and retains the remaining action for retry.

Safe expiry/reclamation of Copy transaction stages is still open. Receipt
ownership and successful Undo do not establish that transaction storage has
been reclaimed. Native Windows now has journaled completion evidence, but no guarded Copy Undo
implementation or public Copy receipts yet; cross-compilation does not establish
Windows Copy acceptance.

Windows Copy candidates use `windows_fs::PerObjectFile` for the documented
`MAXIMUM_ALLOWED` per-object permission-update contract. Keep its constructor
private and preserve the contract through cloning; do not add production
conversions from arbitrary `File` handles or mutable access that permits handle
replacement. `DestinationSecurity::apply` requires this type. Ordinary journals
and text-save creation keep their explicit rights through the same authoritative
atomic-private-creation implementation. The ordinary retained-stage inspector
continues to hold read-only handles. `PermissionStage` validates that epoch,
releases its locks, reopens and compares the paired journals, and explicitly
acquires new typed capabilities. Publication intent precedes a no-replace rename;
root/parent flush and a bound target acknowledgement precede DACL application.
Every entry has ordered paired intent/acknowledgement records; recovery freshly
derives native policy and accepts private/final security only as its recorded
phase permits. Never reapply an already exact final DACL. A final verification
record follows the full payload/security scan, and the writer reserves the remaining
journal tail before publication, including a conservative readonly-reopen allowance.
The schema-5 codec returns the append
sequence/hash; do not reconstruct a second interpretation of the frame chain.
Known links remain no-follow leaves. Single-link guards apply to candidates,
not sources. This contract does not grant Overwrite, Undo or cleanup authority;
those phases and full public Windows Copy acceptance remain required.

Source-free permission recovery also accepts a coherent unfinished permission-plan
prefix after full payload verification. A single streaming appender serves both
live preparation and recovery: freshly rederive native inheritance, compare the
recorded prefix, reserve the remaining plan plus publication/application tail,
then append only missing records. Do not repair mismatched mirrors or treat an
unverified payload stage as resumable.

Read-only file roots and descendants use paired `ReadonlyReopenIntent` /
`ReadonlyReopened` records. Only the pending index may have its readonly bit
cleared; creation/write times, all other attributes, streams, object identity,
single-link status and phase-specific permissions must still match. Verify the
retained tree before acquiring a writer. Under durable intent, clear only that
bit, release the old handle, reopen the verified object with the typed
`MAXIMUM_ALLOWED` contract, revalidate contents and permissions, restore the
original attributes, then flush through the new writable capability before
acknowledging. Retain that capability for publication and DACL flushes. An
interrupted descendant intent is completed before acquiring other file
capabilities. Stream revalidation uses the caller's cancellable progress path;
cancellation leaves the same journaled intent for fresh recovery. Exact completed reopen is read-only and appends nothing. Unknown
changes are retained/refused; do not normalize their metadata to resume.

The format is `OTCPWJ05` / schema 5; superseded schemas are refused, without a
compatibility reader. Log reservation includes prospective readonly reopen
records and a pending acknowledgement; repeated interruption remains bounded by
the total log budget and must fail before an unreserved mutation. These native
ordinary-user fixtures do not certify all inherited denial/privilege, specialized
storage, SACL/label, readonly-link or power-cut cases. The underlying native flush
requires write/append access:
https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntflushbuffersfileex

Native Windows Copy finishes with ordered `FinalEntry` records and a `Finalized`
seal. Final entries freeze the native identity, directory/link role, length,
creation/write/change times and attributes after all DACL acknowledgements.
Stream hashes, namespace topology and final security remain in the authoritative
staging/permission records. Finalization validates the complete tree before
recording, checks the frozen epoch again before sealing, and records a streamed
SHA-256 digest of the final versions. Missing or mismatched seals do not produce
a completed receipt. Cancellation and process-exit recovery append only missing
final entries; an already recorded version is never replaced with a new baseline.

`CompletedCopy` opens the paired journal and retained tree read-only, requires the
seal, and verifies every frozen version, contents, membership and freshly derived
permissions. Native change time protects against rewriting identical bytes and
restoring the visible write timestamp. The completion reader is a temporary
validation epoch, not a long-lived UI file lock, external persistence format or
permission to delete anything. A future inverse must acquire its own explicit
journaled mutation authority after validating this evidence. Unknown changes
remain untouched/refused. Undo, Overwrite and cleanup remain required.

Journal reservation covers the final-entry frames and seal before publication.
Unknown future native change times and digest decimal widths use bounded maximum
JSON encodings, rather than assuming an exact future length. Evidence digests
share one streaming serializer/hash implementation with the permission plan;
no whole serialized evidence buffer is required.

Copy and created-entry receipts guard the original parent and transaction
identities with one shared, nonblocking Linux inotify instance. Watches are
registered through the already-open publication directories, before those
handles close. Deletion, unmount, removed watches, queue overflow, and read
failure invalidate the proof. Directory identities and Linux unique mount IDs
must also match; a bind remount must not revive an old receipt even when its
inodes are unchanged. No watcher thread or periodic polling is needed.

Undo opens and verifies directory handles only for its inverse, then uses
`renameat` relative to those handles. Destination aliases cannot redirect that
mutation after validation. Named parent/stage replacement, including published
hard links preserving file inode identity, is refused. Long-lived directory
handles were rejected after a private namespace probe demonstrated they prevent
ordinary unmount; the replacement watch design has a dedicated ignored private
mount test for ordinary unmount, tmpfs replacement, and same-inode bind remount.

Cleanup admission is implemented separately from Undo admission through
`plan_retained_copy_cleanup`. It captures immutable discovery observations and
revalidates the recorded operation membership, both journals, namespace/object
identities, retained contents and cross-operation references, including an older
private transaction nested inside an ancestor-folder overwrite. A changed visible
publication does not block discarding an otherwise unchanged retained backup;
changed retained data, unexpected stage entries and interrupted publication/Undo
are refused. Plans hold no directory descriptors between calls; admission alone is not
standing deletion authority. Registry records are read for dependencies, but unrelated
visible file contents are not read. Unknown/corrupt registry records and unavailable dependency object evidence
currently refuse admission because references cannot be proven absent. Possible
generation-handle matches across namespaces conservatively retain data; those
matches never grant mutation authority.

The platform payload worker persists `CleanupPending` before its first claim and
revokes Undo for that transaction. Each retained node has an immutable original
path, content and object binding. A compact append-only, hash-chained progress
ledger records a pending node before it is moved to the private quarantine;
revalidation checks every ancestor, the quarantined object and its contents
before unlink. Descendants precede directories; symlink targets are never
followed. Completed payload reclamation persists `CleanupComplete` in both
journals. Journal updates are canonical-first, and a one-step local lag can be
reconciled against the same original evidence. `resume_retained_copy_cleanup`
only reopens existing durable intent; transaction IDs cannot start cleanup.

The worker uses a scoped metadata watch so it need not reread full-tree journals
and registry dependencies for each entry. Registry or journal changes, unknown
stage entries, queue overflow and watch failures invalidate the initial proof.
The active ledger is checked by its pinned identity, mount, size and modification
stamps, including immediately before unlink. Original namespace/object and
content checks remain mandatory; watch events alone never authorize deletion.
Plans and idle history hold no directory descriptors, so they do not pin mounts.

Payload recovery has focused error/drop/reopen and real subprocess SIGKILL tests
at seven durable boundaries, one-sided journal recovery, active-ledger/journal
mutation refusal, replaced ancestors, root symlinks and a metadata-read bound for
128 retained files. These are source/private-fixture checks. Torn ledger writes,
unowned pre-intent artifacts and uncertain evidence are retained for review;
these tests do not establish arbitrary power-loss or physical-removable acceptance.

After all payloads finish, `begin_finalization` persists one immutable group
manifest in the private Copy registry. It binds the complete original records,
registry generation/mount and the contents/generations of each ledger, local
journal and canonical journal. A transaction cannot join overlapping manifests.
`discover_retained_copy_finalizations` and `resume_retained_copy_finalization`
reopen that authority without writing. Unknown or damaged manifests retain their
path for review rather than being adopted or silently omitted.

Finalization removes only matching ledger/local-journal files, then uses `rmdir`
on each empty original stage. Every stage must be absent before any canonical
journal is removed, and the group manifest is removed last. Thus interruption
between canonical journal removals does not lose batch membership or its original
namespace evidence. Named and pinned metadata stamps/mounts are checked again
immediately before unlink; changed bytes, byte-identical replacements, unexpected
children and replaced stage/parent/registry names are refused. Worker-scoped
manifest and stage locks serialize mutation. Explicit lock release also prevents
an unrelated thread's fork from extending a completed worker's OFD lock lifetime.
No directory descriptors survive in an idle plan.

Source/private-fixture finalization checks cover a two-item Copy group, forced
termination at nine pre-mutation/post-fsync boundaries, loss of one canonical
member, in-process error/reopen retries, changed/replaced metadata after pinning,
late unknown children, replacement empty stages and retained visible edits/symlink
targets. An original in-memory plan can confirm all paths are absent and retry the
last directory sync if the manifest unlink succeeded before an error. IDs alone
cannot reconstruct a missing manifest to delete surviving data.

Activity routes explicit per-entry and bulk Clear requests through a background
Copy cleanup worker after confirmation. The UI holds transaction ownership until
the worker confirms durable finalization, blocks Undo while cleanup runs, shows
progress/errors and retains failed entries for retry. Cold discovery groups a
partial finalization by its manifest, including when some canonical journals are
already absent. Stale in-flight Activity snapshots are invalidated after cleanup.
Cleanup-specific observation reads only selected retained payloads and registry
metadata; its evidence cannot restore Undo. A mixed batch with one lagging local
intent is admitted read-only and reconciled only by the mutation worker.

Automatic expiry remains disabled. History still keeps other owned recovery
receipts; an explicit Clear releases Copy ownership only after the complete
cleanup lifecycle succeeds. Source worker checks do not promote a package. Torn or
unowned manifest writes conservatively retain data for review; the interruption
matrix does not establish arbitrary power-loss durability. Prior exact AppImage
directory-ownership acceptance remains at its original scope in
`dist/packages/COPY_DIRECTORY_OWNERSHIP_ACCEPTANCE.json`.

Run the mount fixture only inside an explicitly owned child namespace:

```sh
OTTRIN_QA_PARENT_MOUNT_NAMESPACE="$(readlink /proc/self/ns/mnt)" \
  unshare --user --map-root-user --mount cargo test -p ottrin-platform --lib \
  tests::copy_receipt_does_not_pin_mount_and_refuses_after_unmount_and_replacement \
  -- --ignored --exact --nocapture
```

Linux packages share the pinned PDFium download and license payload in `packaging/linux/package-pdfium.sh`. Native packages install it under `usr/libexec/ottrin/pdfium/lib`; runtime discovery resolves that location relative to the executable installation prefix, without an environment override. The native libc requirement includes the renderer, and release verification checks its payload and code identity alongside the application.

### Windows MSI rebuild identity

Ottrin's local MSI rebuilds retain the semantic package version and receive a new
ProductCode. Treat equal-version packages as replacements using WiX
`AllowSameVersionUpgrades`; otherwise Windows Installer registers both products
against the same files. Lower semantic versions remain blocked. Same-version
builds can replace each other in either direction; source commits are not an
ordered Windows Installer version. Published releases must advance a first-three
semantic version field when ordering/downgrade protection is required. Verify
that upgrading removes the old product registration, retains only the new one,
preserves preferences/source data and migrates the per-user index task.


## Native Linux package lifecycle

DEB/RPM use `packaging/linux/native-package-lifecycle.sh` for native service cleanup and post-install refresh. User files are changed only after switching to that UID with a sanitized environment; effective executable ownership keeps independent portable/AppImage services intact. Removal performs ownership preflight before mutation, refuses unresolved offline overrides, and retains preferences, indexes and source files. Reinstall uses try-restart so stopped registrations remain stopped.

The package's internal `ottrin-indexd --native-package-stop` operation also drains native session daemons without an installed unit. It authenticates a Unix peer, validates the executable path (including a deleted native image), and compares the installed executable entry through both the caller’s filesystem and `/proc/PEER/root`. Different private mount views cannot establish ownership from matching path text alone. It keeps the connection used for validation, requests graceful upgrade preparation, and waits on the captured process fd. It does not create a daemon or load a user profile. Independent portable executables at the same user's endpoint are left alone.

Native acceptance must exercise direct package-manager removal, not only application service removal followed by uninstall. Cover active/offline users, an independent portable service/session, protected Administrator state, cleanup failure before file deletion, preserved data and running/stopped reinstall behavior. Canonical offline configuration is currently supported; custom offline configuration roots and nonstandard runtime endpoints remain separate audit items. Native Fedora and Ubuntu fixtures are under `.hermes/evidence/native-lifecycle-current/` and `.hermes/evidence/deb-lifecycle-current/`; current artifact scopes are in `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json`. Matching installed entries across private mount namespaces, foreign copied entries at the same apparent path, and deleted native executables have a real unprivileged namespace regression:

```sh
python3 packaging/tests/verify-linux-native-daemon-ownership.py \
  --binary target/release/ottrin-indexd --output /tmp/ottrin-native-ownership.json
```

This requires Linux user/mount namespaces. It preserves the invoking UID, uses private profiles/endpoints, and reaps each owned process before deleting its mount sources. Namespace denial is a failed/unavailable environment gate, not a passing ownership test.

Persistent Windows daemon launches must inherit no caller handles. Redirecting only stdin/stdout/stderr is insufficient: other inheritable pipe/file handles can keep CLI readers open or files locked after the client exits. The native launcher uses `CreateProcessW` with handle inheritance disabled while retaining the caller environment. Verify cold CLI output reaches EOF while the daemon remains alive, and run the standalone inherited-file regression, including paths with spaces and Unicode.

Windows root-window startup uses the native primary-monitor work area, excluding taskbars, and converts its physical coordinates using the current system DPI after Winit establishes awareness. The initial viewport hook fits requested/minimum size and centers a fresh main window; explicit caller positions remain intact. Verify real initial bounds against the work area, not only the full monitor size. Wait for the actual titled window during native QA: temporary renderer windows are not UI readiness.

The Windows MSI embeds its current indexd as the transaction controller. Before InstallFiles it captures task metadata/runtime intent and stops the index through the authenticated normal service API. Snapshot/rollback/commit use embedded shared PowerShell source and never depend on installed scripts being available. Successful commit preserves preexisting running/idle intent using the upgraded definition; rollback restores the original definition/security. The controller does not remove saved index/configuration. Engine-driven failure/registration rollback requires separate native verification.


NSIS serializes setup instances with a process-lifetime named mutex so a second
installer cannot consume an active recovery journal. It uses the same embedded
index transaction implementation as MSI and extracts
its current indexd into the NSIS private plugin directory, prepares before any
payload copy, and commits after normal service installation. Installation failure
and confirmed cancellation restore the previous task through that independent
controller. Keep failed recovery journals in the user's Ottrin data directory;
never put the only recovery record in the automatically deleted plugin directory.
A payload-copy error must return failure rather than silently leave an old daemon
inside a new installation. Task restoration does not provide whole-payload
rollback: failed partial replacements require a setup retry and must say so.

The native Windows initialization recovery regression runs as an ordinary,
non-administrator user:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File packaging/tests/windows-initialization-recovery.ps1 -Script C:\path\to\Ottrin\packaging\windows\payload-transaction.ps1 -Report C:\path\to\results.json
```

It force-terminates initializers before the first journal, verifies retry and
rollback for fresh/existing installations, and checks refusal/preservation of
file-containing, malformed and linked layouts. It owns and tears down its temp
fixtures and processes. This helper-level test does not replace full NSIS
package/daemon/scheduler interruption acceptance. A journal-free transaction may
reclaim only its empty root or empty `new` child, using nonrecursive removal;
partial journals, unexpected entries and reparse points must remain refused.


### Live-content provider delivery

`packaging/search-runtime.json` is the shared source of pinned provider archives,
binary hashes and license inputs. Bash package builders use
`packaging/package-search-runtime.sh` (Python 3 reads the manifest); Windows
PowerShell uses `packaging/package-search-runtime.ps1`. The Linux static-musl
provider lives in the artifact's private libexec directory; Windows places
`rg.exe` beside the app and daemon. The resolver prefers private libexec before a
sibling executable, then uses PATH for development/CI. Required-file checks reject
incomplete Windows installer stages. Notices include the upstream ripgrep texts
and the pinned PCRE2 notice for the included library.

Live queries pass literal arguments with no ripgrep user-config loading, apply
file-size/type/exclusion policy before scanning, and parse NUL-delimited native
paths. Provider start, exit and output errors propagate through SearchResponse;
filename results already found remain available with the error. The public
`search --content` CLI exercises this same query route, including daemon IPC.
Exact-package QA must use a clean PATH, verify content-only roots and folder
scope, and check missing/damaged provider errors without reading operator data.


Content scan coverage is the union of admitted metadata and enabled content roots.
`ScanPlan` retains the separate metadata roots and records metadata membership
in each canonical file row. Filename FTS and streamed queries share that completed
membership rather than rechecking filesystem filters at query time; content-only directories are kept
only as structural records for bounded subtree deltas. Unsupported content-only
files are omitted. Nested scan roots retain most-specific ownership, including a
content-root directory that remains a normal metadata item in its parent scope.
Metadata exclusions do not override explicit content selections; content has its
own exclusions, supported groups and size limit. Disabled/fully excluded content
roots and empty group selections create no content-only watch. Content policy
version 3 and cache schema 10 rebuild prior derived data rather than preserve
older cache layouts. Filename postings bind canonical rowids, so stale postings
cannot rejoin a later entry that reuses a pathname.


## Bounded PLY and Affinity source admission

`ottrin-preview/src/preview_source.rs` owns regular-file admission for both
renderers. Unix uses no-follow, nonblocking opens and inode/device/state checks.
Windows rejects device/named-stream syntax, verifies disk file type, final
reparse/directory/delete-pending state, and captures native volume/128-bit file
identity plus length and creation/modification/change metadata. Reopened path
bindings and the held handle must still match after reads. A read-sharing handle
excludes ordinary competing write/delete opens for the snapshot's lifetime.
This is not a claim of universal writer exclusion or bounded blocking I/O on
network/device failures; broader filesystem and junction-isolation acceptance
remains required. Ancestor directory aliases are permitted and their changed
bindings are rejected, rather than treating an old open handle as current path
identity.

PLY retains its bounded in-memory parse and content-hash generation. Affinity
retains two complete streaming hashes and only a bounded preview suffix; its
second pass now rejects growth beyond the captured size. Neither renderer
returns container bytes as an image or accepts a stale raster after source
replacement. Native admission tests cover ordinary/raw Unicode paths, device
and stream rejection, reparse/directory refusal, sharing, ancestor alias
retargeting and PLY size/parser limits. Unit fixtures embed rights-cleared source
assets into owned temporary files, making Linux-cross-compiled Windows tests
independent of build-machine paths. Full suite results and remaining positive
failures are in `.hermes/evidence/windows-preview-admission-current/`.

Microsoft's [FILE_ID_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info)
defines the identity pair, and [CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
defines handle sharing and final reparse behavior. Native tests establish the
verified scope; source/unit acceptance does not substitute for exact-package GUI
and graphics acceptance.
