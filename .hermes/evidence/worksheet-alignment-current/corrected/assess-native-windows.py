"""Bind the exact native worksheet result, reviewed captures and teardown."""
from pathlib import Path
import hashlib,json,subprocess,xml.etree.ElementTree as E,zipfile
r=Path.cwd();e=Path(__file__).resolve().parent;p=e/'windows-native'
def read(path):return json.loads(path.read_bytes().decode('utf-8-sig'))
def sha(path):
 with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
a=read(p/'GUEST_RESULT.json');raw=read(p/'results/RESULT.json');assert a==raw and a['status'].startswith('pass within installed native worksheet scope')
content=read(e/'HOST_PAYLOAD.json');assert a['package']['source_revision']==content['source_revision'];assert a['package']['files']==[{'name':'current-setup.exe','sha256':content['artifacts']['Ottrin-0.1.0-windows-x64-setup.exe']}]
assert a['payload_files']==content['payload_files']==58 and a['test_kind']=='native-worksheet-typography'
assert not a['identity']['administrator'] and a['identity']['session']>0 and a['desktop_ready']
for renderer in ['native','opengl']:
 for surface in ['main','inspector']:assert a['cases'][renderer][surface]['expected_native_cell_names']==['OTTRIN SHEET FIDELITY','Currency totals and calculated formulas']
assert a['source_preserved'] and a['uninstall']=='pass' and a['preferences_preserved_by_removal'] and a['office_temp_removed'] and a['owned_processes_removed']
review=read(p/'VISUAL_REVIEW.json');assert len(review['captures'])==4 and review['status'].startswith('reviewed all four')
for name,value in review['captures'].items():assert sha(p/'results'/name)==value['sha256']
owner=read(p/'OWNER.json');cleanup=owner['cleanup'];assert cleanup['domain_removed'] and cleanup['fixture_removed'] and cleanup['base_still_off'] and set(cleanup['owned_volumes_removed'])=={'ottrin-nsis-client.qcow2','ottrin-nsis-client-fixture.iso'}
assert owner['domain'] not in subprocess.check_output(['virsh','-c','qemu:///system','list','--all','--name'],text=True).splitlines()
provider=read(p/'PROVIDER_BASE.json');xml=E.fromstring(subprocess.check_output(['virsh','-c','qemu:///system','dumpxml',provider['domain']],text=True));assert hashlib.sha256(E.tostring(xml)).hexdigest()==provider['xml_sha256'];assert subprocess.check_output(['virsh','-c','qemu:///system','domstate',provider['domain']],text=True).strip()=='shut off'
archive=read(p/'RESULT_MEMBERS.json');assert sha(p/'RESULTS.zip')==archive['archive_sha256']
for name,digest in archive['members'].items():assert sha(p/'results'/name)==digest
assert (p/'RESULTS.zip').stat().st_nlink==1;assert subprocess.run(['fuser',str(p/'RESULTS.zip')],capture_output=True).returncode==1;(p/'RESULTS.zip').unlink()
for name in ['boot-observed.png','login-observed.png']:(p/name).unlink()
(e/'WINDOWS_NATIVE_ACCEPTANCE.json').write_text(json.dumps({'status':'pass exact installed native worksheet scope and owned cleanup','source_revision':content['source_revision'],'artifacts':content['artifacts'],'payload_files':58,'native_result_sha256':sha(p/'results/RESULT.json'),'native_fixture_identity':a['identity'],'review':str((p/'VISUAL_REVIEW.json').relative_to(r)),'scopes':['Ordinary-user exact staged NSIS install with58 matching payload files','Main Miller and standalone Inspector native worksheet; Native preference and OpenGL override; four directly reviewed captures','Source bytes preserved; normal window exit, supported uninstall preserving preferences, no installed processes','Private clone volumes/display/viewer/bus/profile removed; protected provider base off with unchanged XML'],'limits':['Software VM/default DPI/Dark theme; not physical adapter or all-theme acceptance','Native UIAutomation names establish content readiness; direct screenshots establish visible typography','Both graphics module families loaded; DLL presence does not prove exclusive API/device selection','Actor test_kind is native-worksheet-typography. Generic driver ui_case document and Office provider-base metadata do not claim converted Office/PDF fidelity in this run','No new upgrade/refusal/resource benchmark claim; unchanged inputs compared separately'],'cleanup':{'native_fixture':cleanup,'duplicate_results_archive_removed_after_byte_verification':True,'temporary_boot_transition_captures_removed':True}},indent=2)+'\n')
