# Native Windows Copy

Status: file/directory streams, ordinary metadata, private namespace, recursive
known-link staging, new-copy security inheritance components, paired staging
journals, retained-candidate/private-DACL validation and durable whole-tree final
permission planning have native component acceptance. Public Windows Copy remains disabled until the complete transaction
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

## Existing destination observations for Overwrite

`PinnedTree::freeze_existing` uses the same no-follow traversal and backup-stream
reader as staging to record an existing destination's tree structure, raw names,
native versions, streams, compression, link counts and owner/group/DACL digest.
Original hard links are retained as observations, including aliases outside the
tree; the new-candidate single-link restriction does not apply to originals.
Namespace/version checks bracket the scan, followed by security/compression/alias
rechecks. Cancellation is checked through these phases. Reads may update access
times; the observer never writes metadata, permissions or payloads.

`validate_existing` rereads the held tree and requires exact frozen evidence. The
evidence is serializable for the forthcoming retention journal, but grants no
rename, Undo or deletion authority. The enclosing transaction must still bind the
root through its held destination parent, persist paired retention intent and
revalidate across the reader-to-writer handoff. The observer alone introduces no
Overwrite mutation authority.

Native component checks cover populated folders, raw UTF-16 names, named data,
extended attributes and original hard links; changed data, permissions, aliases,
membership and same-byte replacement are refused even after cached nonidentity
metadata is refreshed. Entry-budget and cancellation refusals preserve the
observed graph. Audit-SACL/mandatory-label completeness, broader original-object
matrices and the durable retention/publication/Undo protocol remain required.
Receipt: `.hermes/evidence/windows-copy-existing-tree-current/ACCEPTANCE.json`.

## Paired original-evidence preparation

Schema 7 (`OTCPWJ07`) adds a begin record with count and whole-tree digest, one
ordered original-entry record per node, and a completion seal to the existing
paired journal. Entries share the staged-stream/version validator; original
hard-link aliases remain valid. Source/candidate aliases and journal/directory
ownership intersections are refused before the begin record is appended.
The full-tree digest is fixed at begin, so recovery cannot silently adopt a
changed child whose individual record was not written before interruption.

`Journal::prepare_original` opens the recorded target through the held parent,
observes it without following links, validates the frozen prefix and rechecks
the live tree/binding before sealing. It reserves remaining evidence frames plus
the existing permission tail before appending. A coherent paired prefix resumes
without sources; sealed replay adds no records. Divergent mirrors stay untouched.
The codec refuses prior schemas without converting or deleting their data.

Partial or sealed original evidence blocks creation-only publication and forward
admission. A completion seal describes recorded observations, not permission to
rename an original. Retention intent/acknowledgement, whole-overwrite mutation-tail
reservation, the writer handoff, publication, restoration and full security
acceptance are still required. Public Copy/Overwrite and cleanup remain gated.
Native preparation and regression receipt:
`.hermes/evidence/windows-copy-original-plan-current/ACCEPTANCE.json`.

## Initial destination admission

Staging takes a `DestinationAdmission`, replacing the raw target argument.
Creation requires native absence; access errors never imply absence. Overwrite
captures the existing graph before source staging and retains no open handles.
Its root version allows cheap pre-staging validation, while its complete graph
count/digest are written into both immutable schema-12 journal headers.

Original preparation revalidates the full graph against that initial approval
before appending any original records. Changed child data cannot be silently
adopted even when the root directory version stays unchanged. The decoder also
requires each original-plan begin to match the header's approved count/digest.
Creation cannot acquire original authority afterward; an approved overwrite
cannot publish as creation merely because its original is absent. Original
retention still requires its separate durable intent and acknowledgement.
User conflict UI/session/batch integration and broader acceptance remain gated.
Native admission and regression evidence:
`.hermes/evidence/windows-copy-destination-admission-current/ACCEPTANCE.json`.

## Native namespace and sharing

### Path and conflict routing

The native dispatcher opens the source through a held ordinary parent, using
read access without source deletion or write rights. It pins the destination
directory and requires a creator-owned private registry supplied by its caller.
Destination and registry ancestors are compared by native identity against a
directory source; copying into that source tree is refused before staging.
Source/destination/registry relocation and source-entry rebinding also refuse.

