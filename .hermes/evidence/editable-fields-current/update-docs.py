from pathlib import Path
import json
r=Path.cwd();e=Path(__file__).resolve().parent;read=lambda p:json.loads(p.read_text())
l=read(r/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json');w=read(r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json');source=read(e/'BUILD_SOURCE.json')['source_revision'];assert l['source_revision']==w['source_commit']==source;assert read(e/'CI_CURRENT.json')['conclusion']=='success';assert read(e/'WINDOWS_DELIVERY.json')['staging_removed'];assert not Path(read(e/'windows/BUILD_SOURCE.json')['stage']).exists()
roadmap=f'''# Ottrin roadmap

## Current delivery and next work

Updated 2026-11-03. This is the single active queue. Historical receipts retain
original source/artifact scope; passing checks do not complete the full roadmap.
Packaged documentation is a build-time delivery snapshot. Generated
`SOURCE_REVISION.txt` identifies the actual source inside each package; these
repository documents and the stable acceptance manifests record later acceptance.

All seven current local packages use clean runtime source `{source}`.
Linux AppImage, portable archive, DEB and RPM are at `dist/packages/`; Windows
ZIP/NSIS/development MSIX 0.1.0.9 are at `dist/windows-candidate/`. The AppImage's
stable path is `dist/packages/Ottrin-0.1.0-x86_64.AppImage`, SHA256
`{l['artifacts']['Ottrin-0.1.0-x86_64.AppImage']}`. Exact scopes/hashes are in
`dist/packages/LINUX_PACKAGES_ACCEPTANCE.json` and
`dist/WINDOWS_CANDIDATE_ACCEPTANCE.json`. All seven artifacts/sidecars are rehashed
after publication. CI41639409624 passes all four exact-source Linux/Windows jobs,
including workspace tests/check/strict lint. No public release or Store submission
is implied; the failed ordinary-user MSI remains held on its exact diagnostic input.

Existing production text controls now have contextual native names: File name,
Folder name, New name, Location, search, command, include/exclude patterns, theme
name/per-color hex and document/source/read-only preview text. A single shared
native-label helper preserves each control's role, value and selection; existing
raw/themed visuals and edit-mode gates are unchanged. Four new rendered Dark/Light
tests check actual composed dialogs, keyboard editing and values. All 469 UI tests
pass (one existing ignored), plus local strict Linux/Windows UI/app lint and format.
This does not accept every numeric/color-picker control or human assistive usability.

The exact new AppImage passes 14 native AT-SPI checks across Dark at 100% and Light at 150%:
New File/Folder with empty outputs, cancel, occupied-name refusal, rename editing/
cancel, Location and search names/values, original bytes preserved and normal native
Close. All eight originals are directly reviewed. The installed current NSIS passes
eight Dark/Light UIAutomation checks for named creation fields, typed values/cancel,
canonical Windows Location and search; source bytes and preferences survive supported
uninstall. All four originals are directly reviewed. These are private X11/software
Linux and ordinary-user software OpenGL/default-DPI Windows scopes.

Development MSIX revision 9 passes actual native production MakeAppx assembly/unpack and
complete 58-file equality with this ZIP/NSIS payload. MSIX8 update/login/removal
acceptance remains historical with its exact original source/artifact hashes and
recorded fixture login recovery. No fresh8-to9 deployment/login is claimed. No
installation/startup/daemon source changes in this cohort; the Linux daemon and
non-GUI payloads match, while the complete Windows daemon differs only in PE
 timestamp/checksum metadata. Earlier picker, full workflow, installer-fault,
Office/media, theme and resource scopes retain original hashes, with explicit
source/payload comparison rather than relabelled as fresh runs. The personal Linux
service/data remain intact; no personal restart or configuration mutation was needed.

Failed first test-fixture and native-role assumptions are preserved. An immediate
Linux post-type assertion was replaced with a bounded native-provider observation;
the same artifact then passed. The first Windows fixture expected an unprefixed
Location and did not save the failing value; the accepted run records/tests the exact
canonical prefix. An unintended retry was cancelled before its product actor and
cleaned up. The canonical custom-actor driver now describes the actor's declared
scope instead of claiming an unrelated preview-safety scenario. Independent stock
`xmessage` also fails Alt+F4 in the private KWin fixture, where
`org.kde.kglobalaccel` is absent: an Ottrin-specific shortcut defect is not established.
Native Close remains verified; Alt+F4 is a separate fixture/acceptance question.

Evidence: `.hermes/evidence/editable-fields-current/`. Four owned Windows cases,
eight volumes, seven private desktops/profiles/buses and all mounts are removed.
Consumed source/transport inputs, extractions, superseded packages and stages are
retired after consumers. Fifteen obsolete incremental sessions were removed through
the guarded producer, preserving newest sessions, current binaries and useful caches.
Exact baseline comparisons/remaining retained paths are in `CLEANUP_FINAL.json`.
Current deliverables and compact original evidence remain; user data, protected
Windows bases and the held MSI are preserved.

Continue in this order:

1. Complete the concentrated requirement-by-requirement Linux/Windows audit.
   `windows-preview-safety-current/AUDIT_REQUIREMENTS.json` retains 54 complete
   roadmap bullets; `release-audit-current/USER_REQUIREMENTS.json` supplements them
   with 15 UI/service/delivery requirements. They overlap and are not a completion
   percentage. `editable-fields-current/RELEASE_GATE_AUDIT.json` reviews seven
   release-gate clauses only. Inspect every clause's direct evidence and unchanged
   responsibility, including all 39 appearance mappings, Services state/actions,
   numeric/color-picker controls, remaining media and storage/cross-volume workflows.
   Keep the observed rename/creation field-height difference for the visual consistency
   audit; do not infer whole-interface acceptance from field-name tests.
2. Close independently executable picker, Linux FileChooser/FileManager1 and
   reversible Windows folder-integration software/caller gaps. Retain supported
   caller/platform boundaries; Windows cooperating-picker checks do not establish
   universal dialog replacement. Signing/Store/hardware/owner review do not stall
   dependency-ready work. Do not repeat fulfilled fault matrices without changed
   inputs or a reproduced defect. Resolve the private Alt+F4 fixture question
   separately from normal native Close behavior.
3. Reconcile the final release matrix and remaining documents, leaving only actual
   [owner/external waits](#ownerexternal-waiting-list). Signing identity/key, Store
   account/submission, unavailable physical hardware, personal administrator setup
   and final owner acceptance remain explicit waits. No macOS build is added.

The ordinary-user MSI registration rollback gate remains failed. Independent
file-only authoring reproduces it; elevated diagnostics do not authorize production
elevation, ACL/policy changes or manual-registry compensation. Keep the original
failure/input and strict terminal assessment. This held gate does not stall other
software work or count as a successful MSI release.

'''.replace('\n timestamp','\ntimestamp')
p=r/'docs/ROADMAP.md';old=p.read_text();p.write_text(roadmap+old[old.index('## Picker Shift selection'):])
p=r/'docs/RELEASE_NOTES.md';s=p.read_text();a=s.index('## Current local candidate');b=s.index('## Highlights',a);s=s[:a]+f'''## Current local candidate

The locally delivered Linux AppImage/portable/DEB/RPM and Windows ZIP/NSIS/
development MSIX 0.1.0.9 use runtime source `{source}`.
Their exact hashes/scopes are in `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json`
and `dist/WINDOWS_CANDIDATE_ACCEPTANCE.json`. All four exact-source hosted jobs
pass, including Linux/Windows workspace tests and strict lint.

Existing text controls now expose contextual native names while preserving roles,
values, selection and editing behavior.469 UI tests pass (one existing ignored).
The new AppImage passes 14 native Dark at 100%/Light at 150% creation/rename/location/search
checks; installed NSIS passes eight Dark/Light creation/cancellation/location/search
checks. Source preservation, normal close, removal-preserved preferences and all 12
original native captures are verified within those stated scopes.

Development MSIX revision 9 passes native MakeAppx assembly/unpack and58-file equality.
Earlier MSIX8 update/login/removal and broader workflow, picker, Office/media,
resource and installer-fault results retain their exact original hashes; the
roadmap records the unchanged source/payload basis. No fresh MSIX9 lifecycle,
physical graphics or complete UI/assistive acceptance is implied. MSI remains held
for ordinary-user registration rollback. No public release or Store submission.

Packaged documents are a build-time delivery snapshot; `SOURCE_REVISION.txt`
identifies actual package source. Repository documents and stable acceptance
manifests record subsequent delivery and acceptance. All task-owned QA VMs/media,
profiles, stages and superseded output are removed; current packages and compact
evidence are retained.

'''+s[b:];p.write_text(s)
p=r/'README.md';s=p.read_text();a=s.index('When a verified release');b=s.index('### Linux',a);s=s[:a]+'''Public releases will be available from [GitHub Releases](https://github.com/hoozter/ottrin/releases).
The current 0.1.0 is a locally tested development candidate; no public release is
claimed. The latest usable local packages and exact source/hashes/tested scopes are
recorded in `dist/packages/LINUX_PACKAGES_ACCEPTANCE.json` and
`dist/WINDOWS_CANDIDATE_ACCEPTANCE.json`. See the [roadmap](docs/ROADMAP.md)
for current delivery and the [release checklist](docs/RELEASE_CHECKLIST.md) for gates.

Local Linux formats are AppImage, portable tarball, DEB and RPM. Windows has portable
ZIP, a per-user NSIS installer and an unsigned development MSIX for local testing.
The MSI remains held: native failed-transaction registration rollback can lose
registration while application files remain. Its normal install/update/removal
receipts do not waive that failure. Store submission waits for the owner's later
account/certification work; a paid signing certificate is not required for this plan.
macOS construction and acceptance are deferred outside the Linux/Windows goal.

The stable Linux AppImage is `dist/packages/Ottrin-0.1.0-x86_64.AppImage`.
Windows candidates are at `dist/windows-candidate/`. Verified replacements use
these stable paths; superseded packages and owned temporary QA resources are
retired after consumers. The current Windows 58-file payload is retained with its
candidate for exact identity checks; no versioned package folders accumulate.
Generated `SOURCE_REVISION.txt` identifies package source. Packaged documentation
is a build-time snapshot; repository manifests/documents record later acceptance.

'''+s[b:];a=s.index('### macOS');b=s.index('\n---',a);s=s[:a]+'''### macOS

Deferred until the Linux/Windows roadmap and release acceptance are complete.
Existing packaging source does not imply a current accepted macOS build, and
macOS signing/launch checks do not block Linux/Windows work.
'''+s[b:];p.write_text(s)
p=r/'docs/USER_GUIDE.md';s=p.read_text();needle='Install the local `.deb` with `sudo apt install ./ottrin_0.1.0_amd64.deb`, or the local `.rpm` with `sudo dnf install ./ottrin-0.1.0-1.x86_64.rpm`.';assert needle in s;s=s.replace(needle,needle+'\n\nCurrent packages do not install an Ottrin APT/DNF repository. Uploading a new GitHub release alone does not update an installed copy; download and install the newer package manually. A signed update repository can be added later, but is not configured by these builds.',1);a=s.index('### macOS');b=s.index('## Basic Navigation',a);s=s[:a]+'''### macOS

Deferred until the Linux/Windows roadmap and release acceptance are complete.
No current macOS package is accepted, and its platform/signing checks are outside
the active Linux/Windows goal.

'''+s[b:];p.write_text(s)
