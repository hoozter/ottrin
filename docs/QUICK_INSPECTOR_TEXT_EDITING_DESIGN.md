# Quick Inspector Text Editing: Limited Linux Safety Boundary

Status: **implemented only for the coordinator-authorized Linux/plain-file
slice**. Text editing is not a v1 release gate. Windows, macOS, BSD, and other
platforms remain read-only/fail-closed; the broader cross-platform proposal
below remains deferred.

The Quick Inspector loads a `PreviewData` snapshot asynchronously. On Linux,
an eligible complete UTF-8 text preview can open a bounded editor and save its
buffer through the platform boundary. Failed saves retain that buffer. This
does not authorize broader editing, format support, or platform enablement.

## Implemented Linux/plain-file support boundary

The current mode accepts only one existing regular UTF-8 file no larger than
128 KiB. Opening pins its parent directory descriptor and basename. Every
subsequent source open, metadata check, staging operation, and publication is
relative to that descriptor; a renamed or substituted pathname parent cannot
redirect the save to an unrelated replacement directory.

Before admission and again immediately before publication, the platform checks
the descriptor-relative file identity, content digest, size, timestamps, mode,
owner/group, link count, sparse layout, inode flags, and xattr list. It rejects
symlinks, hard links, xattrs (including ACLs, security labels, and
capabilities), setuid/setgid/sticky modes, sparse files, nonzero inode flags,
and any source whose owner/group cannot exactly match a freshly staged inode.
The stage starts as a private `0600` file, receives the source's ordinary mode
bits exactly, is synchronized, and is reclassified before it can replace the
source. The parent directory is synchronized before and after the descriptor-
relative rename. Once staging has begun, a failed save deliberately attempts no
automatic cleanup: the public-parent name may have been replaced by another
actor. The resulting error states that a private stage may remain for
inspection rather than asserting ownership of that name.

The final kernel scheduling interval between the last identity check and
`renameat` remains inherent: an actor that can mutate the *already pinned*
directory can replace the target in that interval and have that replacement
overwritten. This implementation removes the earlier parent-path validation and
staging gap; it does not claim compare-and-swap publication. A successful save
uses ordinary replacement semantics and therefore updates file timestamps. If
the final parent-directory sync fails after rename, the result explicitly says
that text may already be published but directory durability is unconfirmed;
the editor keeps its unsaved buffer. There is no crash journal or recovery
claim for this limited editor mode.

## Scope and non-goals

This mode edits one already-existing complete UTF-8 text preview. It has no autosave,
blind overwrite, rebase/merge, file creation, Save As, directory/archive/package
editing, symlink following, elevated retry, or file-operation Undo/Copy/Cut/Move
integration. PDF, Office, images, audio/video, archives, folders, generic
fallbacks, alternate encodings, and rendered previews remain read-only.

Preview completeness remains an admission requirement; truncated or failed
previews never become editable. The UI reports platform and metadata rejection
truthfully.

## Remaining ownership and provenance requirements for any expansion

The future platform boundary, not `ottrin-ui`, owns opaque immutable values:

```text
StorageBinding
ResolvedTextTarget
EditableSnapshotToken
MutationAdmissionKey
MutationLease
TextSavePhase / TextSaveResult / TextSaveError
```

`StorageBinding` captures a stable root handle/identity, a stable parent
handle/identity, root-relative path components, target basename, and target
identity. Display paths, mount points, drive letters, discovery keys, and
canonical paths are diagnostics only. Home is identity-fenced exactly like every
other root; root-string equality is never sufficient.

On Linux, resolution starts from the pinned root and rejects links in every
component using `openat2` resolve constraints where available, or a
descriptor-walk fallback. On Windows, resolution must use the equivalent
root-handle, file-ID, and reparse-safe traversal. A parent rename/replacement,
root replacement, delete/recreate, or target identity change is a conflict;
the implementation must never reconstruct a target from a raw path.

The platform/core mutation-admission service owns reservations keyed by pinned
root plus parent/target identity. It serializes two editor sessions and every
Ottrin mutator that affects that object or path, including `FileCommand` and
independent image rotation. The model must define lease owner, acquisition
conflict, cancellation/shutdown release, and terminal states. A text save never
enters `FileCommand`, privileged retry, or path-based Undo.

## Deferred safe-open and encoding contract

A future open operation returns a snapshot token only after it has opened a
regular non-link object through the resolved target, captured its stable
identity/link count, bounded the read before allocation, and hashed the exact
opened bytes. Missing identity/support is read-only.

