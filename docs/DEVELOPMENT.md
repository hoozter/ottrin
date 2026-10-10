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

## Build And Verify

```sh
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
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

### Windows Office process boundary

Run the native Job Object fixtures on Windows as the ordinary test user:

```powershell
cargo test -p ottrin-preview --test windows_job_native native_job_ -- --test-threads=1
```

The fixtures exercise the actual launcher with disposable child processes and
private temporary files. The ignored `fixture_root` and `fixture_descendant`
entries are subprocess entrypoints selected by the tests, not independent tests
to invoke manually. They cover output and diagnostic cleanup, timeout after the
parent exits, cancellation, process and memory limits, aggregate descendant
memory, and restriction of inherited handles. They do not certify an installed
LibreOffice conversion, its profile cleanup, or provider fallback presentation.

Linux can build the fixture executable with
`cargo test -p ottrin-preview --target x86_64-pc-windows-gnu --test windows_job_native --no-run`.
Cross-compilation is not native acceptance. Keep the Windows Office provider
gated until its remaining installed-provider checks pass.

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

Cross-volume fixtures are separately ignored by default. They require a real
second Windows volume in `OTTRIN_WINDOWS_CROSS_VOLUME_ROOT`, an isolated recovery
root, and `OTTRIN_TEST_WINDOWS_CROSS_VOLUME_MOVE=1`. That variable opens only the
test build gate; the product route stays closed pending native safety acceptance.

```powershell
$env:OTTRIN_WINDOWS_CROSS_VOLUME_ROOT = "Q:\owned-ottrin-fixtures"
$env:OTTRIN_TEST_WINDOWS_CROSS_VOLUME_MOVE = "1"
cargo test -p ottrin-platform windows_native_cross_volume -- --ignored --test-threads=1
```

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
Explicit cross-volume recovery and discovery use the same safety gate as ordinary
Move retry. Native lifecycle evidence covers safe refusal, recovery after protection
removal, repeated completed recovery preserving retained originals and later user
edits, malformed/misnamed journal refusal, and a separate process with the normal
gate closed. Native cross-process exclusion also passes: an independently running
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
.\packaging\windows\package-installer.ps1 -Stage .\dist\Ottrin-0.1.0-windows-x64
```

Windows MSI from the same extracted portable payload (requires WiX Toolset 5):

```sh
./packaging/windows/package-msi.sh ./dist/Ottrin-0.1.0-windows-x64
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
- Create, rename, copy, trash delete, and permanent delete; verify Linux same-filesystem and cross-filesystem Move with Skip/Rename/Overwrite/Cancel and explicit dispositions. Cross-filesystem Move must stage and verify the complete destination tree before publication, revalidate before guarded source cleanup, retain explicit recovery evidence on uncertainty, and offer Undo only from identity/content-bound single-entry evidence. On Windows, extend the native same-volume Skip/Rename/Overwrite fixture evidence with crash, adverse ACL/sharing, retained-transaction lifecycle and packaged checks. Keep cross-volume Move fail-closed until its staged-copy and native safety gates pass; cross-compilation alone is not acceptance.
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
