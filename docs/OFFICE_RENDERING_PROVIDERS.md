# Office rendering providers and compatibility packs

Status: product/engineering design; no provider or download has been accepted for production yet.

## Product contract

Ottrin should produce a useful, truthful preview without requiring a user to install an office suite. A detected compatible LibreOffice installation should be used automatically by default for supported Office documents, with a visible provider label and a simple opt-out. Compatibility packs are explicit, removable additions; they are never silently downloaded, installed system-wide, or run with elevated authority.

The fallback ladder is capability-driven rather than one global renderer:

1. compatible installed LibreOffice, when enabled;
2. an installed Ottrin compatibility pack appropriate to the format;
3. Ottrin-native structured preview;
4. clearly labelled extracted text and metadata.

A provider may be skipped after a typed failure (unsupported feature, encrypted file, resource limit, timeout, invalid output) without presenting flattened extraction as a layout preview.

## Proposed choices

### Use installed LibreOffice

This should be the normal high-fidelity path when `soffice`/LibreOffice is already present and its version/capabilities pass a bounded probe.

- Invoke the documented headless conversion surface through an absolute executable path.
- Use a fresh disposable LibreOffice user profile (`-env:UserInstallation=...`) so previewing does not alter the user's ordinary profile, recovery state, recent files, extensions, or lock state.
- Run conversion in an isolated child with a private output directory, no network, read-only input visibility, process-tree cancellation, and memory/CPU/wall/output limits.
- Validate the resulting PDF before exposing it to Ottrin's existing PDFium preview path.
- Cache only derived preview output, keyed by source identity/content state, provider identity/version, and conversion policy version.
- Show `Rendered with LibreOffice <version>` and expose an opt-out/fallback action.

Detection does not authorize installation, package-manager invocation, elevation, or modification of the detected suite.

### Lightweight Office Preview Pack — MiniPdf

Candidate contents: one pinned Native-AOT MiniPdf executable for the current platform plus Apache-2.0 notices, exact source reference/source offer, signed Ottrin manifest, and health probe.

Measured v0.38.4 Linux x64 footprint:

- download: 5,599,619 bytes;
- executable: 12,535,976 bytes.

It successfully converted Ottrin's two-page DOCX baseline into a valid PDF under a networkless bounded Bubblewrap run. It retained the main heading, styled text, table, spacing and explicit page break, but misplaced a right-aligned paragraph. It remains a beta with incomplete charts/complex layout, insufficient internal archive/resource admission limits, and `System.Drawing.Common` paths that require Linux/Windows parity testing.

Initial recommendation: offer MiniPdf as an explicit lightweight pack during corpus validation rather than silently embedding it in every package. Bundling it by default can be reconsidered only after format-specific fidelity, hostile-input, update and cross-platform gates pass.

### Office Fidelity Pack — portable LibreOffice

For Linux x86_64, LibreOffice's official alternative-download page links AppImages hosted by LibreItalia:

- 26.2.5.1 Basic: 296,379,584 bytes;
- 26.2.5.1 Standard: 345,076,928 bytes.

The distinction matters: LibreOffice itself is maintained by The Document Foundation, while these AppImage artifacts are hosted/package-distributed through LibreItalia and surfaced from LibreOffice's official download page. Ottrin must not describe the AppImage artifact as directly built or supported by TDF without stronger provenance evidence.

The Basic image is the proportionate starting candidate. Standard/Full mainly add language resources and should not be forced on every user. Current official-page links expose x86_64 AppImages; Linux ARM64 needs a separate supported distribution strategy rather than an assumed filename.

For Windows, the analogous portable candidate is LibreOffice Portable from PortableApps.com, which the official LibreOffice page says is packaged with TDF permission and assistance. Installed LibreOffice remains preferable when available.

An AppImage is portable packaging, not a security sandbox. The same hostile-document process isolation is required as for an installed suite. A fidelity pack also makes Ottrin responsible for:

- pinned checksums and signed pack manifests;
- licence/third-party/font/dictionary notices and covered-source availability;
- CVE and upstream-release tracking;
- transactional download, update, rollback and removal;
- per-platform clean-machine tests;
- approximately 300–350 MB per installed Linux pack;
- never sharing the user's normal LibreOffice profile.

## Pack lifecycle

Each pack manifest should contain:

```text
pack ID and display name
renderer and upstream version
platform and architecture
exact download size
SHA-256 for every payload
Ottrin manifest signature
licence/notices/source locations
supported formats and known exclusions
health-probe command and expected protocol version
minimum Ottrin version
```

Lifecycle requirements:

1. Present measured download and installed size before consent.
2. Download to user-owned staging storage with resumable transfer.
3. Verify signature and SHA-256 before activation.
4. Activate atomically by versioned directory plus current-pointer transition.
5. Keep the previous healthy version only until the new health probe passes.
6. Never run a partially downloaded or unverified payload.
7. Check for updates without downloading the payload; ask before a large update.
8. Remove binaries, manifests, derived caches and disposable profiles completely.
9. Keep normal-user and deliberate Administrator/root pack namespaces isolated.

## Provider process boundary

All external renderers should implement one internal host contract:

```text
input: read-only source handle/path + format + bounded rendering policy
output: private PDF path + page count + provider/version + diagnostics
failure: typed unsupported/encrypted/limit/timeout/crash/invalid-output result
```

Minimum controls:

- preflight input size and OOXML archive policy;
- no network and no inherited credentials/secrets;
- no access outside the source file, required local fonts and private work/output directories;
- CPU, wall-time, process-count, memory and output-size limits;
- kill the entire process tree on cancellation or deadline;
- reject symlink escapes and unsafe output paths;
- validate PDF structure/page bounds before PDFium;
- clean private profiles and temporary output after cache admission;
- preserve source bytes and metadata unchanged.

Linux can use Bubblewrap/cgroup or an equivalently verified boundary when available. Windows requires a Job Object, constrained token/process policy, private profile and network-denial strategy. A pack is not production-ready until both platform boundaries are exercised.

## UI

Settings should expose one `Office previews` section:

- **Installed LibreOffice:** detected version/path, enabled by default, opt-out.
- **Lightweight Office Preview Pack:** size/version/install/update/remove/health.
- **Office Fidelity Pack:** size/version/install/update/remove/health.
- **Native preview:** always available where implemented.

Quick Inspector should label the actual path used:

```text
Rendered with LibreOffice 26.2.5
Rendered with MiniPdf 0.38.4 — lightweight preview
Native Ottrin preview
Extracted text fallback — visual renderer unavailable
```

Do not ask users to understand provider internals during ordinary viewing. Show details only for failures, settings and provenance.

## Acceptance corpus

Use rights-cleared DOCX/XLSX/PPTX fixtures on Linux and Windows:

- normal paragraphs, styled runs and exact spaces;
- fonts/fallbacks, CJK and RTL;
- lists, tables, merges, headers/footers, sections and page breaks;
- images, wrapping, shapes, formulas/equations and hyperlinks;
- XLSX formatting, merged/wrapped cells, dates, cached formulas, wide sheets, charts and print areas;
- PPTX masters/layouts, groups, tables, images, gradients, charts and notes;
- malformed/truncated OOXML, encrypted files, XML-depth stress, excessive ZIP entries/inflation, huge images and external relationships.

Pass gates:

- no source mutation or active content;
- zero network requests;
- deterministic bounded completion or typed failure;
- clean cancellation/recovery;
- no missing major blocks, overlap or clipping on fixtures declared supported;
- provider/version-labelled output;
- visual comparison against trusted Office/LibreOffice goldens;
- truthful fallback after failure.

## Implemented provider contract and next slice

`ottrin-preview` now owns a renderer-neutral Office-provider contract. It defines typed provider identity, format capability and probe outcomes; bounded source/output/page/time policy; typed unsupported/encrypted/limit/timeout/crash/invalid-output/cancellation failures; deterministic installed-LibreOffice → lightweight-pack → fidelity-pack fallback; and cache identity containing provider, version and policy version. The production orchestration accepts an injected converter launcher, rejects outputs outside the private output directory plus malformed, encrypted, empty or over-limit PDFs, and preserves typed failed-attempt evidence.

Installed-LibreOffice discovery, probing, conversion command construction, and reusable Linux and Windows process boundaries are implemented behind that contract. Candidates are canonicalized executable paths and probed with a bounded `--version` call. Linux conversion uses a disposable profile, mapped read-only input, private output, sanitized environment, no network, transient systemd-user process/memory ownership, CPU/file-size limits, process-tree timeout/cancellation, bounded diagnostics, and controller-log cleanup. The Windows boundary atomically starts the child in a kill-on-close Job Object, whitelists inherited standard-I/O handles, translates only the private profile/output and admitted input paths, enforces aggregate/process memory and active-process limits, and owns descendant cleanup after timeout or cancellation. It is not a filesystem or network sandbox. On both platforms, provider output passes the contract's PDF validation and source-content generation revalidation before admission.

The Linux production preview pipeline now caches one detected capability outside document parsing, schedules the installed provider before native/extracted fallback, hands only its validated PDF to existing PDFium rendering, and binds in-memory pixels plus provider/version/page metadata to the current Office source generation and provider-aware cache identity. Quick Inspector labels the provider, uses its page count for navigation, rejects stale metadata, and suppresses a failed provider only for the affected source generation before returning to the truthful native/extracted fallback. Provider pixels remain memory-only until a metadata-bearing disk-cache record exists.

No provider downloader, installer, pack lifecycle, settings opt-out, native Windows runtime acceptance, real-installed-LibreOffice runtime acceptance, or physical GUI/fidelity acceptance is complete yet. The Windows Job Object boundary is source-complete, but the Windows provider remains unavailable until native fixtures independently verify conversion, descendant cleanup, limits, inherited handles, private-path cleanup, and fallback behavior. Cross-compilation and Linux containment are not Windows acceptance.

This avoids coupling the future pack manager to conversion semantics and prevents MiniPdf/LibreOffice-specific conditionals from spreading through UI scheduling code.

## Implementation order

1. **Completed:** define and test the provider process/result contract without adding a downloader.
2. **Completed in source:** add detected installed-LibreOffice conversion using a disposable profile and bounded Linux/Windows process isolation. Windows remains unaccepted and disabled pending native fixture evidence.
3. **Completed in source on Linux:** integrate cached capability selection, validated provider PDF rendering, provider/version labels, generation-bound page navigation, stale-response rejection, and truthful typed fallback into Quick Inspector. Real provider and GUI/fidelity acceptance remain pending.
4. Run a MiniPdf pack spike against the common corpus on Linux and Windows.
5. Decide, from evidence, whether MiniPdf graduates to an optional pack or default bundle.
6. Build the generic transactional pack manager only after at least one provider passes its corpus and security gates.
7. Add the LibreOffice fidelity pack after provenance, update, licence, platform and removal evidence is complete.

This order produces immediate value from software users already have, keeps Ottrin useful without it, and avoids building a generic package system before a renderer has earned adoption.