Conflict probes use native no-follow opens under the held destination parent.
Only NotFound means absence. Skip creates no transaction; Rename checks bounded
numbered siblings and treats dangling links as occupants; Overwrite freezes the
existing graph through the same destination admission used by the engine.
Publication remains no-replace, so a late occupant is retained with the staged
copy rather than overwritten. Late changes to an approved original also refuse.

The explicit-registry dispatcher takes an already owned handle. The configured
entry point supplies that handle through the registry admission below. Public
single/batch workflow wiring and the complete release gates remain unfinished.
Focused native cases and the full platform regression suite are recorded in
`.hermes/evidence/windows-copy-dispatch-current/ACCEPTANCE.json`.

### Configured registry admission

The configured registry is `%LOCALAPPDATA%\Ottrin\State\copies`. Its external
`.ottrin-copy-recovery-root` marker sits in `State`, so replacing the entire
registry cannot also replace its identity proof. The bounded `OTCRW01` record
binds marker, parent and root native identities, root name, creator and initial
root security. Marker admission requires an ordinary, single-link private file;
the root must be an ordinary private directory with the recorded security.

Fresh provisioning reserves the private marker before creating the private
registry, writes and flushes its complete record, and flushes the root/parent.
Open readers retain marker and root handles that deny replacement. Validation
checks the current creator, identities, private security and parent bindings.
Read-only discovery never creates missing state. Existing unanchored, partial,
corrupt, moved or replaced state is retained and refused; incomplete setup is a
review condition, with no automatic adoption, repair or deletion authority.
Actual setup power-cut and broader security/storage acceptance remain required.

Configured Copy pins its source before setup and checks the proposed registry's
existing native ancestors before provisioning any missing profile directories.
It refuses a registry inside a directory source without changing that source's
namespace. After provisioning it uses the same admitted-source dispatcher;
configured discovery and recovery also require registry admission. Explicit-root
APIs still require the caller's separate registry authority and retain native
journal admission; they do not act as a fallback for failed configured admission.
Public Copy is still gated.

### Session receipt integration

`DurableStage::execute_with_receipt` joins admitted staging, final security
planning, original retention when approved, publication and receipt admission.
It releases descendant capabilities before directory renames and uses the same
native engines as restart recovery. Failures retain the transaction location;
once publication has a verified completion, a later receipt failure is reported
as committed with recovery work pending.

The session receipt contains an opaque admitted native Undo plan. Issuing it
requires a fresh completion check and matching independently admitted journal
header/completion digest. `DefaultPlatform::undo_copy_with_receipt` executes that
plan; persisted evidence, rather than the cloned receipt's cached state, controls
replay. Activity reserves both the copied publication and retained original paths
for Overwrite, while tracking one transaction ID per receipt.

This bridge does not enable the public forward Copy dispatcher. Path/conflict
routing, normal single/batch workflows, guarded cleanup and package acceptance
remain required.

### Original retention and pre-publication restoration

Schema 8 adds paired original-retain and original-restore intent/acknowledgement
records. The original moves without replacement between its public leaf and the
transaction's sibling `.original` leaf, under the same real parent. Recovery
probes full native identity at both slots; unrelated occupants remain untouched,
and ambiguous or unrecorded locations refuse. The original keeps its bytes,
permissions and attributes. Only a pending rename's native root change time may
differ, and its resulting version is frozen in the acknowledgement.

Both directions share one verifier/move path. It checks the original before
intent, releases descendant readers for Windows rename, reacquires a typed root
capability, and checks the entire tree again before and after the operation.
The root and parent are flushed before acknowledgement; the final held tree
survives the acknowledgement append. Handoff changes retain data and refuse.
Failure messages name the transaction, original slots and staged copy.

Retention pins and verifies the private new copy before moving the public original.
If an interrupted rename already retained the original, its acknowledgement can
finish even when the private copy changed. Restoration before publication can
return the verified original while preserving that changed copy for review. Retention reserves its own
records, pre-publication restoration records and the existing permission tail
before intent. Readonly original file roots are explicitly unsupported by this
access strategy; no attribute clearing is attempted on an original.

