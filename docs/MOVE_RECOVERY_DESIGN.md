# Durable Move Recovery Design

## Status

Move remains fail-closed. This document defines the recovery and ownership boundary that must exist before any Move mutation is re-enabled.

## Problem

A safe Move cannot validate a pathname and later rename or delete that pathname. Another process can replace the named entry between those operations. Holding an open file handle pins identity while Ottrin is alive, but handles do not survive process death.

Linux has no general identity-conditional rename syscall: `renameat2` acts on the entry occupying a parent/name at syscall time, not on an earlier `O_PATH` handle. Claiming the source under a private name therefore transfers a name atomically but does not by itself prove that the claimed entry is the one Ottrin pinned. The claim must be followed by an identity check inside an Ottrin-owned directory before any publication or deletion. A mismatched claim is restored with no-replace or retained as a visible recovery item; it is never published or deleted.

Without a durable, discoverable journal, a crash could also strand user data under a private name. Move therefore needs journal-before-claim ordering, post-claim identity fencing while running, and identity-verified recovery after restart.

## Required invariants

1. **No unverified publication or deletion.** A pathname-based rename may only claim an entry into an Ottrin-owned private directory. The claimed identity must match the pre-pinned identity before Ottrin can publish or delete it. A mismatch is restored with no-replace or retained; it is never treated as the requested source.
2. **Journal before mutation.** A durable recovery record exists before the source name changes.
3. **One recoverable copy at every crash point.** Before publication the claimed source remains complete. After publication, either the destination is proven complete or the claimed source is retained.
4. **No guessed recovery.** Missing paths, changed parent identities, occupied original names, malformed journals, and conflicting identities retain data and require explicit recovery.
5. **Publication precedes source cleanup.** Cross-filesystem Move never removes the claimed source until the destination tree and destination parent are durably published.
6. **Source mutation fencing.** Cross-filesystem publication uses the existing source-tree snapshot rules and rechecks the privately claimed tree before source cleanup.
7. **Conflict behavior is race-safe.** Skip and Rename publish with no-replace primitives. Overwrite is enabled only where the displaced destination is atomically retained.
8. **Recovery is idempotent.** Repeating recovery after interruption produces the same safe state.
9. **Native names are lossless.** Unix byte paths and Windows UTF-16 paths round-trip without lossy Unicode conversion.
10. **Batch accounting is honest.** A published item with retained cleanup is completed-with-warning; an un-published claimed source is not completed.

## Transaction layout

Each Move has a random 128-bit transaction ID.

### Central discovery record

One record per transaction lives under the platform app-data recovery root:

- Linux: `${XDG_STATE_HOME:-~/.local/state}/ottrin/moves/<id>.journal`
- Windows: `%LOCALAPPDATA%\Ottrin\State\moves\<id>.journal`

The central record makes arbitrary source-local transactions discoverable after restart. It is written and directory-synced before source mutation. One-file-per-transaction avoids rewriting a shared registry.

### Source-local transaction

A private sibling directory is created in the source parent:

```text
<source-parent>/.ottrin-move-<id>/
    journal
    claimed
```

On Linux it is mode `0700`, owned by the effective user, opened with `O_DIRECTORY|O_NOFOLLOW`, and identity-checked against its parent entry. Windows must create an equivalent current-user-only directory and open it without following reparse points.

The local journal is authoritative for source ownership. The central record contains enough duplicated identity and native-path information to locate and validate it, but neither record makes a pathname trustworthy by itself.

Cross-filesystem Move also creates a destination-local transaction with a random native name. Its name and stable identity are flushed into the source-local and central records before staging begins. The staged entry identity, attempted publication name, and expected displaced identity are flushed before publication.

Stable identities validate a located object; they do not necessarily locate it after an ancestor is renamed. Windows may reopen by volume/file ID where supported. Linux recovery first tries the path hint, then exposes a **Locate Transaction Folder** action. The user selects a current ancestor or mount subtree, Ottrin searches only that selected scope for the exact `.ottrin-move-<id>` name, and accepts a result only when its recorded identity and journal match. Until located, the central record remains actionable and no cleanup is attempted.

## Journal encoding

Use a versioned schema with atomic replacement:

1. Write a private `journal.next-<random-128-bit-id>` entry with create-new semantics. A stale temporary entry from an interrupted update is retained for recovery and cannot block later replacement.
2. Flush the file.
3. Rename it over `journal` using a platform atomic replacement primitive.
4. Flush the containing directory where the platform supports directory durability.

Records must reject unknown major versions, truncation, duplicate fields, invalid native-path units, and impossible state transitions.

Native paths are encoded as tagged units:

- Unix: hex-encoded `OsStrExt::as_bytes()` bytes.
- Windows: little-endian hex-encoded `encode_wide()` UTF-16 units.

A record contains at least:

- schema version and transaction ID;
- advisory phase;
- source parent native path and stable identity;
- original source name and stable entry identity;
- source transaction directory name and stable identity;
- destination parent native path and stable identity;
- destination-local transaction native name and stable identity, persisted before staging;
- pending destination name, persisted before each publication attempt;
- conflict policy;
- source/destination filesystem relationship when known;
- source-tree snapshot for cross-filesystem cleanup;
- staged destination identity, persisted before publication;
- published destination identity when observed;
- expected and observed displaced destination identities when Overwrite retains one;
- per-entry recursive-cleanup claim intents and verified identities;
- last durable error and whether user attention is required.

The phase accelerates recovery but never overrides observed topology and identity.

## Stable identities

### Linux

Use handle-relative `rustix` operations and record:

- `st_dev`;
- `st_ino`;
- file type;
- transaction directory owner and mode where relevant.

Open parents with `O_DIRECTORY|O_CLOEXEC`; open entries with `O_PATH|O_NOFOLLOW|O_CLOEXEC`. Mutate names with `renameat2`/`unlinkat` relative to verified directory handles. Parent paths are recovery hints only.

`O_PATH` does not bind a later `renameat2` to the opened inode. Every source or cleanup entry is therefore pinned, claimed by name into a private transaction directory, and re-identified there. No claimed entry reaches publication or unlink until that post-claim identity comparison succeeds.

### Windows

Add a direct `windows-sys` dependency and use Win32 handles opened with the minimum required sharing and access. Record:

- volume serial number;
- 128-bit file ID from `FILE_ID_INFO`;
- file/directory type;
- reparse tag where applicable.

Open directories with `FILE_FLAG_BACKUP_SEMANTICS` and reparse-safe flags. Rename through `SetFileInformationByHandle` using `FILE_RENAME_INFO_EX` and a verified destination root-directory handle. A path reopened after restart must match the recorded volume/file ID before use.

Windows support is not considered implemented by a `std::fs::rename` fallback.

## Forward protocol

### 1. Validate without mutation

- Validate all batch sources and destination before processing the first item.
- Reject source roots without a file name, source equals destination, and directory-to-self/descendant moves.
- Open and identity-pin source parent, source entry, and destination parent.
- Resolve the conflict policy without assuming the observed target remains unchanged.

### 2. Prepare recovery

- Create and flush the central discovery record in `Prepared` phase.
- Create and identity-verify the source-local transaction directory.
- Write and flush the local journal.
- Update and flush the central record with the verified local transaction identity.
- Flush source parent and recovery-root directories.

A crash here has not changed the source name. Recovery removes only a proven-empty transaction after confirming the original source identity still matches.

### 3. Claim the source

Persist and flush `ClaimIntent` with the expected source identity. Atomically rename the original source name to `<transaction>/claimed` through the verified source-parent and transaction-directory handles, with no replacement. Immediately open `claimed` without following links and compare its stable identity to the pre-pinned source identity.

- On a match, flush both directories and record `Claimed`.
- On a mismatch, do not publish or delete the claimed entry. Attempt to restore it to the original name with no-replace. If the original name is occupied or the parent identity is unavailable, retain it under the transaction, record `UnexpectedClaimRetained`, and surface the exact recovery record in Activity.

This post-claim fence is mandatory because Linux cannot make the rename itself identity-conditional.

A crash between rename and phase update is resolved from topology: recovery compares the identity under `claimed` with the expected identity in the already-durable `ClaimIntent`. A mismatch follows the same restore-or-retain path and can never proceed to publication.

If a new entry appears at the original name, it is never overwritten during recovery.

### 4A. Same-filesystem publication

