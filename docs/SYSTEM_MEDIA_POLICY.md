# System Media Playback Policy

Approved by David on 2026-08-14. This policy supersedes the Linux metadata-only and no-playback restrictions in `.hermes/video-thumbnail-policy-approved.md` and `docs/VIDEO_THUMBNAIL_SPIKE.md`.

## Product decision

Ottrin may use media frameworks already provided by the user's operating system:

- **Linux:** the system-installed GStreamer framework and the plugins visible to that installation.
- **Windows:** Windows Media Foundation and the codecs visible to that Windows installation.

Ottrin asks the selected system framework to open and play a local media file. The framework—not Ottrin—discovers and selects an available demuxer, decoder, audio sink, and other media components from the user's installation.

## Distribution boundary

Ottrin does not:

- bundle or redistribute GStreamer, FFmpeg, codec packs, or GStreamer codec/plugin payloads;
- download, install, enable, recommend, or provide one-click installation of codec implementations;
- claim that a codec is available merely because a file has a recognized extension;
- guarantee playback of a particular format or codec on every installation.

GStreamer core and its official framework libraries are LGPL, but third-party or distribution-provided plugins can have other licenses. Ottrin therefore does not describe every plugin exposed by the user's system as LGPL. The operating-system distributor, plugin supplier, or user controls which plugins are installed and available under the terms applicable to those components.

Using a system-provided component does not make Ottrin its distributor. This policy records the product's technical and distribution boundary; it is not a warranty or a substitute for jurisdiction-specific legal advice.

## Required user-facing disclosure

Linux settings/about documentation must say:

> On Linux, Ottrin uses GStreamer installed on this system. Ottrin does not include, download, install, or select codec implementations. Playback depends on the media plugins made available by the operating system or user.

When playback fails because no suitable system component is available, the Inspector must say:

> This GStreamer installation does not provide the components needed to play this file. Ottrin does not install codecs. You can manage media support through your operating system or open the file in another application.

The UI must not accuse a file of being invalid when the actual failure is a missing or incompatible system component.

## Implementation contract

- Use the system GStreamer API on Linux; do not invoke or link directly to FFmpeg/libavcodec as an Ottrin media backend.
- Keep GStreamer and all codec/plugin payloads out of AppImage and portable-package contents.
- Detect GStreamer and playback capability at runtime and report the actual result truthfully.
- Accept only local, no-follow regular files through a retained file descriptor. Do not give GStreamer a network URI or reopen a mutable user path after admission.
- Run media state and decoder callbacks away from egui's render loop.
- Keep decoded playback frames transient; never persist them in the thumbnail cache.
- Bound source admission, decoded video dimensions, frame queue depth, command/event channels, cancellation, and teardown time.
- Stop playback and release the media session when the Inspector changes source or closes.
- Preserve **Open externally** as a fallback.

## Packaging and verification

- Linux builds require GStreamer development headers at build time, but release artifacts must not contain the GStreamer runtime or codec/plugin payloads.
- Distro packages may declare a dependency on the GStreamer core runtime. Codec plugin packages remain outside Ottrin's dependency closure and are not automatically installed by Ottrin.
- AppImage acceptance must inventory bundled shared libraries and files to prove that no GStreamer/FFmpeg codec implementation or plugin payload was added.
- QA records playback availability per tested installation; it must not generalize one machine's plugin set into a universal support claim.
- Licensing notices must identify GStreamer as the system media framework and link to its LGPL/licensing guidance without claiming ownership or redistribution of system plugins.

## References

- GStreamer application licensing: https://gstreamer.freedesktop.org/documentation/frequently-asked-questions/licensing.html
- GStreamer licensing advisory: https://gstreamer.freedesktop.org/documentation/application-development/appendix/licensing.html
- Microsoft Media Foundation supported formats: https://learn.microsoft.com/en-us/windows/win32/medfound/supported-media-formats-in-media-foundation
