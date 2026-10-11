# Windows text-save boundary

The Windows save implementation in `crates/ottrin-platform/src/text_edit/windows.rs`
is enabled for admitted local NTFS files. Native public-session and desktop editor
checks pass; focused exact portable-package save/conflict/reload acceptance is recorded below. There is no
environment override. Linux publication and the editor's conflict/reload/Undo
behavior are unchanged. macOS remains read-only. No additional document formats are introduced.

## Current implementation

The native candidate admits bounded UTF-8 text through the existing session API.
It opens a local NTFS parent directory and performs child opens, creation, source
withdrawal, publication and restoration relative to handles. Source/stage reparse
points, hard links, alternate streams, extended attributes, object IDs, unsupported file
attributes, binary controls and files over 128 KiB are refused. A source handle
is held during save with write/delete sharing denied; it is not held while the
user edits. An incompatible existing writer therefore stops saving before staging.

Source identity, content, creation/write/change times and owner/group/DACL are
compared before withdrawal. Files with any SACL are refused before staging,
including audit rules, mandatory labels, resource attributes, scoped policy and
present-but-empty SACLs. The label-security query preserves the SACL-present flag
without requiring audit privilege. Its visible ACE subset cannot establish that
unreadable audit entries are absent, so the candidate never reconstructs a SACL.
The same refusal applies when metadata changes after opening.

The publication file is created beside the source using the captured security
descriptor. `NtCreateFile` does not preserve DACL automatic-inheritance state;
candidate preparation applies the source DACL and protection state through
handle-based `SetSecurityInfo`. Only that owned publication handle requests
`WRITE_DAC`. Its captured security, creation time and attributes must still match
exactly before the original is moved. No merge-error-ignore mode is used.

Each changed save creates an owner-only `.ottrin-text-edit-<random>` directory.
`edits` is an independent flushed draft. A unique sibling ending in `-publication`
holds the staged publication with the source security. Both directories are flushed
before source withdrawal. A no-replace handle rename moves the original to
`candidate`, followed by revalidation and directory flushes. Only then does a
second no-replace handle rename publish the edited file. Content/identity/security
are checked again and publication/directory flush failures stay visible.

Native ACL evaluation also verifies that the actual recovery directory and draft
deny read access to a restricted token, while a positive-control ACL permits that
same token and the owning user can read the draft (`text-privacy.log`). This does
not claim protection from the owner or privileged accounts. The retained original
keeps its original security descriptor.

This is **not an atomic exchange**: the source name is temporarily vacant. If
another process occupies it, publication and restoration refuse replacement.
Recovery attempts a no-replace hard link from the retained original to the vacant
source name, leaving `candidate` intact. Restoration that creates a second link
also makes that file read-only to this editor until the link topology is resolved
manually. A failure after attempting withdrawal returns the existing `RecoveryConflict`,
which keeps the buffer and recovery details visible, suppresses a completed-save
Undo receipt, and disables further saves until explicit reload. Staging failures
retain their paths in `StageRetentionRequired`. Nothing automatically removes
recovery files or unpublished siblings, including after successful saves.

## Evidence and native acceptance boundary

Portable tests in `windows_contract.rs` exercise publication order, failure at
every boundary, successful/failed restoration and collision retention. Windows-only fixtures exercise retained originals/drafts, byte and
security preservation, unchanged saves, external edits, sharing refusal,
occupied names, post-publication flush failure, hard links, alternate streams,
invalid/oversized text and public-session saving/conflict refusal. Most fixtures
call the private native session to exercise failure hooks; the public-session
fixture and desktop checks exercise the ordinary entry point.

Native ordinary-user execution on Windows 11 build 26100 passed all ten text-save
fixtures on 2026-10-10, including inherited, protected and explicit DACLs. The
original run refused publication because candidate/source security descriptors
differed in the automatic-inheritance flag. After fixing candidate ACL preparation,
byte-for-byte descriptor equality, retained original/draft data, directory flushes,
collision/failure retention and closed-gate checks pass. Evidence is in
`.hermes/evidence/windows-20261010/text-dacl-final.log`; the full default native
platform suite passed 96 tests (17 cross-volume fixtures are separately ignored).
Native Unicode-name saves and eight process-exit boundaries also pass
(`text-path-exit.log`, 12 text fixtures total). The opened directory blocked an
ancestor rename with native access-denied; the subsequent save preserved the
original and draft. Successful ancestor relocation is not claimed on this host.
Exits before/after withdrawal, publication and restoration leave the exact original
identity/security and independent draft accessible. A native writable-mapping
fixture closes its creating file handle, then confirms sharing-violation refusal
before staging. Unmapping allows ordinary retry if unchanged; mapped edits require
reload and remain in the retained original after saving (`text-mapping-final.log`). This is private-session
acceptance, not packaged editor or power-loss proof.

Run on a supported Windows host with `%TEMP%` on local NTFS:

```powershell
cargo test -p ottrin-platform text_edit -- --nocapture
```

Linux can compile and link those Windows fixtures with:

```sh
cargo test -p ottrin-platform --target x86_64-pc-windows-gnu --no-run
cargo check --workspace --target x86_64-pc-windows-gnu
cargo test -p ottrin-platform text_edit
cargo test -p ottrin-ui text_
cargo test -p ottrin-ui recovery_conflict
```

Cross-compilation and portable tests do not establish native Windows acceptance.
Extend the ordinary/custom ACL, directory flush and sharing evidence with exact
packaged editor behavior before claiming package acceptance. Recovery-directory and
draft ACL privacy have the native restricted-token evidence above.
Unicode paths, ancestor-rename refusal, publication-step exits and the tested
writable-mapping lifecycle have native evidence; other mapping/relocation behavior
is not implied. Object IDs are queried through the pinned handle and refused before
staging; native tests cover preexisting IDs and IDs added after opening, preserving
the exact ID and source bytes.

