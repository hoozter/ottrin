# Arthur · Ottrin Media Foundation Phase 1 Corrections

Status: complete
Model route: difficult

## Repository boundary

- Repository: `/home/campbell/.hermes/worktrees/Ottrin-media-foundation`
- Branch: `work/media-foundation-phase1`
- Reviewed implementation retained unchanged: `21650a408c57406c388331bf434975bcc2c8e243 Add native Windows video thumbnails`
- Prior source handoff retained unchanged: `60db10843ea040a3f3e6c4dffb675472b040d6f8 Record Media Foundation source handoff`
- Focused correction commit: `024ecff Bound native video thumbnail inputs`
- Shared build output: `/home/campbell/Projects/Ottrin/target`
- Arthur remained the sole mutation owner. Nothing was integrated into `main`, pushed, published, deployed, released, signed, or run against user media; no codecs were installed.

## Corrected blockers

1. Compressed input is now hard-bounded before authorization. The default policy admits at most 512 MiB, rejects an oversize retained handle before `AuthorizedVideoSource` construction, records the accepted length, and gives `IStream::Stat`, `Read`, bounded `Seek`, and every clone only that length. Oversize videos therefore retain the existing unavailable/metadata-only behavior and never reach Media Foundation.
2. Contiguous RGB32 output no longer requires `IMF2DBuffer2`. The backend prefers bounded `Lock2DSize`, then uses an RAII-balanced `IMFMediaBuffer::Lock`/`Unlock` fallback for ordinary contiguous buffers. It obtains signed default stride from the negotiated media type, using Media Foundation's documented stride calculation only when the attribute is absent, validates subtype/dimensions/current and maximum lengths/pointer ranges/row and output arithmetic, and normalizes positive or negative padded rows into one top-down RGBA result.
3. Roadmap, user guide, and spike documentation now state the 512 MiB source policy and the real timing boundary: deadline/cancellation checks are cooperative around synchronous native codec calls, not a hard wall-clock guarantee.

## RED–GREEN evidence

- RED: `cargo test -p ottrin-preview --test video_thumbnail_contract` failed before production edits because the source-byte constant/helper/limit field did not exist and negative stride could not type-check against the old unsigned contract.
- GREEN: the focused contract now has 6 passing tests covering exact source-size admission arithmetic, overflow-side rejection, packed BGRA, padded positive stride, negative bottom-up stride, invalid shape/output bounds, generation/target cache identity, and deterministic non-Windows unavailability.
- A Windows-target sparse-file admission test is compiled and ready to assert that a retained handle sized one byte over the policy returns `ResourceLimit`; it is not represented as native runtime acceptance.

## Final verification at `024ecff`

- `cargo test -p ottrin-preview --test video_thumbnail_contract` — 6 passed.
- `cargo test -p ottrin-preview -p ottrin-ui` — preview 159 unit, 2 PDF integration, and 6 video contract tests passed; 4 packaged-PDFium tests remained ignored by their existing runtime requirement; UI 197 passed; doc tests passed.
- `cargo fmt --all --check` passed.
- `cargo clippy -p ottrin-preview -p ottrin-ui --all-targets -- -D warnings` passed on the host.
- `cargo check -p ottrin-app --all-targets --target x86_64-pc-windows-gnu` passed. It retained only the two pre-existing Windows-only `ottrin-search` warnings (`unused_mut`; dead private rebuild/reconcile methods).
- `cargo clippy -p ottrin-preview --all-targets --target x86_64-pc-windows-gnu -- -D warnings` passed.
- `cargo clippy -p ottrin-ui --all-targets --target x86_64-pc-windows-gnu --no-deps -- -D warnings` passed; dependency output retained the same two unrelated search warnings.
- `git diff --check cc8d7d3..024ecff` passed for the full feature range.
- The correction changed no dependency manifest, lockfile, packaging, installer, runtime-staging, or release file. The feature range still contains only the original target-scoped references to already-resolved `windows 0.58.0` and `windows-core 0.58.0`; no decoder/runtime/package payload drift was introduced.
- The unrelated stale `USER_GUIDE` release-preflight literal was not changed or worked around.

## Self-review boundary

- Reviewed only `024ecff` and its interaction with retained-handle identity, `IStream` clones, Media Foundation media-type negotiation and both lock paths, generation-keyed cache admission, unavailable responses, and the existing UI scheduler.
- All successful native buffer locks have matching RAII unlocks on every return path. The 2-D path uses `Lock2DSize`'s bounded padded-surface range while separately validating current/max lengths; the ordinary path exposes only current valid bytes. Signed row offsets are checked before safe slice access.
- Oversize admission produces no authorized source, so the existing UI request path has no video bytes to decode or cache and retains metadata fallback. Successful output still passes the existing bounded PNG cache validation and generation checks.

## Residual native and release gates

- Cross-compilation does not prove COM/Media Foundation runtime behavior, installed-codec capability, buffer implementation variants, decoded pixel usefulness, or observed timing inside a synchronous hostile codec call.
- Native Windows must test real exact-limit and oversize admission, retained-handle/reparse identity, path replacement and in-place mutation, malformed/truncated/unsupported media, both 2-D and ordinary contiguous RGB32 buffers, positive/negative padded orientation and color, cancellation/deadline observations, repeated lifecycle cleanup, Grid/List and Quick Inspector fallback, and absence of audio/network/sidecar/plugin/process activity.
- The cooperative two-second deadline and cancellation policy is not a preemptive wall-clock bound. Hostile-codec observed latency remains an explicit native acceptance gate.
- Exact portable and installer payloads still require an audit proving no decoder DLL, codec implementation, FFmpeg/GStreamer payload, codec pack/installer, or dependency drift, followed by the separate licensing/patent product-claim review.
- Linux and other platforms remain metadata-only. No native Windows, physical desktop, portable-package, installer, UAC, signing, or release acceptance is claimed.

## Preserved dirty state

- The pre-existing untracked `.hermes/arthur-correction-goal.md`, `.hermes/arthur-goal.md`, `.hermes/arthur-last-message.md`, and `.hermes/worker-project` remain unstaged and unmodified by the product correction.
- No unrelated tracked work is present in this worktree.

## Resume action

Yuki should use source correction `024ecff` for native Windows and exact-package acceptance. No David action is required for source completion.
