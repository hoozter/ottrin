# Video Thumbnail Backend Spike

Status: historical evaluation superseded by the approved native-only Phase 1 policy in `.hermes/video-thumbnail-policy-approved.md`.
Date: 2026-08-06

## Current product decision

Phase 1 uses only video-decoding frameworks supplied by the operating system.
Windows Media Foundation is the first implementation target, for the codecs and
containers available through the user's Windows installation. Ottrin does not
bundle, download, install, recommend, mirror, or invoke FFmpeg, GStreamer,
codec/plugin payloads, codec packs, external decoders, or any additional codec
implementation.

Linux and other platforms remain metadata-only until a separately reviewed,
OS-supplied backend with a stable bounded API is accepted. Ottrin does not
promise a codec list: native capability detection determines whether a given
file can receive a still thumbnail. Unsupported and malformed media return a
deterministic unavailable-thumbnail result while retaining ordinary video
metadata.

## Common implementation contract

The backend must normalize one bounded still frame into the shared validated
PNG/cache contract:

- Admit only a no-follow regular file through a stable handle; do not reopen a
  user path after admission.
- Produce at most one still frame. There is no playback, audio output, network,
  sidecar, plugin, executable lookup, or external process.
- Enforce cancellation, resource, dimension, output-size, and cache-generation
  limits before a result can reach the UI or persistent cache.
- Validate every backend result as a bounded PNG before cache admission.
- Treat admission failure, unavailable native capability, malformed media,
  cancellation, generation change, decoder failure, and cache failure as the
  same unavailable-thumbnail result.

Windows implementation and package acceptance require real native Windows
verification for reparse/file identity, cancellation, malformed input,
capability reporting, resource bounds, cache invalidation, unsupported codecs,
and confirmation that no video decoder/runtime/codec payload or installer was
added. Linux or test-double success cannot establish Windows acceptance.

The Phase 1 source path is implemented and cross-compiles for Windows. That is
source evidence only: none of the native Windows or exact-package acceptance
listed above is claimed complete.

## Existing architecture

- `crates/ottrin-preview/src/lib.rs` classifies `.mp4`, `.mkv`, `.webm`,
  `.mov`, `.avi`, and `.m4v` as `VideoMetadata`. On Windows only, the dedicated
  backend may attach a validated still; otherwise those containers remain
  metadata-only and never reach the generic raw-image fallback.
- `crates/ottrin-ui/src/thumbnail_cache.rs` owns PNG validation, thumbnail cache
  admission, invalidation, and worker-side requests. Video output must use that
  same ownership boundary.

## Historical evaluation matrix

The following records the earlier options considered before the policy decision.
It is evidence only, not an implementation plan or approval to add a runtime.

| Strategy | Historical finding |
| --- | --- |
| App-local FFmpeg subprocess | Previously preferred for common Linux/Windows coverage and process isolation, but superseded because Phase 1 may not distribute or invoke FFmpeg. |
| FFmpeg libraries | Rejected: equivalent license/patent burden and in-process native-decoder risk. |
| GStreamer pipeline | Rejected: plugin/runtime closure and platform-specific deployment complexity. |
| Narrow Rust codec library | Rejected: insufficient common-container coverage and no material license/patent simplification. |
| Windows Media Foundation plus separate Linux backend | Previously rejected as a cross-platform strategy; now selected only as the Windows native-only first backend. Linux remains metadata-only pending an independent decision. |

A disposable host FFmpeg command experiment is likewise historical only. The
host's GPL-enabled FFmpeg is not a candidate runtime and must not be used by
Ottrin.

## Deferred options

An app-local LGPL-only FFmpeg fallback or an explicit user-provided external
decoder needs a new product decision, license/patent review, security design,
package/provenance work, and independent acceptance review before either is
implemented or distributed.

## Sources

- Microsoft Media Foundation supported media formats: https://learn.microsoft.com/en-us/windows/win32/medfound/supported-media-formats-in-media-foundation
- FFmpeg legal guidance (historical evaluation): https://ffmpeg.org/legal.html
- GStreamer licensing guidance (historical evaluation): https://gstreamer.freedesktop.org/documentation/frequently-asked-questions/licensing.html