The initial resource-attribute preservation path was unsafe: an ordinary-user save
of a separately seeded audit-plus-resource SACL retained the resource attribute
but dropped the audit rule (`audit-mixed-after-save.log`). The partial descriptor
comparison missed that loss. The replacement refuses every present SACL rather
than copying its readable subset. `text-sacl-red.log` proves the earlier admission
bug; `text-sacl-mixed-green.log` and `text-sacl-audit-green.log` prove ordinary-user
refusal of actual mixed and audit-only fixtures with unchanged bytes, no new stage
and independent before/after full-SACL inspection. Fixture preparation/inspection
used the existing disposable guest agent's process-local security privilege; Ottrin
ran as the ordinary user. No global audit policy or service was changed.

All 17 ordinary native text tests pass, including resource-attribute refusal both
before and after session opening. The separately seeded audit test is ignored in
an unprepared suite and was run explicitly for both fixtures. The scoped-policy
native descriptor test proves preparation refusal. An actual stored scoped-policy
ACE (`S:AI(SP;;;;;S-1-17-1)`) is also refused by the ordinary-user candidate with
unchanged bytes, no new stage and unchanged full SACL (`scope-native-seed.log`,
`text-sacl-scope-green.log`). This is metadata refusal, not domain policy enforcement.
A separate native extended-attribute fixture passes admission and post-open refusal,
with exact EA bytes and source content retained (`text-ea.log`). These checks close
those native admission cases; the focused packaged editor workflow is recorded below.
Unsupported security cloning or directory flushing stops the save and retains existing staging.

## Public editor acceptance

The public-session regression failed with `NotAvailable` before enablement
(`text-public-red.log`). The authoritative platform capability now includes
Windows; the obsolete public refusal branch and closed-gate assertion are removed.
All 22 native text checks pass, with the separately prepared SACL fixture ignored
in that suite (`text-public-green.log`). Strict Windows platform Clippy passes.

The source-built Windows executable `ottrin-app-editor.exe` (SHA-256
`a5f3107f03cad8a0fabfc316d1d343a0e8a061618df281e7bacb32d2b2c5aeb3`)
ran as the ordinary user in the owned Windows 11 guest with separate application
settings and the native renderer. Quick Inspector saved Markdown to exact bytes,
retained the original and independent draft, refused an external change while
keeping the buffer, supported Keep editing, and loaded the external version only
after explicit discard confirmation. An ordinary-user filesystem check verified
all three retained/current versions (`app-editor-retention.log`). Screenshots and
logs under `.hermes/evidence/windows-20261010/app-editor-*` record these checks.
The application closed after the clean reload. No codec, renderer or service was
installed. This is source-built desktop acceptance, not exact portable-package,
installer or power-loss acceptance.

The tested Windows writable mapping retains a sharing conflict after its creating
file handle closes. That observation does not prove exclusion for every mapping
variant, permission change, same-user tampering or storage failure. Rename itself changes ChangeTime; post-rename comparison excludes that
field and keeps the original indefinitely. No crash/power-loss durability or
continuous external-change monitoring is claimed. Recovery paths are resolved
from the directory handle when reporting a recovery conflict; if resolution fails,
the last observed location is retained and may have moved with an ancestor.

API contracts: [security-query SACL presence](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fsa/0a9f223c-b258-4f61-8b59-92f970341400),
[NtCreateFile](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile),
[FILE_RENAME_INFORMATION](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information),
[SetSecurityInfo](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setsecurityinfo),
and [GetFileInformationByHandleEx](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex).

## Exact portable editor workflow — 2026-10-12

The current portable archive from clean tracked revision
`c3bba32532b0a5a3a0c96c52e047b88eec1f3a58` has SHA-256
`2864bd4522e67f0b6758789f8705b0471818e3aa4fbc7201a389ed111f35e7cb`.
Its application SHA-256 is
`7f63fc4d030b73355555d356fb60be73a428ccce122293cccb2890a2795eda57`.
It ran with a private profile in the network-isolated Windows 11 build 26100
QA guest as the medium-integrity interactive user (Administrators deny-only),
with the diagnostic OpenGL override.

Quick Inspector opened the selected Markdown file through Space, saved exact
edited bytes and retained the Unicode original plus an independent saved draft.
After an external write, Save refused replacement and kept the dirty buffer.
Keep editing preserved it; Copy my edits returned its exact text. Reload prompted
before discarding, then displayed the external version with a clean save state.
Independent filesystem checks verified current, original and saved-draft bytes
throughout. The Inspector and main window closed normally; the application process
was absent and the launch worker completed. The PowerShell Start-Process wrapper
did not capture a usable GUI exit code, so no application exit-code claim is made.

An initial fixture created by the SYSTEM orchestration account had an
Administrators owner. Saving safely refused an owner assignment the limited token
could not perform, preserved source bytes and kept the editor buffer with the
staged recovery path visible. The normal save fixture was then recreated by the
interactive user; its owner remained that user. This does not establish editing
arbitrary owners or bypass Windows permission requirements.

Evidence: `.hermes/evidence/windows-20261011/WINDOWS_TEXT_PACKAGE_ACCEPTANCE.json`,
`text-package-*.png`, the filesystem verifier and compact application logs.
This closes the focused exact-package Markdown save/conflict/reload workflow;
it does not repeat native security fault injection, cover all formats, prove
power-loss behavior or accept native WGPU/hardware/full daily-driver workflows.
The run also shows a partially visible Miller inline preview beside a long ancestor
chain at the private 1100 × 690 window size. This is a horizontally scrolled column,
not the overlay Smart Panel; initial selection/reveal behavior needs separate QA.
