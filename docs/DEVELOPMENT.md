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

Publish packages through `packaging/package-lifecycle.sh`. Replacing any Linux
format invalidates `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json`; failed
replacement preserves the previous artifact and evidence. After the new cohort
is verified, replace that current receipt with all four actual hashes, matching
checksum/provenance sidecars and only the checks proved for those exact files.
Keep earlier receipts under their original artifact identities.

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

Activity card titles must wrap within their column while keeping status badges visible and aligned to the same trailing edge across short and long cards. Toolbars must wrap at narrow widths and larger text sizes rather than widening the panel beyond its viewport. Copy recovery cards separate readable status from technical paths, identities and refusal diagnostics; complete diagnostics remain selectable in expandable Recovery details.

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

Hosted verification saves compiled dependency caches even when a later test fails.
Windows first runs the native Move ownership/cleanup/Undo admission filters, then
the complete workspace suite. The focused gate gives prompt failures for this
security boundary; it does not replace broader native/platform acceptance.

Hosted Linux verification explicitly installs Bubblewrap, the systemd user-bus
package and Ubuntu's restrictive AppArmor profile. The CI-only
`scripts/ci/prepare-linux-isolation.sh` starts the runner's real user manager and
checks a bounded namespace-isolated transient service before tests. It refuses
ordinary developer environments and does not disable namespace restrictions.
This provisions disposable CI; it does not install policy on an Ottrin user's
machine or establish default-install Office-provider acceptance.


A configured native probe also exposed a missing `/bin` sandbox mount: `/usr/lib/libreoffice/program/soffice` exists but its `/bin/sh` interpreter cannot execute. The launcher now exposes `/bin` read-only alongside `/usr`, with the existing network, filesystem and resource boundaries intact.

## Build And Verify

Text-edit UI tests use `text_edit_fixture_root()`: Linux needs a disk-backed
filesystem with FIEMAP, while the ordinary Windows actor uses its owned NTFS
temporary root. Do not probe an unrelated `/tmp` filesystem and return early
from an otherwise reported passing test. Unsupported platforms should have
explicit test exclusions; supported-platform fixture failures must remain failures.

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
    libxkbcommon-dev libssl-dev libglib2.0-dev desktop-file-utils \
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

The Windows Kit helper executes load/export/close on the admitted backend module's
own UI thread, using a current-process-only scoped message hook. It does not change
AppContainer capabilities, source snapshots or parent process/resource limits.
UI imports are resolved only during conversion; `--version` must remain usable in
a noninteractive session. `windows_native_office_metadata_probe_is_desktop_independent`
is the ignored installed-provider regression for that boundary. Current native
format/cancellation/cleanup evidence is in
`.hermes/evidence/windows-office-owner-thread-current/ACCEPTANCE.json`.

Keep native preview tests on the workspace's default preview test profile.
Forcing its `rwml` dependency to optimization level zero exceeds GNU PE's export
ordinal limit; the backend-only `ottrin-platform` test profile is a separate case.

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
with macros enabled in an ordinary-user control), plus
`OTTRIN_NATIVE_OFFICE_MACRO_DENIED_CONTROL_PDF` (the same explicit macro challenge
using the production load options). It requires the enabled control's
`OTTRIN MACRO EXECUTED` marker and the denied control's unchanged baseline with
no marker before checking the actual contained production conversion, source
preservation and private-state cleanup. Pin the fixture, helper, controls and
test binary together in the acceptance receipt. Automatic document load events
did not execute in the Kit control, so they cannot establish suppression by
themselves. The paired control explicitly invokes the harmless document macro;
it does not establish automatic event dispatch or expose macro execution in
the production helper. Do not weaken these prerequisites or treat an ignored
test as passed.

The ignored `native_installed_office_preserves_representative_layout` test uses
three owned embedded fixtures documented in
`crates/ottrin-preview/tests/fixtures/OFFICE_FIDELITY.md`. Set
`OTTRIN_NATIVE_OFFICE_FIDELITY_OUTPUT` to a nonexistent directory owned by the QA
actor. It checks five pages through the production isolated provider and saves
PDFs, page PNGs and text there for visual inspection. The actor must collect and
remove those deliberate QA outputs as well as its private temporary directory.
The Windows run and all five page reviews are recorded in
`.hermes/evidence/windows-office-fidelity-current/ACCEPTANCE.json`; this is
representative layout evidence, not arbitrary Office or packaged-UI acceptance.

Windows UI provider detection and conversion use the same scheduling/rendering
path as Linux, with the platform's native isolation launcher. Detection completion
wakes the GUI through the existing event-driven result notifier. A failure for the
current source generation selects Ottrin's native preview and displays an
explanation in both the Miller preview and Quick Inspector; disabling the provider
or replacing the file does not retain that failure notice. Native source-built
release/OpenGL UI evidence is recorded in
`.hermes/evidence/windows-office-ui-current/ACCEPTANCE.json`. This does not establish
exact-package or default-D3D12 acceptance.

