# Quick Inspector Text Editing Design

Status: architecture only; no production editing is implemented or enabled.

This document specifies a deliberately narrow, fail-closed save path for a
future Quick Inspector text/code editor. It does not change the v1 release
floor. The current inspector loads a `PreviewData` snapshot asynchronously and
uses editable-looking `egui::TextEdit` widgets only to display a copied string;
there is no save operation. That distinction must remain true until the slices
below are implemented and independently reviewed.

## Scope and non-goals

The first mode edits one existing regular file whose exact opened bytes are
valid UTF-8 in an explicit allowlist of text/code extensions. It has no
autosave, no blind overwrite, no rebase/merge, no file creation, no Save As,
no directory/archive/package editing, no symlink following, no elevated retry,
and no integration with file-operation Undo, Copy, Cut, or Move.

It excludes PDF, Office, image, audio/video, archives, folders, generic
fallbacks, alternate encodings, and every format that currently receives a
metadata or rendered preview. A source can remain previewable while being
ineligible for editing. The UI must say why, rather than offering a disabled
button with an ambiguous failure.

The same allowlist should initially be the `PreviewKind::Text` set in
`ottrin-preview`, except `svg` is read-only because the inspector renders it as
an image. The save transaction must take an explicit `EditableTextFormat`, not
a `PreviewKind`, so later preview classification cannot accidentally enable
editing.

## Eligibility and bounded open

`ottrin-platform` owns an `open_text_snapshot` operation with this result:

```text
EditableSnapshot {
  canonical_path: PathBuf,
  operation_binding: StorageOperationBinding,
  file_identity: StableFileIdentity,
  byte_len: u64,
  modified_hint: TimestampHint,
  opened_hash: Sha256Digest,
  encoding: Utf8 { bom: bool, newline: Lf | Crlf },
  text: String,
}
```

`StorageOperationBinding` is promoted from the UI-private concept already used
for file operations into a platform/core request type. It records the discovered
root plus its stable volume/share identity, or the deliberately narrow home
root binding. A path, mount point, drive letter, and discovery key are routing
hints only; none is a durable device identity. `StableFileIdentity` is an
OS-proven `(device,inode)` equivalent on Linux and `(volume serial,file ID)`
equivalent on Windows. If the platform cannot capture it, opening is read-only.

The open operation must, in order:

1. Reject a directory and inspect with no-follow semantics. Reject a symlink,
   reparse point, special file, or anything whose identity cannot be pinned.
   Canonicalization is a diagnostic/path-display aid only, never permission to
   follow a link during a later save.
2. Capture the required storage binding from the current discovery snapshot;
   reject unavailable roots, missing stable identities outside Home, and paths
   outside the binding's root.
3. Check the allowlisted format, a conservative byte limit (initially a named
   `MAX_EDITABLE_TEXT_BYTES`, lower than UI memory limits until measured), and
   a character limit after UTF-8 decoding. Read exactly the captured regular
   file through its opened handle, bounded before allocation, then hash those
   exact bytes with SHA-256.
4. Reject malformed UTF-8, NUL bytes, and control bytes other than tab, LF, and
   CR. Do not guess a legacy encoding. A UTF-8 BOM is accepted and recorded,
   then removed only from the in-memory editing string; save restores it.
5. Record line-ending style from the exact opened bytes. Mixed LF/CRLF is
   rejected in the first mode rather than normalized silently. A no-final-LF
   file retains that property. The editor operates on Unicode text but produces
   the recorded UTF-8/BOM/newline byte representation on save.

No size, timestamp, extension, or text inspection is sufficient by itself.
The open hash and pinned identity describe the byte sequence the user actually
saw. Timestamps are diagnostics and quick conflict hints only.

## Editor state and navigation

The UI owns an `InspectorEditorSession` distinct from `PreviewOverlay`:

```text
ReadOnly(reason) -> Clean(snapshot) -> Dirty(snapshot, undo) -> Saving
Saving -> Clean(new snapshot) | Conflict(reason) | Error(reason)
Clean/Dirty/Conflict/Error -> ClosingPrompt when close, navigation, or app exit is requested
ClosingPrompt -> prior state | Closed
```