- **Skip:** publish `claimed` to the target with no-replace. If occupied, restore the claim to the original name only when that name is absent and the original parent identity still matches; otherwise retain it for explicit recovery.
- **Rename:** choose one candidate, persist and flush it as `PendingDestination`, then attempt handle-relative no-replace publication. If it exists, persist the next candidate before the next attempt. A crash can therefore inspect the exact last attempted name and prove publication from identity. No candidate is attempted before its native name is durable.
- **Overwrite:** persist and flush the pending target name and pinned expected-destination identity before publication. On Linux use atomic exchange so the old destination becomes `claimed` inside the source transaction, then verify the displaced identity before any cleanup. A mismatched displaced entry is retained, never deleted. On Windows, keep Overwrite disabled until an equivalent atomic retain-old/publish-new primitive is implemented for the relevant entry kind.

Flush destination parent and source transaction/source parent as applicable, then record `Published`.

For non-overwrite same-filesystem publication, the published entry must retain the original source identity.

### 4B. Cross-filesystem publication

- Create and identity-verify the destination-local private transaction. Persist and flush its native name and stable identity into both surviving recovery records before copying any data.
- Build and persist a source-tree snapshot from the privately claimed source.
- Copy from the claimed handle/tree into the recorded destination-local transaction.
- Flush the complete destination tree and its transaction directory.
- Persist and flush the staged entry identity, the exact pending destination name, and any expected displaced destination identity before the publication syscall.
- Recheck the claimed source snapshot before publication.
- Publish with the same no-replace/exchange conflict rules used by transactional Copy.
- Flush the destination parent.
- Observe and persist the published destination identity and `Published` phase before source cleanup. Recovery after a crash immediately following publication uses the pre-persisted transaction, staged identity, and pending name to prove the outcome.

The destination transaction must return an owned transaction handle to Move. A retained pathname alone is not sufficient for cleanup ownership.

### 5. Source cleanup and completion

For cross-filesystem Move, recursively remove `claimed` only through the verified source transaction handle and only after:

- the published destination identity matches the recorded staged identity;
- destination durability succeeded;
- the claimed source still matches the persisted source-tree snapshot.

If any check fails, report committed cleanup pending and retain the claim.

The initial whole-tree snapshot is not deletion authority. Recursive cleanup uses a per-entry drain protocol:

1. Enumerate a child name, open it without following links, and record its stable identity.
2. Persist and flush a unique cleanup-claim intent.
3. Rename that child by name into a private cleanup quarantine under the transaction.
4. Reopen it in quarantine and compare identity with the pinned child.
5. If identities differ, restore with no-replace or retain the unexpected child; never unlink it.
6. If identities match, delete a file/link from the inaccessible quarantine, or recursively drain a directory using the same protocol. An externally held directory descriptor may add children, but each child must pass claim verification and final `rmdir` succeeds only when empty.

This converts every validate-then-unlink race into claim-then-verify. Process death at any cleanup claim is recoverable from the durable intent plus quarantine topology.

Remove or retain displaced overwrite data according to its verified identity. Then flush affected directories, remove the local journal/transaction only through owned handles, remove the central discovery record, and flush the recovery root.

## Recovery algorithm

At startup and before accepting a new Move, enumerate central records. Recover one transaction at a time under an exclusive recovery lock.

1. Parse both records strictly. A malformed or mismatched pair is quarantined and surfaced; no filesystem mutation occurs.
2. Reopen source parent, source transaction, destination transaction, and destination parent from recorded path hints. If a source ancestor moved, use Windows file-ID reopen where available or require the Linux user-assisted exact-ID locator before proceeding.
3. Verify every reopened stable identity before inspecting names.
4. Observe original, claimed, destination, staged, and displaced entries without following links/reparse points.
5. Select an action from observed topology; journal phase is advisory.
6. Perform at most one idempotent transition, flush it, update both records, and repeat until terminal or retained-for-user state.

### Recovery matrix

| Observed verified state | Safe action |
| --- | --- |
| Original source matches; no claim; no published target | Remove only a proven-empty transaction and central record. |
| Claim matches; original absent; target absent | Restore claim with no-replace, or retain if original parent/path identity is unavailable. |
| Claim identity differs from durable `ClaimIntent` | Never publish/delete it; restore with no-replace or retain as `UnexpectedClaimRetained`. |
| Claim matches; a different entry occupies original | Retain claim and report conflict; never overwrite the new entry. |
| Same-filesystem target matches original source identity; claim absent | Treat publication as committed; finish journal cleanup. |
| Cross-filesystem target matches recorded staged identity; claim matches snapshot | Treat publication as committed; resume verified source cleanup. |
| Target matches but claim changed since snapshot | Keep both and report committed cleanup pending. |
| Overwrite target matches source and claim matches displaced destination | Treat publication as committed; retain or safely clean displaced data. |
| Parent path no longer locates the recorded identity | Try file-ID reopen where supported; otherwise require Locate Transaction Folder and exact ID/journal verification. |
| Parent identity changed, target identity is ambiguous, or records disagree | Retain all data and require explicit recovery. |

