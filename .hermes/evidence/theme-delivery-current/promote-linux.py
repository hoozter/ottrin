"""Publish only the accepted Linux replacement; preserve exact Windows provenance."""
from pathlib import Path
import hashlib,json,os,shutil,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def passed(name):
 p=read(e/name);assert p['status'].startswith('pass'),name;return p
build=read(e/'BUILD_SOURCE.json');source=build['source_revision'];stage=Path(build['stage']);dest=r/'dist/packages';authority=dest/'LINUX_PACKAGES_ACCEPTANCE.json'
assert stage==r/'.hermes/packaging-tmp/.linux-native-desktop-stage'
ci=read(e/'CI_CURRENT.json');assert ci['headSha']==source and ci['status']=='completed' and ci['conclusion']=='success'
assert {j['name'] for j in ci['jobs']}=={'verify (ubuntu-latest)','verify (windows-latest)','search-performance (ubuntu-latest)','search-performance (windows-latest)'} and all(j['conclusion']=='success' for j in ci['jobs'])
content=passed('PACKAGE_CONTENT_ACCEPTANCE.json');native=passed('LINUX_THEME_NATIVE_ACCEPTANCE.json')
assert content['source_revision']==native['source_revision']==source
assert native['artifact_sha256']==content['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
assert len(native['original_captures'])==8 and all(v['directly_reviewed'] and sha(e/v['path'])==v['sha256'] for v in native['original_captures'])
identity=passed('LINUX_INDEX_ELF_COMPARISON.json');assert identity['mapped_image_equal'] and identity['whole_file_equal']
prior=read(r/'.hermes/evidence/linux-stopped-delivery-current/LINUX_DAEMON_RUNTIME.json')
assert identity['versions'][1]['sha256']==prior['binary_sha256'] and prior['idle']['cpu_one_core_percent']<1
checks=read(r/'.hermes/evidence/theme-file-rows-current/SOURCE_CHECKS.json');assert checks['status']=='pass source checks' and all(v['exit']==0 for v in checks['checks'])
assert all(sha(r/name)==digest for name,digest in checks['source_sha256'].items())
assert read(e/'EXTRACTION_CLEANUP.json')['removed'] and not (r/'.hermes/packaging-tmp/.linux-desktop-extract').exists()
for v in read(e/'CLEANUP_CURRENT.json')['profiles']:assert v['removed'] and not Path(v['path']).exists()
subprocess.run(['git','diff','--exit-code',source,'--','crates','vendor','packaging','scripts','Cargo.toml','Cargo.lock','build-all.sh'],check=True,stdout=subprocess.PIPE)
old_bytes=authority.read_bytes();assert old_bytes==(e/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').read_bytes();old=read(authority);new=content['artifacts'];assert len(new)==len(old['artifacts'])==4
windows=r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json';assert windows.read_bytes()==(e/'UNCHANGED_WINDOWS_CANDIDATE_ACCEPTANCE.json').read_bytes()
files=[authority]
for name,digest in new.items():
 assert sha(stage/name)==digest and sha(dest/name)==old['artifacts'][name]
 for folder in [stage,dest]:
  for suffix in ['', '.sha256','.provenance.json']:
   p=folder/(name+suffix)
   if p.exists():files.append(p)
assert all(p.is_file() and not p.is_symlink() and p.stat().st_nlink==1 for p in files)
assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
retired=r/'.hermes/packaging-tmp/.linux-stopped-retired';assert not retired.exists()
temp=authority.with_suffix('.json.next');assert not temp.exists()
retired.mkdir();undo=[]
def move(a,b):a.replace(b);undo.append((b,a))
receipt={'source_revision':source,'previous_source':old['source_revision'],'status':'promoting','artifacts':new,'windows_manifest_preserved':True}
try:
 move(authority,retired/authority.name)
 for name in new:
  for suffix in ['', '.sha256','.provenance.json']:
   previous=dest/(name+suffix)
   if previous.exists():move(previous,retired/previous.name)
  for suffix in ['', '.sha256','.provenance.json']:
   candidate=stage/(name+suffix)
   if candidate.exists():move(candidate,dest/candidate.name)
 manifest={'status':'pass scoped shared-widget Linux delivery; full roadmap incomplete','source_revision':source,'artifacts':new,'evidence':str(e.relative_to(r)),'checks':content['checks'],'hosted_source':{'run':41809847269,'tested_source_revision':source,'all_four_jobs_success':True},'native_theme':str((e/'LINUX_THEME_NATIVE_ACCEPTANCE.json').relative_to(r)),'payload_comparison':str((e/'FULL_LINUX_PAYLOAD_COMPARISON.json').relative_to(r)),'unchanged_daemon_identity':str((e/'LINUX_INDEX_ELF_COMPARISON.json').relative_to(r)),'unchanged_daemon_runtime_basis':'.hermes/evidence/linux-stopped-delivery-current/LINUX_DAEMON_RUNTIME.json','historical_service_and_update_basis':'.hermes/evidence/linux-stopped-delivery-current/DELIVERY_VERIFICATION.json','limits':native['remaining']+['Fresh native GUI acceptance covers Nord Dark150/Light100 preview control selection only; not all roles/settings/themes, physical hardware or daily-driver acceptance','Prior Services, helper and DEB update receipts retain their original scopes; no fresh matrices implied','Windows stable packages remain f463 while native preview/MSIX acceptance continues','No signing, Store submission, public release or whole-roadmap completion']}

 temp.write_text(json.dumps(manifest,indent=2)+'\n');temp.replace(authority)
 assert all(sha(dest/name)==digest for name,digest in new.items())
 assert windows.read_bytes()==(e/'UNCHANGED_WINDOWS_CANDIDATE_ACCEPTANCE.json').read_bytes()
 receipt['status']='pass promoted verified four Linux formats'
except BaseException:
 if authority.exists() and (retired/authority.name).exists():authority.unlink()
 temp.unlink(missing_ok=True)
 for current,previous in reversed(undo):
  if current.exists():current.replace(previous)
 if retired.exists() and not any(retired.iterdir()):retired.rmdir()
 raise
else:
 retired_files=[p for p in retired.iterdir() if p.is_file()];assert all(p.stat().st_nlink==1 for p in retired_files)
 assert subprocess.run(['fuser',*map(str,retired_files)],capture_output=True).returncode==1
 receipt['superseded_files_retired']=[{'name':p.name,'bytes':p.stat().st_size,'sha256':sha(p)} for p in retired_files]
 shutil.rmtree(retired)
 remaining=list(stage.iterdir());assert all(p.is_file() and not p.is_symlink() and p.stat().st_nlink==1 for p in remaining)
 if remaining:assert subprocess.run(['fuser',*map(str,remaining)],capture_output=True).returncode==1
 receipt['consumed_stage_files_removed']=[p.name for p in remaining]
 shutil.rmtree(stage);receipt['stage_and_retired_removed']=True
finally:
 (e/'PROMOTION.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt,indent=2))
