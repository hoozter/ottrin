"""Bound acceptance to the exercised AppImage Services controls and original captures."""
from pathlib import Path
import hashlib,json,subprocess
E=Path(__file__).resolve().parent;N=E/'native';F=N/'ubuntu'
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as s:return hashlib.file_digest(s,'sha256').hexdigest()
def save(p,v):p.write_text(json.dumps(v,indent=2)+'\n')
r=read(F/'results/RESULT.json');w=r['worker'];b=read(E/'BUILD_RESULT.json');c=read(E/'PACKAGE_CONTENT_ACCEPTANCE.json');o=read(F/'VM_OWNER.json');vm=read(F/'VM_RESULT.json');clean=read(F/'VM_CLEANUP.json')
assert r['status']==w['status']==c['status']=='pass'
assert r['package_source']==b['source_revision']==c['source_revision']==vm['source']
assert sha(N/'actor.py')==o['actor_sha256']
assert sha(Path(o['cleanup_owner']))==o['controller_sha256']
assert clean['domain_removed'] and set(clean['volumes_removed'])==set(o['volumes'])
assert vm['temporary_seed_and_ssh_keys_removed']
assert subprocess.run(['virsh','-c','qemu:///system','domuuid',o['domain']],capture_output=True).returncode!=0
for volume in o['volumes']:
 assert subprocess.run(['virsh','-c','qemu:///system','vol-path','--pool','default',volume],capture_output=True).returncode!=0
assert r['appimage_input']['sha256']==b['artifacts'][r['appimage_input']['name']]
assert w['daemon_sha256']==c['runtime_sha256']['index_daemon']
assert all(w[k] for k in ['database_identity_preserved','saved_roots_preserved','sources_preserved','daemon_running_after_normal_gui_close'])
assert {(v['theme'],v['backend_scale']) for v in w['cases']}=={(t,s) for t in ['Dark','Light'] for s in [1.0,1.5]}
captures={}
for case in w['cases']:
 assert case['gui_sha256']==c['runtime_sha256']['application']
 assert case['registration_round_trip']=='On -> Off -> On through real checkbox and systemctl inspection'
 assert case['running_pid_preserved']=='4691' and case['normal_settings_and_main_close']
 for name in case['captures']:captures[name]=sha(F/'results'/name)
assert len(captures)==12
review={'status':'all twelve original PNG captures directly reviewed','captures':captures,'observed':['Ready index status remains separate from On/Off/Unavailable automatic startup in both themes at100/150 percent backend scaling','On checkbox checked, Off unchecked, unavailable query hides editable checkbox and presents actual exit code1','Installed service actions read Update/reinstall, Start/restart, Stop indexing and Uninstall; no Install action for installed service'],'open_findings':['Light theme green Ready text has weak contrast; full theme audit remains open'],'limits':['Controlled is-enabled failure is simulated; installed service and checkbox operations are real','No full UI, physical desktop or personal-profile acceptance']}
save(N/'CAPTURE_REVIEW.json',review)
receipt={'status':'pass within actual AppImage Services startup-control and controlled-query-error scope','source_revision':b['source_revision'],'appimage_input':r['appimage_input'],'worker':w,'captures':captures,'cleanup':{'retired_uuid':vm['uuid'],'domain_removed':True,'volumes_removed':clean['volumes_removed'],'seed_and_keys_removed':True},'limits':review['limits']+review['open_findings']}
archive=F/'VM_RESULTS.tar.gz';transport=read(F/'TRANSPORT_CONTENT.json')
if archive.exists():
 assert archive.stat().st_size==transport['archive_bytes'] and sha(archive)==transport['archive_sha256']
 assert not archive.is_symlink() and archive.stat().st_nlink==1
 assert subprocess.run(['fuser',str(archive)],capture_output=True).returncode==1
 archive.unlink();save(F/'TRANSPORT_RETIRED.json',{'removed':str(archive),'bytes':transport['archive_bytes'],'sha256':transport['archive_sha256'],'purpose':'Consumed transport; original results and reviewed captures retained'})
receipt['consumed_transport_removed']=not archive.exists()
save(E/'NATIVE_SERVICES_ACCEPTANCE.json',receipt)
print('Passed bounded Services assessment; twelve originals reviewed and consumed archive retired')