That schema-8 checkpoint did not implement publication over the retained original
or Undo after new-copy publication; the schema-9 engine below adds those phases.
Readonly/rights strategies, complete audit/label protection, broader storage and
package acceptance remain required. Public Copy/Overwrite and cleanup stay gated.
Schema-8 native evidence:
`.hermes/evidence/windows-copy-original-move-current/ACCEPTANCE.json`.

### Publication and Undo over a retained original

Schema 9 permits the shared publication engine to continue only after original
retention is acknowledged and before restoration has begun. It holds and
revalidates the exact original tree throughout the publication/permission epoch.
The new copy still publishes with a no-replace native rename: an unrelated
occupant is preserved and stops recovery. Original bytes, native versions,
membership, hard-link counts, compression and owner/group/DACL remain bound.
Forward and Undo admissions bind original evidence as well as the new copy;
both expose the original retention slot to operation conflict checks. Forward
admission accepts pending retention only after its durable intent exists. It
freezes an already-retained original's observed version even before acknowledgement.
Execution compares the admitted proof and paired bytes, invokes the same original
move engine, then opens a new verified epoch for publication. Observation-only
original preparation cannot authorize retention. `RetainingOriginal` remains
resumable through discovery and Activity; restoration owns its transaction and
prevents forward resumption. Native injected/process-exit, read-only admission,
version-drift and same-plan cancellation evidence:
`.hermes/evidence/windows-copy-retention-resume-current/ACCEPTANCE.json`.

The existing Undo engine first withdraws the verified new copy to its candidate
slot and records that native result, then invokes the shared original restoration
engine. A withdrawn replacement alone is `UndoPending`, not `Undone`. Source-free
restart can finish restoration, including when the public name was occupied in
the meantime; it never replaces that occupant. After durable withdrawal, changes
to the private replacement do not block restoring the verified original. Discovery
reports `VerifiedOriginal` rather than claiming both trees are unchanged, and
Activity can admit the remaining restoration. Complete replay adds no records.
All retained versions remain available, with automatic cleanup separately gated.

Retention/publication reserve permission application, final-version sealing,
replacement withdrawal, readonly-root reopen records and original restoration
before the original moves. Begin-only original-evidence prefixes grant no move
authority and can still resume preparation. Public Windows Copy remains disabled
pending session/batch integration, initial conflict admission, security/storage,
power-cut and exact-package acceptance. Native file/folder original and all-prefix
injected/process-exit evidence is recorded at
`.hermes/evidence/windows-copy-overwrite-current/ACCEPTANCE.json`.

### Private candidate namespace

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

## Whole-tree destination permission intent

`security/tree.rs` derives every final DACL before changing any candidate's
permissions. The root uses the real held destination parent; descendants use their
parent's projected final descriptor. Deriving from a still-private candidate would
incorrectly propagate OWNER RIGHTS-only staging permissions. Root and projected
cases share one native derivation function and one held token for the plan.

The plan retains descriptors/handles, exact raw leaf names, parent indexes,
container roles, identities, initial private security and expected final security.
It checks an entire ordinary/known-link graph, rejects alias/duplicate/forward or
missing-parent inputs, and validates actual private membership/bindings, creator
context and parent security around capture. Known links remain leaves without
following targets. Cancellation and failure change no permissions and remove no
data. Native references cover all graph levels, not just the root.

`DurableStage::prepare_destination_security` streams this intent to both journals
only after verified staging: one header, an ordered record per entry, then a
count/digest seal. The digest serializer writes to a SHA-256 sink rather than
materializing the full plan. Existing 16 MiB frame and 64 MiB total-log budgets
remain. Each entry binds to the staged name/identity/kind/private-security graph.
Partial prefixes are valid retained data but cannot report prepared permissions.
Missing, duplicate, reordered or incorrectly sealed entries are refused.

