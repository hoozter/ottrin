# Arthur · Ottrin Media Foundation Phase 1

Status: complete
Model route: difficult

## Repository boundary

- Repository: `/home/campbell/.hermes/worktrees/Ottrin-media-foundation`
- Branch: `work/media-foundation-phase1`
- Starting HEAD: `cc8d7d3 Fix test isolation boundaries`
- Source implementation commit: `21650a408c57406c388331bf434975bcc2c8e243 Add native Windows video thumbnails`
- Shared build output: `/home/campbell/Projects/Ottrin/target`
- Arthur remained the sole mutation owner. Nothing was pushed, published, deployed, released, signed, installed, or run against user media.

## Completed source outcome

- Added a Windows-only Media Foundation source-reader backend using OS-provided APIs and existing-version `windows`/`windows-core` bindings only.
- Admission opens a no-follow regular file once, rejects reparse points, retains the native handle, and derives generation identity from volume/file identity, size, last-write time, and change time. Decode, cache persistence, and response acceptance do not reopen the user path.
- The retained-handle COM `IStream` selects video only, requests bounded RGB32 output, reads at most one frame, checks cancellation/deadline throughout, validates source/output dimensions and byte bounds, normalizes BGRA to one platform-neutral RGBA result, and admits only a bounded PNG to cache/UI.
- Grid/List thumbnails and Quick Inspector use the existing preview scheduler/cache path. Superseding requests cancel prior work; cache keys include admitted generation and target dimension. Unsupported, malformed, cancelled, timed-out, changed, or over-limit media stays deterministically unavailable/metadata-only.
- Non-Windows profiles remain metadata-only and the generic raw-image fallback rejects video containers.
- Roadmap, user guide, and spike documentation now state the native-only implementation and keep native Windows/package acceptance pending.
- Old evidence commit `e674cf1` was inspected but not reused: it supplied a detached mock/backend contract without native file admission, frame decode, cache, or live UI integration.

## Verification evidence

- RED: `cargo test -p ottrin-preview --test video_thumbnail_contract` failed on the intentionally missing video contract symbols before production implementation.
- Focused contract: `cargo test -p ottrin-preview --test video_thumbnail_contract` — 4 passed.
- Affected crates: `cargo test -p ottrin-preview -p ottrin-ui` — preview 159 unit + 2 PDF integration + 4 video contract passed (4 PDF runtime tests ignored by their existing package requirement); UI 197 passed; doc tests passed.
- Workspace: `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings` all passed.
- Canonical source preflight: `./scripts/release-preflight.sh --source-only` passed format, workspace check/tests, strict all-feature Clippy, shell syntax, and its early package-policy guards, then stopped at the pre-existing `docs/USER_GUIDE.md` literal check for `bounded DOCX/XLSX/XLSM content-preview visuals`; starting `HEAD` already lacks that phrase and truthfully documents Office fallback, so this unrelated contradiction was not changed.
- Windows cross-target: `cargo check -p ottrin-app --all-targets --target x86_64-pc-windows-gnu` passed, covering preview, UI, and app integration. Two pre-existing Windows-only `ottrin-search` warnings remain (`unused_mut`; dead private rebuild/reconcile methods).
- Windows strict lint: `cargo clippy -p ottrin-preview --all-targets --target x86_64-pc-windows-gnu -- -D warnings` passed; `cargo clippy -p ottrin-ui --all-targets --target x86_64-pc-windows-gnu --no-deps -- -D warnings` passed. The unscoped UI command reaches the same two unrelated `ottrin-search` warnings.
- `git diff --check` passed before the source commit.

## Package and dependency boundary

- `Cargo.lock` gained only direct `ottrin-preview` references to already-resolved `windows 0.58.0` and `windows-core 0.58.0`; the dependency tree contains no new decoder/runtime package.
- The new dependencies are target-scoped to Windows and enable only implementation, Foundation, Media Foundation, FileSystem, and COM bindings.
- No packaging, installer, release workflow, runtime staging, or codec payload file changed. Searches found no FFmpeg, GStreamer, codec-pack, external-decoder, download, or installer dependency in Cargo/package configuration.
- No exact package was built or mutated; exact-package payload verification remains Yuki's external gate.

## Preserved dirty state

- This worktree intentionally retains untracked `.hermes/arthur-goal.md` and `.hermes/worker-project`; they were not staged or modified as product source.
- Primary checkout evidence was read-only and remains user-owned: modified `.gitignore` plus its existing untracked `.hermes` evidence/artifact directories and files.

## Residual risks and Yuki native Windows gates

- Cross-compilation does not prove COM/Media Foundation runtime behavior, installed-codec capability, frame usefulness, or responsiveness inside a synchronous native codec call. The retained `IStream` and every surrounding boundary enforce the two-second deadline/cancellation, but only native hostile/malformed codec tests can establish observed latency.
- Yuki must test on native Windows: real retained-handle identity and reparse rejection; path replacement and in-place mutation/cache invalidation; cancellation and deadline behavior; malformed/truncated inputs; unsupported installed-codec cases; large dimensions/pixel/output limits; signed pitch and decoded pixel orientation/color; successful single-frame thumbnails in Grid/List and Quick Inspector; deterministic metadata fallback; repeated lifecycle/cleanup; and no audio/playback/network/sidecar/plugin/process activity.
- Yuki must audit the exact portable and installer payloads for absence of decoder/codec DLLs, FFmpeg/GStreamer payloads, codec packs/installers, and dependency drift, then complete the separate licensing/patent product-claim review.
- Linux and other platforms are intentionally metadata-only. No native Windows, physical desktop, portable-package, installer, UAC, signing, or release acceptance is claimed here.

## Resume action

Yuki should check out source commit `21650a408c57406c388331bf434975bcc2c8e243`, then run the native Windows and exact-package gates above against that exact source range. No David action is required for source completion.