The editor buffer is canonical LF Unicode text, distinct from the current
per-frame trimmed preview widget. At open and save it rejects malformed UTF-8,
NUL, bare CR, mixed line endings, and forbidden controls. A BOM is valid only at
byte zero and is recorded separately. The snapshot records LF or CRLF and the
final-LF property. Encoding expands canonical LF exactly once, restoring the
recorded BOM/newline/final-LF policy without double CR. Scalar-count and encoded
byte limits use checked arithmetic; Unicode expansion is tested. There is no
legacy-encoding guess or silent normalization.

The initial mode rejects a link count other than one at open and immediately
before conceptual publication (`st_nlink` on Linux and handle-derived link count
on Windows). Atomic replacement does not preserve hard-link topology.

## Residual native transaction constraints beyond the limited implementation

Before any native GREEN work, the design must be independently reviewed again.
The supported threat model must explicitly acknowledge that neither Linux
`renameat`/`renameat2` nor Windows replacement APIs offer pathname compare and
swap against an earlier inode/file-ID/hash. A final kernel scheduling race may
remain between final validation and publication. The implementation must remove
every earlier application validate-then-mutate gap, expose that residual race
truthfully, and never claim that a replacement observed before publication
cannot be overwritten. `RENAME_EXCHANGE` is not a hidden workaround because it
mutates the destination namespace.

The implemented Linux slice uses a private `0600` staged file in the retained
source-parent descriptor rather than a general transaction directory. Any
future multi-file, cross-directory, or crash-recovery mode needs a newly
created `0700` private staging directory with its own retained descriptor/
handle. Collision handling, ownership marker/identity, close order, cleanup
order, and crash orphan inspection must be explicit and bounded. If ownership
becomes uncertain, retain and report the orphan; never unlink a public-parent
name after a separate identity check.

Metadata policy must be a fail-closed platform matrix, validated by fixtures
before capability becomes writable. Linux must explicitly classify POSIX ACLs,
xattrs, SELinux labels, capabilities, ownership, mode/set-id bits, timestamps,
flags, and sparse layout. Windows must explicitly classify security descriptor,
DACL/SACL, owner/group, integrity label, ADS, EFS, compression, sparse state,
attributes, and EAs. Unsupported, unreadable, or unsettable required metadata
means read-only/reject before replacement: no inheritance, best effort, or
silent downgrade.

Windows feasibility must name exact `CreateFileW` access/share/flags, reparse
checks, the `SetFileInformationByHandle` versus `ReplaceFileW` choice and its
limits, `FlushFileBuffers` behavior, security-descriptor treatment, and
post-replace durability result. Unsupported conditions remain read-only.

## Deferred expansion work

Any expansion beyond the implemented Linux/plain-file mode requires a new value
decision and explicit task authorization. It must not treat this narrow mode as
evidence that the broader ownership, metadata, crash, or cross-platform model
is safe.

RED tests must cover:

- absent/replaced Home or root; all-component link/reparse rejection; parent
  rename; delete/recreate; and target identity mismatch;
- `nlink > 1`, stale hash, malformed UTF-8, BOM placement, LF/CRLF/mixed/bare
  CR/newline rules, forbidden controls, scalar/byte limits, and Unicode
  expansion;
- two editor leases; concurrent `FileCommand` and image mutation admission;
  cancellation before and after conceptual replacement; and prohibition of
  privileged/Undo routing;
- deterministic failure/race injection immediately before conceptual
  publication, with the residual OS race reported rather than hidden.

An eventual crash harness is separately constrained: test-only feature/binary,
process-created nonce-marked temporary root, descriptor-rooted fixtures, no
caller-controlled host target, and no installation. Crash evidence requires
subprocess phase barriers, forced termination, reopening inspection, and
inspect/report-only cleanup unless exclusive ownership is proven.

## Gates before broader future enablement

1. Independent Linux architecture/data-safety review before Linux native GREEN.
2. Independent Windows architecture review, then Windows native runtime and
   crash evidence before Windows native GREEN.
3. Proven metadata and durability matrices for each enabled platform/mode.
4. Explicit platform capability result; UI stays read-only until that result and
   all relevant gates are accepted.
5. Packaged Linux and real Windows conflict/sharing/removable-volume/manual
   evidence plus release-owner review against exact artifacts.

## Cost/Risk Alert

Saving replaces user data across a destructive cross-platform trust boundary.
An accepted design would authorize only a value evaluation, not automatic
implementation. Before RED-0 or native work, the coordinator must compare the
expected value with the substantial provenance, metadata, durability, crash,
and platform-evidence cost. This proposal does not change the v1 boundary.
