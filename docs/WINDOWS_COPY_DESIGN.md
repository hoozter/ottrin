# Native Windows Copy

Status: file/directory streams, ordinary metadata, private namespace, recursive
known-link staging, new-copy security inheritance components, paired staging
journals, retained-candidate/private-DACL validation and durable whole-tree final
permission planning have native component acceptance. Public Windows Copy remains disabled until the complete transaction
and normal workflows are implemented and accepted. Current counts and scope are
in the acceptance section below and the roadmap checkpoint.

## Confirmed temporary-stream isolation gap

Native probes with the earlier temporary-file access mask/private descriptor
confirm that FILE_SHARE_READ does not exclude a second named-stream writer.
Kernel delete-on-close then removes that added data. Automatic directory lifetime
also leaves an empty directory for directory-first close and a hard kill, and
can remove an added directory stream. It must not be used to close the stage
construction gap.

A proposed freeze-then-pending-delete replacement is rejected too. Later-open
writer checks pass, but a WRITE_DAC plus READ_CONTROL handle acquired before the
freeze can restore full access afterward. A native ordinary-account interleave
then adds a named stream and observes automatic deletion remove it. Neither a
fresh descriptor check nor a stream check revokes that pre-held capability.
Read-only security at creation excludes that permission writer but strips the
creator's WRITE_DAC right, preventing the tested in-place promotion.

The replacement must avoid automatic deletion while unrelated data/permission
capabilities can exist. Preserve uncertain empty or partial preparations rather
than deleting from their name or incomplete bytes. Canonical publication must
still require a complete verified frame and allow subsequent restart admission.
Actual factory, affected claim/journal callers and interruption cases need native
acceptance; public Copy stays disabled. The rejected Rust candidate is archived,
not installed. Exact controls and limits:
`.hermes/evidence/windows-ephemeral-security-current/FINDINGS.md`.
Earlier lifetime controls:
`.hermes/evidence/windows-directory-lifetime-current/FINDINGS.md`.

## Current claim preparation lifecycle

The current journal schema is 17 and the claim format is `OTCWCL03`. New controls
are ordinary persistent private files. There is no delete-on-close factory or
in-place temporary disposition transition. A root-level claim preparation is
flushed and verified before held-handle no-replace publication into its target
bucket. Close, an ordinary construction error or process termination preserves
unpublished bytes, including unrelated added streams. Empty/partial frames do not
occupy canonical keys and do not acquire deletion authority from their names.
Published keys always begin with a complete first Claim.

Explicit Copy admission now examines root-level preparation candidates through
held native directories, with a 4,096-entry work budget. A name only selects a
candidate. Cleanup requires an exclusive write/delete handle, one complete valid
first frame, matching transaction name, exact native file identity, current owner,
parent/root/bucket/anchor identities, the same central registry and no retained
local or central transaction record. Private descriptor, single-link count and
complete unnamed-stream bytes must independently revalidate before held-handle
disposal and a root directory flush. Added streams or hard links surface an error.

Live writers, partial/foreign frames, native replacements, misnamed records and
other registries never grant cleanup authority. Read-only recovery does not
collect preparations. Unknown or partial power-loss remnants stay preserved;
this rule adds neither a filename/age-based sweeper nor payload deletion rights.
Focused ordinary-user native regression passes 39 checks, including four new
preparation cleanup tests. The broader journal and post-change guest-power scopes
are recorded separately in the roadmap and acceptance receipts.

## Persistent journal setup and live-owner failure

The local journal and central `.preparing-<transaction>` control are private
persistent creations. The central marker and its directory are flushed before
creating the destination-local stage. Read-only discovery presents unpublished
preparations as review-only records, preserving their exact native path without
reading their contents, following directory/reparse targets or adopting a
transaction ID. This includes empty, partial and complete unpublished journals;
a matching filename grants no Resume, Undo or cleanup authority. The record may
belong to a live writer, so presentation does not claim it is abandoned.
One journal writer and frame encoder handle both initial and
subsequent records; initial binding checks use the held preparation's actual name.
Only after the complete paired Begin frame is flushed is the central handle
renamed without replacement to `<transaction>.copy-journal`, followed by a registry
directory flush and normal canonical binding validation. No payload command runs
before this constructor returns successfully.

A live construction error returns the unstarted claim to its caller for release.
Every setup control/stage is preserved, and the error reports retained setup for
verification. Foreign canonical collisions and added local/central streams stay
intact. The former empty-stage rollback and automatic file-disposition paths are
removed. A fresh destination reservation can proceed after successful live-owner
release. Fresh Copy admission also holds the prior name-claim writer exclusively.
If retained local and central controls form the exact matching Begin-only pair,
admission can finish the old reservation's release before publishing a fresh claim.
This uses the existing paired-journal decoder and identity/security/binding checks,
including the current creator and parent case policy. Both controls stay read-leased
against writes/deletion throughout the release and flush. A canonical collision is
never bypassed in favor of an unpublished record. No old stage, journal control,
added journal stream or unrelated sibling is deleted or modified. Only the name
reservation ledger changes; its ordinary retirement still uses existing validation.

