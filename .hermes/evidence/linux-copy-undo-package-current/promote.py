"""Publish only this verified cohort; retire completed extraction and staging."""
from pathlib import Path
import hashlib,json,subprocess,shutil
root=Path.cwd();e=Path(__file__).resolve().parent;public=root/'dist/packages'
def read(name):return json.loads((e/name).read_text())
def sha(path):
 with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
build=read('BUILD_SOURCE.json');content=read('PACKAGE_CONTENT_ACCEPTANCE.json');stage=Path(build['stage']);extract=Path(content['extraction'])
assert build['source_revision']==content['source_revision']==subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],text=True).strip()
hosted=json.loads((e.parent/'windows-copy-undo-package-current/HOSTED_CURRENT.json').read_text());assert hosted['conclusion']=='success'
assert not subprocess.check_output(['git','diff',hosted['headSha'],build['source_revision'],'--','crates','Cargo.toml','Cargo.lock','packaging','scripts','build-all.sh'],text=True).strip()
for name,digest in content['artifacts'].items():
 assert sha(stage/name)==digest and (stage/(name+'.sha256')).read_text().split()==[digest,name]
for name,digest in read('DELIVERABLE_BASELINE.json').items():assert sha(public/name)==digest
for folder,theme,mode in [('dark100','Dark','List'),('light-miller100','Light','Miller')]:
 r=read(folder+'/TEARDOWN.json');assert r['normal_app_exit'] and r['profile_removed'] and r['status']=='closed'
 assert r['artifact_sha256']==content['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
 assert r['persisted_config']['theme']==theme and r['persisted_config']['default_view_mode']==mode
 assert all(x['exit'] is not None for x in r['owned_processes_reaped'])
 assert r['final_fixture_sha256']['source/note.txt']==hashlib.sha256(b'original packaged QA bytes\n').hexdigest()
 assert 'copy/note.txt' not in r['final_fixture_sha256']
assert read('dark100/GUI_SMOKE.json')['status']==read('dark100/COPY_STATE.json')['status']=='pass'
assert read('light-miller100/VIEW_OBSERVATION.json')['status']=='pass'
assert read('dark100/TEARDOWN.json')['final_fixture_sha256']['stale/note.txt']==hashlib.sha256(b'changed copied bytes must survive\n').hexdigest()
previous=json.loads((public/'LINUX_PACKAGES_ACCEPTANCE.json').read_text());index=sha(extract/'squashfs-root/usr/bin/ottrin-indexd');html=sha(extract/'squashfs-root/usr/libexec/ottrin/ottrin-html-render')
assert index==previous['indexd_binary_sha256'] and html==previous['html_binary_sha256']
files=[str(f) for directory in [stage,extract] for f in directory.rglob('*') if f.is_file() and not f.is_symlink()]
assert subprocess.run(['fuser',*files],capture_output=True).returncode==1
receipt={'status':'pass within four-format payload and scoped actual-AppImage GUI acceptance','source_revision':build['source_revision'],'artifacts':content['artifacts'],'evidence':str(e.relative_to(root)),
 'checks':content['checks']+['Actual AppImage runtime Dark/List100: normal Copy Undo,changed-file Undo refusal with originals and changed data preserved','Clicked read-only VCF text then Space closes Inspector without changing bytes;all native ARGB icon pixels match','Actual AppImage runtime Light/Miller100 displays columns and passes normal Copy Undo','Both product closes exit0,preserve theme/view preferences and remove private displays,buses,daemons,profiles and mounts'],
 'hosted_source_evidence':{'run':40295923460,'source_revision':hosted['headSha'],'conclusion':'success','package_source_revision':build['source_revision'],'production_difference':'None;delivery receipts,roadmap and native QA provider path only','scope':'Full Linux/Windows workspace tests,strict lint,release,production Linux HTML integration and performance job pass'},
 'indexd_binary_sha256':index,'html_binary_sha256':html,
 'indexd_resource_evidence':previous['indexd_resource_evidence'],
 'indexd_resource_scope':'Exact same daemon ELF as the prior measured defc1ada cohort. Previous startup/reconciliation/cancellation and personal idle measurements retain their original dates,fixtures and OS context;no new resource measurement claimed.',
 'html_identity_scope':'Exact same HTML helper ELF as prior direct rendering acceptance;current production source also passes hosted sandbox integration. No new personal privileged setup or fresh distribution lifecycle claimed.',
 'gui':{'dark':'dark100/GUI_SMOKE.json','light':'light-miller100/VIEW_OBSERVATION.json','dark_teardown':'dark100/TEARDOWN.json','light_teardown':'light-miller100/TEARDOWN.json'},
 'scope':'Focused actual AppImage software-X11 Dark/List100 and Light/Miller100,Copy Undo safety,clicked read-only VCF and native icon. No broader appearance,device,Office,fresh-distro,signing or final owner acceptance.',
 'known_follow_up':'Light/Miller text-preview footer appears clipped in initial.png;focused investigation remains required and is not accepted by the view/theme observation.',
 'personal_service':'Same accepted index ELF remains enabled/running on the prior AppImage mount;release of that old active mount is the next separately scoped delivery step.',
 'cleanup':'Owned GUI/runtime/extraction/staging removed;current deliverables and compact receipts retained;personal service preserved.'}
(e/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(previous,indent=2)+'\n')
for name,digest in content['artifacts'].items():
 subprocess.run(['bash','-c','set -euo pipefail;source "$1";ottrin_publish_artifact "$2" "$3"','publish',str(root/'packaging/package-lifecycle.sh'),str(stage/name),str(public/name)],check=True)
 for suffix in ['.sha256','.provenance.json']:
  f=stage/(name+suffix)
  if f.exists():shutil.move(f,public/f.name)
 assert sha(public/name)==digest
(public/'LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n');(e/'ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
assert not any(stage.iterdir());stage.rmdir();shutil.rmtree(extract)
build.update(pending_consumers=[],cleanup_complete=True,delivery_status='Accepted,published and staging/extraction removed');(e/'BUILD_SOURCE.json').write_text(json.dumps(build,indent=2)+'\n')
print('Published four verified Linux formats;completed extraction and staging removed')
