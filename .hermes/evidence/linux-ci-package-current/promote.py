from pathlib import Path
import hashlib,json,os,shutil,subprocess
root=Path.cwd();e=root/'.hermes/evidence/linux-ci-package-current';content=json.loads((e/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text());build=json.loads((e/'BUILD_SOURCE.json').read_text());stage=Path(build['stage']);dest=root/'dist/packages'
def sha(path):
 with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
assert content['status']=='pass' and content['source_revision']==build['source_revision']
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==build['source_revision']
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],text=True).strip()
source=json.loads((root/'.hermes/evidence/linux-ci-current/WORKSPACE_SOURCE_SHA256.json').read_text());assert all(sha(root/n)==h for n,h in source.items())
index=json.loads((e/'INDEX_POPULATED_CONTENT.json').read_text());assert index['status']=='pass' and index['cleanup']['daemon_exited'] and index['cleanup']['private_profile_removed']
for case in ['dark100','light150']:assert json.loads((e/case/'GUI_CHECKS.json').read_text())['status']=='pass within recorded scope'
for case in ['dark100-normal-close','light150']:
 t=json.loads((e/case/'TEARDOWN.json').read_text());assert t['normal_app_exit'] and t['profile_removed'];assert t['persisted_config']['default_view_mode']=='List'
assert json.loads((e/'dark100/TEARDOWN_CORRECTION.json').read_text())['profile_removed']
assert json.loads((e/'POST_CLOSE_FILE_ACCEPTANCE.json').read_text())['status']=='pass'
baseline=json.loads((e/'DELIVERABLE_BASELINE.json').read_text());assert all(sha(Path(n))==h for n,h in baseline.items())
for name,expected in content['artifacts'].items():
 p=stage/name;assert p.is_file() and not p.is_symlink() and sha(p)==expected;assert p.stat().st_dev==dest.stat().st_dev
 assert not Path(str(dest/name)+'.asc').exists(),'cannot preserve a stale signature'
 subprocess.run(['sha256sum','-c',name+'.sha256'],cwd=stage,check=True)
old=dest/'LINUX_PACKAGES_ACCEPTANCE.json';assert old.is_file();shutil.copy2(old,e/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json')
# Reuse the authoritative publisher. Every gate above is fail-fast before mutation.
script='''set -euo pipefail
source "$1/packaging/package-lifecycle.sh"
for candidate in "$2"/*.AppImage "$2"/*.tar.gz "$2"/*.deb "$2"/*.rpm; do
 destination="$3/$(basename "$candidate")"
 ottrin_publish_artifact "$candidate" "$destination"
 for suffix in .sha256 .provenance.json; do
  if [[ -f "$candidate$suffix" ]]; then mv -f -- "$candidate$suffix" "$destination$suffix"; fi
 done
done
'''
subprocess.run(['bash','-c',script,'publish',str(root),str(stage),str(dest)],check=True)
assert all(sha(dest/n)==h for n,h in content['artifacts'].items());assert not list(stage.iterdir());stage.rmdir()
receipt={'status':'pass within scoped Linux package/UI/file-operation/index resource acceptance','source_revision':build['source_revision'],'artifacts':content['artifacts'],'evidence':str(e.relative_to(root)),'verifier_sha256':content['verifier_sha256'],'source_checks':'.hermes/evidence/linux-ci-current/CI_REPAIR_ACCEPTANCE.json','checks':content['checks']+['Exact AppImage Dark100 and Light150: Copy/actual Undo, Move/actual Undo','Both scopes: recreated copied publication reuses the exact device/inode pair; actual Undo refuses without modifying source or replacement','Long selected-name footer is one row; real hover tooltip contains the full name; controls stay visible','Read-only VCF Space opens/closes Inspector and preserves bytes','Exact Linux window icon pixels match source PNG; actual private daemon name is ottrin-indexd','List mode and theme persist through normal close; Light150 four public file hashes match after exit','Current 10000-file content-enabled index verifies freshness/bursts/offline changes/restart; 30-second idle samples use 0.067-0.100 percent of one core, about 10-14 MiB RSS'],'scope':'Targeted private X11/DBus/software rendering and owned same-filesystem operations, package content/namespace proofs and populated index fixture; not personal-service, native distribution install/upgrade, full Office/picker, physical desktop, signing or final user acceptance. Historical receipts retain only their named artifact scope.','ci_followup':{'source':'0f8011f6','linux':'original platform failures resolved; two Office isolation tests fail because runtime prerequisites need provisioning','windows':'test compilation now completes; 449 passed, 9 native cleanup/admission failures, 79 ignored; diagnose principal/DACL behavior without weakening guards'},'corrections':{'dark_original_close':'Harness XDestroyWindow caused winit abort; it is not accepted as normal close. Separate Dark normal-close run and complete Light run use product Close control.','window_selector':'Initial OR PID/title query falsely reported Inspector presence; corrected --all selector confirms real Space close.'},'cleanup':{'private_gui_profiles_processes_displays_buses_daemons_portals':'removed; per-case teardown and original Dark correction retained','source_stage':'removed after verified publication','root_owned_test_core':'3.9 MiB exact task core requires privileged cleanup; WAITING.json. Other work continues.'}}
text=json.dumps(receipt,indent=2)+'\n';(e/'ACCEPTANCE.json').write_text(text);temporary=dest/'.LINUX_PACKAGES_ACCEPTANCE.tmp';temporary.write_text(text);os.replace(temporary,dest/'LINUX_PACKAGES_ACCEPTANCE.json')
(e/'PROMOTION.json').write_text(json.dumps({'status':'complete','source':build['source_revision'],'artifacts':content['artifacts'],'stable_directory':str(dest),'current_receipt':str(dest/'LINUX_PACKAGES_ACCEPTANCE.json'),'candidate_removed':not stage.exists(),'canonical_publisher_sha256':sha(root/'packaging/package-lifecycle.sh')},indent=2)+'\n');print('Promoted all four verified Linux artifacts at the stable path.')