Both mirrors flush each record before acknowledgement. Identical completed
preparation adds no record. Cancellation is checked between records; a coherent
cancelled prefix can resume without duplicates. An uncertain append still poisons
its owner and never repairs a divergent mirror. Source/candidate object IDs and
sibling names use replay-built indexes to avoid repeated full-tree searches.

Readonly retained planning works without sources, rederives native inheritance
and compares it with the recorded full plan or prefix. Cached ACLs remain untrusted
bytes, never Windows descriptor input. Even coherently altered records with valid
frame/plan digests cannot authorize native permissions. Root-parent permission
drift cannot silently reinterpret recorded intent. Payload verification, partial
planning, sealed intent, applied permissions and publication are distinct states.

The aggregate-size codec case exceeds one frame using synthetic recorded entries;
it does not establish real large-tree copying or resource/storage acceptance.
Those gates and the total journal/handle budgets remain explicit requirements.

The plan has no apply/publish/delete API. Per-entry permission-transition intent
and recovery, final timestamp/durability ordering, SACL/audit/label checks, rights
acquisition, Overwrite/publication/Undo and public workflows remain required.

## Acceptance and limits

Activity forward recovery now consumes an opaque `RetainedCopyResumePlan` admitted
from the configured registry. It binds the frozen staging evidence, observed
parent location and projected-security digest, and revalidates before entering
the shared publication engine. Verified unfinished transactions can resume;
unverified staging cannot. Completion enables fresh Undo admission. Current
source-built action acceptance, including changed-data refusal, moved-parent
retry and pre-mutation Stop/retry, retains its preceding source scope in
`.hermes/evidence/windows-copy-resume-volume-current/ACCEPTANCE.json`.
The same nine populated-folder scenarios pass on one volume and with recovery
records/data on distinct NTFS volumes. Root-file, readonly, link, specialized
storage and power-cut acceptance remain separate.
Public new Copy, Overwrite, cleanup and broader release acceptance remain gated;
packages have not been rebuilt for this action.

Streamed-plan checkpoint receipt: `.hermes/evidence/windows-copy-streamed-plan-current/ACCEPTANCE.json`.
The ordinary-user Copy suite passes 91 with 3 specialized/child exclusions; the
full native Windows platform suite passes 191 with 29 exclusions. SYSTEM audit
passes separately. Linux platform tests pass 411 with 3 exclusions, and strict
all-target platform Clippy passes for both targets. These are component/native
results, not public Copy, refreshed package, user or physical-platform acceptance.

Coverage includes stream/EA/compression/metadata preservation; read-only source
ACLs and hard-link independence; private creation/sharing/binding and destination
inheritance; populated/raw-name trees; link non-traversal/retargeting; paired
journal corruption/replacement; 32 injected flush failures and eighteen real process
exits. Retained validation additionally covers source disappearance, restored-time
content edits, ADS/membership/rebinding/extra records, dangling file/directory
links and cancellation/retry. Only a fully recorded completed stage validates
after a child exit. Private security cases cover extra ACEs and exact inheritance
flag drift on candidates/journals/transaction directories, permission changes
during staging and validation, malformed/foreign records and an isolated native
primary-group change. Whole-tree planning adds native reference-tree comparisons for six parent ACL
configurations, actual link-policy comparisons, graph/cancellation/drift refusals,
source-free rederivation, durable idempotence, plan-write failure/exit and forged
cached-ACL rejection. Permission application/publication is not introduced.
Streamed intent adds aggregate-over-frame codec, ordered/sealed-record validation
and coherent cancellation/resume cases; real exits now span every plan-append
boundary. These remain source/native components, not public Copy acceptance.
Earlier private-group fixture evidence and its test-only WRITE_OWNER setter are
recorded in `.hermes/evidence/windows-copy-private-security-current/ACCEPTANCE.json`.
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


## Creation times after native rename

NTFS filename tunneling can replace creation time when a native rename enters a
recently vacated name. Schema 11 records a narrow creation-time intent and
acknowledgement for publication, withdrawal, original retention and restoration.
The intent binds the observed native version after the recorded rename; the
write changes only creation time, then flushes and verifies before acknowledgement.
Whole-tree payload/security and unrelated metadata checks remain mandatory.
Completed metadata is strict; this does not authorize general metadata repair.
Read-only action admission freezes observed pending epochs through writer handoff.
Optional repair frames are included in forward/inverse journal reservations.

