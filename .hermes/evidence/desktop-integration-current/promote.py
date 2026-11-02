"""Accept Linux-only desktop/helper changes without promoting the Windows candidate."""
from pathlib import Path
import hashlib,json,shutil,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;public=r/'dist/packages'
def read(p):return json.loads(p.read_text())
def sha(p):
 with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
b=read(e/'BUILD_SOURCE.json');source=b['source_revision'];stage=Path(b['stage']);content=read(e/'PACKAGE_CONTENT_ACCEPTANCE.json');extract=Path(content['extraction'])
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==source
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],text=True).strip()
assert content['status']=='pass' and content['source_revision']==source
hosted=read(e/'HOSTED_SOURCE.json');assert hosted['headSha']==source
for name in ['verify (ubuntu-latest)','search-performance']:
 assert next(j for j in hosted['jobs'] if j['name']==name)['conclusion']=='success'
assert read(e/'NAMESPACE.json')['status']=='pass'
artifact=content['artifacts']['Ottrin-0.1.0-x86_64.AppImage'];sessions={}
for name in ['main-dark100','main-light150','properties-dark100','properties-light150','icon','upgrade-view']:
 a=read(e/name/'RESULT.json');assert a['artifact_sha256']==artifact
 assert a['teardown']['profile_removed'] and not Path(a['teardown']['profile']).exists()
 if name.startswith('main-'):assert a['normal_exit']==0 and a['initial_files']==a['final_files'] and a['index_survived_window_close']
 elif name.startswith('properties-'):assert a['normal_exit']==0 and a['source_and_config_preserved'] and a['no_index_started']
 else:assert a['status']=='pass'
 sessions[name]=str((e/name/'RESULT.json').relative_to(r))
assert read(e/'VISUAL_REVIEW.json')['status']=='reviewed'
for distro,package in [('fedora','ottrin-0.1.0-1.x86_64.rpm'),('ubuntu','ottrin_0.1.0_amd64.deb')]:
 a=read(e/distro/'VM_RESULT.json');cleanup=read(e/distro/'VM_CLEANUP.json')
 assert a['status']=='pass within actor scope' and a['source']==source and not a['setup_control']
 assert a['package_sha256']==content['artifacts'][package]
 assert cleanup['domain_removed'] and len(cleanup['volumes_removed'])==3 and a['temporary_seed_and_ssh_keys_removed']
previous=read(public/'LINUX_PACKAGES_ACCEPTANCE.json');indexd=sha(extract/'squashfs-root/usr/bin/ottrin-indexd')
assert indexd==previous['indexd_binary_sha256'],'Changed daemon requires renewed resource acceptance'
resource=read(r/'.hermes/evidence/desktop-package-current/linux/POPULATED_INDEX.json');assert resource['status']=='pass'
for name,digest in read(e/'DELIVERABLE_BASELINE.json').items():assert sha(r/name)==digest
for name,digest in content['artifacts'].items():assert sha(stage/name)==digest
files=[p for root in [stage,extract] for p in root.rglob('*') if p.is_file() and not p.is_symlink()]
assert all(p.stat().st_nlink==1 for p in files)
assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
(e/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(previous,indent=2)+'\n')
receipt={'status':'pass within exact Linux package, private GUI and fresh native distribution scopes','source_revision':source,'artifacts':content['artifacts'],'evidence':str(e.relative_to(r)),'indexd_binary_sha256':indexd,'html_binary_sha256':sha(extract/'squashfs-root/usr/libexec/ottrin/ottrin-html-render'),'checks':content['checks'],'gui_sessions':sessions,'native_distribution_lifecycle':['Fedora44 DNF/RPM','Ubuntu24.04 APT/DEB'],'native_scope':'Normal setup without controls, actual portal chooser, idle status, same-version reinstall, modified-asset refusal, online/offline native removal and preference restoration, portable picker surviving native removal. Source/config unchanged.','resource_identity':{'previous_receipt':'.hermes/evidence/desktop-package-current/linux/POPULATED_INDEX.json','daemon_binary_identical':True,'note':'Earlier scoped resource samples retain their original package identity; not rerun or represented as new-package measurements.'},'hosted_source':{'run':hosted['databaseId'],'linux':'success','performance':'success','windows_state_at_linux_promotion':next(j for j in hosted['jobs'] if j['name']=='verify (windows-latest)')['status']},'scope':'Local four-format Linux delivery. Physical graphics/devices, external production picker use and final owner acceptance remain separate. Windows internal candidate and failed MSI rollback gate unchanged.','cleanup':'Owned GUI profiles/processes/displays/buses/mounts and both native VM domains/volumes/credentials removed; staging/extraction retired after promotion. Current compiler caches and compact receipts retained.'}
for name in content['artifacts']:
 subprocess.run(['bash','-c','set -euo pipefail;source "$1";ottrin_publish_artifact "$2" "$3"','publish',str(r/'packaging/package-lifecycle.sh'),str(stage/name),str(public/name)],check=True)
 for suffix in ['.sha256','.provenance.json']:
  side=stage/(name+suffix)
  if side.exists():shutil.move(side,public/side.name)
 assert sha(public/name)==content['artifacts'][name]
for path in [public/'LINUX_PACKAGES_ACCEPTANCE.json',e/'ACCEPTANCE.json']:path.write_text(json.dumps(receipt,indent=2)+'\n')
assert not any(stage.iterdir());stage.rmdir();shutil.rmtree(extract)
b.update(pending_consumers=[],cleanup_complete=True,delivery_status='Accepted local Linux cohort promoted; staging/extraction removed');(e/'BUILD_SOURCE.json').write_text(json.dumps(b,indent=2)+'\n')
print('Promoted four Linux packages; staging and extraction removed.')
