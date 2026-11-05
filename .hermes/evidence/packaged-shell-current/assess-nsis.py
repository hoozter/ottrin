"""Accept the current installed Services case only within its observed scope."""
from pathlib import Path
import hashlib,json,subprocess
R=Path.cwd();E=Path(__file__).resolve().parent;N=E/'native-nsis'
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
owner=read(N/'OWNER.json');cleanup=owner['cleanup']
assert cleanup['domain_removed'] and cleanup['fixture_removed'] and cleanup['base_still_off'] and len(cleanup['owned_volumes_removed'])==2
assert not Path(owner['private_console']['home']).parent.exists()
identity=subprocess.run(['virsh','-c','qemu:///system','domuuid',owner['domain']],capture_output=True,text=True);assert identity.returncode or identity.stdout.strip()!=owner['uuid']
v=read(N/'results/RESULT.json');content=read(E/'HOST_PAYLOAD.json');payload=read(E/'PAYLOAD.json');identity=read(N/'APPLICATION_INPUT.json')
assert v['status']=='pass current NSIS Services recovery and uninstall scope' and v['preferences_preserved'] and v['sources_preserved']
assert identity['source_revision']==content['source_revision']==v['package']['source_revision']
assert identity['actor_sha256']==sha(N/'actor.ps1')==sha(E/'nsis-services.ps1') and len(payload)==59
controls_sha=sha(N/'NATIVE_CONTROLS_SOURCE.ps1');assert controls_sha==read(N/'NATIVE_CONTROLS_RECOVERY.json')['sha256']
assert len(v['live_faults'])==len(v['transitions'])==len(v['services'])==2
assert {f['theme'] for f in v['live_faults']}=={'Dark','Light'}
for fault in v['live_faults']:
 assert fault['suspended_threads']>0 and fault['diagnostics'] and fault['diagnostic_bounds']
 assert fault['completed_action_history'].startswith('Last completed action: ')
 for name,rect in fault['ready_geometry'].items():
  assert len(rect)==4 and all(abs(a-b)<=1 for a,b in zip(rect,fault['error_geometry'][name]))
for t in v['transitions']:assert t['states']==['Ready','Stopped','Ready'] and t['startup_preserved']
assert v['installed_service_scope']['startup']=='On' and v['installed_service_scope']['ready']
review=read(N/'CAPTURE_REVIEW.json');assert review['reviewed'] and len(review['captures'])==6 and review['complete_window_visible'] and review['history_distinct_from_live_status']
for name,digest in review['captures'].items():assert sha(N/'results'/name)==digest
artifact_root=Path(read(E/'ASSEMBLY_ACCEPTANCE.json')['artifact']).parent
assert all(sha(artifact_root/name)==digest for name,digest in content['artifacts'].items())
receipt={'status':'pass current ordinary-user installed Services history, timeout/recovery and uninstall scope','source_revision':content['source_revision'],'artifacts':content['artifacts'],'payload_files':len(payload),'actor_sha256':identity['actor_sha256'],'native_controls_sha256':controls_sha,'capture_review':review,'live_faults':v['live_faults'],'transitions':v['transitions'],'installed_service_scope':v['installed_service_scope'],'preferences_preserved':True,'sources_preserved':True,'ordinary_folder_status':v['ordinary_folder_status'],'cleanup':cleanup,'limits':['Dark/Light targeted installed Services only; no complete theme/property or physical graphics acceptance','Exact same-process fault recovery and On startup retention; other scopes retain their own receipts','No administrator Copy, MSI rollback, signing, Store or whole roadmap acceptance']}
(E/'NATIVE_WINDOWS_UI_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(receipt['status'])
