# Native Windows Copy

Status: file/directory streams, ordinary metadata, private namespace, recursive
known-link staging, new-copy security inheritance components, paired staging
journals and completed retained-candidate/private-DACL validation have native component
acceptance. Public Windows Copy remains disabled until the complete transaction
and normal workflows are implemented and accepted. Current counts and scope are
in the acceptance section below and the roadmap checkpoint.

## Complete outcome

Copy must leave the original namespace and bytes untouched. It must not simulate
a Move followed by restoration or request source delete/data-write access merely
because Move uses those rights. Native Move identity and handle-relative rename
primitives can be reused where their contracts apply.

The complete implementation must support files, populated directories and
supported link entries, single and mixed batches, unoccupied targets and
Skip/Rename/Overwrite. It must retain complete displaced destinations, preserve an
independently verified staged result, provide guarded Undo and interruption/restart
recovery, and integrate with normal clipboard/Tandem flows. Unsupported metadata
must produce an explicit error while retaining source and recovery data.

Before enabling the public path, implement and accept:

1. **Native authority.** Pin destination parents and source entries. Read sources
   without deletion/data-write rights. Refuse alternate-stream syntax at leaf-name
   boundaries, handle reparse entries explicitly and create private entries with
   their restrictive descriptor atomically. Revalidate bindings through held
   parents; remembered paths never authorize mutations.
2. **Complete staging.** Preserve primary/named/extended/property data, directory
   children and metadata, and supported links. Sparse/EFS, case-sensitive directory
   policy and specialized storage/provider/property semantics remain required.
   Final permissions follow the accepted Copy policy independently of staging.
3. **Durable publication/recovery.** Record intent and ownership before mutation.
   Retain the exact displaced Overwrite target and both versions at interruption
   boundaries. Publish only verified data under recorded native authority. Copy
   records remain distinct from Move records and never grant source removal.
4. **Undo/cleanup.** Validate current identities, contents, streams and metadata
   before inverse operations. Refuse stale/reused entries, changed data and occupied
   restoration names. Delete only proven transaction data; unexpected entries
   remain with actionable status. Windows recovery discovery and UI are required.
5. **Actual workflows.** Wire platform, batch, clipboard and Tandem callers. Pass
   the existing positive Windows Copy UI/clipboard tests without excluding them.
   Accept native fault/exit, ACL/sharing, path-rebinding, conflict, Undo and cleanup
   cases, then rebuilt portable/NSIS/MSI workflows. Broader storage, blocked/network
   I/O cancellation and release gates remain required.

## Native namespace and sharing

`copy_transaction/windows/namespace.rs` pins the real destination parent. It
creates `.ottrin-copy-ID` and the sibling `.ottrin-copy-ID.candidate` directly
under that parent with create-new semantics. Existing entries are never adopted
or overwritten. The shared `windows_fs.rs` primitive preserves raw UTF-16 names,
rejects invalid/non-leaf names and reparse parents, requires persistent filesystem
ACLs, and supplies the protected owner-only DACL in the native create operation.
There is no inherited-permission interval or path-based repair/Drop cleanup.

Parent/stage identities and bindings are checked around creation. Ancestor aliases
cannot redirect held-parent operations; collisions or rebound names retain all
entries. Known candidates remain bound to their full native type/tag and volume/
file identity. `same_object` ignores type only for alias refusal and an explicitly
planned link-type transition, never as a general ownership/cleanup bypass.

Sharing is operation-specific. Create-only staging uses read sharing to exclude
ordinary new writers. Text saving uses the same atomic creation primitive with
read/write directory sharing needed to retain its original. Publication/Overwrite
must establish and test their own sharing protocol. These locks do not promise
an atomic filesystem snapshot or exclude pre-existing writable mappings and
independently held authority. Non-ACL filesystems need an explicit safe strategy;
refusal does not complete support. Windows Move's remaining private-directory
hardening is separate work, not implied by Copy's shared primitive.

## Payloads, metadata and trees

`windows.rs::stage_entry_streams` and `metadata::stage_entry_payload` are the single
file/directory/known-link payload path. They require compatible buffered synchronous
handles and an independent, matching-kind candidate with no existing payload.
Directory handles bypass byte-offset seeking; directory children belong to the
tree orchestrator. Native BackupRead/BackupWrite contexts are released on success,
cancellation, failure and unwind. Local chunk boundaries check cancellation; this
is not blocked-device/network I/O isolation.

The codec streams primary data, alternate streams, extended attributes and admitted
property records through bounded buffers. Names retain raw UTF-16, each stream has
length and SHA-256 evidence, and both candidate and reread source must agree.
Empty file primary streams are explicit even if omitted by native enumeration;
directories have no synthetic primary stream. Source hard-linked names produce
independent candidate data. Security, hard-link relationship, sparse and unknown
backup records are refused rather than silently omitted.

