# Native Windows Copy

Status: native streams, private namespace, regular-file metadata and new-copy destination DACL policy verified; public Copy remains disabled until the complete transaction is accepted.

Copy must leave the original namespace and bytes untouched. It must not simulate a Move followed by restoration, or require source delete/data-write access merely because Move uses those rights. The existing Windows Move identity and handle-relative rename primitives can be reused where their contracts apply.

## Complete outcome

The implementation must support files, populated directories and supported link entries, single and mixed batches, unoccupied targets and Skip/Rename/Overwrite. Copy must retain complete displaced destinations before replacement, preserve an independently verified staged result, provide guarded Undo and interruption/restart recovery, and integrate with the normal clipboard/Tandem flows. Unsupported entry metadata must produce an explicit error while retaining source and recovery data; it must never silently disappear.

Before enabling the public path, implement and accept:

1. **Native namespace authority.** Pin the requested destination parent and each source entry. Open source data for reading, without deletion rights. Refuse alternate-stream syntax at the source-entry boundary and handle reparse entries explicitly. Create the private transaction with its restrictive security descriptor at creation, relative to the pinned parent. Revalidate bindings rather than reopening an alias to authorize mutations.
2. **Complete staging.** Copy named streams and extended attributes as well as primary data. Extend this to directory trees and their metadata. Validate the supported file-attribute, compression, sparse, EFS, reparse and property cases; unresolved cases stay explicit requirements. Final destination permissions must follow the accepted Windows Copy policy, independently of the private staging permissions.
3. **Durable publication and recovery.** Persist the intended destination and staged evidence before mutation, retain the exact displaced target for Overwrite, publish only verified data with the recorded parent/entry authority, and preserve both versions at every interruption boundary. Copy recovery must never acquire source-removal authority. Keep Copy records distinct from Move records.
4. **Undo and cleanup.** Validate current identities, contents, streams and metadata before an inverse. Refuse stale/reused entries and occupied restoration names. Delete only data proven to belong to the transaction; unexpected files or edits must remain with actionable recovery status. Recovery discovery and UI must work on Windows too.
5. **Actual workflows.** Connect the platform, batch, clipboard and Tandem callers. Retain and pass the existing positive Windows clipboard tests rather than excluding them. Accept native fault/exit, ACL/sharing, path-rebinding, conflict, Undo and cleanup cases, then the rebuilt portable/NSIS/MSI public workflows. Broader storage, network I/O cancellation and release gates remain required.

## Verified stream component

`crates/ottrin-platform/src/copy_transaction/windows.rs` operates only on already-open regular file handles and an empty, independently owned candidate supplied by the transaction owner. The stream component does not create, publish, rename or remove filesystem entries. The private namespace component below supplies its owned candidate. The public Windows dispatcher still rejects Copy; this component is not a usable Copy implementation by itself.

The component:

- Reuses the native volume/file identity implementation, rejects source/candidate aliases and pre-existing candidate data or named streams, and checks source versions around copying.
- Streams primary data, alternate data, extended attributes and admitted property records through bounded buffers. UTF-16 stream names are retained without lossy conversion. Security, link, reparse, sparse and unknown backup records are refused; sparse and EFS inputs remain unimplemented requirements.
- Captures each stream's size and SHA-256, compares the candidate and a reread source, and represents an empty primary stream explicitly even when the native backup enumeration omits it.
- Requires buffered synchronous handles, reports read/write failures, and releases native backup contexts on success, cancellation, error and unwind. Only normal completion flushes buffered output. Cancellation is checked at local chunk boundaries; this is not blocked-device/network I/O isolation.
- Does not implement final file attributes/security, directory/link staging, durable journals, namespace publication, Undo or recovery cleanup yet. The caller remains responsible for the candidate after failure.

Thirteen ordinary-user native tests cover Unicode and empty alternate streams, extended attributes, large and empty primary data, tiny files and buffer-boundary tails, source ACLs without delete/data-write rights, hard-link independence/alias refusal, occupied candidates, mid-copy changes, cancellation, write failure and incompatible I/O modes. This is bounded source evidence, not exact-package or release acceptance. The broader native platform suite passes 113 with 26 specialized tests excluded. Its symlink-creation fixture requires Developer Mode, enabled only inside the disposable COW guest; the 13 stream tests also passed before that setting. Evidence: `.hermes/evidence/windows-copy-current/`.

## Native API references

Microsoft documents the backup stream format and context lifetime in [BackupRead](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-backupread) and [BackupWrite](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-backupwrite). Both require synchronous handles and explicit context release; EFS uses separate APIs. [CopyFile2](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-copyfile2) documents Windows preservation of named streams and extended attributes, which a primary-stream-only copy does not provide. These references inform the implementation; native tests establish the current verified scope.

## Verified private namespace component

`copy_transaction/windows/namespace.rs` pins the destination parent, creates a random private directory without replacement, and creates its candidate relative to that directory. Each native creation supplies a protected owner-only DACL atomically; there is no inherited-permission interval. The shared primitive in `windows_fs.rs` rejects non-leaf names and parents with reparse points, requires persistent filesystem ACLs, and opens entries without following reparse targets. Raw UTF-16 names are preserved. Copy source handles request read access without DELETE or data-write rights.

