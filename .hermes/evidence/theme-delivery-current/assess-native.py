"""Bind completed native Windows UI cases to exact NSIS bytes and cleanup."""
from pathlib import Path
import hashlib,json,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;d=e/'native-sendinput'
read=lambda p:json.loads(p.read_text())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
v=read(d/'GUEST_RESULT.json');i=read(d/'APPLICATION_INPUT.json');o=read(d/'OWNER.json');host=read(e/'HOST_PAYLOAD.json')
assert v['status']=='pass native settings rejection/save-retry and Search status workflow' and v['source_preserved'] and v['preferences_preserved'] and v['remaining_owned_installed_processes']==0
assert i['source_revision']==host['source_revision']=='45830098b6d58eeb8a08252f8d8b9c6fe35d9eef'
assert sha(d/'actor.ps1')==i['actor_sha256']
assert i['files']==[{'name':'current-setup.exe','sha256':host['artifacts']['Ottrin-0.1.0-windows-x64-setup.exe']}]
assert len(v['cases'])==9 and len(v['preview_controls'])==8
for renderer in ['Native','OpenGL']:
 cases=[x for x in v['cases'] if x.get('renderer')==renderer];actions=[x for x in v['preview_controls'] if x['renderer']==renderer]
 assert len(cases)==3 and len(actions)==4
 assert any(x['case']=='native failed theme save retains draft' and x['original_bytes_preserved'] and x['radius']==24 for x in cases)
 assert any(x['case']=='native repaired save retry persisted' and x['dark_radius']==24 and x['light_radius']==0 for x in cases)
 assert any(x['case']=='native Search separates daemon runtime and login' and x['dpi']==144 for x in cases)
 assert any(x['action']=='Observed pointer click selects Selected row' for x in actions)
assert all(x['original_bytes_preserved'] for x in v['cases'] if 'original_bytes_preserved' in x)
assert v['pointer_input_method']=='Native SendInput absolute movement,100ms settling, native button edges'
assert o['cleanup']['domain_removed'] and o['cleanup']['fixture_removed'] and len(o['cleanup']['owned_volumes_removed'])==2
assert not Path(o['private_console']['home']).parent.exists()
identity=subprocess.run(['virsh','-c','qemu:///system','domuuid',o['domain']],capture_output=True,text=True);assert identity.returncode or identity.stdout.strip()!=o['uuid']
reviewed=['Native-preview-pointer-row.png','OpenGL-preview-pointer-row.png','Native-preview-tabs.png','OpenGL-preview-titlebar.png','Native-save-refused.png','OpenGL-save-retried.png','Native-search-health.png','OpenGL-search-health.png']
result={'status':'pass bounded exact NSIS native Windows UI/lifecycle scope','source_revision':i['source_revision'],'artifact_sha256':i['files'][0]['sha256'],'payload_files':host['payload_files'],'case_count':9,'preview_action_count':8,'cases':v['cases'],'preview_controls':v['preview_controls'],'pointer_input_method':v['pointer_input_method'],'pointer_target':v['pointer_target'],'pointer_input_trace':v['pointer_input_trace'],'original_captures_directly_reviewed':{str((d/'results'/name).relative_to(r)):sha(d/'results'/name) for name in reviewed},'preservation':{k:v[k] for k in ['source_preserved','preferences_preserved','remaining_owned_installed_processes']},'cleanup':o['cleanup'],'diagnostic_resolution':'Earlier separate Cursor.Position/legacy button injection failed despite stable OS coordinates. Normal SendInput movement with settling and button edges passes identical row assertions in both renderers. No product code changed to satisfy pointer QA; precise distinction between motion API and settling timing is not claimed.','limits':['Dark custom test theme at144DPI, software Native/OpenGL desktop only; not every field/theme/scale or physical acceptance','MSIX16-to-17 upgrade and cohort promotion remain pending','Full theme audit including shared status controls, List/Grid preview and role ownership remains open']}
(e/'WINDOWS_NATIVE_ACCEPTANCE.json').write_text(json.dumps(result,indent=2)+'\n');print('Exact NSIS native UI acceptance:9 settings cases,8 preview actions,8 original captures reviewed')