Ordinary metadata includes creation/access/write times and read-only, hidden,
system, archive, temporary, offline and not-content-indexed attributes. A copy keeps
its own change history. Unknown/specialized attributes stay explicit requirements.
NTFS compression is queried/applied through native handles, including clearing an
inherited compression state for an uncompressed source. Broader filesystem support
is not established by NTFS component tests.

Streams/compression are verified before final timestamp application: native backup
reads can update access times through internal handles. Strict staging metadata
checks query the result without another payload reread. A later publication phase
that rereads payloads must order final timestamp application and durability again.
Recovery's read-only timestamp rules are defined separately below.

`tree.rs::PinnedTree` discovers the complete source tree before destination
mutation, retaining read-only handles, versions and exact child names. Iterative
native enumeration enforces the caller's entry/handle budget. Destination and
central-store identities found inside that source graph are refused before any
stage is created. Broader source-root/central-store placement remains required;
there is no compensating ignore list or mutation of the selected source.

Each child is created private and new relative to its held candidate parent, then
uses the shared payload path. Known links are leaves. Before success the tree
checks source versions, exact source/candidate membership, child bindings and
stream evidence, then applies final metadata bottom-up after all reads. It rechecks
identities, bindings and metadata. `verify_and_finalize` repeats verification for
a completed stage and refuses changed metadata before normalization. Root binding
and durable ownership belong to the enclosing journal owner. Move and Copy share
the same native directory parser/enumerator in `windows_fs.rs`.

## Known symbolic links and junctions

`links.rs` captures `IO_REPARSE_TAG_SYMLINK` and directory
`IO_REPARSE_TAG_MOUNT_POINT` records. It checks the 16 KiB bound, lengths/reserved
fields, tag/type, supported flags and counted UTF-16 ranges. Relative/absolute
form, print/substitute names and raw units are preserved without normalization.
Unknown/provider-managed tags, malformed data and unsupported flags are refused.

The codec includes exact reparse evidence and compares BackupRead's record with
the separately captured record. Ordinary payload records are copied and flushed
first; validated link data is installed through FSCTL_SET_REPARSE_POINT on the
held, still-ordinary matching-kind candidate. Files must have empty primary data;
directories must have no children. Opaque reparse records never go to BackupWrite.
Existing links are never adopted/retargeted. Named streams stay attached to the
same native object; installed bytes and the planned type transition are checked.

Source and candidate opens never follow link targets, including dangling links,
root links and links out of the source tree. Retargeting with the same file ID/tag
is detected through version/raw-record/stream checks. Native tests include denied
file-data/directory-listing targets with only the descriptor rights needed to
restore fixture ACLs. Production never enables Developer Mode, enables privileges,
elevates automatically, substitutes junctions for symbolic links or falls back to
target contents. Existing-token privilege/elevation integration remains required.

## Destination security

`security.rs` derives new-copy owner/group from the effective token and DACL
inheritance from the real final parent using CreatePrivateObjectSecurityEx with
file generic mappings, including token defaults without inheritable parent ACEs.
Source ACLs and the private staging DACL are not creator inputs. Native descriptors
and effective thread/process tokens have owned lifetimes.

Applying the DACL requires a matching name/type/identity directly under the final
parent. Parent identity and owner/group/DACL are checked around SetSecurityInfo,
and the resulting owner/group, ACL bytes and protection/auto-inheritance flags
must match the derived policy. Applying under the private transaction is refused.
No rename/delete/restore is performed. Failure, including after security mutation,
must retain the entry under durable transaction ordering.

Direct-child candidate creation lets Windows supply ordinary new-child audit SACL
and mandatory-label inheritance, including NO_PROPAGATE entries. Only its private
DACL is supplied. Production requests no audit/security privilege. The audit case
runs separately as SYSTEM with test-owned SeSecurityPrivilege; mandatory-label
cases run as an ordinary user. Applying the final DACL preserves SACLs/labels.

This remains a component policy. Absent/null derived DACLs, complete security drift,
complete SACL/audit/label recovery drift validation, Overwrite descriptors and tree-wide
final inheritance remain required. Private owner/group/DACL validation is described
with journal ownership below. Descendant final permissions must derive from
final destination inheritance, not private parent DACLs. Missing WRITE_DAC and
changed parents must fail with exact bytes/ACLs retained.

## Paired staging journals

`journal.rs::DurableStage` owns separate append-only local and central journals,
bound to native parents/directories/file identities. Format v2 also records the
effective token's default owner SID, private transaction/journal security and each
candidate's private security at acknowledged creation. Owner/group SIDs, a protected
explicit OWNER RIGHTS full-access DACL and its inheritance flags are validated;
exact native owner/group/DACL/control evidence is rechecked. Old private formats
are rejected without a compatibility reader or mutation. Copy's strict schema has
no source-deletion event. Diagnostic parent paths and source labels do not authorize
source access/removal. Frames have monotonic sequences, a SHA-256 chain, a 16 MiB
frame budget and a 64 MiB log budget.