The preceding schema-11 component evidence is in
`.hermes/evidence/windows-copy-registry-current/`: `SCHEMA11_FULL_*` records
271 passing ordinary-user native tests, zero failures and no temporary entries.
This includes 72 injected failures and 72 real process exits across normal and
readonly creation/overwrite/Undo fixtures. Coherent prefixes recover without the
source; divergent local-only prefixes preserve data and refuse writes.
`SCHEMA11_CODEC_*` separately records the subsequently added malformed-transition
and epoch checks passing against the newer test binary. `SCHEMA11_DRIFT_*`
records 16 native cases that change creation or change time after read-only
admission, before/after repair intent across all four rename roles. Each refuses
without changing either journal or any observed payload/version. Current-schema
Activity integration and the separate explicit SYSTEM audit fixture also pass.
These are component
receipts; they do not establish physical power-cut, broad storage/security,
current package or public Copy acceptance. Full evidence and cleanup results are recorded in `ACCEPTANCE.json` in that
directory.


## Guarded cleanup backend

Cleanup derives its authority from the existing paired transaction evidence.
It requires a completed forward transaction or completed Undo, verified native
bindings, creator/private security and journal-only stage membership. Published
copies and restored originals remain user data; cleanup never requires their
contents to remain unchanged and never treats them as removal input.

| Terminal phase | Retained payload | Public user data |
| --- | --- | --- |
| New copy completed | None | Published copy |
| Overwrite completed | Displaced original at the recorded private original name | Published copy |
| Undo completed | Withdrawn copy at the recorded candidate name | Restored original or reused public name |

Schema 12 records `Cleanup::Begin` with an immutable retained-role/count/digest
binding, followed by ordered per-entry intents and acknowledgements. Reverse
manifest order removes children before parents. Native contents, identity,
metadata, permissions, compression and link counts remain strict. Only an
acknowledged unlink may advance its immediate parent's modification/change times
and surviving aliases' change time. Outside aliases grant no removal authority.
Per-object alias groups avoid rescanning the complete deletion order for each
record. The journal reserves the entire remaining cleanup tail before mutation.
Starting cleanup revokes ordinary forward/Undo authority.

`cleanup.rs::Admission` is the shared, handle-free read-only observation for
immediate execution and future confirmation plans. It freezes the header,
journal/private-metadata and observed-version digests, pending absence, registry
catalog fingerprint, destination location and parent permissions. Native registry
names are hashed as raw UTF-16. The writer reacquires and compares those exact
observations; it never replaces a confirmation's pending epoch with a fresh one.
Raw journal buffers and version graphs are released after observation.
`cleanup_dependencies.rs` pins registry records and refuses unverified peers or
references to selected native objects. An independent peer remains untouched;
a peer sharing a retained original blocks single-transaction cleanup. New registry
entries, changed bindings, unknown stage entries and changed retained data stop
further mutation.

`cleanup_tree.rs` opens only recorded names relative to verified parents. Deletion
uses READ/DELETE capabilities, POSIX disposition with readonly-ignore and image
section checks, explicit closure of all clones of the delete FileObject, real
NotFound verification, parent flush and surviving metadata verification before
acknowledgement. Access Denied is never evidence of absence. Readonly attributes
are not cleared. Closed-handle writable mappings block these deletion capabilities.
A pending own unlink can resume after actual absence without accepting unrelated
payload/version changes. The ordinary/readonly three-hardlink probes in
`UNLINK_PROBE.json` and `UNLINK_FORCE_PROBE.json` establish the close/flush/alias
clock boundary; the compiled cleanup tests exercise the complete protocol.

