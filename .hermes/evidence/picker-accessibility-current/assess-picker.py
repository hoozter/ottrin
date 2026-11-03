"""Assess exact native installed picker results, retaining raw PowerShell shapes."""
from pathlib import Path
import hashlib,json
r=Path.cwd();e=Path(__file__).resolve().parent;p=e/'windows-picker-install-deadline'
d=json.loads((p/'GUEST_RESULT.json').read_text());o=json.loads((p/'OWNER.json').read_text());build=json.loads((e/'windows/BUILD_RESULT.json').read_text())
assert d['status'].startswith('pass') and d['package']['source_revision']==build['source_revision']
assert d['package']['files'][0]['sha256']==build['artifacts']['Ottrin-0.1.0-windows-x64-setup.exe']
assert len(d['cases'])==len(d['named_fields'])==14
assert all(c['normal_exit']==0 and c['no_index'] and c['main_preferences_unchanged'] for c in d['cases'])
assert d['source_bytes_preserved'] and d['preferences_preserved_by_removal'] and d['remaining_owned_installed_processes']==0
assert o['cleanup']['domain_removed'] and o['cleanup']['fixture_removed']
assert {f['name'] for f in d['native_location_fields']}=={'Location','Filter this folder','File name'}
for c in d['named_fields']:
 # PowerShell pipelines unwrap a one-item result for folder-only mode.
 fields=[c['fields']] if isinstance(c['fields'],dict) else c['fields']
 names=[f['name'] for f in fields]
 expected={'Filter this folder'} if c['case'].endswith('-folder') else {'Filter this folder','File name'}
 assert len(names)==len(expected) and set(names)==expected,(c['case'],fields)
images={f.name:hashlib.sha256(f.read_bytes()).hexdigest() for f in (p/'results').glob('*.png')};assert len(images)==4
receipt={'status':'pass actual installed native picker field-name and cooperating-request scope','source_revision':build['source_revision'],'artifacts':d['package']['files'],'native_result':str((p/'results/RESULT.json').relative_to(r)),'actor_sha256':d['package']['actor_sha256'],'cases':14,'named_edit_controls_in_all_cases':True,'location_fields':d['native_location_fields'],'native_capture_review':{'status':'four original native captures directly reviewed','sha256':images},'install_seconds':json.loads((p/'results/install-process.json').read_text())['seconds'],'scope':'Owned ordinary Windows stock-client desktop, explicit bundled software OpenGL, default DPI; source bytes/preferences/native caller ownership/cancellation/concurrency/overwrite/theme-role checks; no index process during picker cases','limits':['No universal Windows dialog replacement, physical GPU or human screen-reader acceptance','Original120-second installer attempt timed out and is retained as failed; successful retry uses existing canonical180-second installer deadline','Custom font/color/corners observed in native screenshot; no exhaustive39-setting theme-editor acceptance claimed'],'cleanup':o['cleanup']}
(e/'WINDOWS_NATIVE_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
# Exact extracted-member hashes were recorded before retiring this transport copy.
transport=p/'RESULTS.zip'
if transport.exists():
 members=json.loads((p/'TRANSPORT_MEMBERS.json').read_text())
 assert all(hashlib.sha256((p/'results'/name).read_bytes()).hexdigest()==sha for name,sha in members.items())
 transport.unlink()