Both journals flush creation intent and acknowledged native identity before a
candidate receives payload writes. Expected link data precedes its type change;
staged identity must name the same object with the planned final type. Per-entry
payload evidence precedes terminal whole-tree verification. Candidate security is
checked before acknowledging payload success; parent security is checked before
child creation and all candidates are checked before terminal verification. An uncertain append
poisons its writer: torn, foreign, reordered, rebound or unequal mirrors are
retained, never automatically repaired or retried under a second interpretation.

Journal-only `inspect` reopens through held parents and checks exact paired bytes,
framing, state transitions and namespace bindings. Its verified record is not a
live content check or deletion/publication authority. Interrupted entries need a
separate recovery protocol; a created identity alone never authorizes removal of
partial bytes or later user data.

## Retained candidate validation

`RetainedStage::open` requires paired terminal staging verification and opens the
transaction/journals with read-sharing only, refusing an active writer. Each
candidate opens through its recorded parent/raw leaf without following links or
requesting write/delete rights. Original sources are never opened or required.
The effective default owner must match the creator; private transaction/journal
and candidate owner/group/DACL/control evidence must match before and after the
scan. Broadening permissions, changing even still-private inheritance flags or
altering the group is retained as a conflict. No recorded descriptor is applied.
A read-sharing handle does not exclude a separately authorized WRITE_DAC change;
post-scan security revalidation detects that drift without repairing it.

Before hashing, native identities, creation/write times, attributes, length and
compression must match the evidence. Raw known-link records must match. Exact
candidate directory membership, including empty directories, follows raw UTF-16
ordering. The transaction directory must contain only its journal. The shared
backup codec rereads every supported stream and compares size/SHA-256. Journal
bytes, observed versions and bindings/membership are checked around the scan.

Returned read handles support revalidation before a later phase. Windows maintains
access times during ordinary reads, so recovery allows those updates. Strict
staging still checks access time when applying final metadata. Recovery never
repairs timestamps, permissions or changed data. Completed payload/private-DACL
validation is not final security, publication or cleanup authority: rights acquisition with revalidation, guarded
mutation, Overwrite retention, central discovery, interrupted-entry recovery, Undo
and public/package workflows remain required.

## Acceptance and limits

Current source receipt: `.hermes/evidence/windows-copy-private-security-current/ACCEPTANCE.json`.
The ordinary-user Copy suite passes 79 with 3 specialized/child exclusions; the
full native Windows platform suite passes 179 with 29 exclusions. SYSTEM audit
passes separately. Linux platform tests pass 411 with 3 exclusions, and strict
all-target platform Clippy passes for both targets. These are component/native
results, not public Copy, refreshed package, user or physical-platform acceptance.

Coverage includes stream/EA/compression/metadata preservation; read-only source
ACLs and hard-link independence; private creation/sharing/binding and destination
inheritance; populated/raw-name trees; link non-traversal/retargeting; paired
journal corruption/replacement; 22 injected flush failures and eight real process
exits. Retained validation additionally covers source disappearance, restored-time
content edits, ADS/membership/rebinding/extra records, dangling file/directory
links and cancellation/retry. Only a fully recorded completed stage validates
after a child exit. Private security cases cover extra ACEs and exact inheritance
flag drift on candidates/journals/transaction directories, permission changes
during staging and validation, malformed/foreign records and an isolated native
primary-group change. The initial group fixture's missing WRITE_OWNER right is
recorded with rejected logs/binary; only that test setter requests the added right.
Production creation/recovery rights and validation assertions remain intact.
The preceding access-time correction retains distinct evidence in
`.hermes/evidence/windows-copy-retained-current/ACCEPTANCE.json`.

Separate earlier Developer Mode-off acceptance (ordinary-user symlink denial with
error 1314 and positive junction staging after reboot) remains in
`.hermes/evidence/windows-copy-links-current/ACCEPTANCE.json`. It was not repeated
for the reader change. Test-only Developer Mode exists solely in the disposable
no-network COW. Cleanup checks original profile/installation hashes, removes owned
processes/fixtures and disposes only the owned VM/volume/NVRAM/TPM after normal
shutdown. Original guests remain stopped and preserved.

## Native API references

[BackupRead](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-backupread)
and [BackupWrite](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-backupwrite)
define synchronous backup streams and context release; EFS needs separate APIs.
[CopyFile2](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-copyfile2)
describes named-stream/EA preservation. Metadata uses
[FILE_BASIC_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_basic_info)
and [FSCTL_SET_COMPRESSION](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_set_compression).
Security derivation follows
[CreatePrivateObjectSecurityEx](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createprivateobjectsecurityex).
Link contracts are the [native record layout](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_reparse_data_buffer),
[handle-based installation](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_set_reparse_point)
and [reparse restrictions](https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-points).
Native tests establish the verified implementation scope; references alone do not.

Group/owner queries need READ_CONTROL; setters need WRITE_OWNER, as specified by
[SECURITY_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/secauthz/security-information).
Only the native group-drift fixture opens the setter right. Production records
security through read queries and never applies serialized descriptors.