Recovery never concludes that a target is Ottrin-owned from name or content alone.

## API boundary

Move should return a typed publication result rather than `Result<(), PlatformError>`:

- `Published`;
- `Skipped`;
- `RestoredAfterConflict`;
- `CommittedCleanupPending { recovery_records }`;
- `ClaimedRecoveryPending { recovery_records }`.

Batch aggregation must count only `Published` and committed-cleanup outcomes as completed. Claimed-but-unpublished outcomes stop the batch and expose recovery records without claiming completion.

The platform crate also needs explicit APIs to:

- discover pending Move records;
- run idempotent automatic recovery;
- list retained/manual recovery items for the Activity panel;
- restore a claimed source to a user-selected safe name;
- locate a transaction after an ancestor rename by searching a user-selected scope for the exact transaction ID and verifying identity/journal;
- finalize cleanup only after identity revalidation.

## Implementation milestones

These are independently testable boundaries, not release claims:

1. **Journal substrate:** native path encoding, atomic record replacement, central discovery, local/central consistency, corruption handling, and identity types. Move remains disabled.
2. **Claim and restore:** handle-relative source claim plus startup recovery for every crash point before publication. Move remains disabled outside tests.
3. **Linux same-filesystem Skip/Rename:** no-replace publication, durability, crash injection, and recovery. Enable only these proven modes on Linux.
4. **Cross-filesystem Skip/Rename:** destination transaction handle ownership, source snapshots, durable publication, and verified claimed-source cleanup.
5. **Overwrite:** atomic displaced-entry retention and explicit retained-data lifecycle. Enable per platform and entry kind only when proven.
6. **Windows parity:** handle/identity implementation and crash tests on native Windows. Do not enable Windows Move from Linux cross-compilation alone.
7. **UI recovery:** startup recovery reporting, Activity-panel actions, batch semantics, documentation, and packaged manual QA.

## Mandatory tests

Each supported platform/mode must test process termination—not only injected returned errors—at every durable boundary:

- before and after central record flush;
- before and after local journal flush;
- immediately before and after source claim and post-claim identity verification;
- after claim-directory flush but before phase update;
- during destination staging;
- immediately before and after destination publication;
- after destination flush but before `Published` update;
- immediately before and during source cleanup;
- before and after each journal removal.

Additional required cases:

- source replacement between pin and claim, including mismatch restore and retained-conflict paths;
- source, source parent, destination parent, and target replacement races;
- parent rename/retarget followed by Windows file-ID reopen or Linux user-assisted exact-ID location;
- non-UTF-8 Unix names and nontrivial Windows UTF-16 names;
- files, directories, symlinks/reparse points, dangling links, and hard links;
- same-path and descendant rejection before journal creation;
- Skip/Rename/Overwrite publication races;
- process death after each persisted Rename candidate and immediately after its publication syscall;
- destination transaction identity and staged identity persistence before every publication mode;
- EXDEV source mutation through an existing open descriptor or hard link;
- per-child replacement during recursive cleanup, including mutation through an already-open directory descriptor;
- malformed/truncated/unknown-version journals;
- central record missing local record and local record missing central record;
- repeated recovery and process death during recovery;
- multi-source partial-batch accounting;
- disk-full and permission failures at every record update/flush;
- native Linux and Windows packaged tests.

## Release gate

Move remains disabled for any platform, conflict mode, or entry kind until all of the following are true for that exact combination:

- handle-relative primitives are implemented;
- durable journal discovery and recovery are implemented;
- crash-injection and real process-termination tests pass;
- retained/manual recovery is visible and actionable in the UI;
- native packaged QA passes;
- an independent adversarial review confirms that no publication or deletion can occur without post-claim identity proof, and every mismatched pathname claim has a durable restore-or-retain path.
