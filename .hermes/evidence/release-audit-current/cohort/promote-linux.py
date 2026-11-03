"""Promote the exact locally verified startup cohort after exact-source hosted gates."""
from pathlib import Path
import json,hashlib,subprocess,shutil
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
build=read(e/'BUILD_SOURCE.json');source=build['source_revision'];ci=read(e/'CI_CURRENT.json');content=read(e/'PACKAGE_CONTENT_ACCEPTANCE.json');native=read(e/'LINUX_NATIVE_ACCEPTANCE.json');comparison=read(e/'FULL_LINUX_PAYLOAD_COMPARISON.json');scope=read(e/'SOURCE_SCOPE.json')
assert ci['headSha']==source and ci['status']=='completed' and ci['conclusion']=='success'
assert {j['name'] for j in ci['jobs']}=={'verify (ubuntu-latest)','verify (windows-latest)','search-performance (ubuntu-latest)','search-performance (windows-latest)'} and all(j['conclusion']=='success' for j in ci['jobs'])
assert content['source_revision']==native['source_revision']==comparison['candidate_source']==scope['candidate_source']==source
assert content['status']=='pass' and native['status'].startswith('pass') and comparison['status'].startswith('pass')
subprocess.run(['git','diff','--exit-code',source,'--','.',':!.hermes/**',':!docs/ROADMAP.md',':!dist/**'],check=True,stdout=subprocess.PIPE)
stage=Path(build['stage']);assert stage==r/'.hermes/packaging-tmp/.linux-native-desktop-stage' and not stage.is_symlink();destination=r/'dist/packages';authority=destination/'LINUX_PACKAGES_ACCEPTANCE.json';old=read(authority)
assert old['source_revision']==comparison['previous_source']
for n,h in old['artifacts'].items():assert sha(destination/n)==h
for n,h in content['artifacts'].items():assert sha(stage/n)==h and (stage/(n+'.sha256')).read_text().split()[0]==h
files=[p for p in stage.rglob('*') if p.is_file()];expected={n+s for n in content['artifacts'] for s in ['', '.sha256','.provenance.json']};assert {str(p.relative_to(stage)) for p in files}<=expected;assert all(not p.is_symlink() and p.stat().st_nlink==1 for p in files);assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
previous=e/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json';assert not previous.exists();previous.write_bytes(authority.read_bytes())
receipt={'status':'pass exact current package content and default startup scope; broader audit incomplete','source_revision':source,'artifacts':content['artifacts'],'evidence':str(e.relative_to(r)),'checks':content['checks'],'current_native_startup_receipt':str((e/'LINUX_NATIVE_ACCEPTANCE.json').relative_to(r)),'source_scope':str((e/'SOURCE_SCOPE.json').relative_to(r)),'full_payload_comparison':str((e/'FULL_LINUX_PAYLOAD_COMPARISON.json').relative_to(r)),'indexd_binary_sha256':old['indexd_binary_sha256'],'unchanged_responsibility_basis':{'previous_source':old['source_revision'],'previous_receipt':str(previous.relative_to(r)),'scope':'Every non-GUI runtime/helper/asset/library/launcher unchanged. Shipping GUI change is one constructor reveal call. Earlier installer/index/preview/picker scopes retain original hashes and are not fresh reruns.'},'hosted_source':{'run':41605875200,'tested_source_revision':source,'all_four_jobs_success':True},'limits':['Local unsigned delivery; no public release','Private software X11 Dark100/Light150; physical GPU and final owner acceptance separate','Complete daily-driver workflow and concentrated remaining roadmap audit not accepted'],'cleanup':'Private startup desktops/profiles and completed extraction removed; exact staging retired after publication; compiler caches preserved.'}
for n,h in content['artifacts'].items():
 subprocess.run(['bash','-c','set -euo pipefail;source "$1";ottrin_publish_artifact "$2" "$3"','publish',str(r/'packaging/package-lifecycle.sh'),str(stage/n),str(destination/n)],check=True)
 for suffix in ['.sha256','.provenance.json']:
  p=stage/(n+suffix)
  if p.exists():shutil.move(p,destination/p.name)
 assert sha(destination/n)==h
assert not any(stage.iterdir());stage.rmdir();tmp=authority.with_suffix('.next');tmp.write_text(json.dumps(receipt,indent=2)+'\n');tmp.replace(authority);(e/'LINUX_DELIVERY.json').write_text(json.dumps(receipt,indent=2)+'\n');build.update(pending_consumers=[],cleanup_complete=True,delivery_status='Verified7d Linux cohort promoted; staging removed');(e/'BUILD_SOURCE.json').write_text(json.dumps(build,indent=2)+'\n');print('Promoted Linux four-format startup cohort; exact stage retired.')
