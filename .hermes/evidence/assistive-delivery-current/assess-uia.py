"""Bind native whole-cohort UIA and idle observations to preserved output bytes."""
from pathlib import Path
import hashlib,json
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
payload=read(e/'PAYLOAD.json');host=read(e/'HOST_PAYLOAD.json');reports={}
for renderer in ['native','opengl']:
 case=e/'windows'/('cohort-'+renderer);a=read(case/'GUEST_RESULT.json');assert a==json.loads((case/'results/RESULT.json').read_bytes().decode('utf-8-sig'))
 assert a['status']=='pass native Windows UIAutomation observations' and a['renderer']==renderer
 assert a['input']['source_revision']==host['source_revision'] and a['input']['archive_sha256']==host['artifacts']['Ottrin-0.1.0-windows-x64-portable.zip']
 assert a['input']['application_sha256']==payload['bin/ottrin.exe'] and a['input']['indexd_sha256']==payload['bin/ottrin-indexd.exe']
 for key in ['desktop_ready','source_preserved','settings_foreground','inspector_foreground','settings_native_close','inspector_native_close','picker_preferences_preserved','forward_disabled','parent_enabled']:assert a[key] is True,key
 for key in ['service_cleanup_exit','picker_native_close_exit','main_native_close_exit']:assert a[key]==0,key
 assert a['inspector_glyph_buttons']==[] and a['inspector_value_controls']==[] and not a['picker_response']['accepted']
 assert not a['identity']['administrator'] and a['identity']['session']>0
 idle=a['index_idle'];assert idle['indexed_items']>=10101 and idle['watcher_ready'] and idle['sha256']==payload['bin/ottrin-indexd.exe'] and idle['seconds']>=30 and idle['cpu_percent_one_core']<.25
 for name,meta in read(case/'RESULT_MEMBERS.json').items():
  raw=(case/'results'/name).read_bytes();assert len(raw)==meta['bytes'] and hashlib.sha256(raw).hexdigest()==meta['sha256']
 cleanup=read(case/'OWNER.json')['cleanup'];assert cleanup['domain_removed'] and cleanup['fixture_removed'] and cleanup['base_still_off'] and len(cleanup['owned_volumes_removed'])==2
 viewer=read(case/'viewer/TEARDOWN.json');assert viewer['profile_removed'] and not Path(viewer['profile']).exists()
 reports[renderer]={'input':a['input'],'result_sha256':hashlib.sha256((case/'results/RESULT.json').read_bytes()).hexdigest(),'idle':idle,'cleanup':cleanup}
report={'status':'pass within current complete portable cohort ordinary-user UIAutomation and private populated index scope','source_revision':host['source_revision'],'renderers':reports,'limits':'Native WGPU and bundled OpenGL in software VM at default DPI; no physical GPU, screen-reader usability or extended scaling claim. CPU observed with main GUI open, not pure unattended post-login workload.'}
(e/'WINDOWS_UIA_ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n');print(report['status'])
