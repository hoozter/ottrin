"""Publish the exact accepted four-format cohort; keep prior artifacts until every scoped gate passes."""
from pathlib import Path
import hashlib,json,subprocess,shutil
r=Path.cwd();e=Path(__file__).resolve().parent;public=r/'dist/packages'
def read(p):return json.loads(p.read_text())
def sha(p):
 with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
build=read(e/'BUILD_SOURCE.json');source=build['source_revision'];stage=Path(build['stage']);content=read(e/'PACKAGE_CONTENT_ACCEPTANCE.json');extract=Path(content['extraction'])
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==source
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],text=True).strip()
assert content['status']=='pass' and content['source_revision']==source
# Finish clean-source Windows assembly/native consumers before mutating tracked delivery receipts.
assert read(e.parent/'windows/NATIVE_OWNER.json')['status']=='pass'
assert read(e.parent/'windows/gui/RUNTIME_OWNER.json')['status']=='pass'
assert read(e.parent/'windows/HOST_PAYLOAD.json')['source_revision']==source
hosted=read(e.parent/'HOSTED_SOURCE.json');assert hosted['headSha']==source
for name in ['verify (ubuntu-latest)','search-performance']:
 assert next(j for j in hosted['jobs'] if j['name']==name)['conclusion']=='success'
artifact=content['artifacts']['Ottrin-0.1.0-x86_64.AppImage'];checks={}
for name in ['main-dark100','main-light150','properties-dark100','properties-light150','resources','icon','picker']:
 a=read(e/name/'RESULT.json');assert a.get('artifact_sha256',a.get('app_sha256'))==artifact
 assert a['teardown']['profile_removed'] and not Path(a['teardown']['profile']).exists()
 if name.startswith('main-'):
  assert a['normal_exit']==0 and a['initial_files']==a['final_files'] and a['index_survived_window_close']
  assert a['persisted_config']['default_view_mode']=='List'
 elif name.startswith('properties-'):
  assert a['normal_exit']==0 and a['native_close_button_clicked'] and a['source_and_config_preserved'] and a['no_index_started']
 else:assert a['status']=='pass'
 checks[name]={'receipt':name+'/RESULT.json','owned_profile_removed':True}
folder=read(e/'GUI_RESULT.json');cleanup=read(e/'LINUX_CLEANUP_RESULT.json')
for a in [folder,cleanup]:assert a['status']=='pass' and a['binary_sha256']==artifact and a['teardown']['profile_removed']
assert folder['later_batch_child_reaped_while_first_open'] and folder['active_window_survived_backend_restart']
assert cleanup['blocked_cache_retained_registration'] and cleanup['modified_activation_retained'] and cleanup['preferences_and_source_preserved']
populated=read(e/'POPULATED_INDEX.json');assert populated['status']=='pass' and populated['provenance']['sha256']==artifact and populated['cleanup']['daemon_exited'] and populated['cleanup']['private_profile_removed']
assert read(e/'VISUAL_REVIEW.json')['status']=='reviewed'
indexd=sha(extract/'squashfs-root/usr/bin/ottrin-indexd');html=sha(extract/'squashfs-root/usr/libexec/ottrin/ottrin-html-render')
for name,digest in read(e/'DELIVERABLE_BASELINE.json').items():assert sha(r/name)==digest
for name,digest in content['artifacts'].items():assert sha(stage/name)==digest
files=[f for root in [stage,extract] for f in root.rglob('*') if f.is_file() and not f.is_symlink()];assert all(f.stat().st_nlink==1 for f in files);assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
previous=read(public/'LINUX_PACKAGES_ACCEPTANCE.json');(e/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(previous,indent=2)+'\n')
receipt={'status':'pass within exact Linux content and private native GUI/integration/resource scopes','source_revision':source,'artifacts':content['artifacts'],'evidence':str(e.relative_to(r)),'indexd_binary_sha256':indexd,'html_binary_sha256':html,'checks':content['checks'],'gui_and_resource_sessions':checks,'integration':['D-Bus activation/signatures,native paths,reveal,Properties,startup token,independent child reaping and active-window continuity','Blocked-cache/modified-registration uninstall refusal and successful ownership-aware removal;original defaults/config/source preserved'],'resource':'Exact10000-file content index:live/offline changes,restart,queries and three30-second quiet observations;see POPULATED_INDEX.json. Chooser repeated/concurrent cancel and quiet sample in resources/RESULT.json. These are scoped samples,not a sustained personal-workload guarantee.','hosted_source':{'run':hosted['databaseId'],'linux':'success','performance':'success','windows_state_at_linux_promotion':next(j for j in hosted['jobs'] if j['name']=='verify (windows-latest)')['status']},'scope':'Local Linux four-format contents and actual AppImage X11/software-rendered acceptance. Fresh-distribution lifecycle,physical seats/devices,external production app picker use,final owner/release acceptance and Windows installer rollback remain separate. Earlier receipts retain their original artifact hashes.','cleanup':'All private profiles/processes/mounts removed. Completed staging/extraction retired at promotion; current compiler outputs and compact receipts retained.'}
for name in content['artifacts']:
 subprocess.run(['bash','-c','set -euo pipefail;source "$1";ottrin_publish_artifact "$2" "$3"','publish',str(r/'packaging/package-lifecycle.sh'),str(stage/name),str(public/name)],check=True)
 for suffix in ['.sha256','.provenance.json']:
  side=stage/(name+suffix)
  if side.exists():shutil.move(side,public/side.name)
 assert sha(public/name)==content['artifacts'][name]
(public/'LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n');(e/'ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
assert not any(stage.iterdir());stage.rmdir();shutil.rmtree(extract)
build.update(pending_consumers=[],cleanup_complete=True,delivery_status='Promoted accepted local Linux cohort;staging/extraction removed');(e/'BUILD_SOURCE.json').write_text(json.dumps(build,indent=2)+'\n')
print('Published four verified Linux formats; owned staging/extraction removed.')