The Windows converter uses the packaged `ottrin-office-render.exe` helper with
installed LibreOfficeKit rather than the CLI, whose global named pipe is denied
by AppContainer. The helper reads provider version metadata without starting
LibreOffice; conversion uses the stable C ABI, a disposable profile, batch mode
and explicitly disabled macros. It exports `source.pdf` on both platforms.
Each invocation stages helper bytes inside the private sandbox ACL, so portable
builds need no permanent permission changes. Development/native test builds use
`--features windows-office --bin ottrin-office-render` and set
`OTTRIN_OFFICE_HELPER` to that absolute helper path. Packaged builds discover the
helper beside Ottrin. The Windows UI provider is enabled in source after the
current contained helper and native UI checks described above. Earlier CLI
acceptance predates the AppContainer work and does not establish this boundary.

The historical pre-AppContainer CLI fixture passed macro-free DOC, DOCX, ODT,
XLS, XLSX, XLSM, ODS, PPTX and ODP (`office-formats-final-results.log`, 27.95s).
The pre-owner-thread contained Kit sweep passed DOC/DOCX/ODT and timed out on
the other six formats at 30 seconds each (`windows-20261011/helper-formats.log`).
An uncontained Calc control also timed out; worker/message-pump and SDK runLoop
controls did not convert it and were rejected. The subsequent owner-thread
repair passes all nine formats; current receipts are listed above. The
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

Completed same-volume Replace and cross-volume Move Undo publish a compact
`<transaction>.move-done` receipt in the same registry. Publication follows
verified paired terminal state, uses a private delete-on-close preparation and
no-replace rename, and flushes the registry before success. Its native registry
and file identities plus checksum reject copied or damaged state. Activity and
recovery use this receipt without reopening former payload folders; retained
cross-volume controller and inverse IDs resolve to the same completion. Old
journals are superseded after this handoff; their explicit retirement uses the
same receipt ownership proof.
Receipt format 2 binds their original native file identities, byte counts and
SHA-256 hashes of canonical default-stream data, and the private directory/parent
identities. Version 1 was an internal pre-release format and is not accepted by
the current decoder. The new fields do not authorize deleting unknown streams or
other files; reclamation requires native queue-time admission and worker
revalidation. Completed plan capture preserves the ownership snapshot so receipt
publication never reopens former private folders after admission.
This does not provide volume-independent recovery for an unfinished operation.

`WindowsMoveUndoPlan::capture` is read-only and belongs off the UI thread. It keeps
an immutable session receipt, canonical native namespace paths for scheduling,
and separate visible file pairs for refresh/selection. `execute` strictly pins the
captured directory names for the existing inverse; it never resolves a new queue
footprint at worker time. Drive bindings are checked before admission, after
admission, and again in the worker. Original receipt ownership remains available
to history reconciliation after an attempted Undo.

Same-volume atomic and retained Replace inverses resolve original native parents
rather than adopting a recreated label. Incomplete same-volume Replace recovery
captures those same current namespaces. Paired writers pin the exact private
directory and write through its native final name. Source-claim and regular-file
publication namespace flushes precede their next journal phase. Canonical native
paths supply mutation/scheduling authority; preserving an existing ordinary path
spelling is only a presentation decision. The nested-ancestor fixture exercises
live namespace stability and releases every handle after the inverse, consistent
with the native [directory rename contract](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fsa/87f86c9b-6c2a-4803-84b7-131a74a434fa).
Cross-volume forward recovery, session Undo and retained Replace controllers use
that same native-parent admission. Serialized paths remain immutable historical
labels. Current source, destination and canonical private-child paths come from
the recorded native directory IDs; local journal reads/writes use that original
private directory. A private directory moved independently of its containing
parent is refused. Queue plans reserve the captured names and refuse a further
rename instead of silently resolving a new footprint in the worker.

Cross-volume recovery keeps source/destination parent leases through teardown.
The engine owns the private-directory lease and releases it before taking its
verified DELETE handle; an outer recovery-plan lease must not block its own
teardown. If the canonical private child is absent after SourceRemoved, recovery
also proves the original private native ID is retired on that original volume.
A missing old name alone never authorizes retiring the central journal. A
central-only Published phase re-verifies and flushes the publication and converges
both journals before destructive source cleanup. Empty central-first Rename
intent repair allows the requested-name occupant and leaves normal no-replace
candidate selection to preserve it. Native checks and scope are in
`.hermes/evidence/windows-cross-parent-current/ACCEPTANCE.json`; exact packaged
Activity acceptance remains separate.

**Clean up records** explicitly captures a `WindowsMoveCleanupPlan` off-thread
and carries it through the existing queue's complete path/drive admission. The
worker holds native namespace/metadata leases, validates original journal IDs,
streamed hashes, EAs, link counts and the two known private access states, then
persists an immutable `<transaction>.move-cleaning` intent. Known metadata ACLs
exclude ordinary late writes; `MAXIMUM_ALLOWED` per-object capabilities prevent
child propagation. No payload ACL is modified. Native non-POSIX deletion retains
mapped/open journal protection. Local records and eligible empty directories are
retired before central records, with namespace flushes. Unknown/renamed folders
and directory streams remain, and preserved directory access is restored and
flushed before `<transaction>.move-clean` is published. Immutable intent,
completion and cleanup markers stay in the local registry; arbitrary journal
replacements or marker damage never acquire cleanup authority.

