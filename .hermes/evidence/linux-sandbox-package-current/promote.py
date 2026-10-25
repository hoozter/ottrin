from pathlib import Path
import json,hashlib,subprocess,shutil,gzip
root=Path.cwd();p=root/'.hermes/evidence/linux-sandbox-package-current';content=json.loads((p/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text());build=json.loads((p/'BUILD_SOURCE.json').read_text());stage=Path(build['stage']);public=root/'dist/packages';teardown=json.loads((p/'dark100/TEARDOWN.json').read_text());gui=json.loads((p/'dark100/GUI_SMOKE.json').read_text());assert teardown['normal_app_exit'] and teardown['profile_removed'] and teardown['persisted_config']['theme']=='Dark' and teardown['persisted_config']['default_view_mode']=='List';assert all(v['exit'] is not None for v in teardown['owned_processes_reaped']);assert gui['status']=='pass' and gui['artifact_sha256']==content['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
def sha(f):return hashlib.file_digest(f.open('rb'),'sha256').hexdigest()
for name,digest in content['artifacts'].items():assert sha(stage/name)==digest
baseline=json.loads((p/'DELIVERABLE_BASELINE.json').read_text())
for name,digest in baseline.items():assert sha(public/name)==digest
previous=json.loads((public/'LINUX_PACKAGES_ACCEPTANCE.json').read_text());(p/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(previous,indent=2)+'\n')
extract=Path(content['extraction']);index_sha=sha(extract/'squashfs-root/usr/bin/ottrin-indexd');assert index_sha==previous['indexd_binary_sha256']
for name,digest in content['artifacts'].items():
 subprocess.run(['bash','-c','set -euo pipefail; source "$1"; ottrin_publish_artifact "$2" "$3"','publish',str(root/'packaging/package-lifecycle.sh'),str(stage/name),str(public/name)],check=True)
 for suffix in ['.sha256','.provenance.json']:
  src=stage/(name+suffix)
  if src.exists():shutil.move(src,public/(name+suffix))
 assert sha(public/name)==digest
receipt={'status':'pass within four-format content and scoped native AppImage GUI acceptance','source_revision':build['source_revision'],'artifacts':content['artifacts'],'evidence':'.hermes/evidence/linux-sandbox-package-current','checks':content['checks']+['Actual AppImage Dark/List100 startup and all 256x256 ARGB icon pixels match the source asset','Read-only VCF Quick Inspector closes with Space and preserves all VCF bytes','Actual product Close exits zero and persists Dark/List; owned app/daemon/display/bus/WM/profile teardown passes'],'scope':'Software X11 Dark/List100 and exact package content; no new broad UI suite, operation workflow, actual HTML/Office provider GUI, fresh distro install, personal service, physical desktop, Windows, signing or final owner acceptance. Earlier 1b6e/fb6f workflow receipts retain their own artifact scopes.','included_source_fixes':['Preview resource boundary LimitCORE=0 from a230','HTML workspace mode0700 at creation and RAII before child preparation from 7d5'],'html_restricted_host_status':'Trusted launcher/profile experiment accepted separately; production integration remains unfinished and is not claimed by these builds.','source_checks':'Five focused HTML tests, two real local HTML renders, strict preview lint; current Office fixture repair executes 17 Linux provider/boundary checks. Local namespace restriction0 is distinct from restricted Ubuntu prototype1.','indexd_binary_sha256':index_sha,'indexd_resource_evidence':'Byte-identical to previous cohort and fb6f measured 0.067–0.100% of one core; no new sustained resource/personal-service claim.','cleanup':'TEARDOWN and FINAL_TEARDOWN receipts; completed extraction/stage/wrappers removed; both previously recorded root-owned cores remain owner waiting.'}
(public/'LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n');(p/'ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
assert not any(stage.iterdir());stage.rmdir();shutil.rmtree(extract)
shutil.rmtree(p/'rpm-bin')
for f in [p/'dark100/ICON_RAW.txt',p/'fixture-repair.patch']:
 if f.exists():f.unlink()
for folder in p.rglob('__pycache__'):shutil.rmtree(folder)
for f in p.rglob('*.log'):
 if f.stat().st_size>65536:f.with_suffix(f.suffix+'.gz').write_bytes(gzip.compress(f.read_bytes(),mtime=0));f.unlink()
(p/'PROMOTION.json').write_text(json.dumps({'status':'complete','source_revision':build['source_revision'],'artifacts':content['artifacts'],'stage_removed':not stage.exists(),'extract_removed':not extract.exists(),'publisher_sha256':sha(root/'packaging/package-lifecycle.sh')},indent=2)+'\n')
(p/'FINAL_TEARDOWN.json').write_text(json.dumps({'status':'pass','profile_removed':teardown['profile_removed'],'app_exit_zero':teardown['normal_app_exit'],'owned_processes_reaped':teardown['owned_processes_reaped'],'daemon_cleanup':teardown['daemon_cleanup'],'portal_mounts_unmounted':teardown['owned_portal_mounts_unmounted'],'extract_removed':not extract.exists(),'stage_removed':not stage.exists(),'rpm_wrappers_removed':not(p/'rpm-bin').exists(),'current_deliverables_preserved':True,'previous_root_owned_cores':'Remain on explicit owner waiting list; no new core created under LimitCORE0 harness'},indent=2)+'\n');print(json.dumps(content['artifacts'],indent=2))
