"""Publish the exact locally accepted Linux cohort, preserving historical scope."""
from pathlib import Path
import hashlib,json,subprocess,shutil
r=Path.cwd();e=Path(__file__).resolve().parent;public=r/'dist/packages'
def read(p):return json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
build=read(e/'BUILD_SOURCE.json');content=read(e/'PACKAGE_CONTENT_ACCEPTANCE.json');source=build['source_revision'];stage=Path(build['stage']);extract=Path(content['extraction'])
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==source
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],text=True).strip()
assert content['source_revision']==source and content['status']=='pass'
assert read(e.parent/'windows-picker-package-current/NATIVE_OWNER.json')['status']=='pass', 'Finish clean-source Windows assembly before changing the tracked Linux receipt'
proof=read(e.parent/'picker-name-current/SOURCE_ACCEPTANCE.json')
for name,digest in proof['source_sha256'].items():assert sha(r/name)==digest
hosted=read(e/'HOSTED_PROGRESS_LATER.json');assert hosted['headSha']==source
for name in ['verify (ubuntu-latest)','search-performance']:assert next(x for x in hosted['jobs'] if x['name']==name)['conclusion']=='success'
checks=[];sessions={}
for name in ['dark100','light150','main-dark100','main-light150']:
 a=read(e/name/'RESULT.json');assert a['artifact_sha256']==content['artifacts']['Ottrin-0.1.0-x86_64.AppImage'];assert a['teardown']['profile_removed'] and a['normal_exit']==0;assert a['initial_files']==a['final_files']
 assert all(x['exit'] is not None for x in a['teardown']['owned_processes_reaped'])
 if name.startswith('main-'):
  assert a['index_survived_window_close'];events=[json.loads(x) for x in (e/name/'actions.jsonl').read_text().splitlines()];assert any(x.get('file_checkpoint')=='readonly-files-preserved' for x in events)
  assert a['persisted_config']['default_view_mode']=='List'
 else:
  assert a['main_config_bytes_unchanged'] and a['exact_native_path_bytes_verified'] and a['actual_second_process_reopen_and_cancel'] and a['caller_location_and_explicit_name_edit_verified'] and not a['index_socket_created']
  assert all(a['picker_preferences'][k] for k in ['grid','hidden','preview'])
 sessions[name]={'receipt':name+'/RESULT.json','normal_exit':0,'profile_removed':True,'fixture_files':len(a['initial_files'])}
resource=read(e/'resources/RESULT.json');assert resource['status']=='pass' and resource['teardown']['profile_removed'] and resource['index_not_started'] and resource['fixture_bytes_unchanged'];assert resource['artifact_sha256']==content['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
icon=read(e/'icon/RESULT.json');assert icon['status']=='pass' and icon['actual_pixels_equal_source_icon'] and icon['teardown']['profile_removed']
index=read(e/'INDEX_RESOURCE_BINDING.json');assert index['status']=='pass' and index['artifact_sha256']==content['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
populated=read(e/'POPULATED_INDEX.json');assert populated['status']=='pass' and populated['cleanup']['daemon_exited'] and populated['cleanup']['private_profile_removed']
assert sha(extract/'squashfs-root/usr/bin/ottrin-indexd')==index['indexd_binary_sha256']
checks=['Dark100/Light150 actual AppImage native-byte Save after Grid,Preview,hidden-file and unedited-Location controls; fresh overwrite confirmation and unchanged original','Fresh-process reopening shows remembered grid/preview/visibility and native folder; cancellation exits normally','Caller location overrides remembered folder; actual filename editing returns the intended new path without writing files','Main Dark100/Light150 read-only VCF body click,Right navigation and Space close preserve exact files','Main and Inspector use coherent full-width themed window chrome; Dark Settings/Services and shared internal folder chooser inspected and canceled','Main closes normally while the exact private daemon remains running; theme/view preferences survive','Repeated/concurrent chooser cancellation,quiet CPU/RSS and no automatic indexing','Complete actual chooser X11 icon pixels match source Ottrin icon; truncated main-prefix control is scoped separately','Exact packaged 10000-file content index: live changes,offline edits,restart,queries and bounded quiet intervals']
gui={'status':'pass','source_revision':source,'artifact_sha256':content['artifacts']['Ottrin-0.1.0-x86_64.AppImage'],'sessions':sessions,'checks':checks,'scope':'Isolated X11/software rendering; no physical desktop,external portal or final owner acceptance'}
(e/'GUI_ACCEPTANCE.json').write_text(json.dumps(gui,indent=2)+'\n')
for name,digest in read(e/'DELIVERABLE_BASELINE.json').items():assert sha(public/name)==digest
for name,digest in content['artifacts'].items():assert sha(stage/name)==digest and (stage/(name+'.sha256')).read_text().split()==[digest,name]
files=[f for root in [stage,extract] for f in root.rglob('*') if f.is_file() and not f.is_symlink()];assert all(f.stat().st_nlink==1 for f in files);assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
previous=read(public/'LINUX_PACKAGES_ACCEPTANCE.json');(e/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(previous,indent=2)+'\n')
receipt={'status':'pass within Linux source,four-format contents and scoped actual-AppImage GUI/resource acceptance','source_revision':source,'artifacts':content['artifacts'],'evidence':str(e.relative_to(r)),'checks':content['checks']+checks,'gui_acceptance':'GUI_ACCEPTANCE.json','indexd_binary_sha256':index['indexd_binary_sha256'],'html_binary_sha256':sha(extract/'squashfs-root/usr/libexec/ottrin/ottrin-html-render'),'indexd_resource_evidence':str((e/'POPULATED_INDEX.json').relative_to(r)),'indexd_resource_scope':populated['scope']+'; measured quiet samples are scoped observations,not a personal-home or sustained workload guarantee.','chooser_resource_evidence':'resources/RESULT.json','native_source_evidence':'picker-name-current/SOURCE_ACCEPTANCE.json','hosted_source_evidence':{'run':41294424152,'source_revision':source,'linux_job':'success','performance_job':'success','windows_job_at_linux_promotion':next(x for x in hosted['jobs'] if x['name']=='verify (windows-latest)')['status']},'scope':'Usable local Linux packages; external chooser ownership/portal/folder integration,Windows GUI/installer rollback,physical devices,release identities and final owner acceptance remain separate.','cleanup':'All private GUI and populated-index profiles/processes/displays/buses are removed. Completed staging/extraction retired after publication; current compiler outputs and compact receipts retained.'}
for name in content['artifacts']:
 subprocess.run(['bash','-c','set -euo pipefail;source "$1";ottrin_publish_artifact "$2" "$3"','publish',str(r/'packaging/package-lifecycle.sh'),str(stage/name),str(public/name)],check=True)
 for suffix in ['.sha256','.provenance.json']:
  side=stage/(name+suffix)
  if side.exists():shutil.move(side,public/side.name)
 assert sha(public/name)==content['artifacts'][name]
(public/'LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n');(e/'ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n');assert not any(stage.iterdir());stage.rmdir();shutil.rmtree(extract)
build.update(pending_consumers=[],cleanup_complete=True,delivery_status='Published accepted Linux cohort; staging/extraction retired');(e/'BUILD_SOURCE.json').write_text(json.dumps(build,indent=2)+'\n')
print('Published four accepted Linux formats; completed staging/extraction removed.')
