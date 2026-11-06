# Native Windows Copy

This document records the native transaction and security contracts. The current
ordered work, deliverable sources and remaining acceptance gates are maintained
in [the roadmap](ROADMAP.md); historical test/candidate receipts below keep their
original source and platform scopes.

Ordinary-user Copy/Paste uses the native engine with single/batch Undo, retained
recovery and per-operation Stop. Administrator Copy remains disabled until the
original caller's registry, staging, destination security, receipt, Undo and
cleanup authority are connected and accepted together. Authenticated caller
context and caller-owned private entry creation/reopen are implemented components;
they do not complete that transaction or packaged/alternate-administrator gates.

## Authenticated helper command routing

The helper routes Copy, Copy history, Undo and cleanup through its existing
kernel-authenticated `HelperChannel`. It captures one original-caller context
per command and retains it for the whole batch. The helper's environment,
profile and `DefaultPlatform` recovery configuration do not select that registry.
This routing does not enable the advertised Windows `--copy-recovery` capability.

Copy uses the existing batch-result and native transaction implementations.
Completed receipts survive a later item failure; Skip counts a completed source
without inventing a transaction. History independently inspects the caller's
canonical registry without provisioning it. Its paths and IDs are presentation
and lookup references, never serialized authority; discovery reports zero for the
original completed-source count because journals cannot establish skipped items.

Undo validates bounded, unique transaction IDs against fresh caller discovery,
then re-admits each inverse in reverse request order. Each inverse remains subject to the live overlap and journal guards. Cleanup admits
the caller's existing cohort writer; interrupted finalization retains the full
original membership even after some central journals disappear. A partial
selection of that pending cohort is refused.

The native command fixture sends actual framed requests and responses between an
ordinary caller and its distinct SYSTEM companion, using the same routing method
as the helper executable. It checks file/directory conflict handling, reverse
batch Undo, history before/after recovery, partial batch receipts and invalid
selection refusal. All 46 framed commands and 13 top-level native invocations
pass, including the existing caller engine and eleven ordinary recovery parents.
Independent originals retain their native identities, security and complete bytes;
only the two expected persistent caller-bound claim controls augment the parent.
The initial retained-overlap and coordinator fixture assumptions are corrected
without changing production guards. Exact verification and owned teardown are recorded in
`.hermes/evidence/caller-routing-current/`. This is the existing wire command set:
explicit Resume, end-attempt and name-release commands and their UI remain to be
connected. Operation Stop/progress must also reach the helper before UI enablement.
Newly authenticated process-restart, alternate-administrator UAC,
remaining native matrices and packaged acceptance are separate gates.

## Explicit recovery wire contract

Protocol4 adds `RecoverCopy { transaction, action }` for Resume, EndAttempt and
ReleaseName. The ID selects a freshly admitted plan in the original caller's
canonical registry; request and history paths never authorize recovery. The
shared core command classifier requires Copy capability negotiation for every
Copy/history/recovery action. Older helper protocols are rejected; a future
application package using this contract requires a matching installed helper.
Current stable packages retain their separately accepted protocol/source scopes.

History advertises an explicit recovery action only after its read-only native
admission succeeds. An ended attempt and a pending cleanup cohort have distinct
states. Review errors take precedence over lifecycle presentation. Pending name
release is admitted without opening the retained payload; verified unfinished
staging can Resume, and unverified staging can end without adopting its data.
Every command re-admits immediately before execution and retains original-caller
validation through the existing engine. No repair, caller environment fallback
or serialized authority is introduced.

Linux keeps its existing Copy/Undo/cleanup engine and presents interrupted cleanup
as CleanupPending. It rejects Windows-only recovery history before UI merge and
returns Unsupported for those explicit actions. Windows recovery frontend
integration and operation Stop/progress transport remain required before normal
administrator Copy capability is enabled. Its frontend must merge ordinary and
administrator entries by transaction identity: both use the same caller registry,
and live receipts must retain their ownership.

The eight-case native fixture passes within 128 actual authenticated requests
and all 13 top-level native invocations. It uses actual framed recovery requests
after deliberately prepared lower-level interrupted states. Sources, caller
preferences/index identity/security/bytes and edited unknown payloads are preserved;
terminal name release leaves both journals byte-identical. Exact source/build,
native assertions and independent teardown are checked in
`.hermes/evidence/caller-recovery-routing-current/ACCEPTANCE.json`.
The duplicate wrapper log's UTF8-to-ANSI artifact is exactly reconciled against the
independently exported native log; `WRAPPER_ENCODING.json` retains that limitation
and the original used actor. That historical receipt retains its encoding limitation; the newer protocol5
run below independently verifies exact UTF8 wrapper/native equality. No assertions
or result fields are omitted. This setup is
not public-dispatcher mutation-prefix or new-process/reconnection acceptance.

## Operation progress and Stop

Protocol5 adds typed progress and terminal frames for Windows Copy operations,
plus one Stop control byte on the same authenticated duplex connection. The
client checks cancellation again after elevation/connection and before sending
request bytes. A control reader can stop the helper's operation-owned synchronous
IO thread even while filesystem admission is blocked. All control/response readers
and the native IO watcher are joined before completion. A lost terminal response
closes the pipe and waits for the launched helper to exit before returning an
uncertain-operation error. Completed-prefix receipts survive operation or stream
cleanup errors; a completed operation wins a late Stop race.

This implementation passes all 26 native invocations and 150 authenticated framed
requests in `caller-stream-current/ACCEPTANCE.json`: nine pipe/lifecycle cases,
actual streamed caller Copy/history/Undo/cleanup, and real source-oplock Stop with
zero completed members or an already completed directory prefix. Both Stop cases
report native error995 before the blocking lease is released; completed-prefix
receipts remain actionable through streamed history, Undo and cleanup. Exact
source/executable/export bindings, UTF8 wrapper/native equality and independent
VM/disks/private desktop/profile teardown pass. Linux also passes 66 scoped tests
and nine actual unprivileged helper protocol/action probes. These cases do not establish Windows frontend, actual UAC,
the entire caller failure-prefix matrix or packaged acceptance. Protocol4 helpers
are rejected; future application packages require a matching helper. Current
stable packages retain their earlier accepted source/protocol scopes.
The first run was deliberately interrupted after source review found a
cancel-before-next-read race during callback unwinding. Its registered teardown
removed all owned resources; no partial runtime scope is accepted. The reader now
checks a stop flag around bounded reads while preserving partial JSON frames.
Invocation teardown closes the connection and observes helper exit on unwinding.
Panic, fragmented-frame and no-pending-read cases are in the fresh native lane;
exact pre-correction source and interruption evidence remain with its receipts.
The exact current executable passes all nine native transport/lifecycle tests in
`STREAM_PREFLIGHT_ACCEPTANCE.json`, under the guest-agent SYSTEM token, with owned
VM/disks removed. Earlier failed fixture observations remain retained separately.
This is a separate transport preflight; the full ordinary-caller run also passes.

