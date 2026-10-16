# Native Windows Copy

Status: native stream staging verified; public Copy remains disabled until the complete transaction is accepted.

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

`crates/ottrin-platform/src/copy_transaction/windows.rs` operates only on already-open regular file handles and an empty, independently owned candidate supplied by the transaction owner. It does not create, publish, rename or remove filesystem entries. The public Windows dispatcher still rejects Copy; this component is not a usable Copy implementation by itself.

The component:

- Reuses the native volume/file identity implementation, rejects source/candidate aliases and pre-existing candidate data or named streams, and checks source versions around copying.
- Streams primary data, alternate data, extended attributes and admitted property records through bounded buffers. UTF-16 stream names are retained without lossy conversion. Security, link, reparse, sparse and unknown backup records are refused; sparse and EFS inputs remain unimplemented requirements.
- Captures each stream's size and SHA-256, compares the candidate and a reread source, and represents an empty primary stream explicitly even when the native backup enumeration omits it.
- Requires buffered synchronous handles, reports read/write failures, and releases native backup contexts on success, cancellation, error and unwind. Only normal completion flushes buffered output. Cancellation is checked at local chunk boundaries; this is not blocked-device/network I/O isolation.
- Does not implement final file attributes/security, directory/link staging, durable journals, namespace publication, Undo or recovery cleanup yet. The caller remains responsible for the candidate after failure.

Thirteen ordinary-user native tests cover Unicode and empty alternate streams, extended attributes, large and empty primary data, tiny files and buffer-boundary tails, source ACLs without delete/data-write rights, hard-link independence/alias refusal, occupied candidates, mid-copy changes, cancellation, write failure and incompatible I/O modes. This is bounded source evidence, not exact-package or release acceptance. The broader native platform suite passes 113 with 26 specialized tests excluded. Its symlink-creation fixture requires Developer Mode, enabled only inside the disposable COW guest; the 13 stream tests also passed before that setting. Evidence: `.hermes/evidence/windows-copy-current/`.

## Native API references

Microsoft documents the backup stream format and context lifetime in [BackupRead](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-backupread) and [BackupWrite](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-backupwrite). Both require synchronous handles and explicit context release; EFS uses separate APIs. [CopyFile2](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-copyfile2) documents Windows preservation of named streams and extended attributes, which a primary-stream-only copy does not provide. These references inform the implementation; native tests establish the current verified scope.