A fresh process admits remaining original controls against the same intent after
interruption. A valid cleanup marker needs no former-volume access. A corrupt
cleanup marker reports only a cleanup-status error; original completed Undo is
still recognized. On NTFS, a stale native file reference returns invalid-parameter
in the owned native tests; classification as retired requires the identical
lookup to succeed for the live same-volume anchor. Other errors refuse cleanup.
Native ID lookup never adopts recreated pathnames. This is not a physical
power-loss or arbitrary-filesystem guarantee.

The canonical native runner adds `--move-retirement`, `--copy-security`, and
`--ui-move-suite` (the last requires `--crate ottrin_ui` and the owned second
volume). `.hermes/evidence/windows-move-retirement-current/run-cases.py` preserves failed runs and
resumes the same observer on timeout. Its `case(ROOT, NAME, ARGUMENTS, ...)`
entry point requires the evidence root explicitly, so later checks do not write
into an older checkpoint. Reused binaries can be pinned using
`--reuse-receipt CASE/TEST_BINARY.json`, which checks the recorded guest hash
without rereading a compiler output that another build might replace.
`run-offline.py --evidence-root ROOT --prefix NAME --binary-receipt CASE/TEST_BINARY.json` exercises both original
completion and cleaned records across detach/reconnect, checking actual volume
attachment and cleaning identity-owned persistent fixtures in its failure path.

The retained native QA harness has `--move-completion-offline` stages
`failure_teardown`, `prepare`, `prepare_retired`, `verify`, and `cleanup`. Each uses a fresh ordinary
actor process with the same exported platform test binary. The fixture deliberately
persists outside actor TEMP between stages. Observe every actor to terminal state;
use `scripts/windows-cross-move-qa/volume.ps1 -Action detach` after preparation,
then `-Action attach` before cleanup. The controller checks the actual attachment
state in its cleanup path, including after verification failure. Preparation owns
its fresh directories until its complete manifest transfers ownership; cleanup
validates native directory IDs and tokens before removing them, and propagates
verification failure after teardown. The full Move suite skips these dependent
stages; run them only through the owned-volume workflow and remove the VHD using
the existing guarded `remove` action afterward.

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

Bundled-font notices come from `crates/ottrin-ui/assets/THIRD_PARTY_NOTICES.md`.
Package recipes copy it unchanged to `third-party-notices/BUNDLED_FONTS.md`
(under `usr/share/doc/ottrin` for AppImage/native Linux). Windows installers
inherit the portable tree. Verify notice bytes in extracted packages and native
MSI payload comparison; a source copy instruction alone is not delivery proof.

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

Use `./build-all.sh --linux-only` to refresh the Linux set together. It stages
the selected formats beside the output directory and completes their build,
helper-policy and checksum checks before replacing published Linux artifacts.
Failed builds or checks preserve the previous Linux files and receipts. Source
snapshots are captured before publication invalidates the old cohort receipt;
temporary stages are removed on exit. Runtime and platform acceptance is recorded
separately after testing the resulting packages.

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