The invocation owner additionally passes two native parents exercising three
actual child processes in `caller-invocation-current/ACCEPTANCE.json`. Disconnect,
a lost terminal response and an actual response callback unwind all observe the
retained process handle signalled before returning. The child deliberately keeps
working after pipe closure; neither EOF nor a witness file alone can pass the
assertion. Every child exits normally without its bounded watchdog intervening.
Production source and the preceding nine stream cases are unchanged, and native
VM/disks teardown is independently verified. This proves the invocation owner's
process lifetime mechanism under a native same-user fixture; it does not replace
actual ShellExecute/UAC, frontend or packaged helper acceptance.

The command adapter forwards a single fallible progress callback through Copy,
history admission, Undo, cleanup, Resume, end-attempt and name release. One freshly
captured original-caller context still owns the entire request. Callback failure
before a batch member starts preserves earlier completed receipts. History treats
invalid record evidence as a review state, but a failed callback ends the request;
it must not become a damaged-record row or a successful partial history response.
The callback-routing native fixture passes all 16 invocations and 137 actual
framed requests. Seven command kinds cancel at the first callback with unchanged
history and no completed Copy references. Successful requests forward actual
engine callbacks for every command kind, including positive copied-byte progress.
Three policy tests distinguish damaged-record review, interrupted requests and an
incorrectly swallowed callback failure. The full preceding caller/recovery lane
still passes; exact UTF8 wrapper/native receipt equality, source/build binding and
independent teardown pass in `.hermes/evidence/caller-progress-current/ACCEPTANCE.json`.
These pre-work stops do not establish mid-operation or blocked-helper cancellation.

Native acceptance of progress/cancellation frames and Windows frontend
integration remain required. Stop must reach the running helper even when its
filesystem thread is blocked, preserve its original caller binding, and await a
terminal result before releasing Activity ownership. A requested Stop is not
evidence that the operation stopped; a completed batch prefix and its receipts
must remain actionable. No elapsed-operation timeout may imply failure or restart
the producer. Administrator Copy remains disabled until the remaining gates pass.

## Authenticated private reopen

`CallerCopyContext::open_private_entry` reopens an existing single native component
through a held ordinary parent directory. It never creates, truncates or repairs
an entry. The returned handle must match the independently admitted native identity,
ordinary file/directory type, frozen caller owner/group and protected private DACL.
Read-only and writable access remain distinct. Scoped backup/restore privileges use
only a duplicate helper thread token; caller/profile validation surrounds the open.

This is a private-handle primitive, not transaction admission. Serialized identity
or a matching filename alone grants no mutation/cleanup authority. Canonical registry
anchors, naming/location/security, complete journals, links/streams and phase rules
remain mandatory. The ordinary-client/SYSTEM native fixture verifies successful
reopen, read-only write denial and unchanged refusal cases; it does not establish
alternate-administrator UAC or packaged Copy acceptance. See
`.hermes/evidence/caller-private-reopen-current/ACCEPTANCE.json`.

## Held original-caller registry authority

`CallerCopyContext::copy_registry_under` opens the fixed `copies` root and external
anchor under an independently admitted native parent. The returned
`WindowsCopyRegistry` is the existing anchored-root implementation with explicit
process or authenticated-caller authority. The authority is held in memory, never
serialized into a binding. Caller creation derives live native security from the
original token; caller reopen checks its frozen owner/group and private DACL.
Namespace identity validation uses the appropriate no-follow native opener through
one shared validator. Read-only discovery does not create state.

The registry holder retains caller/profile liveness checks for validation and
writable reopen. Its sibling cleanup coordinator receives the same authority.
Directory-flush handles retain the existing read/write mask without `DELETE` or
`WRITE_DAC`. Naming-policy initialization first observes the live native flag after
ordinary-directory and emptiness checks. An already-correct inherited value
requires no write. A different value still requires the native setter, its access
checks and exact postcondition. Required changes on caller-owned candidates use
an explicit original-token metadata operation, described below. Anchor identity,
parent location/security/case policy, root identity
and complete private security still require exact validation. Existing foreign,
unanchored or altered state is retained and refused, never repaired or adopted.

The low-level constructor requires the engine to admit its parent and reject
source overlap before initialization. `CallerCopyContext::copy_registry` now
performs canonical profile admission: it resolves the fixed `Ottrin/State/copies`
location from the pinned kernel-known profile, pins every selected source and
rejects native source overlap before creating even the first missing directory.
New initialization requires a bounded, nonempty absolute source selection.
Read-only discovery accepts an empty selection and never creates missing ancestry.

Existing `Ottrin` and `State` directories retain their contents and full native
security; their owner must match the authenticated user or distinct default file
owner. New directories use caller-derived private creation. Held ancestry pins
the profile, both fixed native child bindings, locations, full owner/group/DACL
and case policy. Validation and sibling coordinators retain this ancestry.
The handle-free registry binding now requires an `ancestry` array: empty for
ordinary-process roots, three directory records for canonical caller roots.
Missing cached fields are refused; serialized ACL bytes are never applied.
Fresh admission with different ancestor evidence cannot extend an earlier saved
binding. This is source/runtime admission, not full transaction authority.

The components below carry original caller authority through private staging,
required metadata changes, final destination policy and destination claims. The
full transaction still needs caller-aware publication, receipts and restored
Undo/cleanup plans; paired journal construction/reopening is covered below. Ordinary configured entry points retain their ordinary-process
environment locator; helper dispatch must select the canonical caller factory.
Administrator Copy remains disabled pending the complete bridge and native
alternate-administrator/package acceptance. Exact native acceptance is recorded
separately in the roadmap.

## Original-caller private metadata authority

`CallerCopyContext::set_private_directory_case_policy` accepts the engine's held,
newly created empty private directory and its independently admitted native ID.
The directory must match that ID, ordinary type, frozen caller owner/group and
protected private security. Existing retained state is never repaired; the engine
must establish create-new ownership and journal intent before this operation.

