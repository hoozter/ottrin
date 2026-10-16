# Preview Interoperability Policy

Approved by David on 2026-08-14.

## Product direction

Ottrin should lean toward useful preview interoperability when a vendor or
format clearly exposes a preview artefact for file managers, catalogues, or
other non-editing consumers. It should also use established system/user
rendering facilities where doing so avoids bundling another implementation.
Unclear cases remain reviewable; this policy is a strong default, not a claim
that every proprietary format has identical legal or technical terms.

## Preferred paths, in order

1. **Vendor-intended embedded preview.** Validate and display a standard,
   bounded thumbnail or composite deliberately stored in the user's file for
   preview/interoperability. Do not parse proprietary document semantics.
2. **Operating-system or desktop renderer.** Ask an installed framework or
   thumbnail service to render the local file. The framework uses only handlers
   exposed by the OS or user.
3. **Common system-installed open-source renderer.** Prefer a stable,
   appropriately licensed system API or tool—especially permissive or LGPL
   components—without bundling, downloading, installing, enabling, or
   recommending renderer/plugin payloads.
4. **Direct format implementation.** Implement only from an open/public
   specification or another reviewed interoperability basis. Do not infer
   permission to reverse-engineer proprietary editing semantics from the mere
   ability to recognize a container.

Established file-manager behavior is useful evidence of ecosystem intent and a
technical reference. It is not, by itself, a license grant or a substitute for
checking the actual component and format terms.

## Common boundaries

Every path must:

- operate only on the user's selected local file or retained descriptor;
- prohibit network fetching and undeclared sibling/resource access;
- bound source bytes, candidate count, dimensions, decoded pixels, memory,
  time, process output, and cache admission;
- validate the produced standard image before display;
- support cancellation and deterministic failure;
- identify the result truthfully as an embedded preview, system-rendered
  preview, or extracted fallback where that distinction matters;
- retain **Open externally** when no admitted renderer is available;
- keep package inspection and notices accurate.

Ottrin must not claim full format support merely because it can display a
thumbnail. It must not execute document macros, scripts, active content, or
vendor code embedded in a document.

## Affinity

Affinity `.af`, `.afphoto`, `.afdesign`, and `.afpub` files may receive a visual
preview when they contain a bounded, fully validated standard PNG composite.
Ottrin displays that PNG only. It does not interpret layers, objects, effects,
or other proprietary Affinity document semantics, and it does not claim that
the preview is current, full-resolution, or universally present across
versions. Absence or rejection of a valid composite falls back honestly.

This matches the practical role of the embedded preview used by file managers
and third-party cataloguing tools while preserving a narrow technical boundary.

## Media

Linux playback follows `SYSTEM_MEDIA_POLICY.md`: Ottrin asks the user's system
GStreamer installation to play the local file, and GStreamer selects from
plugins exposed by that system. Windows uses Media Foundation. Ottrin does not
bundle or install codecs/plugins.

## EPS and PostScript

EPS/PostScript does not have an LGPL renderer path on the reference Linux
system:

- installed `libspectre` is GPL-2+ rather than LGPL; and
- installed Ghostscript is AGPL/commercially dual-licensed.

On Linux and Windows, Ottrin may ask a separately installed Ghostscript
executable to render EPS as an optional host capability. Ottrin does not bundle,
link, download, or install Ghostscript, and EPS metadata/external-open remains
usable without it. Windows discovers console executables in versioned
`Program Files/gs` installations and absolute PATH entries; relative PATH entries
and shell launchers are not candidates.

Linux uses Ottrin's non-networked bubblewrap/systemd boundary. Windows uses a
fresh AppContainer without network capabilities, private profile/output access,
a read-only input copy and a kill-on-close Job Object. Both launch directly
without a shell, with bounded source/output bytes, output dimensions/pixels,
memory, process count and child lifetime. Linux additionally sets a CPU-time
limit. Windows bounds elapsed child time rather than claiming a separate CPU-time
quota. Each renderer receives a bounded, identity-checked EPS snapshot; PNG reads
also reject oversized or redirected final entries before decoding. Snapshot I/O,
like other filesystem reads, has no independent remote-storage timeout.

Renderer absence, sandbox setup failure or render failure falls back truthfully;
there is no unrestricted retry or Ghostscript launch dependency. This describes
component interoperability, not a claim that Ghostscript is LGPL or legal advice.

## Documentation language

Use factual language such as:

> Ottrin displays the standard preview embedded by the document producer.

or:

> Ottrin asked a renderer installed on this system to preview the file. Format
> availability depends on the handlers enabled by the operating system or user.

Do not claim that this policy guarantees legal immunity, that all system
handlers share one license, or that a preview proves complete document-format
compatibility.