Linux portable, AppImage, `.deb`, and `.rpm` package sources are available. Windows has portable zip, NSIS per-user installer, and per-user MSI package sources. macOS has an unsigned app-bundle ZIP package source. Native Windows MSI construction, administrative extraction and scoped installation/upgrade/repair/removal have recorded acceptance; failed-transaction registration rollback remains unresolved. Native macOS archive construction and platform acceptance are deferred until the Linux/Windows roadmap is complete. See the [current roadmap checkpoint](ROADMAP.md#current-checkpoint-and-next-work) for current artifacts and unfinished work. Exact native package installation and desktop acceptance remain part of the final Linux candidate pass.

## Privileged Helper Model

Privileged operations are per action, never a global root/admin mode.

- The UI requests a typed privileged file operation through `ottrin-platform`.
- The platform crate serializes a request to `ottrin-priv-helper`.
- Linux launches the helper through `pkexec` and a polkit policy.
- Windows launches the helper through the UAC flow.
- The helper validates an allowlist of supported filesystem operations and returns a typed result.

Linux packaging should install:

```sh
cargo build --release -p ottrin-platform --bin ottrin-priv-helper
cargo build --release -p ottrin-preview --bin ottrin-html-render
# The platform helper and HTML helper must be adjacent in the setup payload.
sudo ./target/release/ottrin-priv-helper --install
```

Runtime helper discovery checks explicit override, environment variable, app directory, standard install paths, and finally `PATH`.

## QA Checklist

### Linux

- Retained Move recovery must carry the `LinuxMoveRecoveryPlan` captured by the
  background Activity scan into the queue and worker. `capture`/`capture_at` read
  the anchored registry and paired journals without creating state; `paths()`
  supplies all affected directories for conflict and drive admission. Execution
  revalidates exact journal bytes, native directory identities, watches and unique
  mount IDs under the existing recovery lock. A stale plan requires a refresh.
  Keep plan lifetimes free of directory handles so ordinary unmount remains
  possible. Native plan checks do not accept arbitrary pathname races inside the
  existing recovery engine or replace exact-package GUI/device acceptance.
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
- Admission for large batches must use component-prefix indexes rather than a cross-product of path pairs. Read-only claims may overlap; writers must still exclude ancestors and descendants. Keep the index local to one check, and verify lexical semantics separately from native filesystem authority.
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

## Linux pathname Undo identity

A device/inode pair alone is not entry-lifetime evidence: local filesystems can
reuse an inode immediately after unlink, including for symlinks. Session pathname
Undo binds the filesystem generation handle captured through a short-lived
`O_PATH | O_NOFOLLOW` descriptor. Renames and in-place edits retain that identity;
a recreated entry does not. The proof retains bytes, not an open filesystem
handle. Missing generation evidence cannot authorize a pathname inverse. Chmod
has its separate retained-descriptor protection.

Copy journal schema 5 stores generation inside each pathname identity rather
than a second parent/transaction handle field. Older schemas are refused without
mutation or a compatibility reader; their user data and retained records stay in
place for review. Restoring an interrupted Undo binds the verified current
published/withdrawn topology before issuing the session receipt. Content checks
and mount/directory ownership guards still apply independently.

A fresh directory-watch generation may replace an invalidated registration;
clones of the old proof stay invalid. Dropping an old proof must never remove the
new generation's watch.

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
been reclaimed. Native Windows has journaled completion evidence and a private creation-Undo
implementation. Public Copy receipts and UI integration remain gated;
cross-compilation does not establish Windows Copy acceptance.

Native backend correctness tests use the workspace `target/` and the canonical
per-package test profile in `Cargo.toml` (platform/UI optimization and debug data
both disabled): `cargo test -p ottrin-platform --target x86_64-pc-windows-gnu
--no-run --message-format=json`. Hosted test jobs limit compilation to two workers
to avoid simultaneous large LLVM jobs exhausting Windows runner memory. Keep the offline
Windows QA VM stopped during compilation on memory-constrained hosts, then start
it for native execution. Optimized resource measurements are a separate scope;
do not spend an optimized backend test build on ordinary correctness fixtures.

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
The schema-6 codec returns the append
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

The format is `OTCPWJ12` / schema 12; superseded schemas are refused, without a
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

Private Windows creation Undo appends paired `UndoIntent` records before a
no-replace retraction to the original destination-local candidate name, then
flushes the root and parent and verifies the full retained tree before appending
`Undone` with the root's post-retraction native version. It never reads or mutates
the original source, deletes data, overwrites an occupied name or changes final
permissions. Retained data keeps its verified final permissions; it is not a
secure erase or automatic storage reclamation. The original completion versions
remain immutable; only the identity-resolved retained root may have the inverse
rename's change time, and a paired root readonly intent may authorize its temporary
bit difference. The `Undone` acknowledgement freezes the new root version.

The same held-tree verifier serves inspection and the explicit writer epoch.
Read-only inspection releases its locks before writer acquisition; paired bytes
are compared again, the typed root capability is acquired, and the whole tree is
revalidated. Native Windows refuses directory rename with open descendant files,
so readers close for the narrow rename handoff. Reopen and verify every frozen
child version, bytes, membership and freshly derived permissions afterward.
Unexpected changes refuse completion and retain all data; this is a retention
contract, not atomic exclusion of every descendant writer. The error includes the
retained candidate path. Never mark such a transaction complete or reclaim its
retained data merely because the published name is absent.

Recovery resolves target/candidate ownership by full native identity. An occupied
candidate is preserved, and an unrelated new publication after retraction is left
untouched. Mismatched journal mirrors remain unchanged/refused. Completed Undo
reopens read-only and adds no records; forward publication and completed-copy
receipt APIs refuse transactions that have entered Undo. Cancellation after the
handoff leaves explicit recovery work. Overwrite restoration, automatic cleanup,
public receipts, Activity/clipboard/Tandem integration and full platform acceptance
remain required.

Native rename constraints:
https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information

Overwrite's existing-destination evidence now uses `PinnedTree::freeze_existing`
and `validate_existing`. This shares source discovery, no-follow binding checks,
backup-stream reading and native security capture with staging. It records raw
names, versions, stream hashes, compression, original link counts and an
owner/group/DACL digest; original hard links are observed without the new-copy
single-link restriction. Revalidation performs no writes or normalization.
Six drift cases remain refused after cached nonidentity metadata is refreshed;
cancellation and entry-budget failures preserve the original graph. Native
receipt: `.hermes/evidence/windows-copy-existing-tree-current/ACCEPTANCE.json`.
The observations alone grant no mutation authority. Paired original-retention
records, handoff/root binding, overwrite publication/restoration and complete
audit-SACL/label handling are still required. The observation alone grants no
retention authority.

Schema 10 binds the initial destination decision in the paired begin headers.
`DurableStage::create` takes a `DestinationAdmission`; its creation constructor
requires native absence, while overwrite captures a complete original graph
before source staging. The token binds parent identity/location and target leaf,
retains no handles, and freezes the original root version for cheap pre-staging
checks. The full original count/digest survives in the immutable journal header.
Original preparation must match that approval before appending any records, and
the decoder rejects original-plan begins that disagree with it. Creation cannot
be converted into overwrite, and an approved overwrite cannot publish without
its original merely because the old name became vacant. Existing source-free
original preparation/recovery uses the same persisted initial proof.
Native evidence: `.hermes/evidence/windows-copy-destination-admission-current/ACCEPTANCE.json`.
This is the engine admission contract; public user-conflict/session/batch wiring
and the remaining security/storage/package gates still apply.

Schema 7 now records original-destination preparation in both journals: a begin
count/digest, ordered per-entry evidence and a completion seal. The begin digest
binds the whole observed tree, including unwritten children of an interrupted
prefix. Source/candidate aliases and intersections with journal ownership refuse
before append. Recording uses the held parent/target binding and revalidates the
live original before sealing; source-free prefixes can resume without rebasing.
Sealed replay adds no records, and local-only tails are never repaired.
Partial or sealed original evidence blocks creation-only publication/admission.
That preparation-only checkpoint did not move originals. Receipt:
`.hermes/evidence/windows-copy-original-plan-current/ACCEPTANCE.json`.

Schema 8 adds explicit original retention and pre-publication restoration. One
`Journal::move_original` engine serves both directions, with paired intent before
no-replace rename and a frozen root-version acknowledgement after root/parent
flush and full tree revalidation. Root location is resolved by identity at the
public and `.original` sibling slots, never by name presence. A pending rename
allows only its root change time; completed replay remains exact and adds no
records. Descendant readers close for writer handoff and native rename, then the
entire original is checked again. The final held tree survives acknowledgement.

Retention checks/pins the new private copy before moving an original that is
still public. If an interrupted rename already retained the original, finishing
its acknowledgement remains possible when the private copy changed; verified
restoration can then return the original while preserving that changed copy.
Space for pre-publication restoration is reserved before retention. Readonly
original file roots refuse without attribute clearing. At that checkpoint,
publication over the retained original and Undo after new-copy publication were
not implemented. Native evidence:
`.hermes/evidence/windows-copy-original-move-current/ACCEPTANCE.json`.

Schema 9 extends the existing publication, completion and Undo engines to retained
originals. Original evidence is revalidated and held through publication and
permission application. Forward and Undo admissions include its digest and expose
its retention slot to conflict detection. No-replace publication refuses unrelated
occupants. Undo first acknowledges withdrawal of the new copy, then restores the
original through `move_original`; it stays pending until both phases are durable.
Restart can finish original restoration without sources, even after private
replacement drift; discovery reports original-only verification and Activity
admits that remaining inverse. Completed replay remains read-only.
Full application/finalization/withdrawal/restoration journal space is
reserved before original retention. The native space regression lowers a
test-only reservation ceiling while leaving the real journal bytes/lengths valid;
it verifies both pre-retention refusal and complete publication/Undo within the
full reserved amount. Production keeps the 64 MiB ceiling. Readonly-original rights, complete audit/label
protection, initial admission, session/batch integration, cleanup, power-cut and
exact-package acceptance remain required. Public Windows Copy remains disabled.
Native evidence: `.hermes/evidence/windows-copy-overwrite-current/ACCEPTANCE.json`.

Forward recovery admission handles verified unfinished Windows creation and
overwrite transactions without reopening their sources. Overwrite requires a
sealed original graph and a durable retention intent; observation alone grants
no mutation authority. A pending retention acknowledgement resumes through the
existing original-move engine after read-only admission, paired-byte comparison
and fresh writer acquisition, then reacquires/revalidates both trees before the
shared publication engine runs. Publication itself still requires the original
retention acknowledgement.

Admission binds the staged/original graphs and destination policy. If the original
is already at its retention slot, its observed root version is frozen even when
the acknowledgement is pending. A root still public has its pre-move version
bound by original evidence; its native post-rename change time is acknowledged
by the existing protocol. Discovery/Activity expose `RetainingOriginal` as a
resumable phase. Cancellation leaves the same admitted action retryable; changed
originals, copies, recorded versions or restoration state refuse without writes.
Native receipt: `.hermes/evidence/windows-copy-retention-resume-current/ACCEPTANCE.json`. It captures the native header/registry IDs,
actual parent location, a digest of the frozen node/source/candidate evidence
and a digest of the freshly projected destination security. No handles or
per-entry security trees survive admission.
Execution reacquires permitted parent access by ID and checks the binding inside
the retained-tree epoch before the shared publication engine obtains its journal
writer. The existing paired byte comparison protects the reader-to-writer gap.
Fresh coherent journal contents cannot replace an earlier admission's frozen
proof; changed data, paths, registry objects or destination policy are refused.

Live publication and admitted resume now share `PermissionStage::from_retained`
and one security-preparation function. Forward replay after completion verifies
read-only evidence and never enters the Copy journal writer epoch. A forward plan
cannot republish an inverse-owned or undone copy. Publication/permission retries
keep their original binding; cancellation before mutation retains exact journals.
Unverified staging is not admitted. The public `RetainedCopyResumePlan` is opaque;
its factory freshly admits one transaction by ID. Activity offers **Resume copy**
only after that admission succeeds for a verified unfinished transaction, and
rejects a plan for another row. Its background worker revalidates before mutation,
uses the shared Activity overlap check for its observed candidate/publication
paths, supports cooperative **Stop recovery**, and requests fresh discovery
after success, failure or cancellation. Only fresh completion admission offers
Undo. Failures remain visible after discovery; Clear finished retains the owner.
Stop immediately changes to a disabled **Stopping…** control until the worker
acknowledges the request. The native integration test holds the real worker before
its first platform callback (test builds only), clicks Stop, verifies exact
central/local journal bytes and payload preservation, then retries and Undoes the
copy. All nine Activity scenarios pass with both one-volume storage and a
separate NTFS data volume, using the same pinned executables. The producer checks
distinct native volume serial numbers in every two-volume case. Owned VHDX
permissions are established before attachment; production ACLs are not changed.
Receipt: `.hermes/evidence/windows-copy-resume-volume-current/ACCEPTANCE.json`.
Link/specialized-storage and interruption matrices remain required. Public new
Copy, Overwrite and cleanup are still gated.

Native finalization recovery is also covered after the last permission
acknowledgement and after the first final-version record, for plain files,
populated folders, readonly file roots and readonly children. The source is
removed before recovery. Each case resumes the originally admitted plan after
an injected central-journal flush failure, verifies the completion seal and
checks exact payload bytes and persistent metadata against the interrupted
state. Finalization needs journal writes but must not mutate the payload.

After a held cleanup writer commits control records, it must finish validating
and returning its `ControlChange` before another cancellation callback. The group
records that exact writer epoch before checking Stop and before removing payload.
An interrupted acknowledgement must not make the next retry mistake our own
journal append for an external catalog change. Filesystem, binding, security and
content validation errors still propagate; this is not permission to reopen an ID
and adopt whatever is there.

Native Stop is a request, not proof that pending Windows I/O has ended. The
operation stays **Stopping…** and retains its path/worker ownership until the
native request completes and the worker acknowledges cancellation. Do not free
buffers, close operation state or report completion merely because a cancellation
API returned. Some driver requests complete normally despite cancellation; the
ordinary callbacks must still stop subsequent work. See
[Microsoft's cancellation considerations](https://learn.microsoft.com/en-us/windows/win32/fileio/canceling-pending-i-o-operations).
Explicit `throttled_io` tests require an owned secondary NTFS disk, a responsive
control directory on another disk and a host controller that always removes its
throttle. They stop actual Copy/Undo/cleanup journal I/O, verify retained bytes,
and retry without the source. The control scripts and precise acceptance scope
live under `.hermes/evidence/windows-throttled-journal-current/`.

The explicit `disk_full` tests use the same token-bound small NTFS volume and the
actual held journal writer handles. They preserve journal bytes while controlling
spare allocation, fill only the owned volume, require a native disk-full error,
and release the filler by its retained handle before recovery or assertion cleanup.
Matched complete journals must resume source-free. Divergent journals must be
refused byte-for-byte by both fresh and retained plans. An allocation set through
a closed setup handle is not proof that the active writer still has that reserve;
the fixture asserts its requested mirror state rather than accepting either result.

The shared storage fixture provisioner accepts explicit `Serial` and `Token`
PowerShell parameters. Its host teardown requires `--evidence-dir` and `--case`,
checks the exact dispatch/executable even for a failed native run, validates the
original guest definition and volume dependencies, and retires only the owned disk
through libvirt. The current disk-full receipt and driver are under
`.hermes/evidence/windows-full-disk-current/`.

The shared `run-case.py` driver requires `--evidence-dir PATH` with the matching
ownership and actual-volume token receipts. It runs all five explicit disk-full
cases on one executable. The separate-volume cases place either destination or
registry on the owned full disk and the other on the ordinary actor's temp volume;
native volume serial numbers must differ. Only the owned disk may be filled.
Destination-full must preserve a matching pair and recover after space is freed;
registry-full must preserve and refuse its divergent pair on fresh and retained
retries. Current four-case acceptance is in
`.hermes/evidence/windows-split-full-disk-current/ACCEPTANCE.json`. This covers
forward journal writes after staging, not initial payload, Undo or cleanup writes.

The fifth case fills the owned destination after the first primary-stream data
callback. Windows may already have reserved that stream's allocation, so a second
named stream is required to force a subsequent payload allocation failure. Require
the actual native disk-full error, unchanged primary and named source bytes,
retained incomplete candidate and paired journals, no public destination, and
byte-preserving refusal by fresh resume and direct publication. Filling the disk
alone is insufficient proof of a failed write. The exact native payload receipt
and rejected first fixture are under
`.hermes/evidence/windows-payload-full-current/`. This adds one payload failure
scope; it does not establish automatic recovery or cleanup of incomplete data.

Windows recovery admission now binds the authenticated registry/header,
sealed immutable completion digest and actual parent location before creation
Undo. It retains no file handles, does not read the source and does not modify
journals at admission. Execution reopens the registry and destination through
native identities, acquires the parent's normal permitted writer access only at
execution (read access cannot flush a directory), checks the captured binding
inside the retained-tree epoch,
then uses the same paired journal and Undo writer protocol as the backend.
A moved display path requires fresh admission even if its original object is
still found by ID; a new occupant at the old name is never adopted. Changed
payloads, replaced registries and journals refuse before intent. Cancellation
keeps the plan pending; an inverse interrupted after rename can retry against
the original completion binding. The platform exposes this through the opaque `RetainedCopyUndoPlan` and fresh
`restore_retained_copy_undo_plan[_at]` admission. No mutation authority is derived
from public discovery fields.

Windows central Copy discovery is exposed through the platform's read-only
`discover_retained_copy_transactions[_at]` facade, with a cancellable explicit-root
variant for background workers. It never creates a missing registry, repairs a
journal, grants an Undo token or keeps file handles after returning. Public status
fields are presentation only. Phase describes the authenticated central journal;
`VerifiedTree`, `UnverifiedStage` or `NeedsReview` separately describes current
evidence. The identity-bound root location is distinct from the possible
publication and retention paths, and remains an observation rather than action
authority. Callback byte counts describe the currently inspected tree and can
reset; callback errors cancel the entire discovery rather than becoming row errors.

Only canonical transaction filenames are inspected. Each ordinary non-reparse,
single-link central file must match its recorded native ID, registry ID, current
creator and private descriptor. Parent resolution uses native volume/file IDs,
not the cached parent path: open by ID from the registry volume or from a bounded
local-volume-manager enumeration, using ordinary access checks without enabling
privileges. NTFS IDs use the full admitted 64-bit value without truncating nonzero
upper bytes; ReFS uses its 128-bit descriptor. The opened directory must match the
complete recorded native identity. Missing/offline/unreadable parents stay review
rows; no cached-path or network-hint fallback is attempted.

Fresh paired journal locks, bytes, namespace, payload, final versions and native
security are then checked through the existing retained-tree verifier. A staged
permission-plan prefix/seal is freshly projected even before DACL application.
Corrupt, linked, busy, mismatched or changed entries remain visible as individual
review rows instead of hiding other records. Incomplete payload staging is shown
as unverified, with no claim that it can be resumed. UI worker integration, action
admission and native package acceptance remain separate required work.

Windows Activity uses the existing asynchronous history loader. Only verified
completed creations or interrupted creation inverses attempt fresh admission.
The resulting plan supplies the displayed phase/location and execution receipt;
a plan for another transaction is rejected. A verified tree alone cannot enable
Undo. Incomplete or changed copies remain owned review rows; a verified completed
inverse shows as undone. Cleanup and public Windows Copy/Overwrite stay gated.
The existing operation worker and path-conflict checks execute the plan, retain
retry progress and refresh status after refusal. The plan revalidates its native
binding before writing. Retained entries survive Clear finished, including after
Undo while their data and bookkeeping remain owned.

Windows session Copy receipts also carry a freshly admitted opaque native Undo
plan. `DurableStage::execute_with_receipt` is the shared forward orchestration;
`DefaultPlatform::undo_copy_with_receipt` reopens persisted evidence before the
inverse, including on cloned-receipt replay. Activity obtains conflict paths and
transaction IDs from receipt methods, preserving the displaced-original
reservation without duplicating the transaction in restored history. Native
receipt tests cover source removal, creation/file/folder Overwrite, changed
publication refusal and late committed receipt failure. Public forward routing
and complete session/batch workflow acceptance remain separate gates.

The internal Windows path dispatcher takes a caller-owned private registry
handle and supplies native destination admission to that orchestration. Its
`windows_native_copy_dispatch` tests exercise actual paths for file/directory
creation, Skip/Rename/Overwrite, dangling-name collisions, source/registry
ancestry and hard-link guards, and destination changes during staging. Changes
injected for Overwrite must wait until staging has a journal: admission itself
reads the original under a lock. The configured entry point now provisions and
admits its registry through the external native-identity anchor, with source
overlap checked before setup. Explicit-registry fixtures supply their own
handle; configured Activity fixtures provision the managed root before staging.
Neither entry point enables public workflows yet.

Native Activity integration uses an ignored platform fixture producer and a UI
consumer. The producer only accepts a root/profile owned by that consumer. The
consumer creates separate child processes and private profiles, renders Activity,
clicks its actual Undo/Resume action and verifies exact data/history outcomes for
normal Undo, changed payload, moved destination and interrupted inverse. A forged
row cannot adopt another transaction's valid receipt. Run in an owned guest with
`OTTRIN_TEST_WINDOWS_PLATFORM_FIXTURE_EXE` pointing to the same-source platform test
binary, selecting `tests::windows_copy_recovery_activity_integration --ignored`
in the UI test binary. The consumer owns fixture teardown; the native controller
also watches and terminates its owned process tree on timeout before removing its
private temp namespace. Set `OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_VOLUME` only to an
owned secondary NTFS fixture directory to run the same consumer across volumes.
The consumer creates and owns a temporary destination there for each scenario;
the producer requires different native volume serials for registry and data.
Destination moves stay on that destination volume. The controller verifies the
owned disk-image/drive association, scopes user write access to its fixture
folder, checks that folder is empty after the consumer, then detaches and removes
the owned image. For a VHDX, grant the test user Full Control on that owned backing
file before attachment too. Modify-only backing access can deny WRITE_DAC and
WRITE_OWNER operations on otherwise Full-Control inner files. The native
cross-volume run reproduced that denial and passed with unchanged binaries after
the owned backing-file grant and detach/reattach; production ACL code was unchanged. This accepts the source UI/worker path on owned NTFS volumes,
not a packaged native window or physical/removable-device interaction.

Native ID API contract:
https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-openfilebyid

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


## Native Windows incomplete Copy staging-end checks

`RetainedCopyAbortPlan` ends only an unverified canonical staging attempt. It
captures private controls at read-only admission, rechecks them as an exclusive
writer, writes the paired terminal `StagingAborted` event, then uses ordinary
claim release. It never opens or deletes the payload or source. Activity requires
explicit confirmation and retains the originally reviewed plan across refresh.
Schema 18 preserves older records for review without converting them.

Focused native tests are in `journal/tests/abort.rs`. They retain edited partial
files, an added named stream and an unknown file, deny publication after ending,
refuse live/stale/verified controls and protect a later durable stage's reservation.
One-sided journal failure stays reserved; a complete paired terminal prefix can
finish through the normal name-release action. The native Activity integration
also exercises cancel, confirm and changed controls through rendered clicks.
The canonical QA runner exports and verifies its current platform fixture rather
than relying on an executable left by an earlier task; teardown removes that
fixture after checking its hash. Receipts and limits belong in
`.hermes/evidence/windows-copy-abort-current/ACCEPTANCE.json`.

Ended attempts must not block unrelated work solely because their payloads were
never verified. `State::dependency_claims_complete` is shared by Overwrite and
cleanup admission. A terminal partial graph is closed only with its exact paired
journal proof; recorded creation identities remain dependency claims. The shared
`RegisteredJournal::paired_leases` reader pins both controls and their native
bindings. Cleanup retains those leases for the held dependency scope and rereads
fresh pairs at writer handoff; it never opens the partial payload. The native
`abort_dependencies` fixtures exercise unrelated dispatch and actual cleanup,
recorded hard-link overlap, live controls and divergent local mirrors. Evidence:
`.hermes/evidence/windows-ended-dependencies-current/ACCEPTANCE.json`.

This action does not reclaim incomplete payloads, remove their recovery records,
complete resource/concurrency/workflow acceptance, or enable public Windows Copy.

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

A late-failure fixture must prove the queued operation script ran before the
failure. An immediate custom action before InstallExecute tests a different
boundary from a deferred in-script failure, even when its sequence number is
nearer to PublishProduct. The current generated InstallExecute is conditional on
`NOT Installed`; a fault inserted after it during uninstall must not be accepted
as post-execution without native log evidence of the actual file/registration
operations. The read-only upstream-package comparison and exact table hashes
are in `.hermes/evidence/msi-authoring-comparison-current/COMPARISON.json`.


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

## Native Office cancellation readiness

A provider-start cancellation test must observe the real private profile lock
within the request's child timeout, retaining a cancellation margin. Do not
replace that observation with an assumed startup delay. A cancellation result
alone does not prove cancellation happened after provider startup. The current
test records readiness duration and budget and still requires source preservation
and private-process/profile cleanup. Package source and test-runner identities
are separate when only acceptance code changes after packaging.

## Windows Copy directory case policy

Read the actual directory policy through the held parent handle with
[NtQueryInformationFile / FileCaseSensitiveInformation](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntqueryinformationfile).
Do not infer it from Windows defaults or current filename occupancy. The
immutable transaction header retains the admitted policy; native writer and
recovery validation require it again. Unsupported or unknown flags do not grant
admission. This is one prerequisite for durable native target reservations.

The normal-user native fixture sets and queries the actual flag. Empty-directory
transitions exercise both directions. Staging/publication fixtures start with
case sensitivity enabled and disable it only with unambiguous names; they do not
try to re-enable it on populated directories. Windows imposes
[directory-state and permission constraints](https://learn.microsoft.com/en-us/windows/wsl/case-sensitivity)
on those transitions. A fixture setup refusal is a failed run, never a passing
policy test or an implicit skip. Sources and journal/publication bytes must remain
unchanged when authority is refused.

### Linux HTML sandbox policy

HTML rendering uses the installed, root-owned `ottrin-html-render` fixed-protocol
entry, the shared Bubblewrap filesystem/network namespace, Chromium's own
sandbox, and a bounded user-systemd unit. It accepts only a private sanitized
HTML workspace and image dimensions. Setup never changes global AppArmor or
user-namespace sysctls, and never adds permissions to the generic Bubblewrap
profile. Caller-controlled executable mappings are denied under the rendering
profile; the hosted check proves its preload attack works before testing denial.

The separate OS policy `kernel.apparmor_restrict_unprivileged_unconfined`
controls whether an already-unconfined process can change its own AppArmor
profile. Ottrin cannot promise to prevent that system-wide operation when the
host permits it. Hosted evidence records the setting and direct-transition
result; on hosts where the setting is 1, the transition must be denied. This
policy distinction does not disable Chromium's sandbox or allow original HTML
scripts, network access or arbitrary helper commands.