The context snapshots the kernel-bound original process token into an impersonation
token and checks its user/default owner/primary group against admitted values.
The native naming-policy setter runs under that original caller, without adding
helper backup/restore privileges. Native access checks, emptiness and exact policy
postcondition remain mandatory; no failed change is ignored. The private
owner/group/DACL record and native identity must remain unchanged afterwards.
Caller/profile liveness is validated around the operation.

One shared thread-token guard serves this operation and existing scoped helper
file privileges. It captures the exact prior thread token, installs the selected
token only for the synchronous operation, and restores the prior token on return,
error or unwind. A restoration failure aborts rather than continuing with an
unknown effective identity. Neither original process token nor prior impersonation
token is modified. This operation is carried by explicit private staging authority;
full caller transaction/recovery integration is still required. It does not enable
administrator Copy or prove
alternate-administrator UAC/package behavior. Native mechanism acceptance:
`.hermes/evidence/caller-metadata-current/ACCEPTANCE.json`.

## Explicit private staging authority

One in-memory `Authority` implementation now serves registry and private staging:
ordinary process or authenticated original caller. It is never chosen by serialized
permissions or an ambient global override. The existing ordinary opener remains
shared; caller opens use native no-follow handles and explicit helper file privileges.

A private stage retains its authority through creation and namespace validation.
Its top-level and nested candidates are created through the same explicit authority.
Caller candidates derive their protected private descriptor from the admitted
original token, with exact owner/group/DACL verification after creation. A genuine
`PerObjectFile` is constructed by the native create request using `MAXIMUM_ALLOWED`
and the explicit required read/write/delete/DACL mask; an arbitrary `File` cannot
be promoted to this capability.

The real pinned-tree/payload path takes authority explicitly. It creates private
children, applies required naming changes under the original token, validates
candidate bindings with the matching no-follow opener, and retains authority in
the staged tree for subsequent namespace checks. Progress boundaries validate
caller liveness. No alternate payload copier or separate staging implementation
is introduced.

Ordinary `DurableStage` construction still selects ordinary authority. The caller
fixture invokes this same private staging/payload engine with an observational
journal adapter; it does not persist caller claims or durable journals, publish a
file, or admit receipt/Undo/cleanup. Its retained root final-policy check is now
covered by the final permission-plan checkpoint below.
The dispatcher must retain the canonical registry and pinned source admission and
carry authority through those remaining transitions. Caller candidate creation and
per-object ACL behavior now have the dedicated original-token controls below.
Caller links, full final-permission/journal/recovery integration and package/UAC
contexts remain separate gates. Administrator Copy remains disabled. Exact scoped evidence is maintained at
`.hermes/evidence/caller-staging-current/ACCEPTANCE.json`.

## Caller candidate security controls

The fresh ordinary-caller/SYSTEM fixture now compares caller-owned private
candidates against children created by the actual original token under the same
native parent policy. Fifteen comparisons cover files, directories and descendants:
noninherited/default labels, inherited labels with `OICI` and `OICINP`, and audit
ACEs with `OICI` and `OICINP`. Private DACL protection does not change those native
inheritance results. Audit privilege is scoped to fixture setup/observation;
candidate creation uses the production caller API outside those observer scopes.