Before and after candidate creation, Copy checks the held parent and stage identities and observes the stage name through the pinned parent. An ancestor alias change cannot redirect creation. A changed stage binding returns an explicit retained-data error; no path-based deletion or automatic Drop cleanup is attempted. Durable ownership records and destructive cleanup remain required before this component can become a transaction.

Sharing is explicit per operation. Copy's current create-only staging holds read sharing, excluding ordinary new directory writers. Text saving reuses the same atomic private creation but requires read/write directory sharing so Windows can rename the retained original into it. A native regression caught that rename failing with error 32 when the directory was locked too tightly; the final full suite passes without weakening that test. Future Copy retention/publication must select and verify its own sharing policy. These locks do not claim exclusion of pre-existing writable mappings or same-user changes through independently held authority.

Eight added native tests cover immediate protected DACLs in a permissive parent, create-new collisions, invalid and raw UTF-16 names, reparse refusal, ancestor alias changes, replaced stage bindings and directory writer exclusion. Together with the stream cases, 21 tests pass as an ordinary Windows user. The full native platform suite passes 121 with 26 specialized exclusions, including the existing text-save cases. Developer Mode is enabled only in the owned COW guest for symlink fixtures. Strict Windows and Linux platform all-target Clippy passes. Evidence: `.hermes/evidence/windows-copy-namespace-current/`.

This remains source-only evidence. Specialized attributes, complete final security, directory/link staging, journals, publication, Undo/recovery, clipboard integration and exact-package acceptance remain open. Non-ACL filesystem staging needs an explicit safe strategy; refusal is not completion of that requirement. Existing Windows Move private-directory creation has not been migrated to this shared atomic primitive and requires separate hardening and acceptance.

## Regular-file metadata staging

The metadata component stages ordinary regular-file timestamps and attributes,
then revalidates the source and candidate. It preserves creation, access and
modification times; the copy retains its own change history. Supported ordinary
attributes are read-only, hidden, system, archive, temporary, offline and
not-content-indexed. Unknown attributes are refused explicitly rather than
silently dropped. Sparse, EFS and specialized storage attributes remain required
future work. NTFS compression is queried and applied through native handles;
both compressed sources and uncompressed sources staged into compressed
candidates are covered. This does not claim broader filesystem acceptance.

Stream and compression verification precede final timestamp application.
Native backup-stream reads update access time even when the supplied handle
suppresses it. Final metadata checks therefore query attributes and identity
without rereading file data. Any later publication phase that rereads data must
apply and recheck final timestamps afterward. A native negative control catches
the exact access-time mismatch; neither the assertion nor system access-time
policy is weakened.
The [FILE_BASIC_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_basic_info)
and [compression API](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_set_compression)
contracts define the metadata operations used here.

The candidate remains protected by its private staging DACL throughout this
phase. The new-copy DACL component below handles destination inheritance.
Complete security and publication integration remain required; copies must not inherit the private transaction's owner-only DACL accidentally. Public Copy,
publication, recovery and package acceptance remain open.

Six new native metadata cases pass, bringing the Copy component suite to 27.
The full ordinary-user native platform suite passes 127 with 26 specialized
exclusions; strict Windows and Linux platform all-target Clippy passes.
The source without final-metadata ordering reproduces the historical access-time
mismatch. Evidence is `.hermes/evidence/windows-copy-metadata-current/`.
No public Copy or rebuilt package is claimed by these component results.

## New-copy destination DACL policy

New copies use the effective caller token's default owner and group and the
real destination parent's DACL inheritance, as an ordinary child created there
would. Source ACLs and the private staging DACL are not creator inputs. The
native component in `copy_transaction/windows/security.rs` uses
[CreatePrivateObjectSecurityEx](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createprivateobjectsecurityex)
with file generic-right mappings to derive this policy, including token defaults
when the parent has no inheritable ACEs. The allocated descriptor and effective
thread/process token have owned native lifetimes.

Application requires the candidate already to be under the real destination
parent: its exact name binding, type and identity must match the held handle.
Parent identity and owner/group/DACL are checked before and after the mutation.
Only then does the component apply the DACL with `SetSecurityInfo`, enabling
normal inheritance. Applying it under the private stage is refused. It compares
the resulting owner, group, DACL bytes and protection/auto-inheritance flags to
the derived result. It never renames, deletes, cleans up or restores an entry.
The complete journal owner must retain an entry on every failure, including
failure after the security mutation, and provide durability/recovery ordering.
No publication durability is claimed by this component alone.

Six new ordinary-user native cases compare against actual kernel-created files
and directories. They cover four parent ACL configurations (inheritable allow,
no inheritance/token defaults, creator-owner substitution, and deny entries),
directory child inheritance, later parent ACL propagation, changed-parent
refusal, private-stage/wrong-name/type refusal and missing WRITE_DAC with exact
byte/ACL retention. The full Copy component suite passes 33; the native platform
suite passes 133 with the same 26 specialized exclusions. Evidence is
`.hermes/evidence/windows-copy-security-current/`.

This is the new-copy DACL component, not complete Windows security support.
SACL/audit and mandatory-label policy, absent/null derived DACL handling,
Overwrite security integration and exact
retained destination descriptors remain required. Directory/link staging,
publication journals, Undo/recovery and public clipboard/package acceptance
also remain open. The public dispatcher still rejects all Windows Copy modes.
