"""Assess only current native settings/error/retry scopes after original review."""
from pathlib import Path
import json,hashlib,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;f=e/'native';read=lambda p:json.loads(p.read_text());sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
v=read(f/'GUEST_RESULT.json');owner=read(f/'OWNER.json');source=read(e/'BUILD_SOURCE.json')['source_revision'];host=read(e/'HOST_PAYLOAD.json')
assert v['status'].startswith('pass native settings rejection/save-retry and Search status workflow')
assert v['package']['source_revision']==source
assert v['package']['actor_sha256']==sha(f/'actor.ps1')==sha(e/'native-actor.ps1')
assert v['package']['files'][0]['sha256']==host['artifacts']['Ottrin-0.1.0-windows-x64-setup.exe']
assert v['source_preserved'] and v['preferences_preserved'] and v['remaining_owned_installed_processes']==0
assert v['dpi_setup']['actual_settings_dpi']==144
assert len(v['cases'])==9
for row in v['cases'][:3]:assert row['exit']==1 and row['original_bytes_preserved']
assert 'preserved' in v['cases'][2]['visible_error']
for renderer in ['Native','OpenGL']:
 rows=[c for c in v['cases'] if c.get('renderer')==renderer];assert len(rows)==3
 assert rows[0]['case']=='native failed theme save retains draft' and rows[0]['radius']==24 and rows[0]['original_bytes_preserved'] and 'read-only' in rows[0]['error']
 assert rows[1]['case']=='native repaired save retry persisted' and rows[1]['dark_radius']==24 and rows[1]['light_radius']==0
 assert rows[2]['case']=='native Search separates daemon runtime and login' and rows[2]['dpi']==144
review=read(e/'WINDOWS_ORIGINAL_REVIEW.json');assert review['directly_reviewed_originals']==7
expected={'invalid-settings-native.png'}|{renderer+'-'+suffix+'.png' for renderer in ['Native','OpenGL'] for suffix in ['save-refused','save-retried','search-health']}
assert set(review['captures'])==expected
for name,digest in review['captures'].items():assert sha(f/'results'/name)==digest
assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed'] and len(owner['cleanup']['owned_volumes_removed'])==2
assert subprocess.run(['virsh','-c','qemu:///system','domuuid',owner['domain']],capture_output=True).returncode!=0
assert not Path(owner['private_console']['home']).parent.exists()
receipt={'status':'pass scoped Windows native settings rejection/save-retry and Search status','source_revision':source,'installer_sha256':v['package']['files'][0]['sha256'],'cases':9,'full_window_captures':7,'captures':review['captures'],'cleanup':owner['cleanup'],'checks':['Exact58-file NSIS installation by ordinary user','CLI and daemon reject invalid config without replacing bytes','Native startup error dialog visible and dismissible; exit1 with preserved file','Both renderers retain editable24px draft and original bytes after read-only save refusal, persist24/0 after repaired retry','Actual144DPI Search distinguishes independent daemon from optional login startup','Ordinary close and supported uninstall preserve source/preferences'],'limits':['Private signed-VirtIO software VM, Dark144DPI; not all appearance controls or physical/human acceptance','No fresh GUI CPU/long-duration performance claim; earlier unchanged idle-observation acceptance keeps47a scope','MSIX runtime/update/login, Store/signing and whole-roadmap completion separate']}
(e/'WINDOWS_NATIVE_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n');print(receipt['status'])
