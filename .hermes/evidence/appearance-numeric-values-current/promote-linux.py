"""Publish the exact scoped native-verified Linux cohort after hosted gates."""
from pathlib import Path
import hashlib,json,shutil,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
SOURCE_SCOPE_TEXT=read(e/'SOURCE_SCOPE.json')['shipping_behavior']+' Earlier receipts retain exact source/artifact scope; no fresh CPU or full-roadmap acceptance.'
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
build=read(e/'BUILD_SOURCE.json');source=build['source_revision'];ci=read(e/'CI_CURRENT.json');content=read(e/'PACKAGE_CONTENT_ACCEPTANCE.json');native=read(e/'LINUX_NATIVE_ACCEPTANCE.json');comparison=read(e/'FULL_LINUX_PAYLOAD_COMPARISON.json');scope=read(e/'SOURCE_SCOPE.json')
assert read(e/'WINDOWS_NATIVE_ACCEPTANCE.json')['status'].startswith('pass') and read(e/'WINDOWS_NATIVE_ACCEPTANCE.json')['cases']==34
assert ci['headSha']==source and ci['status']=='completed' and ci['conclusion']=='success' and len(ci['jobs'])==4 and all(j['conclusion']=='success' for j in ci['jobs'])
assert content['source_revision']==native['source_revision']==comparison['candidate_source']==scope['candidate_source']==source
assert all(v['status'].startswith('pass') for v in [content,native,comparison,scope])
subprocess.run(['git','diff','--exit-code',source,'--','.',':!.hermes/**',':!docs/ROADMAP.md',':!dist/**'],check=True,stdout=subprocess.PIPE)
stage=Path(build['stage']);assert stage==r/'.hermes/packaging-tmp/.linux-native-desktop-stage' and not stage.is_symlink();destination=r/'dist/packages';authority=destination/'LINUX_PACKAGES_ACCEPTANCE.json';old=read(authority);assert old['source_revision']==comparison['previous_source']
for name,digest in old['artifacts'].items():assert sha(destination/name)==digest
for name,digest in content['artifacts'].items():assert sha(stage/name)==digest and (stage/(name+'.sha256')).read_text().split()[0]==digest
files=[p for p in stage.rglob('*') if p.is_file()];expected={n+s for n in content['artifacts'] for s in ['', '.sha256','.provenance.json']};assert {str(p.relative_to(stage)) for p in files}<=expected;assert all(not p.is_symlink() and p.stat().st_nlink==1 for p in files);assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
previous=e/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json';assert previous.read_bytes()==authority.read_bytes()
receipt={'status':'pass scoped current Linux appearance numeric-control preview; full roadmap incomplete','source_revision':source,'artifacts':content['artifacts'],'evidence':str(e.relative_to(r)),'checks':content['checks'],'current_native_fields':str((e/'LINUX_NATIVE_ACCEPTANCE.json').relative_to(r)),'source_scope':str((e/'SOURCE_SCOPE.json').relative_to(r)),'full_payload_comparison':str((e/'FULL_LINUX_PAYLOAD_COMPARISON.json').relative_to(r)),'indexd_binary_sha256':read(e/'LINUX_INDEX_ELF_COMPARISON.json')['versions'][1]['sha256'],'unchanged_responsibility_basis':{'previous_source':old['source_revision'],'previous_receipt':str(previous.relative_to(r)),'scope':SOURCE_SCOPE_TEXT},'hosted_source':{'run':ci['databaseId'],'tested_source_revision':source,'all_four_jobs_success':True},'limits':['Dark100/Light150 private X11/software named numeric typing/restore/Cancel; other workflow scopes retain original hashes','Current Windows native144DPI checks separate; broader appearance/platform and human acceptance incomplete','Prior strict software GUI CPU failure remains open; no new GUI or daemon benchmark','Local unsigned delivery; no public release or full roadmap acceptance'],'cleanup':'Both owned Linux desktops/profiles/processes/mounts and content extraction removed; exact stage retired by publication; compiler-cache cleanup separately receipted'}
for name,digest in content['artifacts'].items():
 subprocess.run(['bash','-c','set -euo pipefail;source "$1";ottrin_publish_artifact "$2" "$3"','publish',str(r/'packaging/package-lifecycle.sh'),str(stage/name),str(destination/name)],check=True)
 for suffix in ['.sha256','.provenance.json']:
  p=stage/(name+suffix)
  if p.exists():shutil.move(p,destination/p.name)
 assert sha(destination/name)==digest
assert not any(stage.iterdir());stage.rmdir();tmp=authority.with_suffix('.next');tmp.write_text(json.dumps(receipt,indent=2)+'\n');tmp.replace(authority);(e/'LINUX_DELIVERY.json').write_text(json.dumps(receipt,indent=2)+'\n');build.update(pending_consumers=[],cleanup_complete=True,delivery_status='Verified named-control Linux cohort published; staging removed');(e/'BUILD_SOURCE.json').write_text(json.dumps(build,indent=2)+'\n')
