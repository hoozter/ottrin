"""Reconcile current delivery prose only after verified stable publication."""
from pathlib import Path
from datetime import datetime
from zoneinfo import ZoneInfo
import json, re

r = Path.cwd()
e = Path(__file__).resolve().parent
read = lambda p: json.loads(p.read_text())
source = read(e / 'BUILD_SOURCE.json')['source_revision']
linux = read(r / 'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json')
windows = read(r / 'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json')
assert linux['source_revision'] == windows['source_commit'] == source
assert read(e / 'DELIVERY_VERIFICATION.json')['source_revision'] == source
assert read(e / 'CLEANUP_FINAL.json')['pending_disposable_consumers'] == []
appimage = linux['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
installer = windows['artifacts']['Ottrin-0.1.0-windows-x64-setup.exe']
p = r / 'docs/ROADMAP.md'
s = p.read_text()
s = re.sub(r'Updated \d{4}-\d{2}-\d{2}\.', 'Updated ' + str(datetime.now(ZoneInfo('Europe/Stockholm')).date()) + '.', s, count=1)
a = s.index('All seven current local packages')
b = s.index('The earlier `9c914bd6`', a)
s = s[:a] + f'''All seven current local packages use runtime source
`{source}`: four Linux formats at `dist/packages/`, plus
ZIP/NSIS/development MSIX 0.1.0.13 at `dist/windows-candidate/`.
The stable AppImage is `dist/packages/Ottrin-0.1.0-x86_64.AppImage`, SHA256
`{appimage}`. Exact source, artifact hashes and acceptance scopes are in
`dist/packages/LINUX_PACKAGES_ACCEPTANCE.json` and
`dist/WINDOWS_CANDIDATE_ACCEPTANCE.json`. This is a local functional preview;
full roadmap, GUI performance and final owner acceptance remain open.
No public release, signing or Store submission is implied. MSI remains held.

| Current work | Verified state |
| --- | --- |
| Source | Committed/pushed `{source[:8]}`: pinned egui DragValue now publishes its final committed value and correct boundary actions. Named appearance controls preserve roles/ranges and match the live model in the input frame. Layout, palette, geometry, daemon and service implementations are unchanged. |
| Source checks | 27 appearance tests and 476 UI tests pass; one existing ignored. Strict native/Windows UI/app all-target lint and formatting pass. |
| Linux delivery | All four formats pass package-content checks. Exact AppImage passes six native typing/Restore/Cancel cases in Dark100/Light150; all four original captures reviewed. Normal close and fixture source/saved-theme preservation pass. |
| Windows delivery | ZIP/NSIS have identical 58-file payloads; installer SHA256 `{installer}`. Installed Native/OpenGL pass 34 complete native-region/DPI/work-area observations and eight focus-acknowledged numeric commits. Named Restore, Cancel, normal Close and supported uninstall preserve source/preferences. All 22 unobscured full-window captures reviewed. Background owners have region observations while editor input is active; separate captures cover all six window types. MSIX13 passes native MakeAppx/unpack and exact 58-file equality; no fresh MSIX runtime/update/login claim. |
| Hosted gates | CI 41728865733: all four exact-source Linux/Windows verification and isolated release performance jobs pass. These jobs do not establish static GUI idle CPU acceptance. |
| GUI performance | Earlier strict default-DPI software-VM samples failed: Native33.54%, OpenGL3.70% of one core, with unchanged CBC Native baseline34.21%. No fresh GUI CPU measurement is claimed by this delivery. The unresolved failure remains a release hold, separate from daemon CPU. |

Current evidence and registered producers:
`.hermes/evidence/appearance-numeric-names-current/`.
`DELIVERY_VERIFICATION.json` rehashes all seven stable artifacts/sidecars and all
58 Windows payload files. `CLEANUP_FINAL.json` records two owned VMs/four volumes,
three private profiles, source archive, consumed transports/extractions and candidate
stages removed; superseded stable output retired. Fifteen obsolete incremental
sessions were removed, newest per-configuration sessions retained; final inspection
finds zero obsolete candidates. Current packages, compact evidence, useful shared
caches, user data and protected bases remain. The small signed display-driver cache
is retained for future isolated DPI acceptance.

The Linux daemon is byte-identical to the previous 0aac cohort. Windows daemon
loaded code/data and all executable sections match; timestamp/checksum and
nonloaded COFF metadata differ. This is identity evidence, not a fresh daemon
benchmark. The earlier CBC read-only 30-second personal-daemon sample remains
historical:0.267% one core/about26MB RSS. Personal services/settings/data were
not changed by this task. Earlier pointer, native-region, preview, picker, folder,
service and media receipts retain their exact original source/artifact scopes.

''' + s[b:]
s = s.replace('The 100-run GitHub audit snapshot contains 30 failures, 18 cancellations and\n52 successes after the current exact-source run completed.', 'The historical 100-run GitHub audit snapshot recorded 30 failures, 18 cancellations\nand 52 successes at its original 0aac checkpoint; these are not current rolling counts.')
s = s.replace('Current appearance mapping/draft/preview functions and\nsemantic roles remain byte-identical to the original 39-control Linux acceptance.', 'The original 39-control Linux appearance acceptance retains its source/artifact\nscope; current numeric naming and actions have additional native checks above.')
s = s.replace('scoped above. Nondefault Windows DPI, physical graphics and full-window visual\nlayout remain separate acceptance gaps.', 'scoped above. Current Dark Windows144DPI complete-window captures now pass in\nboth software renderers; other scales/themes, physical graphics and whole-interface\ncoverage remain separate acceptance gaps.')
a = s.index('1. Deliver the current appearance numeric naming correction,')
b = s.index('2. Complete the concentrated', a)
s = s[:a] + '''1. Investigate and fix the unresolved strict GUI idle CPU failure, including
   unconditional frame refresh, using a controlled same-driver baseline/current
   comparison. Keep GUI CPU separate from index daemon CPU. Named numeric controls,
   actual Windows144DPI native regions, Restore/Cancel, full-window capture delivery
   and this exact seven-format publication are complete at the scopes above.
   Do not repeat fulfilled matrices without changed inputs or a reproduced defect.
   The archive producer now retains the same guest process through known transient
   observation failures and transfers a size/hash-verified archive in offset-addressed
   chunks; earlier partial/failed capture-delivery cases are explicitly retained.
   Continue broader appearance/control/platform acceptance; this is not a full
   Windows39-control or physical/human assistive acceptance. Watcher polling retains
   its deterministic lock/recovery/error/hit scope; current source CI is green.
   Evidence/prose-only pushes skip redundant CI; shipping-source changes retain gates.
''' + s[b:]
p.write_text(s)
p = r / 'README.md'
s = p.read_text()
a, b = s.index('The current seven local artifacts'), s.index('### Linux')
s = s[:a] + f'''The current seven local artifacts use clean runtime `{source[:8]}`.
All four exact-source CI jobs pass; 476 UI tests pass with one existing ignored.
The exact AppImage passes named numeric typing/Restore/Cancel checks in Dark100
and Light150. Installed Windows Native/OpenGL pass34 native window/DPI observations
and22 reviewed complete-window captures at actual144DPI. Normal Close and supported
uninstall preserve fixture source/preferences. Development MSIX0.1.0.13 passes
native assembly/unpack and58-file equality; no fresh MSIX update/login claim.
Prior GUI idle CPU failure, full-interface/platform and final daily-driver acceptance
remain open. See the roadmap and stable acceptance manifests for exact scopes.

''' + s[b:]
p.write_text(s)
p = r / 'docs/RELEASE_NOTES.md'
s = p.read_text()
a, b = s.index('The current Linux'), s.index('## Highlights')
s = s[:a] + f'''The current Linux AppImage/portable/DEB/RPM and Windows ZIP/NSIS/development
MSIX0.1.0.13 use runtime `{source}`. All seven artifacts/sidecars
and58 Windows payload files are independently rehashed at stable paths.
Exact-source CI41728865733 passes all four jobs. Source checks pass27 appearance
and476 UI tests (one existing ignored), native/Windows strict lint and formatting.

Native numeric metadata now describes the committed model value, replacing its
pre-edit snapshot. Appearance numeric fields retain visible names and property-specific
Restore actions. Native typing, Restore and Cancel pass on the exact Linux AppImage
in Dark100/Light150. Installed Windows Native/OpenGL pass34 complete native-region,
actual144DPI and work-area observations; all22 unobscured full-window captures were
reviewed. Live background owners have region observations, with separate full
captures for every window type. Normal close and supported NSIS uninstall preserve
source/preferences. Earlier pointer, preview, picker, folder, service and media
acceptance retains its original source/artifact scope.

MSIX13 passes production MakeAppx assembly/unpack and complete58-file equality;
deployment/update/login acceptance is historical. Prior strict GUI idle CPU failure,
required MSI rollback, full roadmap/platform coverage, signing/Store and final
owner acceptance remain open. No public release or personal service/settings
change is claimed. Owned fixtures, consumed archives/extractions, superseded
packages and obsolete compiler sessions are removed. Packaged manuals are build-time
snapshots; repository manifests/documents record later acceptance.

''' + s[b:]
p.write_text(s)
