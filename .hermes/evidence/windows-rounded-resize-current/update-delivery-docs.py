"""Update current-delivery prose only after the stable cohort is really published."""
from pathlib import Path
import json
r=Path.cwd();e=Path(__file__).resolve().parent;read=lambda p:json.loads(p.read_text());source=read(e/'BUILD_SOURCE.json')['source_revision']
assert read(r/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json')['source_revision']==read(r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json')['source_commit']==source
p=r/'docs/ROADMAP.md';s=p.read_text();start=s.index('All seven current local packages');end=s.index('The earlier `9c914bd6`',start)
s=s[:start]+'''All seven current local packages use runtime source
`0aac86f98a6411b690ede891f48438d02a9701ea`: four Linux formats at
`dist/packages/`, plus ZIP/NSIS/development MSIX0.1.0.11 at
`dist/windows-candidate/`. The stable AppImage is
`dist/packages/Ottrin-0.1.0-x86_64.AppImage`, SHA256
`510096524216f60b92b803b7cdbb31b57c4cc01ea98204beb7a0b9be3bc5cce6`.
Exact scopes/hashes are in `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json` and
`dist/WINDOWS_CANDIDATE_ACCEPTANCE.json`. This is a local functional preview;
strict GUI idle CPU, full roadmap and final acceptance remain open. No public
release, signing or Store submission is implied. The failed MSI remains held.

| Current work | Verified state |
| --- | --- |
| Source | Committed/pushed0aac: shared Windows native viewport regions, rectangular maximized frames, theme-radius-aware resize on visible arcs, and exclusive resize/titlebar drag ownership. Search changes only a deterministic lock/recovery test. |
| Source checks | 473 UI tests pass, one existing ignored; strict native/Windows UI/app all-target lint and formatting pass. |
| Linux delivery | All four formats built and content-checked. Exact AppImage passes8 native mouse gestures in Dark100/Light150 at0/24 radius: mainSE and SettingsNW. All8 originals reviewed; normal close preserves source, both private profiles removed. |
| Windows delivery | ZIP/NSIS complete58-file payload equality; exact installer6f9327654e9775040d90dfff2bb4746b40319575ab79ed4c0204d47e9773a06d. Installed Native/OpenGL pass8 real pointer gestures plus18 live-editor/Cancel native-region checks. All26 originals reviewed with explicit taskbar/screen-clipping limits. Source/preferences survive normal close and supported uninstall. Native36 region/geometry checks across six window types retain source30ba1 with exact unchanged backend proof. MSIX11 passes actual MakeAppx/unpack and58-file equality, not a fresh update/login lifecycle. |
| Hosted gates | CI41708636305: all four exact0aac Linux/Windows verification and isolated release performance jobs succeed. These benchmark jobs do not cover static GUI idle CPU. |
| GUI performance | Strict below2percent one-core software-VM criterion fails: Native33.54percent, OpenGL3.70percent. Unchanged CBC Native baseline34.21percent. Native region round-trips match; root cause remains unproven. This is GUI CPU, not daemon CPU, and remains a release hold. |

Current evidence/registered producers: `.hermes/evidence/windows-rounded-resize-current/`.
`DELIVERY_VERIFICATION.json` rehashes all seven stable artifacts/sidecars and all58
Windows payload files. Three owned VMs/six volumes, four private desktops/profiles,
source archive, consumed extractions/transports and candidate stages are removed;
superseded stable package output is retired.21 obsolete compiler sessions were
removed earlier; the final exact-root inspection identifies0 more candidates.
Current packages, compact original evidence, newest useful compiler sessions,
user data and protected Windows bases remain. See `CLEANUP_FINAL.json` for scope.

Full Linux daemon load segments match CBC except its GNU build-ID and the ELF
section-header-table offset; all Windows executable sections match, with header
timestamp/checksum and nonloaded COFF symbol metadata differences. Whole-file
identity is not claimed. This is an executable identity comparison, not a new
sustained daemon benchmark. The earlier CBC read-only30-second personal-daemon
sample remains historical:0.267percent one core/about26MB RSS, no restart/config
change. Personal services, settings and data were untouched by this delivery.

''' +s[end:]
a=s.index('Native installed Windows radius comparison now confirms a defect:');b=s.index('Remaining service states/prompt acceptance',a)
s=s[:a]+'''The historical native Windows comparison confirmed opaque outside corners.
That defect is now source-fixed and delivered with actual native-region evidence
on all six window types, pointer gestures and live editor/Cancel propagation as
scoped above. Nondefault Windows DPI, physical graphics and full-window visual
layout remain separate acceptance gaps. '''+s[b:]
a=s.index('1. Finish the Windows window-corner delivery.');b=s.index('2. Complete the concentrated',a)
s=s[:a]+'''1. Continue the remaining Windows appearance/performance acceptance. Shared native
   window-corner clipping and visible-edge/titlebar resize are now delivered in the
   exact0aac cohort above. Do not repeat fulfilled saved-radius/native-region,
   pointer or live0/24/Cancel matrices without changed inputs or a reproduced defect.
   Verify real nondefault Windows DPI and full-window visibility/layout on a
   sufficiently sized software desktop. UIAutomation focus plus typed numeric
   input is not a complete visible-control/accessibility audit.
   Investigate the strict GUI idle failure, including unconditional frame refresh,
   without masking the failed threshold or confusing GUI CPU with index CPU.
   The watcher polling correction remains closed at its deterministic-test scope:
   b18 CI41693567945 and current0aac CI41708636305 all four jobs succeed, original
   polling deadline and strict error/hit assertions retained. Evidence/prose-only
   pushes skip redundant full CI; production-source changes keep all gates.
''' +s[b:]
s=s.replace('   Next appearance implementation: correct the confirmed opaque Windows corners\n   at the shared viewport boundary; test all intended windows, live radius changes,\n   resizing/maximizing and both supported renderers before publishing replacements.', '   The shared Windows opaque-corner correction is delivered at the scopes above;\n   continue actual DPI, fully visible layout and remaining whole-interface controls.\n   The CPU failure is not waived by functional shape/resize success.')
p.write_text(s)
p=r/'README.md';s=p.read_text();a=s.index('The current seven local artifacts use clean runtime');b=s.index('### Linux',a);s=s[:a]+'''The current seven local artifacts use clean runtime `0aac86f9`. All four
exact-source hosted jobs pass.473 UI tests pass with one existing ignored test.
The exact AppImage passes8 native resize gestures in Dark100/Light150; installed
Windows Native/OpenGL pass8 gestures and18 live radius/Cancel region checks.
Native36 full-region checks retain their original30ba1 scope with unchanged
backend identity. Development MSIX0.1.0.11 passes native assembly/unpack and58-file
payload equality; its update/login lifecycle is not freshly claimed. The strict
software GUI idle CPU gate still fails; Windows DPI, full-interface, daily-driver
and final owner acceptance remain open. See the roadmap for exact limits.

'''+s[b:];p.write_text(s)
p=r/'docs/RELEASE_NOTES.md';s=p.read_text();a=s.index('The delivered Linux');b=s.index('## Highlights',a);s=s[:a]+'''The current Linux AppImage/portable/DEB/RPM and Windows ZIP/NSIS/development
MSIX0.1.0.11 use runtime `0aac86f98a6411b690ede891f48438d02a9701ea`.
All seven stable artifacts/sidecars and58Windows payload files are rehashed;
exact source/hashes/scopes are in the stable acceptance manifests. All four
exact-source CI41708636305 jobs pass.473 UI tests pass, one existing ignored;
strict native/Windows lint and formatting pass.

Windows now applies theme corners to the actual native window region in both
renderers. Shared resize targets follow the visible arc, and titlebars yield
edge presses to resizing. Exact-package Linux Dark100/Light150 and installed
Windows Native/OpenGL pass16 actual mouse gestures. Windows also passes18 live
Window corners edits/Cancel region checks. All34 current originals are reviewed,
with screen/taskbar-clipping limits explicitly recorded. Earlier36 native-region
checks across six window types retain source30ba1 and exact backend identity.
Normal Close and supported Windows uninstall preserve source/preferences.

MSIX11 passes actual native MakeAppx assembly/unpack and58-file equality; no
fresh MSIX11 deployment/update/login acceptance is implied. Full daemon executable
contents match CBC aside from specified build-ID/header/nonloaded symbol metadata;
whole-file identity and new CPU benchmarks are not claimed. Earlier lifecycle,
picker, Office/media and broader workflow evidence retains original artifact scope.

The strict software GUI idle CPU gate remains failed: Native33.54percent of one
core and OpenGL3.70percent; unchanged CBC Native baseline34.21percent. Root cause,
nondefault Windows DPI, fully visible layout and whole-roadmap acceptance remain
open. This is GUI CPU, not index CPU. MSI registration rollback remains held.
No public release, Store submission or personal service/settings change occurred.

Owned VMs/media/profiles/stages, superseded packages, consumed archives/extractions
and obsolete compiler sessions are removed. Current stable packages, compact
original evidence, useful caches, user data and protected bases remain. Packaged
manuals are build-time snapshots; repository manifests/documents record later
acceptance. The roadmap goal remains active.

'''+s[b:];p.write_text(s)