This retry does not grant Resume, Undo or payload-cleanup authority to the retained
setup. A partial/unknown or mismatched pair, replaced control, changed parent policy
or any record after Begin cannot authorize this release. Controlled reclamation and
recovery for those interrupted setups remain open, along with resource bounds and
broader independent deletion/cleanup interference cases.

The current source passes ten focused ordinary-account setup/recovery tests,
the full core journal selection (138 passed, six explicit exclusions) on the same
executable, three grouped-cleanup shared-prepare tests, five separate dispatch
tests, and strict Windows all-target Clippy and formatting. Coverage includes live-owner
exclusion, real factory hard kills at empty and complete unpublished setup,
retained foreign streams/files, refusal of post-Begin intents/mismatches/replaced
controls/canonical collisions/parent-policy drift, all six claim-release fault
boundaries and write/delete exclusion on both journals during release. Broader
regression, exact source/executables and teardown:
`.hermes/evidence/windows-setup-recovery-current/ACCEPTANCE.json`.

These are native status/admission checks, not newly rendered Activity or new
whole-Copy power-loss acceptance. Earlier guest cuts at complete-unpublished and
published-claim barriers passed reservation recovery with protected bytes intact;
they retain their original source/executable scope:
`.hermes/evidence/windows-persistent-controls-current/ACCEPTANCE.json`.
Earlier review-visibility evidence is retained in
`.hermes/evidence/windows-unpublished-setup-current/ACCEPTANCE.json`.

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
   children and metadata, and supported links. NTFS sparse-stream component scope
   is recorded below; EFS and broader storage/provider/property semantics remain
   required. Final permissions follow the accepted Copy policy independently of staging.
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

## Operation admission and native target reservations

The application queue owns one complete path footprint per running operation,
independent of whether that operation supports Undo. Delete and permission changes
reserve their targets; Rename reserves both names; Copy and all Move variants
reserve their destination directory, including conflict-selected names. Copy sources
are read claims: independent readers may run together while excluding writers;
Move source claims are mutations.
Forward Copy recovery retains every plan path pair, including displaced originals.
Undo checks those footprints, and new file operations check running operations and
Undo before queuing a worker. The same admission applies to administrator retries,
text saves, image rotation and Trash actions. Completed work drops queue-only path
storage; Undo paths remain in its actual receipt. Informational Properties does not
reserve data.
Unknown mutation footprints cannot claim a disjoint-path exemption. Cleanup retains
its existing whole-queue exclusion until its worker actually terminates.

These are in-process scheduling guards, not native ownership or cross-process
reservations. Lexical path checks do not establish native alias equivalence.
The existing-target case-alias regression now proves ordinary native admission
rejects creation at a differently cased occupied name and admits Overwrite without
changing original bytes. All four destination-admission tests pass on the same
ordinary-account Windows executable. This does not establish absent-name claims,
explicitly case-sensitive directory behavior or cross-process exclusion. Exact
scope and teardown are in
`.hermes/evidence/windows-native-name-admission-current/ACCEPTANCE.json`.
An additional normal-user native regression now explicitly enables and queries
NTFS directory case sensitivity. Differently cased names are admitted as distinct
only when actually absent; two existing variants retain independent bytes, and a
late occupant invalidates creation admission. Disabling case sensitivity makes the
previously absent spelling resolve to the existing file and invalidates its old
creation approval. All five destination-admission checks pass on one executable.
Evidence: `.hermes/evidence/windows-case-policy-current/ACCEPTANCE.json`.
This does not establish short-name alias equivalence or cross-process exclusion.
Admission now captures the native case policy independently of current target
occupancy. A changed policy invalidates both creation and Overwrite approval,
even if the name remains absent or the approved original is unchanged. Policy
queries use the held parent handle, reject unsupported/unknown results and are
rechecked around target validation. Schema 14 persists that decision in both
journals; writer and snapshot validation enforce it during later operations.
This pins naming semantics but does not itself claim or reserve a target name.
The six focused ordinary-user native tests pass. The same executable accounts
for all 181 journal cases in disjoint core/cleanup batches: 172 pass, zero fail,
and nine specialized fixture/child entries are ignored with exact reasons
recorded. Original source, retained/public data and journal bytes survive policy
refusal in staged and published fixtures. Source, binary, case lists and clean
teardown: `.hermes/evidence/windows-admission-policy-current/ACCEPTANCE.json`.
The native target-reservation gate remains required before public Windows Copy:

- Bind claims to the actual destination-parent identity and native target name;
  account for native case policy, directory aliases and changed locations.
- Persist intent before any displacement/publication and admit new transactions
  against outstanding forward/Undo claims, including currently absent targets.
- Close simultaneous-admission races without globally serializing independent
  copies or relying on cached Activity records. Process exit must not lose claims.
- Read-only discovery must preserve unknown, corrupt or divergent evidence and
  refuse uncertain admission. Cleanup may release only durably relinquished claims.
- Accept separate-process concurrent creation/Overwrite/Rename, interrupted
  withdrawal/restoration, parent rename/alias, changed evidence and unrelated-copy
  progress with exact native data-preservation assertions.

