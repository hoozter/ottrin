"""One authorized service restart; preserve configuration and verify exact bytes."""
from pathlib import Path
import argparse,hashlib,json,subprocess,time,os
root=Path.cwd();parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True);args=parser.parse_args();p=args.output.resolve();assert p.is_relative_to(root/'.hermes/evidence');unit='ottrin-index.service'
def sha(path):
 with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def props():
 out=subprocess.check_output(['systemctl','--user','show',unit,'--property=MainPID','--property=ActiveState','--property=SubState'],text=True)
 return dict(line.split('=',1) for line in out.splitlines())
paths={'config':Path.home()/'.config/ottrin/config.json','unit':Path.home()/'.config/systemd/user/ottrin-index.service','wrapper':Path.home()/'.cache/ottrin/ottrin-index-appimage'}
assert subprocess.check_output(['systemctl','--user','is-enabled',unit],text=True).strip()=='enabled','Preserve changed service startup preference'
database=Path.home()/'.cache/ottrin/search.db';database_before=database.stat();before=props();old=int(before['MainPID']);assert old>0 and before['ActiveState']=='active';oldexe=Path(os.readlink(f'/proc/{old}/exe'));oldmount=oldexe.parents[2]
assert str(oldmount).startswith('/tmp/.mount_Ottrin')
expected=json.loads((root/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json').read_text())['indexd_binary_sha256']
previous=json.loads((p/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').read_text());assert sha(f'/proc/{old}/exe')==previous['indexd_binary_sha256']
r={'status':'restart request prepared','before':before,'old_exe':str(oldexe),'old_mount':str(oldmount),'before_sha256':{k:sha(v) for k,v in paths.items()},'expected_daemon_sha256':expected,'reason':'Activate the newly accepted package runtime while preserving service configuration and index data. Separate scoped packaged and personal resource receipts define resource claims.'}
def save(): (p/'PERSONAL_SERVICE_DELIVERY.json').write_text(json.dumps(r,indent=2)+'\n')
save()
try:
 subprocess.run(['systemctl','--user','restart',unit],check=True,timeout=30)
 deadline=time.monotonic()+90
 while time.monotonic()<deadline:
  after=props();pid=int(after['MainPID'])
  if pid and pid!=old and after['ActiveState']=='active':
   assert sha(f'/proc/{pid}/exe')==expected
   result=subprocess.run([str(root/'target/release/ottrin-app'),'index','status','--json'],capture_output=True,text=True,timeout=10)
   if result.returncode==0:
    state=json.loads(result.stdout)
    r.update(after=after,index_status=state,old_mount_removed=not oldmount.exists());save()
    if state.get('status')=='ready' and state.get('watcher_ready') and not state.get('error') and not state.get('watcher_error') and not oldmount.exists():break
  time.sleep(.5)
 else:raise RuntimeError('Service did not reach watched ready state or retire old mount within90s')
 assert (database.stat().st_dev,database.stat().st_ino)==(database_before.st_dev,database_before.st_ino);r['database_inode_preserved']=True
 r['after_sha256']={k:sha(v) for k,v in paths.items()};assert r['after_sha256']==r['before_sha256']
 r['enabled']=subprocess.check_output(['systemctl','--user','is-enabled',unit],text=True).strip();assert r['enabled']=='enabled'
 runtime_gui=Path(os.readlink(f'/proc/{pid}/exe')).with_name('ottrin');assert sha(runtime_gui)==sha(root/'target/release/ottrin-app');r['runtime_gui_sha256']=sha(runtime_gui);r['runtime_gui_matches_current_release']=True
 r['source_revision']=json.loads((root/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json').read_text())['source_revision']
 r['status']='pass';r['daemon_sha256']=sha(f'/proc/{pid}/exe');r['configuration_unit_wrapper_unchanged']=True;r['personal_user_files_not_modified_by_agent']=True
 save();print(json.dumps({'status':'pass','pid':pid,'indexed_items':state.get('indexed_items'),'watcher_ready':state.get('watcher_ready'),'old_mount_removed':r['old_mount_removed'],'configuration_preserved':True},indent=2))
except BaseException as error:
 r['status']='failed or incomplete;inspect exact service state before further mutation';r['error']=repr(error);save();raise
