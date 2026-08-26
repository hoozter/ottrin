# Ottrin Video Thumbnail Product Policy

Approved by David on 2026-08-06. This policy supersedes all earlier FFmpeg-first and hybrid recommendations.

> **Historical policy:** The Linux metadata-only, no-plugin, and no-playback
> restrictions below were superseded by `docs/SYSTEM_MEDIA_POLICY.md`, approved
> by David on 2026-08-14. The bounded Windows still-thumbnail contract remains
> relevant where it does not conflict with that newer policy.

## Phase 1: native-only, lowest licensing risk

1. Use only video-decoding frameworks supplied as part of the operating system.
   - Windows: Media Foundation for codecs and containers available through the user's Windows installation.
   - Other platforms: add a backend only after confirming it is an OS-supplied framework with a stable bounded API and no codec/runtime redistribution by Ottrin.
2. Ottrin does not bundle FFmpeg, GStreamer codec/plugin payloads, codec packs, external decoders, or any additional codec implementation in Phase 1.
3. Ottrin does not download, install, recommend, mirror, or provide one-click installation of external codec implementations in Phase 1.
4. Ottrin does not guarantee a codec list beyond capabilities reported by the native OS backend. Unsupported media remains metadata-only with a deterministic unavailable-thumbnail result.
5. No GPL, AGPL, nonfree, or additional patented codec implementation is distributed by Ottrin.

## Common safety contract

- One bounded still frame only; no playback or audio output.
- No network, sidecars, plugins, executable lookup, or external process/runtime installation.
- No-follow regular-file admission and handle-based backend access; never reopen a user path after admission.
- Hard time, memory, output-size, dimension, cancellation, and cache-generation limits.
- Deterministic unavailable-thumbnail fallback on every failure.
- All platform backends normalize into one platform-neutral validated-image result so cache/UI behavior cannot diverge.

## Platform acceptance

- Windows Media Foundation is the first implementation target.
- It requires actual native Windows tests for file-handle identity/reparse rejection, cancellation, malformed media, resource bounds, capability reporting, cache invalidation, and clean unsupported-codec behavior.
- Linux and other platforms remain metadata-only until a separately reviewed OS-native backend is approved. Linux success or test doubles must not be represented as Windows acceptance.
- Packaging verification must prove that no video decoder, codec DLL/shared library, FFmpeg binary, GStreamer plugin closure, or codec installer was added.
- A brief licensing/patent review remains a release gate for product claims and use of Media Foundation, but Ottrin itself distributes no codec implementation in Phase 1.

## Deferred options

A minimal LGPL-only FFmpeg fallback and an explicit user-provided external decoder are deferred. Either requires a new product decision, exact license/patent review, security design, package/provenance work, and independent acceptance review before implementation or distribution.

## Documentation correction

Before committing the earlier spike documents, revise `docs/VIDEO_THUMBNAIL_SPIKE.md`, `docs/ROADMAP.md`, and `.hermes/autonomy-state.md` to mark their FFmpeg-first recommendation as superseded by this native-only Phase 1 policy. Preserve the matrix as historical evaluation evidence, but do not imply that an FFmpeg runtime is approved or planned for the first slice.