`Clean` holds the opened snapshot and editor buffer. Any user text mutation
moves it to `Dirty`; reverting to the exact opened text returns it to `Clean`.
`Saving` disables save, navigation that would replace the session, and a second
save, but must not block directory browsing in unrelated tabs. `Conflict` and
`Error` preserve the unsaved buffer and diagnostic reason. They never rewrite
the target or silently reload it.

Undo/redo is bounded by count and byte budget in this session only. It is not
an `OperationRecord`, is discarded when the session closes, and must never
expose the file-operation Undo wording. Tab changes, selection synchronization,
Space/Escape close, explicit close, and app exit prompt only when dirty or
saving. Saving asks the user to wait/cancel only if the transaction has not
crossed its replace point; after replacement, the result is reconciled before
any close decision. There is no autosave or recovery file in the first mode.

## Pre-save validation

`save_text_snapshot(request)` accepts the complete immutable opened snapshot
plus new editor text; it must not accept a raw path plus contents.

Immediately before writing any temporary file it must:

1. Rediscover locations and validate the exact captured
   `StorageOperationBinding`. A missing, remounted, or differently identified
   root is a `Conflict::StorageUnavailable`, not a retry against the pathname.
2. Reopen the parent and target with platform no-follow/handle-relative APIs,
   then confirm the target is a regular non-link object with the captured stable
   identity. A delete/recreate, replacement, or symlink/reparse substitution is
   `Conflict::TargetReplaced` with no mutation.
3. Read the reopened target with the same bounded rules and SHA-256 hash it.
   Its bytes must exactly equal `opened_hash`; byte length and timestamps only
   improve the message. Any difference is `Conflict::ChangedOnDisk`.
4. Re-encode the supplied editor string using the captured BOM/newline policy,
   reapply byte/character limits, and hash the intended bytes. This prevents a
   UI-side conversion from expanding an otherwise bounded document.

There is intentionally no v1 "overwrite anyway" action. A future explicit
rebase/force mode needs its own design, conflict UI, authorization boundary,
and independent review.

## Atomic durable save transaction

The platform transaction owns all mutation. It must use verified parent and
target handles/descriptors where the OS permits; string paths are not reused
after initial routing. The transaction has these phases:

```text
Validated -> TempCreated -> TempWritten -> TempFlushed -> Replaced -> ParentFlushed
                 |              |              |              |             |
                 +--------------+--------------+--------------+-------------+
                                      failure -> cleanup owned temp / truthful result
```

1. Create an unpredictable `.ottrin-edit-<128-bit-random>` temporary entry in
   the target's verified parent with exclusive create/no-follow semantics and
   mode `0600` (or platform-equivalent owner-only creation). A collision retries
   with fresh randomness a bounded number of times. Never select or delete an
   existing name.
2. Copy required metadata intentionally from the verified old target before
   publication. The initial policy preserves permissions and ownership only
   where the current user can do so without broadening access; inability to
   preserve required metadata aborts before replacement. ACLs, xattrs, labels,
   and platform-specific metadata require an explicit per-platform support
   table before enablement. Never inherit a permissive umask or silently widen
   permissions.
3. Write all encoded bytes to the owned temporary handle, detect short writes,
   flush/sync its data and required metadata, and close it. ENOSPC, I/O, or
   flush failure leaves the original target untouched.
4. Revalidate target identity and current hash through the verified target
   handle immediately before publication as far as the platform supports.
   Atomically replace the target using same-directory/same-filesystem native
   semantics. Linux uses descriptor-relative rename only after no-follow
   identity fencing; Windows uses a handle-aware replace path that rejects
   sharing violations and reparse-point substitution. Never fall back to
   delete-then-rename or copy-then-delete.
5. Flush the parent directory where meaningful. Linux must fsync the verified
   parent descriptor after rename. On Windows, use the strongest documented
   replacement/write-through primitives and report when directory durability
   cannot be proven rather than claiming it.
6. Before replacement, any error removes only the temporary object while it is
   still proven owned by its name, parent handle, and captured identity. After
   replacement, do not delete the old target or an unknown entry as cleanup.
   A failure after replacement but before durable parent flush is
   `CommittedDurabilityUnknown`: the visible file may already be the complete
   new bytes, but durability is ambiguous and the UI must say so.

