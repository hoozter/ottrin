# Windows text-save boundary

The Windows save implementation in `crates/ottrin-platform/src/text_edit/windows.rs`
is an acceptance candidate. `text_editing_supported()` and public session creation
still refuse Windows editing. There is no environment override. Linux publication
and the editor's conflict/reload/Undo behavior are unchanged. macOS remains
read-only. No additional document formats are introduced.

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
every boundary, successful/failed restoration, collision retention and the closed
public gate. Windows-only fixtures exercise retained originals/drafts, byte and
security preservation, unchanged saves, external edits, sharing refusal,
occupied names, post-publication flush failure, hard links, alternate streams,
invalid/oversized text and public-gate enforcement. These fixtures call the private
native session; they do not enable editing in an application build.

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
Before changing the public gate, extend the ordinary/custom ACL, directory flush
and sharing evidence with exact packaged editor behavior. Recovery-directory and
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
those native admission cases; exact packaged editor acceptance remains pending.
Public editing stays disabled. Unsupported security cloning or directory flushing stops the save and retains existing staging.

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
