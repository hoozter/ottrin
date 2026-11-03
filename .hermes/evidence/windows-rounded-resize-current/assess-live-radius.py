"""Accept native-region propagation and Cancel, with capture visibility limits."""
from pathlib import Path
import hashlib,json,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;f=e/'native-live-radius'
read=lambda p:json.loads(p.read_text())
v=read(f/'GUEST_RESULT.json');o=read(f/'OWNER.json');source=read(e/'BUILD_SOURCE.json')['source_revision'];assert v['status'].startswith('pass') and len(v['cases'])==18 and all(c['native_expected_region_match'] for c in v['cases'])
assert v['source_preserved'] and v['preferences_preserved'] and v['remaining_owned_installed_processes']==0 and v['package']['source_revision']==source
assert o['cleanup']['domain_removed'] and o['cleanup']['fixture_removed'] and len(o['cleanup']['owned_volumes_removed'])==2
assert subprocess.run(['virsh','-c','qemu:///system','domuuid',o['domain']],capture_output=True).returncode!=0
captures={c['capture']:hashlib.sha256((f/'results'/c['capture']).read_bytes()).hexdigest() for c in v['cases']};assert len(captures)==18
receipt={'status':'pass exact native window-region live radius propagation and Cancel rollback','source_revision':source,'installer_sha256':v['package']['files'][0]['sha256'],'native_region_cases':18,'originals_directly_reviewed':18,'captures':captures,'input':'Actual Window corners numeric control focused through UIAutomation, followed by Ctrl+A, typed0/24 and Enter; no config mutation while running','behavior':'Main, Settings and editor actual GDI regions all match0/24; Cancel restores0 on existing windows and saved custom-theme radius stays0. Both Native and OpenGL; normal Close and supported uninstall preserve source/preferences.','limits':['Dark/default96DPI software VM; nondefault Windows DPI and physical GPU remain unverified','Some full-window captures include taskbar overlap or screen clipping; reviewed visible regions do not prove full-window visual/layout acceptance. Native GetWindowRgn equality independently checks the complete owned window shape.','UIAutomation focusing a numeric control is not a visible mouse-control or complete accessibility/usability acceptance','Strict software GUI idle CPU failure remains open; no new benchmark or waiver'],'cleanup':o['cleanup'],'result':str((f/'GUEST_RESULT.json').relative_to(r))}
(e/'WINDOWS_LIVE_RADIUS_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('Accepted18native-regionlive/Cancelchecks; explicitcapture/DPI/CPUlimits retained')