The unavoidable final OS race is documented rather than hidden: no portable
API makes every final pathname replacement conditional on an earlier object
identity. Handle-relative Linux operations and Windows handle/reparse checks
must eliminate all avoidable application-level validate-then-mutate gaps.
Unsupported platforms/modes return `UnsupportedSafeEdit` before creating a
temporary file.

## Result truth and integration

Only `ParentFlushed` returns `Saved { new_snapshot }`. The UI then replaces the
session snapshot/hash with the confirmed written bytes, clears dirty/undo state,
invalidates the Quick Inspector preview and image/text thumbnail cache for that
path, refreshes the listing, and submits exactly one existing search/watcher
mutation notification. The notification occurs after success and carries a
transaction/request ID so an observed watcher event cannot create a duplicate
Activity record.

`Conflict`, pre-replace `Error`, and `CommittedDurabilityUnknown` retain the
editor buffer. They do not mark the preview clean, update its snapshot, emit a
successful activity event, enable Move, or create a path-based Undo action.
An Activity entry may record save status, but it is informational and explicitly
not restorable. Existing Copy/Cut and privileged retry plumbing cannot carry a
save request; editing has no privileged mode in the first implementation.

## Adversarial TDD matrix and invariants

The platform crate owns deterministic failure injection at every phase and a
headless test harness. Each behavior starts RED against the pure transaction,
then receives the smallest GREEN implementation; UI enablement follows only
after platform review.

| Case | Expected result | Required invariant |
| --- | --- | --- |
| External modify, delete/recreate, same-root replacement | conflict before temp mutation | original/replacement object is never overwritten |
| Symlink/reparse substitution or mount disappearance | conflict/unavailable | no followed substituted object is mutated |
| Permission change, ENOSPC, short write, temp collision | pre-replace error | original bytes preserved; only proven-owned temp is removed |
| fsync failure before replace | error | original bytes preserved |
| rename/replace failure or Windows sharing violation | error | original bytes preserved |
| crash after each listed phase | recovery inspection | target is old or complete new bytes, never partial; unknown temp is never deleted |
| parent flush failure after replace | durability-unknown | no false saved claim; complete replacement may exist |
| malformed UTF-8, NUL/control bytes, mixed endings, oversized/binary data | read-only reason | no editor/save surface |
| UTF-8 BOM, LF/CRLF, no final newline | saved exact encoding policy | no silent conversion/data loss |
| watcher event races / navigation while saving | one truthful completion | no duplicate event, stale reload, or lost dirty buffer |

The harness is a CLI/test-only binary, initially `ottrin-platform` integration
tests or `ottrin-edit-harness`, with commands such as `open`, `save`, and
`inject <phase>:<failure>`. It accepts fixture roots and emits structured JSON
with snapshot identity/hash, phase, result, and observed bytes. It must create
all fixtures in disposable test directories and supports no arbitrary host-path
mutation. This allows Linux/Windows transaction behavior, crash fixtures, and
failure semantics to be exercised before GUI/manual testing.

## Ownership and implementation slices

1. **Platform/core RED→GREEN transaction:** add pure snapshot/request/result
   types in `ottrin-core` or a narrow `ottrin-platform` public API; implement
   eligible open, identity/hash conflict detection, and failure-injection tests.
   Do not add a UI control.
2. **Linux safe save:** descriptor-relative temp/write/fsync/rename protocol,
   metadata policy, crash fixtures, and harness. Independent data-safety review
   is required before the API can be called by UI code.
3. **Windows safe save:** native handle/reparse/sharing/replace implementation
   plus the equivalent test/harness evidence. Unsupported Windows conditions
   remain read-only.
4. **UI session:** add bounded editor state, prompts, truthful errors, and save
   integration behind an explicit capability result from the platform. No
   privileged retry, no Move/Undo reuse, and no enablement on a platform whose
   transaction remains unsupported.
5. **Manual/native gates:** packaged Linux and real Windows edit/conflict/
   sharing/removable-volume smoke tests, assistive keyboard flow, crash/power
   interruption review, and release-owner sign-off against exact artifacts.

## Cost/Risk Alert

Text saving crosses a new destructive trust boundary: previews can safely read
paths, but a save replaces user data and must preserve identity, metadata, and
durability truth across OS-specific semantics. This is why the first slice is a
platform transaction plus adversarial harness, not a Quick Inspector button.
The measured editable-size budget and Windows durability/metadata support must
be established before enabling any production mode.