Two further scenarios finalize the candidate's DACL through the existing policy
engine under the original token, once directly and once through a cloned typed
handle. Root permissions must actually change, while three populated, unprotected
inherited descendants retain their full owner/group/DACL records and payload.
All handles permit sharing, so sharing denial cannot explain the lack of
propagation. These are controls for the native per-object handle contract, including
[the SDK's `MAXIMUM_ALLOWED` behavior](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setsecurityinfo).
Native descriptor derivation follows
[`CreatePrivateObjectSecurityEx`](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createprivateobjectsecurityex).

All controls pass without changing production candidate creation. Exact source,
executable, native results and teardown are recorded in
`.hermes/evidence/caller-candidate-security-current/ACCEPTANCE.json`.
The original-token finalization callback is a component control. Helper-side final
plans, destination claims and paired journals are covered below; publication,
receipts and restored Undo/cleanup still need explicit caller authority. This does not enable administrator Copy or establish
alternate-administrator UAC, in-place TokenOwner mutation, caller links or package
profile-context acceptance.

## Original-caller final permission plans

`TreeSecurityPlan` and its retained `DestinationSecurity` policies carry explicit
in-memory authority. One duplicated original caller token derives the complete
graph through native inheritance; the helper token never supplies creator policy.
Private candidate creation uses the same checked token-snapshot implementation.
Caller/profile liveness, namespace binding and native parent security remain
mandatory throughout validation and application. Consuming the graph into a root
policy retains that authority.

Saved plan bytes contain evidence, never a token selection or installable native
descriptor. Re-admission freshly derives the graph with the admitted authority and
compares the entire evidence and permission phase. Already-final pending entries
are verified without repeating a native ACL update. Existing journal entry points
retain their independently admitted authority; caller-aware paired construction
and reopening are covered below, while complete durable recovery remains open.

Native testing reproduced `SetSecurityInfo` returning success with an empty DACL
for a caller-owned parent with creator-owner inheritance when helper file
privileges were no longer active. The policy's exact postcondition rejected that
result. Caller-authorized application now retains the existing scoped helper
backup/restore privileges during that native call and restores the previous thread
token afterwards. The descriptor still comes from the original user, and no
postcondition, parent check or per-object capability was weakened.

Fresh ordinary-client/SYSTEM controls compare five-node graphs under six parent
policies with actual original-token references. Thirty per-entry applications and
thirty pending replays preserve unprocessed entries and pass saved-phase
re-admission; forged creator evidence and changed parent DACLs refuse. The real
four-entry private staging path also derives and applies its retained root policy.
All 59 top-level tests across 13 native invocations and strict paired Windows
platform/UI lint pass. Three earlier trials retain their failure evidence: initial
and diagnostic inheritance failures, then a fixture-only five-second client timeout.
The fixture now matches production operation waiting, with its existing outer
180-second owned-process deadline and disconnect detection unchanged.

Exact source/binary/native binding and teardown:
`.hermes/evidence/caller-permission-plan-current/ACCEPTANCE.json`.
This is final-permission component acceptance, not a durable caller operation,
alternate-administrator UAC, packaged Copy or completed Undo/cleanup bridge.
Administrator Copy remains disabled.

## Original-caller destination claims

Claim admission, reopening and terminal observation require explicit authority.
The anchored destination root and native-name key bucket retain that authority,
including the frozen caller's owner/group/private DACL and case policy. Claim
creation uses the same caller-derived private entry primitive as other staging.
Stored claim evidence never selects an impersonation token or supplies native
permissions to install. All ordinary journal and test callers now select process
authority explicitly; the caller dispatcher still needs integration.

Read-only identity observation remains distinct from private writer admission.
An old terminal observation can ignore a different native object without parsing
or adopting its bytes. A matching object must pass complete record, native
identity, private security, stream and phase checks. Writable reacquisition still
requires exclusive native sharing; a read-only handle cannot mutate its ledger.
Original-caller namespace probes and private reopen use the matching native
no-follow opener. Identity-based old-registry resolution retains scoped helper
file privileges rather than following a recreated pathname.

Native testing reproduced an access-denied claim publication after private
creation had succeeded. The same no-replace rename now runs inside the shared
explicit-authority file-operation wrapper, retaining scoped helper backup/restore
privileges for that call. The wrapper also serves final DACL application; it
restores the exact prior thread token and preserves native errors. No retry with
a different principal or permissive descriptor is introduced.

Fresh ordinary-client/SYSTEM controls cover both case-policy contexts and short
and Unicode long names: original-token reads, read-only write refusal, active
writer exclusion, wrong-helper/forged-creator refusal, dead-owner replacement,
old-terminal replacement observation, read-only-to-writer release, complete
preparation reclamation with partial bytes preserved, and changed-DACL refusal
without altering permissions or ledger bytes. The complete 100-test/23-invocation
matrix includes ordinary claim publication, process exclusion, orphan recovery,
release/repair/retirement, paired-journal initialization, name release and abort
regressions, plus prior caller/permission/staging/security controls. Strict paired
Windows platform/UI lint passes. Exact binding and teardown:
`.hermes/evidence/caller-claims-current/ACCEPTANCE.json`.

This checkpoint accepts caller pre-journal reservations and complete orphan
preparations. The subsequent paired-journal checkpoint below extends that scope;
ordinary paired-journal tests alone do not establish caller-aware acceptance.
Administrator Copy stays disabled; packaged/alternate-administrator UAC and full
profile-context proof remain separate gates.

## Original-caller paired journal construction and reopening

The admitted destination decision now retains explicit in-memory authority.
`DurableStage` inherits it rather than accepting a separate caller selection.
Private stage creation, local journal and central preparation creation use that
same authority; journal headers freeze the caller creator and actual native
private security. Canonical source-overlap checks run before any initialization.
Directory-source traversal retains scoped helper file privileges while observing
private registry ancestors, preserving all existing overlap and liveness checks.
The superseded ordinary-only stage constructor and journal opener are removed.

`Journal` and `Snapshot` retain authority through creation and fresh reopening.
Native entry opening has one implementation, with separate inspection/read/write
masks and sharing rules. Read-only recovery excludes an active writer; inspection
retains readonly handles. Wrong ambient helper authority cannot adopt caller
controls. Fresh writer admission inherits the already admitted snapshot context;
creator validation compares with the frozen runtime caller, never a saved SID as
a token selector. Control binding checks use the matching no-follow opener.
Central no-replace publication retains scoped helper file privileges.

Begin-only admission uses the exclusive validated name claim and reads the paired
controls with its retained authority. A complete published or unpublished Begin
can release only its reservation, preserving all setup data. A payload intent
keeps the reservation. Names, partial bytes and mismatched headers still provide
no permission to delete or repair records.

Native ordinary-user/SYSTEM controls run both a regular-file source and a real
four-entry populated directory through the shared durable staging engine. They
verify caller owner/group/private DACLs, original-token reads, readonly write
denial, active-writer exclusion, readonly and exclusive writer reopening, and
native original-token permission-plan evidence appended to both mirrors. Each
source context covers published Begin-only, unpublished complete Begin-only and
intent-retention cases. Both records remain byte-identical during reservation
recovery; intent refusal preserves the claim digest. The actual canonical registry
admission and original source pins remain held, with preferences/index bytes,
identities and permission digests and full source evidence unchanged.

The earlier canonical-ancestor fault deliberately changes access. Its already
acquired readonly preference/index observation handles remain held through the
new controls; no permissions are repaired and those handles grant no journal or
payload mutation authority. Two initial fixture fresh-open failures and the
subsequent directory-overlap failure are retained with exact source/binary/native
bindings. The final 100 tests across 23 native invocations and strict paired
Windows platform/UI lint pass. Evidence:
`.hermes/evidence/caller-journals-current/ACCEPTANCE.json`.

Retained copied-tree verification now retains the snapshot/journal's authenticated
runtime authority. Its shared no-follow source opener uses scoped helper access
for caller entries, while keeping the same native type, parent and link checks.
Held payload readers carry that authority through namespace binding checks and
native destination-permission re-admission; saved records never select an identity.
The native file and populated-directory fixtures reopen the planned paired journals,
verify every payload and derive the identical original-token native policy again.
Ordinary source-free publication/permission replay and receipt/public Undo also
pass against the shared retained-tree verifier. That checkpoint has 102
passing tests across 25 native invocations and strict paired Windows platform/UI
lint. Evidence: `.hermes/evidence/caller-retained-tree-current/ACCEPTANCE.json`.

Creation publication now retains caller authority through the exclusive paired
journal handoff, genuine native per-object reopening, no-replace publish rename,
creation-time recovery binding and final permission-plan capture/application.
Reopened caller capabilities retain MAXIMUM_ALLOWED in the native request and
explicitly request payload/metadata/rename/DACL rights under scoped helper file
privileges. Readonly file data stays behind its durable attribute lease; directories
retain their write capability. No arbitrary File is promoted and no policy is repaired.

Two failed native attempts reproduced the publication admission denial; explicit
native access rights alone did not resolve it. The shared creation-time recovery
code still checked the renamed caller-owned destination under process authority.
It now uses the retained journal authority; pending copy-epoch readers use their
snapshot authority. Publication IO errors name the operation while typed refusals
keep their existing variants. Both actual file and four-entry directory fixtures
now publish, apply all final permissions, reopen completed content/security evidence
and replay through a fresh writer. Original source and pinned preferences/index
bytes, identities and security remain unchanged. All 102 tests across 25 native
invocations and strict paired Windows platform/UI lint pass. Failed input/result
bindings and the final acceptance are retained at
`.hermes/evidence/caller-publication-current/ACCEPTANCE.json`.

The unified creation executor now holds canonical caller registry admission before
its first claim/stage write and through receipt construction. The shared retained,
publication and completion constructors require explicit runtime authority; ordinary
callers select Process. Completion/Undo receipt admission and refresh retain caller
authority, validate the caller's canonical anchored registry, reopen the parent by
native identity under scoped helper access and revalidate both journal mirrors and
completed payload evidence. Opaque live Undo plans retain authenticated caller/profile
context, not payload handles; no saved path, SID or ACL recreates authority. Their
runtime authority equality is conservative: only aliases of the same authenticated
context compare equal. Registered journal leases also retain their authority through
paired observations. Undo uses genuine per-object reopening, scoped rename and the
same creation-time recovery bindings. The uncoordinated-cohort refusal gate inspects
the selected caller's registry ancestry and sibling, without provisioning it.

Actual file and four-entry directory fixtures exercise the manual published receipt
and the full unified execute-with-receipt path, canonical refresh, source-free public
Undo and repeated Undo. Wrong process authority, cancellation and a foreign registry
are refused; the latter leaves both foreign registry and destination empty. Original
source evidence and pinned preferences/index bytes, native IDs and permissions remain
unchanged. A first native trial reproduced a missed Process argument at the session
receipt's Undo-admission handoff. The corrected trial passes all 102 tests across 25
native invocations and strict paired Windows platform/UI lint; failed source/result
binding and final acceptance are retained at
`.hermes/evidence/caller-receipts-current/ACCEPTANCE.json`.

Live forward recovery plans now retain the authenticated runtime authority too.
Admission and execution hold fresh canonical registry admission, inspect registered
journals with that authority and resolve parent identity under scoped helper file
access. The bound recovery engine carries the selected authority through retained
payload validation and exclusive journal reopening. Persisted fields still cannot
choose a token; no payload handles survive admission. Completion remains a read-only
replay when another admitted owner already finished the exact copy.

The actual one-file and four-entry directory fixtures now close all old stage/writer
owners and independently re-admit four paired states: verified/planned staging,
publish rename before acknowledgement, acknowledged publication before permissions,
and a native permission change whose durable intent is still pending. Each rejects
process authority, preserves both journals on cancellation, finishes through a fresh
plan, replays completion without changing journal bytes and then performs public Undo.
The original source and user-state evidence stay unchanged. Ordinary source-free and
readonly-root replay plus publication/permission outcome-retry regressions also pass.
The checkpoint has 104 passing tests across 27 native invocations and strict paired
Windows platform/UI lint. Evidence:
`.hermes/evidence/caller-resume-current/ACCEPTANCE.json`.

Overwrite now retains caller authority through original destination admission,
namespace checks, dependency observation, original evidence preparation, both native
renames and held-original verification. The shared pinned-tree reader requires explicit
read authority and keeps it through descendant opening and binding/content/security
validation. Staged trees retain source and destination authorities for their respective
bindings. Ordinary callers explicitly select Process; original/source data is never
assigned the staged copy's permissions. Original root capabilities use the same genuine
native per-object reopen path; scoped helper access covers retention/restoration renames.
Registered dependency records retain their caller authority and native-ID parent resolution.
The dependency guard moved from the outer dispatcher into the shared execute-with-receipt
engine, retaining its leases across the whole operation, so direct callers cannot omit it.

Before preservation tests intentionally leave unfinished peers, both actual source
contexts replace caller-private originals (a file and four-entry directory) in two ways:
unified execution and interruption immediately after the original retention rename.
Fresh forward admission finishes the latter, then receipt refresh and public Undo restore
the exact original. Repeated Undo is idempotent. Baseline comparisons verify original IDs,
streams, links, compression, membership, case policy, owner/group/DACL digests and metadata;
only root change time is updated for the legitimate renames. No original permissions are
rewritten. Source and pinned preferences/index bytes, identities and permissions remain
unchanged. Ordinary source-free overwrite replay and drift/occupant refusals also pass.
The checkpoint has 106 passing tests across 29 native invocations and strict paired
Windows platform/UI lint. Evidence:
`.hermes/evidence/caller-overwrite-current/ACCEPTANCE.json`.

Admitted registry objects now provide read-only retained discovery and restored
Resume/Undo admissions with their retained runtime authority. Ordinary and caller
inspection share one implementation; registered journals, native-ID parent
resolution, snapshot loading and payload bindings use the selected authority.
Caller inspection independently rechecks the canonical registry and native root
identity. Presentation rows and persisted fields grant no mutation capability.

Both actual source contexts now drop old stage/writer owners, independently capture
a fresh context of the same authenticated client, discover ReadyToPublish evidence
and restore/execute Resume. Another fresh capture discovers Completed evidence,
restores/executes Undo and discovers Undone evidence. Cancellation and invalid IDs
are refused, completed Resume/Undo admissions are refused, discovery preserves
journal bytes and source/preferences/index evidence stays unchanged. All 108 test
passes across 31 native invocations and strict Windows platform/UI lint pass.
The bad-record/symlink discovery regression requires SYSTEM's symlink capability;
ordinary phase-classification discovery remains an ordinary-user control. An initial
ordinary-user routing failure occurred during symlink fixture creation (error 1314),
before product discovery; the unchanged executable passed with the corrected role.
Evidence: `.hermes/evidence/caller-discovery-current/ACCEPTANCE.json`.

Retained-payload cleanup now carries explicit runtime authority too. Its handle-free
admission retains the authenticated context through snapshot/writer handoff and
fresh observations. Dependency leases use that authority for private control reads,
native parent resolution and binding rechecks. The remaining-tree reader retains it
for no-follow descendant access, narrowly acquired deletion capabilities, absence
and alias checks, native disposition and post-unlink recovery. Peer observations and
successful payload completion proofs keep the same context. Ordinary entry paths
explicitly select Process; persisted fields still cannot choose a token.

Actual file and four-entry directory sources reclaim caller-private retained
originals after overwrite and retained copies after Undo. Wrong-process access and
cancelled reads/mutations preserve journals. An injected failure after unlink is
re-admitted through a fresh context, verifies the pending absence and completes.
Public contents and source/preferences/index bytes, IDs and security remain intact.
The focused 108 passes and 89 ordinary cleanup tests use the initial exact build;
only two test fixtures then change. Three exact tests on the rebuilt source verify
the guarded-overlap fixture correction, a real parent-DACL mutation under SYSTEM
and the final cohort case. All 91 non-worker cleanup tests are covered, including
full metadata/cohort, alias, failure-prefix, drift and real-exit controls. The initial
whole-module timeout is retained as incomplete evidence; per-test results preserve
the later successful long matrices. There are 200 test passes across 123 top-level
native invocations in total, with strict Windows platform/UI lint passing.
Evidence: `.hermes/evidence/caller-cleanup-current/ACCEPTANCE.json`.

This does not complete the full caller mutation-prefix matrix, readonly/link
publication recovery, the complete original-retention/restoration failure matrix
(including readonly original roots), caller cohort/metadata retirement, public
cleanup admissions or actual
client/helper restart with newly authenticated connections. Fresh captures of the
same live client do not establish process-restart recovery. Caller cleanup records
can still report NeedsReview until their own authority integration is complete.
Live caller receipts/public Undo do not establish process-restart or
complete helper-dispatcher/UAC acceptance.
The actual dispatcher must retain original source pins and canonical registry
admission for the entire operation. Actual alternate-administrator UAC, caller
links, in-place TokenOwner mutation and package/profile contexts remain separate.
Administrator Copy stays disabled and stable packages remain unchanged.

## Canonical native security evidence

DACL evidence contains the native ACL header and all ordered access-control entry
bytes, with its recorded size set to the occupied byte count. Windows ACLs may
have spare allocation capacity; `IsValidAcl` and `GetAclInformation` validate the
native count/used/free accounting before observing only `AclBytesInUse`. Spare
storage must not create a false permission mismatch. ACE order, payload, masks,
flags, SIDs, owner/group and the recorded inheritance/protection controls remain
part of equality. Cached records must consume the full recorded logical size;
noncanonical padded or truncated evidence is refused rather than repaired.

This representation is observation only. Neither cached bytes nor normalized
observations become Windows security descriptors. Creation and permission
application continue to use owned native descriptors, derived from live parent
security and the authenticated token; native descriptors are never rewritten to
make an evidence comparison pass.

## Historical temporary-stream isolation finding

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
acceptance; public Copy was disabled at that checkpoint. The current persistent
claim lifecycle is described below, and the current administrator gate is in the
roadmap. The rejected Rust candidate is archived, not installed. Exact controls and limits:
`.hermes/evidence/windows-ephemeral-security-current/FINDINGS.md`.
Earlier lifetime controls:
`.hermes/evidence/windows-directory-lifetime-current/FINDINGS.md`.

## Current claim preparation lifecycle

The current journal schema is 18 and the claim format is `OTCWCL03`. New controls
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

## Ending unverified staging

A canonical incomplete transaction can admit an opaque `RetainedCopyAbortPlan`
without opening any payload or source. The plan captures the registry anchor,
paired journal bytes, native parent location/security and exact claim epoch.
Explicit confirmation in Activity permanently ends that attempt and releases its
reserved destination name. It keeps all partial files, edits, additional streams,
unrecognized files and recovery journals; it does not reclaim storage.

The exclusive writer rechecks the captured controls before persisting the paired
`StagingAborted` record. That record is terminal: no subsequent staging,
verification, publication, Undo or payload-cleanup transition is valid. Ordinary
durable claim release follows only the paired terminal record. A one-sided append
remains refused and reserved; a fully paired terminal prefix can use the existing
non-destructive name-release action after restart. An older plan cannot release
a later durable copy's reservation.

An ended attempt also closes its recorded dependency claims. Unrelated Overwrite
and cleanup can proceed after both journals and their native directory bindings
are validated and read-leased. Every recorded creation identity remains protected,
including a partial file that has no completed stream evidence. Its former
original-destination observation grants no mutation authority after ending. No
partial payload is opened, adopted or deleted to establish this closed set.
Active incomplete manifests, live writers, corrupt records and divergent pairs
remain refused. Cleanup rechecks the paired terminal proof at writer handoff;
observing a central terminal frame alone is insufficient.

Schema 18 adds this terminal state. Older schemas remain review-only and are not
converted or deleted. UI confirmation retains the exact originally admitted plan;
refresh cannot silently replace the action the user reviewed. Current native
acceptance is recorded in `.hermes/evidence/windows-copy-abort-current/`.
Incomplete-payload reclamation, broader transaction/workflow acceptance and public
Windows Copy remain unfinished.

## Persistent journal setup and live-owner failure

The local journal and central `.preparing-<transaction>` control are private
persistent creations. The central marker and its directory are flushed before
creating the destination-local stage. Read-only discovery presents unpublished
preparations as review-only records, preserving their exact native path without
reading their contents, following directory/reparse targets or adopting a
transaction ID. This includes empty, partial and complete unpublished journals;
a matching filename grants no Resume, Undo or cleanup authority. The record may
belong to a live writer, so presentation does not claim it is abandoned.
Activity keeps this observed recovery location as a native path, separate from
display text and mutation plans. Recovery details show the path and offer an exact
text clipboard action when representable. Missing discovery clears stale locations.
Incomplete staging explains why Resume and automatic cleanup are unavailable;
showing or copying a recovery path grants no permission to mutate its contents.
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
entry point supplies that handle through the registry admission below. Ordinary
single/batch Copy is wired through the native engine; complete release and
administrator-Copy gates remain open in the roadmap. The historical dispatcher
fixture is `.hermes/evidence/windows-copy-dispatch-current/ACCEPTANCE.json`;
its original scope does not describe current whole-product acceptance.

### Configured registry admission

The ordinary-process configured registry is
`%LOCALAPPDATA%\Ottrin\State\copies`. This locator currently reads that process
environment; it must not be called under the helper to select the original caller
registry. The authenticated context resolves and pins the original profile, but
its explicit registry/transaction authority is not yet connected. Its external
`.ottrin-copy-recovery-root` marker sits in `State`, so replacing the entire
registry cannot also replace its identity proof. The bounded `OTCRW02` record
binds marker, parent and root native identities, root name, creator, expected case policy and initial
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
Ordinary Copy is enabled; administrator Copy remains disabled until the original
caller authority is carried through registry, staging, destination security and
retained recovery. No ambient helper environment or temporary process-wide
identity switch is a substitute for that integration.

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
`.hermes/evidence/windows-copy-blocked-io-current/ACCEPTANCE.json`.

The later NTFS oplock fixture blocks actual central/local journal and payload
opens inside both forward recovery and cleanup. All six Stop cases return native
operation-aborted while the lock is still held, leave the paired journals and
payload byte-identical, and retry source-free after lock release. Forward retry
publishes; cleanup retry fully reclaims; unrelated destination data is retained.
These are pre-mutation open interruptions, not mutating-write, remote/device or
physical power-loss acceptance. The final fixture observes 12–28 ms cancellation,
not a general latency guarantee. Exact scopes:
`.hermes/evidence/windows-copy-oplock-current/ACCEPTANCE.json`.

The following Activity integration composes the actual rendered Stop control with
those kernel-blocked operations. All 24 scenarios pass: the previous 18 plus
central/local/payload blocks in both forward recovery and cleanup. The worker
retains path/data ownership while blocked, ends before lock release after Stop,
preserves journal/payload bytes, and retries through ordinary rendered actions.
The test releases its oplock before waiting for any queued asynchronous status
refresh. The shared test-only oplock implementation is in
`tests/support/windows_oplock.rs`; it is not copied or exposed in production.
This is native egui action/layout evidence, not physical desktop or package
acceptance. Exact executable/fixture scopes and cleanup:
`.hermes/evidence/windows-copy-blocked-activity-current/ACCEPTANCE.json`.

Copy Undo now registers its own Stop flag with the existing worker, forwards it
through session receipts and retained/batch inverses, and binds the same native
cancellation guard. Activity exposes **Stop Undo** until the worker result arrives;
Stop neither releases scheduling ownership early nor discards the completed batch
cursor. Registration ends on result delivery or failed worker creation; closing
requests Stop for each remaining operation independently.

The expanded Activity integration passes all 27 scenarios, including blocked Undo
central/local/payload opens, actual rendered Stop, byte preservation and normal
source-free retry. These checks exposed ordinary/verbatim DOS-path scheduling
mismatch. Operation claims and storage-root comparisons now share lexical drive/UNC
prefix normalization; it resolves no filesystem aliases and grants no native
authority. Focused native controls verify both prefix directions, independent
paths, per-operation flags and the existing admission/editing behavior. Exact
scopes and remaining gates:
`.hermes/evidence/windows-copy-undo-stop-current/ACCEPTANCE.json`.

The native fixture follows [Microsoft's level-1 oplock contract](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_request_oplock_level_1)
and [acknowledgment/close semantics](https://learn.microsoft.com/en-us/windows-hardware/drivers/ifs/acknowledging-oplock-breaks).

See
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
journal schema 18 remain authoritative; the stream sparse attribute selects this
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


### Caller-authorized metadata retirement

The shared metadata engine accepts explicit runtime Authority for paired-intent
observation and execution. It uses that authority for native no-follow opens,
creator checks, dependency admission, namespace bindings, absence proofs and
native file dispositions. Completion retains its authenticated context through
the metadata-intent writer handoff. A serialized Plan stores exact evidence only;
executing or re-observing it requires separately admitted live authority.

The four native caller file/directory × overwrite-retained-original/post-Undo-
retained-copy scenarios pass. Independently recaptured contexts of the same live
client re-observe paired intent without writing. Cancellation and wrong-process
execution preserve journal bytes. An interrupted local-journal removal resumes
from the surviving central intent, and completed removal replays idempotently.
Public content/native identities and source/preferences/index evidence are
checked after metadata retirement. Strict Windows platform/UI lint passes.
All 120 test passes across 43 top-level invocations pass, including the 12
affected ordinary metadata/cohort parent regressions. The ordinary audit-SACL
test requires its dedicated SYSTEM lane, where it explicitly passes. Source,
compiler profile, executable text, actor sources and normalized native results
are bound in `.hermes/evidence/caller-metadata-retirement-current/ACCEPTANCE.json`.
VM/disks/private desktop/profile/process teardown is independently verified.
Caller cohort retirement, public cleanup entry points, real process restart/new
connections, alternate-administrator UAC and packaged/profile acceptance remain
separate. Administrator Copy remains disabled.


### Caller-authorized cohort cleanup

Runtime cohort observations retain explicit Authority. Selection catalogs and
serialized metadata Plans/cohort snapshots remain evidence only. Canonical caller
registry admission is re-established before every bound registry use; parent IDs,
peer journals, shared payload operations and metadata inspections use the same
retained authority. No ambient helper profile or persisted selection chooses a token.

The external coordinator creates its unpublished control with a live caller-derived
private descriptor, FILE_CREATE and atomic delete-on-close. Publication uses the
held native handle and no-replace rename; delete-on-close is cancelled only after
that rename. Reopen, namespace binding, checkpoint inspection and retirement keep
caller authority and preserve the existing identity/security/content/exclusion
checks. Ordinary paths select Process explicitly and share the same engine.

Eight native caller cases pass: file/directory sources, mixed original/post-Undo
cohorts and shared retained aliases, each direct or interrupted. Shared file-root
renames finish before either journal acknowledges the final shared epoch; directory
children include a local duplicate and an outside alias. The public dependency guard
still refuses the second overlap. Lower-level transitions construct only historical
retained state, without rewriting records or fabricating timestamps. Post-unlink
recovery uses a fresh context of the same live client and preserves outside bytes,
native identity/security, both public trees and source/preferences/index evidence.

The first focused source passes 120 tests/43 invocations and strict platform/UI lint.
Only the caller test fixture then gains shared-alias cases; reconstruction verifies
all existing fixture bodies and all 11 production file hashes remain unchanged.
The rebuilt shared-fixture source passes the paired caller/helper plus all 42
additional ordinary group/cohort parents: 44 passes/44 invocations. Combined,
164 passes/87 invocations cover the eight caller scenarios and all 54 selected
metadata/group/cohort parents. The ordinary audit-SACL case is intentionally
ignored without its privilege, then explicitly passes in the SYSTEM lane.
Both native runs have independent VM/disks/private desktop/profile/process teardown.
Scope and provenance: `.hermes/evidence/caller-cohort-current/ACCEPTANCE.json`.
Complete caller failure/readonly/link matrices, actual process restart/new
connections/UAC and package/profile contexts remain separate. Administrator Copy
remains disabled. Public cleanup/discovery admission is described below.

### Public caller cleanup and discovery admission

The admitted WindowsCopyRegistry exposes cleanup and pending-cleanup plans using
its retained runtime Authority. Canonical registry admission is validated before
and after inspection; the resulting existing cohort writer retains that authority
through mutation. Ordinary public entry points choose Process explicitly and use
the same implementation. Persisted records still never select a caller token.

Cleanup status discovery now passes the admitted authority into payload and
metadata observations, preserving all native identity, security, namespace and
content validation. Pending recovery uses the original cohort rather than a list
of surviving local journals. Cancellation remains an error and performs no repair.

The native fixture exercises public single-member, mixed and shared plans for
file/directory sources; read-only payload/metadata discovery; cancellation;
wrong-process and noncanonical registry refusal; repeated completion; original
cohort IDs after local metadata removal; and post-unlink shared recovery. It keeps
the outside-alias, public tree, source and original-user preferences/index checks.
Cleanup runs before the journal fixture's deliberately incomplete creation states:
the production guard requires complete peer dependency evidence even when a peer
appears unrelated. The first native run demonstrated that refusal; the fixture
ordering correction preserves every case and does not relax the guard.
All 16 top-level native invocations pass: the ordinary client, SYSTEM helper and
14 selected ordinary configured/pending/discovery parents. The helper covers ten
public cleanup flows and four payload/metadata discovery pairs. Exact eleven-file
source/build/staged-text binding, strict Windows platform/UI lint and independent
owned VM/disks/private desktop/profile/process teardown pass. Evidence:
`.hermes/evidence/caller-public-cleanup-current/ACCEPTANCE.json` and its source,
build, first-failure and continuity receipts. Existing application packages retain
their separately accepted source and hashes.
Recapturing the same live client remains distinct from real process restart or a
new authenticated connection. Full dispatcher and package/UAC acceptance remain
required before administrator Copy can be enabled.

### Caller path and conflict dispatch

`CallerCopyContext::copy_entry_with_progress` selects the authenticated caller's
runtime authority and executes the existing dispatcher. Source admission opens
the native parent and leaf with that authority, retains both handles and validates
the original binding at progress boundaries. Caller source paths require the same
unambiguous absolute form as registry initialization. Native source sharing denies
replacement; no Source is reconstructed from a display label or persisted record.

The canonical registry factory borrows those original Source admissions through
profile provisioning. It refuses mismatched runtime authority before any write.
Read-only existing-registry discovery still accepts an empty source selection.
The shared conflict loop takes authority from its admitted Source and retains the
canonical registry through creation, Skip, Rename, Overwrite, publication and
receipt admission. Existing rich preparation errors keep their retained-data path.
Caller destination opens and native-ID parent resolution share one required-access
policy; ordinary Process dispatch uses the same engine with its existing access.

Native first provisioning and eight file/directory conflict cases pass through the
public caller API. They verify refused pre-write cancellation/relative source/
profile overlap/Process-source admission, an actual source-replacement sharing
violation, copied stream/preserved metadata equality, distinct native IDs, receipt
refresh, repeated public Undo, cleanup, exact original restoration and source/
preferences/index preservation. Two Skip cases preserve the original/catalog
and return no receipt; the other six conflict cases exercise receipt/Undo/cleanup.
All five ordinary dispatcher and nine registry/
namespace parents pass, alongside the ordinary client and full SYSTEM caller role:
16 top-level invocations. Strict Windows platform/UI lint, exact source/build/
staged-text binding and independent owned teardown pass. Evidence:
`.hermes/evidence/caller-dispatch-current/ACCEPTANCE.json`.

The public engine/API is covered, not final helper command routing. Caller-aware
end-attempt and terminal name-release integration is described below; the complete
helper bridge and failure matrix remain separate gates.
Full caller mutation-prefix, readonly/link, process-restart/new-connection,
TokenOwner, alternate-administrator UAC and package/profile gates remain open.
Administrator Copy stays disabled and stable package hashes remain unchanged.

### Caller end-attempt and terminal name release

Both runtime plans retain explicit Authority and the complete original registry
Binding. Canonical registry admission is re-established before execution; matching
only an anchor ID is insufficient. Parent resolution, paired control snapshots
and the existing claim writer use that same authority. Ordinary entry points
select Process once and share these implementations. No persisted field supplies
a caller token or security descriptor to apply.

`WindowsCopyRegistry::abort_plan_with_progress` admits only an unverified attempt
with exact paired controls. Ending it writes the existing paired StagingAborted
record, then uses the existing terminal claim handoff. It never adopts, opens or
removes the payload. `name_release_plan_with_progress` admits only a durably
terminal attempt whose claim still needs release; finishing changes claim
bookkeeping and preserves both journals and all payload data. Cancellation,
stale controls, changed registry/parent evidence and already completed plans
continue to refuse through the existing validation paths.

The caller fixture covers edited/unknown payloads under exclusive root leases in
Undone and CleaningUp phases, and file/directory creation/overwrite attempts ended
directly, after a complete paired prefix, or after a divergent local prefix.
Complete terminal evidence permits name reuse; divergence stays reserved without
repair or data deletion. Fixture ordering keeps all public dispatcher/cleanup
controls ahead of deliberately uncertain shared-registry records.

All four caller terminal name-release cases and twelve end-attempt cases pass.
Together with the ordinary client, full SYSTEM caller role and all eleven ordinary
abort/name-release/claim-repair/dependency parent tests, the run passes 13 top-level
invocations. Strict platform/UI lint, exact seven-file source/build/staged-text
binding and independent owned teardown pass. The initial fixture attempted a
writer flush through an inspection-only registry and correctly failed before data
transfer; corrected writer admission preserves all production checks. The failed
source is exactly reconstructible from the retained correction receipt. Evidence:
`.hermes/evidence/caller-terminal-current/ACCEPTANCE.json` and its first-failure,
continuity, source/build and cleanup records.

Complete helper command/history/recovery routing, caller mutation-prefix and
readonly/link matrices, actual restart/new connections, TokenOwner mutation,
alternate-administrator UAC and package/profile contexts remain separate. The
native engine/API checkpoint does not enable advertised administrator Copy or
replace the stable application packages.

### Shared Activity caller integration

The Linux helper Activity implementation is shared with Windows. History, Undo
and cleanup use the progress-aware privileged API; Windows Resume, end-attempt
and name release use the existing recovery worker and confirmation flow. Helper
references remain opaque lookup IDs, never ordinary filesystem admission. History
merging checks every transaction, preserves live receipts/cleanup ownership and
coalesces only complete inactive restored cleanup cohorts. Ordinary discovery
cannot replace helper-owned records. Loading history remains explicit, cancellable
and absent from startup authentication. Unknown entries remain visible for review.
A shared core capability constant keeps both the Windows helper and its Activity
history controls disabled until remaining acceptance passes.

The checkpoint passes 22 native Windows cases, 20 scoped Linux tests, nine actual
Linux helper CLI probes, formatting and strict Linux/Windows lint. Exact source,
executable/helper hashes, unchanged stable packages and independent VM/volume/
staging teardown pass in `.hermes/evidence/caller-history-ui-current/ACCEPTANCE.json`.
Linux scoped UI/platform tests, strict Linux/Windows checks and helper CLI probes
are separate from native Windows headless interaction/adapter tests under the
SYSTEM guest-agent token. None establishes ordinary-user rendered interaction,
interactive elevation or a newly delivered package. Stable package hashes remain
unchanged. Forward administrator Copy still needs asynchronous worker/progress/
Stop integration; absent/partial history reconciliation and the full caller failure,
process-restart, alternate-administrator UAC and package matrices remain open.
