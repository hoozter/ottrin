from pathlib import Path
import json,hashlib,zipfile
r=Path.cwd();e=Path(__file__).resolve().parent;p=e/'windows-fields-native-path';read=lambda f:json.loads(f.read_text())
d=read(p/'GUEST_RESULT.json');o=read(p/'OWNER.json');b=read(e/'windows/BUILD_RESULT.json')
assert d['status'].startswith('pass') and d['package']['source_revision']==b['source_revision'] and len(d['cases'])==8
assert d['package']['files']==[{'name':'current-setup.exe','sha256':b['artifacts']['Ottrin-0.1.0-windows-x64-setup.exe']}]
assert hashlib.sha256((e/'windows-fields.ps1').read_bytes()).hexdigest()==d['package']['actor_sha256']
for theme in ['Dark','Light']:
 rows=[x for x in d['cases'] if x['theme']==theme];assert {x['case'] for x in rows}=={'creation-File','creation-Folder','location','search'}
 for x in rows:
  if x['case'].startswith('creation-'):
   label=x['case'].split('-')[1];assert x['initial']=={'name':label+' name','role':'Edit','value':'','read_only':False};assert x['typed']=={'name':label+' name','role':'Edit','value':'draft-'+label.lower(),'read_only':False};assert x['cancel_preserved_source']
  else:
   f=x['field'];assert f['role']=='Edit' and not f['read_only'];assert f['name']==('Location' if x['case']=='location' else 'Search files and folders')
   if x['case']=='location':assert f['value']==x['expected_native_path'] and f['value'].startswith('\\\\?\\C:\\Users\\') and f['value'].endswith('\\Documents\\OttrinFields')
assert d['source_preserved'] and d['preferences_preserved'] and d['remaining_owned_installed_processes']==0
assert o['cleanup']['domain_removed'] and o['cleanup']['fixture_removed'] and len(o['cleanup']['owned_volumes_removed'])==2
assert read(p/'viewer/TEARDOWN.json')['profile_removed']
images={f.name:hashlib.sha256(f.read_bytes()).hexdigest() for f in (p/'results').glob('*.png')};assert set(images)=={'Dark-File.png','Dark-Folder.png','Light-File.png','Light-Folder.png'}
receipt={'status':'pass native installed main-field names/values/cancellation/source and removal-preserved preferences','source_revision':b['source_revision'],'artifacts':d['package']['files'],'cases':8,'native_result':str((p/'results/RESULT.json').relative_to(r)),'actor_sha256':d['package']['actor_sha256'],'frozen_executed_actor_sha256':hashlib.sha256((p/'actor.ps1').read_bytes()).hexdigest(),'native_capture_review':{'status':'four original native captures directly reviewed','sha256':images},'install_seconds':read(p/'results/install-process.json')['seconds'],'scope':'Owned ordinary Windows stock-client desktop, bundled software OpenGL/default DPI. Exact File name/Folder name, typed values/cancel, canonical Location and search native Edit roles in Dark/Light, original bytes and removal-preserved preferences.','limits':['Not native acceptance of every theme/document/numeric control or human assistive usability','Earlier unprefixed-path assertion failed; no failing Location value was captured. Current result records exact canonical prefix and value; product source unchanged between native attempts.','One retry cancelled before product actor when its intended fixture patch failed to apply; all owned resources removed.','MSIX9 assembly is separate; prior MSIX8 lifecycle keeps its exact original scope.'],'cleanup':o['cleanup']}
(e/'WINDOWS_NATIVE_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
archive=p/'RESULTS.zip'
manifest=read(p/'TRANSPORT_MEMBERS.json')
for name,digest in manifest.items():assert hashlib.sha256((p/'results'/name).read_bytes()).hexdigest()==digest
archive.unlink()
