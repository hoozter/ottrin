"""Accept the exact native installed timeout/layout case after original review."""
from pathlib import Path
import hashlib,json,subprocess,zipfile
R=Path.cwd();E=Path(__file__).resolve().parent;N=E/'native-windows'
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
owner=read(N/'OWNER.json');assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed'] and owner['cleanup']['base_still_off']
assert len(owner['cleanup']['owned_volumes_removed'])==2
archive=N/'RESULTS.zip';results=N/'results'
if archive.exists() and not results.exists():
 with zipfile.ZipFile(archive) as z:
  assert all(not Path(n).is_absolute() and '..' not in Path(n).parts for n in z.namelist())
  z.extractall(results)
r=read(results/'RESULT.json');content=read(E/'HOST_PAYLOAD.json');payload=read(E/'PAYLOAD.json');identity=read(N/'APPLICATION_INPUT.json')
assert r['status'].startswith('pass') and r['preferences_preserved'] and r['sources_preserved']
assert identity['source_revision']==content['source_revision']==r['package']['source_revision']
assert identity['actor_sha256']==sha(N/'actor.ps1')==sha(E/'actor.ps1')
assert len(r['cases'])==12 and len(r['density_proof'])==4 and len(r['thumbnail_cases'])==2
assert len(r['live_faults'])==len(r['transitions'])==2
for fault in r['live_faults']:
 assert fault['theme'] in ['Dark','Light'] and fault['suspended_threads']>0 and fault['diagnostics']
 for name,rect in fault['ready_geometry'].items():
  assert len(rect)==4 and all(abs(a-b)<=1 for a,b in zip(rect,fault['error_geometry'][name]))
for transition in r['transitions']:assert transition['states']==['Ready','Stopped','Ready'] and transition['startup_preserved']
review=read(E/'WINDOWS_CAPTURE_REVIEW.json');assert review['status']=='direct targeted original review complete' and len(review['captures'])==6
for name,digest in review['captures'].items():assert sha(results/name)==digest
assert all(sha(Path(read(E/'windows/BUILD_RESULT.json')['stage'])/name)==digest for name,digest in content['artifacts'].items())
receipt={'status':'pass scoped ordinary-user installed Windows timeout and recovery layout; whole interface audit remains open','source_revision':content['source_revision'],'artifacts':content['artifacts'],'actor_sha256':identity['actor_sha256'],'capture_review':review,'live_faults':r['live_faults'],'transitions':r['transitions'],'installed_service_scope':r['installed_service_scope'],'preferences_preserved':True,'sources_preserved':True,'cleanup':owner['cleanup'],'scope':'Actual exact NSIS install, temporary suspension of authenticated owned daemon threads, bounded timeout status, recovery geometry/disclosure, same-process resume and On startup preservation in both themes. Automated density/thumbnail cases pass; six targeted Services originals directly reviewed. No whole-theme, physical GPU, MSI or MSIX runtime claim.'}
(E/'NATIVE_WINDOWS_UI_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
if archive.exists():
 assert archive.stat().st_nlink==1 and subprocess.run(['fuser',str(archive)],capture_output=True).returncode==1
 receipt['consumed_transport']={'sha256':sha(archive),'bytes':archive.stat().st_size,'removed_after_extraction_and_guest_teardown':True};archive.unlink()
 (E/'NATIVE_WINDOWS_UI_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(receipt['status'])
