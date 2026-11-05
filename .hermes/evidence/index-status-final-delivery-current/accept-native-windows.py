"""Bind actual installed UI/lifecycle observations to current package hashes and direct capture review."""
from pathlib import Path
import hashlib,json
E=Path(__file__).resolve().parent;N=E/'native-windows';R=Path.cwd()
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
build=read(E/'windows/BUILD_RESULT.json');content=read(E/'HOST_PAYLOAD.json');result=read(N/'results/RESULT.json');owner=read(N/'OWNER.json');inputs=read(N/'APPLICATION_INPUT.json');review=read(N/'CAPTURE_REVIEW.json')
assert build['source_revision']==content['source_revision']==inputs['source_revision']
assert inputs['files']==[{'name':'current-setup.exe','sha256':build['artifacts']['Ottrin-0.1.0-windows-x64-setup.exe']}]
assert sha(N/'actor.ps1')==inputs['actor_sha256']
assert result['status'].startswith('pass') and result['preferences_preserved'] and result['sources_preserved']
assert len(result['cases'])==12 and len(result['density_proof'])==4 and len(result['thumbnail_cases'])==2 and len(result['services'])==2
assert len(result['transitions'])==2 and {v['theme'] for v in result['transitions']}=={'Dark','Light'}
for transition in result['transitions']:
 assert transition['states']==['Ready','Stopped','Ready'] and transition['startup_preserved']
for proof in result['density_proof']:
 a,b,c=proof['heights'];assert 0<a<b<c and proof['scales']==[0.75,1,1.85]
assert result['installed_service_scope']['startup']=='On' and result['installed_service_scope']['ready']
cleanup=owner['cleanup'];assert cleanup['domain_removed'] and cleanup['fixture_removed'] and cleanup['base_still_off'] and len(cleanup['owned_volumes_removed'])==2
assert not (N/'RESULTS.zip').exists() and read(N/'RESULT_TRANSFER_ACCEPTANCE.json')['transport_retired']
expected={v['capture'] for v in result['cases']+result['thumbnail_cases']+result['services']}
for v in result['transitions']:expected.update([v['stopped_capture'],v['recovered_capture']])
assert len(expected)==20 and set(review['captures'])==expected and review['status']=='direct original review complete'
for name,digest in review['captures'].items():assert sha(N/'results'/name)==digest
receipt={'status':'pass scoped ordinary-user installed Windows UI, status stop/restart, main-close continuity and uninstall preservation','source_revision':build['source_revision'],'artifacts':build['artifacts'],'capture_review':review,'cleanup':cleanup,'actual_observations':result,'input_receipt':inputs,'limits':['Native software guest/default DPI only; physical graphics and full theme audit remain open','Actual installed Ready/Stopped/Ready UI transitions; live-endpoint error separately exercised on Linux only','MSIX assembly, deployment/update/login, helper transport and administrator Copy have separate evidence and gates']}
(E/'NATIVE_WINDOWS_UI_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(receipt['status'])