`cleanup_finalize.rs` records paired `MetadataRemovalIntent` only after
`PayloadRemoved`. It verifies metadata has exactly the known journal data or
empty directory streams, single native links, private ownership and expected
bindings. Control permissions/version are checked immediately before each
metadata unlink. Removal proceeds local journal, empty stage, then central
record, closing each handle and flushing its parent before continuing. Unknown
entries, streams, replacement occupants and control aliases are preserved.
A surviving central intent can admit a fresh source-free retry after local/stage
removal. An already admitted opaque proof permits terminal replay after central
removal; absence alone cannot construct that authority. Registry roots, external
anchors, other transactions and public files are outside the removal set.

Read-only metadata admission now uses that same validation sequence with read-only
local/stage/control handles. It performs no flush, disposition or journal write,
and returns a handle-free proof only after validating the remaining layout. The
inspection accepts the authenticated local-removed and stage-removed prefixes,
rejects unknown remaining content, and cannot initiate payload removal or create
the paired metadata intent. The scoped native suite passes 26 tests, including
72 read-only recovery-prefix inspections, seven unknown-content refusals and six
pre-authorization refusals. Evidence: `METADATA_INSPECTION_ACCEPTANCE.json` in the
cleanup evidence directory.

Current native component evidence is
`.hermes/evidence/windows-copy-cleanup-current/ACCEPTANCE.json`: 294 platform
tests pass with zero failures and 36 specialized exclusions. All 21 regular
cleanup tests pass, including 187 injected failures and 187 real process exits.
They cover readonly roots/descendants, outside hardlinks, source-free coherent
prefix recovery, divergent-mirror refusal, ordered metadata teardown,
post-admission unknown data/occupants, control permission drift, writable mappings
and independent/shared/corrupt registry peers. Tests run as a non-administrator
and leave zero temporary entries. Original profile/installation hashes remain
unchanged in the disposable no-NIC VM; source domains remain shut off.

The separate disabled-policy test confirms symlink installation refusal retains
the private candidate, its named data and the target. Full-suite link fixtures
use Developer Mode only in the owned COW. The actor's temporary root grants its
creator Full Access, including the WRITE_OWNER capability needed by label
fixtures. Earlier fixture failures and their causes are preserved in the receipt.
Policy, link/label smoke and full-suite lanes use the same checksum-pinned
executable; deterministic stripping preserves compiler dates and all non-debug
sections. Strict Windows platform/UI all-target Clippy and formatting pass.

Configured registry cleanup can now acquire an existing-only writable root from
the same anchor/ownership validation used by discovery. It requests read/write
rather than DELETE on the registry itself, so the manager's pinned root remains
compatible when it is also the destination parent. Sixteen ordinary-user native
cases pass for file/directory, creation/overwrite and completed/undone copies at
both registry and separate destinations; public bytes, root and external anchor
remain intact. `REGISTRY_WRITER_ACCEPTANCE.json` records this added capability's
source/binary proof. This does not initialize missing state or enable public Copy.

The shared admission now passes 25 native cleanup tests, including the prior
187 injected failures and 187 process exits. Twelve new cases verify read-only
observation/handle release, public reuse and refusal of payload, metadata,
catalog, progress and later pending-parent epoch changes. Exact source/binary
proof: `ADMISSION_ACCEPTANCE.json` in the cleanup evidence directory.

Discovery now distinguishes `VerifiedCleanup` from complete-tree/original
verification. It uses the shared payload or metadata inspection before requiring
a complete Snapshot, then rechecks exact central bytes. Local/stage removal is
therefore a valid authenticated cleanup prefix; unknown content still needs
review. The current central journal is the displayed recovery location. The
scoped native suite passes 26 tests, including 72 partial-metadata discovery
prefixes, seven unknown-content cases and four started-payload cases. Evidence:
`CLEANUP_DISCOVERY_ACCEPTANCE.json`. Activity's new status/action-separation
handling is statically checked; native UI execution is not claimed here.

Public cleanup remains unfinished: it needs the opaque configured action wrapper
around this admission and Activity confirmation/execution/retry.
Co-owned batch references, interrupted staging reclamation and realistic registry/
large-operation resource limits remain open. Process exits do not establish
physical power-cut acceptance; specialized storage/security and current package/UI
gates retain their separate scopes. Public Windows Copy remains gated.