The queue correction does not satisfy that native gate by itself. Current source,
checks and package scope belong to the roadmap's operation-path checkpoint.

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
cancellation, failure and unwind. Candidate admission, copy rereads, metadata
verification and tree finalization all forward the caller's cancellation callback
through each backup-stream chunk. Verification reports the last copied-byte
count, so rereads neither reset progress nor count as extra copied data. This is
cooperative cancellation between native calls, not blocked-device/network I/O
isolation.

Activity forward/name-release recovery and cleanup now bind native cancellation
to their existing dedicated worker thread. `NativeIoCancellation` duplicates a
real thread handle with only the required cancellation access, watches the existing
Stop flag, and retries `CancelSynchronousIo` across call-entry races. A thread-bound
guard stops and joins that watcher before unrelated work or completion delivery.
Drop also joins on unwind; explicit finish errors retain the original operation
error and cleanup completion information. No watcher survives an idle operation.

Three ordinary-account native tests pass for genuinely blocked synchronous pipe
reads/writes, early requests, repeated Drop and later successful I/O on the same
worker while the old flag stays set. These tests prove that native cancellation
can abort those calls, not that every filesystem/device supports it. A request
cannot release path ownership or announce completion until the worker has actually
stopped and its journal outcome has been inspected. Windows cancellation can
complete normally, abort or fail. Device/network, other workflow and full resource
acceptance remain open. Exact scopes:
`.hermes/evidence/windows-copy-blocked-io-current/ACCEPTANCE.json`. See
[CancelSynchronousIo](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-cancelsynchronousio)
and [Microsoft cancellation considerations](https://learn.microsoft.com/en-us/windows/win32/fileio/canceling-pending-i-o-operations).

The codec streams primary data, alternate streams, extended attributes and admitted
property records through bounded buffers. Names retain raw UTF-16, each stream has
length and SHA-256 evidence, and both candidate and reread source must agree.
Empty file primary streams are explicit even if omitted by native enumeration;
directories have no synthetic primary stream. Source hard-linked names produce
independent candidate data. Security, hard-link relationship and unknown backup
records are refused rather than silently omitted. Sparse backup records use the validated path below.

Ordinary metadata includes creation/access/write times and read-only, hidden,
system, archive, temporary, offline and not-content-indexed attributes. A copy keeps
its own change history. Unknown/specialized attributes stay explicit requirements.
NTFS compression is queried/applied through native handles, including clearing an
inherited compression state for an uncompressed source. Broader filesystem support
is not established by NTFS component tests.

Native versions now include case sensitivity for ordinary NTFS directories;
files and reparse links have no directory-policy value. The shared metadata path
applies the captured source policy to a new, still-empty candidate before streams
or children are staged. Each nested directory keeps its own policy independently
of its parent. Staging/final/retained metadata validation rejects policy drift
without repairing it. Source-version and final-version comparisons include the
policy, and malformed directory/file/link policy shapes are rejected by the
journal codec. Schema 15 records this evidence in both paired journals.

The native workflow covers empty/populated trees across all four source/parent
policy combinations, nested opposite policies, distinct case-variant file bytes,
publication, Undo and byte-preserving refusal of retained-candidate policy drift.
Exact source and verification scope are recorded in
`.hermes/evidence/windows-directory-policy-current/`. This does not establish
all specialized storage, interruption or public workflow gates.

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

### Sparse NTFS streams

The stream engine now admits sparse primary and alternate data streams on files
and directories. It validates each `BACKUP_SPARSE_BLOCK` against its current data
stream, rejecting missing contexts, malformed sizes/attributes, overlap, range
overflow, incomplete data and records after EOF. The native offset-bearing EOF
record preserves trailing holes. Only validated native records reach BackupWrite;
security and opaque reparse records retain their separate guards. EFS is still
explicitly refused.

Sparse evidence stores logical length and a SHA-256 fingerprint of fixed 64 KiB
logical chunks containing nonzero data, plus the logical EOF. Zero chunks are
implicit. The domain is `ottrin-windows-sparse-v1`; offsets and EOF are encoded
little-endian. This fingerprints logical contents independently of native buffer
and allocation boundaries without reading or hashing large holes. Dense-stream
SHA-256 remains a hash of the ordinary stream bytes. Existing evidence fields and
journal schema 17 remain authoritative; the stream sparse attribute selects this
newly admitted fingerprint. The base file sparse attribute is checked against its
primary stream, independently of sparse named streams.

Six focused native tests pass. Coverage includes all-hole/empty files and named
streams, directories, a compressed candidate, a 1 TiB logical sparse file,
cancellation, same-size tampering, malformed ranges and boundary-independent
fingerprints. The 1 TiB payload fixture reports 128 KiB through the copy-data
callback, with at most 128 KiB allocated to each checked target data stream.
Verification rereads are separate; this is not a whole-application I/O benchmark.
The durable workflow stages, publishes, removes its source, Undoes and cleans up
mixed sparse/dense trees while preserving an unrelated destination file. Both
sparse-primary and sparse-named-only cases pass through the journal validator.
On the final corrected executable, the core journal suite passes 139 tests with
six explicit exclusions, all six sparse checks pass, and all 50 ordinary
stream/metadata/tree tests pass. The EA fixture buffer now has explicit longword
alignment and retains its native-status diagnostic. This fixes the test producer;
EA preservation checks remain intact.

Exact source/executable scopes, earlier raw-format probes, fixture corrections,
broader regressions and teardown are recorded in
`.hermes/evidence/windows-sparse-current/ACCEPTANCE.json`.
The later cross-volume test reuses the same durable workflow on two owned NTFS
volumes, with 4 KiB versus 64 KiB allocation units. All four cases pass: both
copy directions, each with and without a sparse primary. Sparse file/directory
named streams and unaligned data offsets exercise allocation-independent
fingerprints through publication, discovery, source-free Undo and cleanup.
The 1 TiB logical primary also passes on the small secondary volume. This is a
logical preservation check, not throughput or physical crash acceptance.
Evidence: `.hermes/evidence/windows-sparse-cross-volume-current/ACCEPTANCE.json`.
Broader filesystem/provider, EFS, blocked-I/O and resource acceptance remain
separate gates. No package changed; public Windows Copy stays
disabled. The native format is documented in [Microsoft's sparse block format](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-bkup/6be866e6-6d1f-4183-8b78-b10c2941228a).

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
bound to native parents/directories/file identities. The current format records the
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

Cleanup derives authority from existing paired transaction evidence: a completed
forward transaction or completed Undo, verified native bindings, creator/private
security and journal-only stage membership. Published copies and restored
originals remain user data. Cleanup neither requires their contents to remain
unchanged nor treats them as removal input.

| Terminal phase | Retained payload | Public user data |
| --- | --- | --- |
| New copy completed | None | Published copy |
| Overwrite completed | Displaced original at its recorded private name | Published copy |
| Undo completed | Withdrawn copy at its recorded candidate name | Restored original or reused public name |

The current journal schema is `OTCPWJ15`. The immutable paired header also
binds the destination parent's native case-sensitivity policy. Writer bindings
and reopened snapshots reject a changed or unreadable policy before granting
publication or recovery authority. Prior schemas remain review conditions.
Paired `Cleanup::Begin` binds the
immutable retained role, count and manifest digest. Entry intents and
acknowledgements follow reverse manifest order, removing children before parents.
Starting cleanup revokes forward/Undo authority. A peer's claims then cover its
remaining entries, pending unlink and transaction metadata, rather than historical
publication/source graphs. A local-only Begin does not release the central
record's earlier claims. Older schemas remain review conditions; they are neither
converted nor silently deleted.

### Admission and dependency catalog

`cleanup::Admission` is a shared, handle-free read-only observation. It freezes the
header, journal/private-metadata and observed-version digests, pending own/peer
absence, registry catalog, destination location and parent permissions. Raw
journals and decoded graphs are released after observation. A writer reacquires
and compares those exact observations; opening an ID again cannot refresh a
confirmation's authority.

The registry is existing-only and anchored to its native identity, owner and
private security. Writable registry access requests read/write, not DELETE, so
it remains compatible when the registry is also the destination parent. Missing
registries are not initialized during observation or cleanup. Registry roots,
external anchors, unrelated transactions and public files are never deletion
inputs.

Peer inspection retains compact control leases, native versions, creator/private
security and record digests, releasing decoded graphs after checking references.
Held claims recheck bytes, version, bindings and single-link state. Identical-byte
rewrites, timestamp drift, aliases and unverified peers refuse mutation. Catalog
fingerprints encode native names as raw UTF-16; suspicious suffix/case variants,
including unpaired UTF-16, are preserved as NeedsReview without a transaction ID
and block destructive admission. Ordinary unrelated registry files remain allowed.
An independent peer stays untouched; a shared retained object blocks an ordinary
single-transaction cleanup.

### Native deletion and ordinary action

`cleanup_tree::Remaining` opens only recorded names relative to verified parents.
Deletion uses READ/DELETE capabilities, POSIX disposition with readonly-ignore and
image-section checks, explicit closure of all clones of the delete FileObject,
real NotFound verification, parent flush and surviving-metadata verification
before acknowledgement. Access Denied never means absence. Readonly attributes
are not cleared; closed-handle writable mappings block deletion capabilities.

Contents, streams, identity, permissions, compression and link counts stay strict.
Only an acknowledged unlink advances its immediate parent's modification/change
times and surviving aliases' change time/count. Outside aliases grant no deletion
authority. Per-object accounting avoids rescanning the full deletion order for
each record. A pending own unlink can resume after actual absence only if every
other recorded observation still agrees. Retained-root absence is rechecked on
every tree revalidation; a later occupant invalidates that absence proof.

The writer reserves the complete remaining journal tail before mutation. Any
uncertain append poisons the writer. Matching durable mirrors can be freshly
admitted; a local-only append leaves divergent mirrors for review, without
repair. Cancellation and foreign changes stop further work.

`RetainedCopyCleanupPlan` provides configured, existing-only admission for one or
several selected transactions through the same group executor. It exposes the
selected IDs, durably completed IDs and whether retry requires retaining the
existing proof. Completion is idempotent without reopening removed controls.
Uncertain payload/intent writes require fresh admission; known metadata/shared
removal retains its original proof. Error classification reflects acknowledged
cleanup progress and keeps the recovery location available.

### Group execution and known writer handoffs

`cleanup_group::Observation` admits canonical, unique, present selected IDs while
freezing selected and unselected control proofs, registry epoch/names and private
security. It records cross-member references and shared retained objects, then
rechecks every member and the full catalog. It retains no open files, raw journals
or decoded peer graphs. Selected IDs permit inspection, never a blanket physical
dependency bypass. Unselected claims and unrelated metadata remain protected.

The start barrier validates every confirmed payload and partial-metadata proof,
including parent permissions, before the first Begin. Each successful writer
returns its exact prior/new control digest and native control epoch. The group
advances compact member catalogs only from that acknowledgement; it publishes
known progress before a later cancellation/revalidation can fail. Uncertain
writes require fresh read-only admission. Repeating a completed barrier appends
nothing.

Physical writers also acknowledge the registry's verified post-unlink names and
version. When the destination is the registry, only the expected retained-root
name may disappear; all unrelated names and root attributes remain frozen.
Completed payload proofs survive other members' known changes without retaining
duplicate filename lists or adopting new claims by ID.

`Observation::reclaim_all` prioritizes durable pending peer/owner intents, then
advances the actual next entry in journal order. Shared aliases use the protocol
below. Ordinary entries use a current-entry dependency scope that protects the
actual native object and private metadata without exempting selected peers.
Whole-member and single-entry cleanup call the same native deletion primitive.
All payloads must finish before metadata removal.

### Shared-original alias protocol

A peer intent binds one remaining local original-file alias to a distinct owner
transaction/control, native parent/raw name, entry index, pending sequence and
intent-prefix digest. Same-entry names, directories, unbound references and
impossible link counts are refused. Pending peer intent blocks ordinary cleanup
transitions and protects the owner's canonical control until acknowledgement.
Preparation reserves the intent, worst-case acknowledgement and full remaining
tail before any write. It refuses switching to another pending owner and supports
idempotent preparation retry.

`cleanup_peer::Observation` pins the canonical control, authenticated prefix ending
at the owner's exact pending entry intent, paired mirrors and native retained
slot. Transaction, control, parent/name, object version and link count must agree.
Historical prefixes remain verifiable after owner acknowledgement. Snapshot
loading duplicates the held control instead of reopening with incompatible
Windows sharing flags. Creator/private security, identity, single-link state,
namespace bindings and mirror equality retain the common validation path.

The physical step permits overlap only for selected co-owners with the exact
matching pending intent. It unlinks one owner alias, publishes the known native
and catalog change, validates and durably acknowledges each peer, then records
the owner's EntryRemoved. Completed shared claims are retired; objects with fewer
than two distinct selected owners return to ordinary handling. No other metadata
or unselected claim receives this exemption.

Peer acknowledgement permits only the recorded native version with updated change
time and exactly one fewer hard link. All surviving local aliases must still
match content, streams, permissions, compression and every other metadata field.
Its read-only Remaining capability refuses acquire/remove. Local and peer proofs
remain held through final revalidation and paired append. Incompatible owner
control leases are released before opening that validator.

All local aliases share the resulting expected version/count. The deduplication
key binds owner transaction/control/entry, independently of local representative,
sequence or digest, so replay cannot count one physical unlink twice. A fresh
observer can validate the recorded pending-peer native change without inventing
authority for a missing name. Acknowledged successful physical changes can be
retained for an in-session peer-busy retry; uncertain native/write results require
fresh admission or review.

### Metadata removal and history after interruption

Paired MetadataRemovalIntent is allowed only after PayloadRemoved. Metadata must
contain exactly the verified journal data or empty directory streams, single
native links, private ownership and expected bindings. Permissions/version are
checked immediately before each unlink. Removal proceeds local journal, empty
stage, central journal, closing and flushing each parent before proceeding.
Unknown entries/streams, replacement occupants and control aliases are preserved.

A group retains the original authorized Plan while metadata removal is pending,
freezing other actions. Reconciliation permits only that Plan's expected central
journal disappearance and, when applicable, stage removal inside the registry.
All surviving controls, unrelated names, security and other root attributes remain
frozen; root modified/changed times advance only with the expected removal. The
Plan inspector verifies the authorized layout before and after reconciliation.
Once its final absence/flush checks succeed, the member stays Complete without
reopening a deleted control.

Read-only metadata inspection accepts authenticated local-removed and stage-removed
prefixes without flush, disposition or append. A surviving central intent can
admit a fresh source-free retry. Absence of the final central journal alone cannot
construct authority for an old ID; a retained opaque Plan can finish its own
known removal. History lists surviving records, and a new selection must be
freshly admitted from those records.

Discovery uses the same payload/metadata inspections and rechecks central bytes.
VerifiedCleanup describes observed remaining cleanup evidence, never a deletion
token or complete-tree verification. The surviving central journal is the recovery
location after local/stage removal. Unknown or divergent evidence remains
NeedsReview. A shared dependency can require group admission even if an individual
history inspection cannot authorize that member. Activity must keep observation
and action authority separate; constructed-row tests are not rendered acceptance.

### Windows Activity integration

Activity offers cleanup for completed copies, completed Undo and interrupted
cleanup. Staged/unfinished forward recovery and unidentified records have no
unsupported discard action. A new discard requests confirmation; existing cleanup
intent and explicit retry retain prior authorization. Canceling confirmation
performs no cleanup. Existing theme styles apply to the dialog and controls.

The Windows worker admits the selected cohort before mutation and performs native
validation/deletion off the UI thread. Stop uses the native progress callback.
A shared mutex slot holds the actual opaque plan before thread creation, preserving
its authority across worker outcomes and failed spawns. Poisoned state refuses
further execution for review. No held files are stored in the UI.

When metadata removal needs the same proof, retry reuses that object even if its
final journal is absent. Retrying any card in the previously authorized cohort
resumes its remaining members. Unrelated selections cannot replace pending
proof. Activity removes only IDs reported durably complete; partially completed
cards retain their remaining ownership. Failed cleanup clears stale Undo/forward
capabilities and leaves an explicit review/retry state. Discovery remains an
observation, and IDs never become removal authority.

### Verification and remaining gates

All receipts are under `.hermes/evidence/windows-copy-cleanup-current/` and bind
their own source, executable, profile and scope. Earlier component receipts are
historical evidence, not claims about a newer executable.

- `ACTIVITY_ACCEPTANCE.json` and `ACTIVITY_COVERAGE.json`: two native public-action
  tests pass in 45.62s, including the 64-shape single-member metadata-retry matrix
  and a mixed three-owner cohort. One Windows integration parent asserts all 15
  subprocess scenarios in 12.56s: the prior nine Undo/forward-recovery cases plus
  confirmation/cancel, changed retained data, busy-file retry, stop after final
  journal removal, a two-operation retry from one card, and partial completion.
  A separate status/action test passes on the same UI executable. Native lists
  match all four reported parent test names; processes and temporary root are
  absent afterward. These exercise egui events/layout and real filesystem workers,
  rather than native desktop rendering or a packaged build. Broader cleanup/journal
  suites were not repeated for this adapter change; their earlier receipts keep
  their source/executable scopes.
- `GROUP_EXIT_ACCEPTANCE.json` and `GROUP_EXIT_COVERAGE.json`: all 32 enumerated
  grouped-cleanup cases on one executable, 31 passed/zero failed/one parent-owned
  child entry ignored in 508.37s. Both that run and its separate focused run
  execute all 35 actual child process exits. Only test source differs from the
  preceding full cleanup checkpoint; production source is unchanged.
- `GROUP_MIXED_ACCEPTANCE.json` and `GROUP_MIXED_COVERAGE.json`: six focused mixed
  workflow tests in 51.23s; all 74 enumerated cleanup cases on that same executable,
  72 passed/two existing ignored/zero failed in 633.69s. Three-owner mixed trees,
  cancellation, uncertain ordinary unlink/fresh admission, prepared shared owner
  and foreign changes are covered.
- `GROUP_SHARED_UNLINK_ACCEPTANCE.json` and its coverage receipt: seven focused
  cases, including multiple owners/local aliases, an outside alias, registry-root
  data, unselected owners, foreign changes and native/paired-write failures.
- `PEER_BINDING_ACCEPTANCE.json` and its coverage receipt: all 153 journal cases
  accounted for across two disjoint completed batches on one executable: 145
  passed/eight existing ignored. These include cleanup 51/two ignored and other
  journal tests 94/six ignored; a separate four-test peer-binding focus passes.
- `PEER_UNLINK_CODEC_ACCEPTANCE.json` and its coverage receipt retain the earlier
  timed-out all-in-one run and the completed disjoint remainder. No timeout is
  relabelled a complete one-shot pass.
- Earlier invariants and fixture evidence remain in `ACCEPTANCE.json`,
  `UNLINK_PROBE.json`, `UNLINK_FORCE_PROBE.json`, and the registry-writer,
  admission, metadata-inspection, discovery, configured-action, catalog, start,
  group-observation/start/payload/metadata, peer-ack and shared-preparation receipts.
  These include the earlier 187 injected failures/187 process exits, partial
  metadata prefix inspections, private-security/stream/mapping refusals and the
  constructed native Activity status test.

Native correctness runs use an ordinary account, a private temporary root and an
owned offline COW. Developer Mode for link fixtures applies only to that owned
image; source profiles/installations and backing images are preserved. The runner
records exact executable hashes and retains non-debug PE sections when stripping.
Its watchdog terminates the owned process tree and checks temporary-root cleanup;
terminal tasks without a result are failures. Full Windows platform/UI strict
checks and formatting remain required for affected code.

The focused group crash/history matrix passes two parent tests in 238.62 seconds.
They assert exit code 86 at all 35 child boundaries: Begin; ordinary intent,
deletion and acknowledgement; owner/peer intents; shared unlink; peer/owner
acknowledgements, including later peer and final owner with three co-owners;
metadata intent and all local/stage/central native removal boundaries. Ten
local-only appends preserve divergence; 25 paired/native prefixes fresh-admit and
complete. Retained-copy discovery is read-only at every interrupted state and
empty after completion; source trees are absent and public/outside bytes are
preserved.
The child entry is explicitly ignored in ordinary enumeration and invoked only
by its owning parent. The full grouped-cleanup subset passes as recorded above.
Exact native enumeration matches every reported outcome. The ordinary-account
task finishes successfully with no owned native process or temporary root left.
Unrelated cleanup/journal suites were not repeated for this test-only change;
their earlier source/executable scopes remain explicit.

Native desktop Activity acceptance, reserved-target protection,
interrupted-staging reclamation,
measured resource/storage/security matrices, physical power-cut acceptance,
broader workflows and exact current package/UI acceptance remain open. These
unoptimized backend tests establish neither application performance nor rendered
acceptance. Public Windows Copy stays gated; application packages are unchanged.

### Activity cleanup wording after Undo

Native-window inspection found that an already-undone Copy incorrectly offered
“Discard Copy undo.” The action and confirmation now describe clearing retained
data; selections with available Undo retain the explicit Undo-loss warning.
The Windows Activity integration passed on the corrected executable (13.87s,
ordinary QA user, no temporary entries left); the two focused Linux cleanup
tests also passed. `WORDING_ACCEPTANCE.json` records the exact source and native
test hashes. Clean candidate packages from `b87228d` now have scoped native-window
acceptance: Windows single cleanup/Cancel/changed-data refusal/two-operation
cleanup, and Linux AppImage available-Undo warning/Undo/retained cleanup in Light
at 100%. Sources and public files are preserved; test apps, daemons and the Linux
private display/profile are removed. The original Windows profile/installation
hashes and login configuration are unchanged. Portable/NSIS/MSI payload identity
covers all 57 files; all four Linux formats pass current content/source checks.

Receipt: `.hermes/evidence/windows-activity-desktop-current/ACCEPTANCE.json`.
That checkpoint is superseded for Activity presentation by clean source
`693e515`: readable summaries, expandable selectable diagnostics, wrapping
controls and consistently aligned status badges now have packaged native-window
acceptance. All four Linux formats are delivered at `dist/packages/`. Linux
Light/Dark at 100% and paired custom Light/Dark at 150% with text1.35 pass; the
ordinary Windows default-renderer window passes Light100 refusal/disclosure and
custom Dark text1.35 disclosure. Windows 150% DPI is not claimed. The portable,
NSIS and MSI payloads each match all 57 files; Windows remains an internal
candidate at `.hermes/packaging-tmp/windows-activity-candidate/`.
Current receipt: `.hermes/evidence/windows-activity-presentation-current/ACCEPTANCE.json`.
Actual desktop Stop/retry and broader current-package lifecycle/workflow gates
remain open. These scoped checks do not establish daily-driver readiness or
enable public Windows Copy.

### Recovery discovery on a separate NTFS volume

The specialized secondary-volume fixture now has ordinary-user native execution
on the current schema-15 test binary. It places the central journal on C: and the
copied directory on a separate owned NTFS VHDX, asserts different native volume
serials, publishes and discovers one verified completed transaction, and checks
the nested file's exact bytes. The separately selected ignored-by-default test
passes (1/1, 0.20s); earlier suite counts retain their original scope.

The controller guards the image-to-disk association, original disk identities,
volume identity, actor SID and ownership marker. Its disposable backing VHDX must
grant the actor Full Control before attachment: inherited Modify-only access
allowed file creation but blocked DACL changes inside the attached drive. That
fixture-only correction and the initial failure/control probe are retained.
The owned image is detached/deleted after testing; original disks, profile,
installation and login settings are unchanged and the VM is shut off.
Receipt: `.hermes/evidence/windows-secondary-discovery-current/ACCEPTANCE.json`.
This establishes neither physical/removable/network acceptance nor public Copy.

### Existing-object Overwrite dependency admission

Path dispatch now captures the native identities of the complete admitted
Overwrite tree and compares them with the authoritative retained-object references
already used by cleanup. This prevents displacement of another retained Copy's
publication, descendants, transaction directory or control objects. Native
identity matching also covers a case-variant spelling of an occupied target.
The check runs before creating a new stage or journal. Unrecognized recovery
filenames, incomplete evidence, changed registry catalogs and disagreeing paired
journals refuse admission without modifying recovery data.

Only read-only native leases remain after each peer is decoded; its bytes and
graph are dropped. Parent, stage and both control bindings remain held through
the admitted Overwrite. An unrelated existing file remains usable against a
complete, unchanged catalog. The original-tree identities come from the existing
admission scan rather than a second traversal.

This is existing-object dependency protection within the supplied registry.
It is not durable absent-name reservation, a simultaneous-admission barrier or
cross-registry exclusion. Interrupted metadata cleanup with missing local controls
must be resolved before Overwrite; the check does not infer release from a lone
central record. Public Copy and the complete target-reservation gate remain open.

### Native claim-namespace foundation

`AnchoredRoot` now provides the single external-anchor implementation for the
configured recovery registry and a destination-local target-claim namespace.
The latter is created through the held ordinary destination parent, with its
actual volume and directory case policy. Only a newly owned empty root has its
policy configured. Existing mismatched policy, unanchored contents, foreign or
partial anchors are retained and refused; observation never initializes them.
Both root roles keep the existing native identity/creator/private-security checks.
The owned-empty-directory case setter is shared with ordinary Copy metadata.

The target namespace uses filesystem create-new behavior, rather than a
user-space case-folding or Unicode-normalization algorithm. This is the namespace
foundation only. Target claim records, journal binding, safe release/recovery,
short-name canonicalization and normal Copy routing must still be implemented;
public Windows Copy remains gated. Existing-object Overwrite dependency protection
continues to apply in the current dispatcher.

Native verification passes all seven registry/namespace parent tests (1.10s,
zero failures/ignores), including three barrier-controlled two-process cohorts.
Colliding names produce one owner; distinct names produce two. Flushed entries
survive hard child exit and remain occupied. Both native directory case modes
cover ASCII/non-ASCII variants, distinct normalization forms and raw UTF-16.
The existing mixed-tree directory-policy regression passes separately on the
same executable (2.67s). Strict Windows platform all-target Clippy and formatting
pass. The owned VM is shut off after temporary data/executable removal and
profile/installation/login-state verification. Receipt:
`.hermes/evidence/windows-native-claim-root-current/ACCEPTANCE.json`.


### Durable name claims bound to Copy journals

The dispatcher now acquires a destination-local name claim before private staging.
The claim records the destination parent, anchored root and bucket, ledger file,
central registry, native target name, transaction and creator/security identities.
Schema 16 binds that generation and its frame digest into both Copy journals.
An append-only, checksummed ledger retains reservations across owner exit. A
writer capability remains held from admission through staging/publication; a
busy owner's complete verified active generation reports a name conflict.
Unknown, empty, torn, changed or unanchored controls are retained and refused.
Skip and Rename use this same admission, including an absent but reserved name.
Overwrite resolves an existing native leaf and validates that leaf's identity
before claiming it. Existing-object dependency protection still applies.

Two anchored buckets preserve native case matching without exposing claim lookup
to DOS 8.3 aliases. Targets of at most 12 UTF-16 units receive a fixed suffix in
the short-name bucket; longer targets retain their complete name in the long-name
bucket. Every lookup exceeds the maximum 8.3 alias length. Separate buckets prevent
an encoded short name from colliding with a literal longer user name. No case
folding, normalization, truncation or name hashing supplies namespace authority.
The ordinary-user NTFS probe confirms that native control creation generates an
8.3 alias and that opening it resolves the long-name object; this is a measured
constraint, not an assumed filesystem property.

Only verified paired terminal Undo or cleanup-start state releases a generation.
Before journal construction is attempted, cancellation can release the claim
without payload authority. A completed generation remains verifiable and a new
owner appends a distinct generation to the same native ledger. A restored public
original ceases to be a retained-object dependency once restoration is durable;
retained candidates and control objects remain protected.

Claim admission also reserves the exact serialized release frame against the
ledger's size bound before writing the new generation. A full claim frame alone
is insufficient capacity: rejection must leave both file bytes and in-memory
sequence unchanged. No later claim can consume the active generation's reserved
space because the ledger admits only one active owner.

This is a source checkpoint, not completion of the reservation/recovery gate.
Released-log retirement, orphan/partial construction discovery, interrupted release
recovery and the complete separate-process integrated Copy/alias/ancestor matrix
remain required. Public Windows Copy stays disabled and packages are unchanged.
Current scoped source and native evidence are recorded in
`.hermes/evidence/windows-copy-claim-binding-current/ACCEPTANCE.json`.


The staged-process acceptance checkpoint adds three ordered native cohorts with
six child processes: one case-insensitive contender is refused and five owners
stage while live, then exit without destructors. Their absent public names remain
claimed. After source removal, all five retained stages publish, Undo and permit
name reuse. Independent owners reach readiness before either is allowed to exit.
This verifies cross-process held staging and post-exit recovery; simultaneous
initial claim construction and the complete Overwrite/Rename matrix remain open.
A recovered paired cleanup Start may append its pending bound release, while the
paired journals and all unrelated bytes remain unchanged. Scoped evidence:
`.hermes/evidence/windows-copy-claim-process-current/ACCEPTANCE.json`.


### Guarded completion of an interrupted claim release

Read-only snapshots may recognize an incomplete claim tail only when it is the
exact canonical release prefix for that binding's own last active generation
and the local journal state is terminal. Recognition grants no mutation authority:
both journal bytes, native identities, private security and namespace bindings
must independently revalidate before writer construction. Ordinary admission
continues to reject incomplete claim logs.

The explicit terminal writer reacquires the claim with write/delete exclusion,
rechecks its complete contents and native controls, truncates only that exact
known release prefix to the valid preceding boundary, then appends and flushes
the canonical release. An error consumes the writer; it does not retry an uncertain
append. Frame encoding has one implementation shared by append and recognition.
Unknown tails, checksum damage, incomplete initial claims, extra streams and a
later generation's partial release are never adopted or truncated.

The native focused set passes all 16 parents. A real release is cut at every byte
boundary, observed without mutation and completed by source-free internal Undo
replay while both journals remain byte-identical and retained data stays intact.
The negative matrix verifies divergent/nonterminal pairs, unknown/corrupt records,
ADS and later-generation ownership refusal. Exact source/binary scope:
`.hermes/evidence/windows-copy-claim-release-current/ACCEPTANCE.json`.

This is core writer recovery. Fresh completed-Undo admission still rejects an
already-undone operation. A distinct non-destructive finish-release admission and
visible pending-release status after restart remain necessary; cleanup reaches
the writer but also authorizes deletion of retained payload. Repair-I/O interruption,
physical-power and the remaining claim reclamation/workflow gates stay open.
