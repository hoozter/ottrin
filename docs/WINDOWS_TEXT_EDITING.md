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
points, hard links, alternate streams, extended attributes, unsupported file
attributes, binary controls and files over 128 KiB are refused. A source handle
is held during save with write/delete sharing denied; it is not held while the
user edits. An incompatible existing writer therefore stops saving before staging.

Source identity, content, creation/write/change times, owner/group/DACL,
mandatory-label security and security resource attributes are compared before
withdrawal. The publication file is created beside the source using the captured
security descriptor, so its inherited ACL comes from the same parent. Its captured
security, creation time and attributes must match before the original is moved. If
Windows cannot apply resource attributes to the publication, that exact comparison
fails closed before withdrawal. No merge-error-ignore mode is used.

Each changed save creates an owner-only `.ottrin-text-edit-<random>` directory.
`edits` is an independent flushed draft. A unique sibling ending in `-publication`
holds the staged publication with the source security. Both directories are flushed
before source withdrawal. A no-replace handle rename moves the original to
`candidate`, followed by revalidation and directory flushes. Only then does a
second no-replace handle rename publish the edited file. Content/identity/security
are checked again and publication/directory flush failures stay visible.

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
Before changing the public gate, require native evidence for ordinary and custom
ACLs, directory flush support, sharing modes, writable mappings, Unicode paths,
ancestor renames, process termination between each publication step, recovery
privacy, and exact packaged editor behavior. Security resource attributes are now
included in the descriptor that is captured, applied and compared, but this
preservation path still needs a native fixture with an actual resource-attribute
ACE. Audit SACLs, scoped policy IDs, object IDs and other NTFS metadata are **not
yet covered by the admission/preservation checks**. Extend admission or
preservation on evidence; do not enable the gate while these metadata gaps remain.
Unsupported security cloning or directory flushing currently stops the save and
retains staging.

The sharing restriction does not exclude writable mappings whose creating handles
have already closed, changes to permissions, same-user tampering or storage
failure. Rename itself changes ChangeTime; post-rename comparison excludes that
field and keeps the original indefinitely. No crash/power-loss durability or
continuous external-change monitoring is claimed. Recovery paths are resolved
from the directory handle when reporting a recovery conflict; if resolution fails,
the last observed location is retained and may have moved with an ancestor.

API contracts: [NtCreateFile](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile),
[FILE_RENAME_INFORMATION](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information),
and [GetFileInformationByHandleEx](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex).
