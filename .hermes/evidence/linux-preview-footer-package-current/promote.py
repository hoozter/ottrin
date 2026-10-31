"""Publish this Linux-scoped local cohort after source,content and actual-AppImage checks."""
from pathlib import Path
import hashlib,json,subprocess,shutil
root=Path.cwd();p=Path(__file__).resolve().parent;public=root/'dist/packages'
def read(f):return json.loads(f.read_text())
def sha(f):
 with f.open('rb') as s:return hashlib.file_digest(s,'sha256').hexdigest()
build=read(p/'BUILD_SOURCE.json');content=read(p/'PACKAGE_CONTENT_ACCEPTANCE.json');hosted=read(p/'HOSTED_SOURCE.json');gui=read(p/'GUI_ACCEPTANCE.json');stage=Path(build['stage']);extract=Path(content['extraction'])
source=build['source_revision'];assert source==content['source_revision']==hosted['headSha']==gui['source_revision']==subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],text=True).strip()
for name in ['verify (ubuntu-latest)','search-performance']:
 assert next(j for j in hosted['jobs'] if j['name']==name)['conclusion']=='success',name
assert not subprocess.check_output(['git','diff','71f0cdb0',source,'--','crates','Cargo.toml','Cargo.lock'],text=True).strip()
for folder in ['light-miller100','dark-list150']:
 r=read(p/folder/'TEARDOWN.json');assert r['status']=='closed' and r['normal_app_exit'] and r['profile_removed'];assert r['artifact_sha256']==content['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
 assert all(v['exit'] is not None for v in r['owned_processes_reaped']);assert read(p/folder/'READ_ONLY_INSPECTOR.json')['status']=='pass'
assert read(p/'dark-list150/ICON_VALIDATED.json')['all_argb_pixels_match']
for name,digest in read(p/'DELIVERABLE_BASELINE.json').items():assert sha(public/name)==digest,name
for name,digest in content['artifacts'].items():assert sha(stage/name)==digest and (stage/(name+'.sha256')).read_text().split()==[digest,name]
previous=read(public/'LINUX_PACKAGES_ACCEPTANCE.json');index=sha(extract/'squashfs-root/usr/bin/ottrin-indexd');html=sha(extract/'squashfs-root/usr/libexec/ottrin/ottrin-html-render');assert index==previous['indexd_binary_sha256'] and html==previous['html_binary_sha256']
files=[str(f) for directory in [stage,extract] for f in directory.rglob('*') if f.is_file() and not f.is_symlink()];assert subprocess.run(['fuser',*files],capture_output=True).returncode==1
receipt={'status':'pass within Linux source,four-format contents and scoped actual-AppImage preview acceptance','source_revision':source,'artifacts':content['artifacts'],'evidence':str(p.relative_to(root)),'checks':content['checks']+['Actual AppImage Light/Miller100:wrapped text/DOCX metadata,native and extracted bodies,Inspector footer/read-only Space close with unchanged sources','Actual AppImage Dark/List150:short/long text Inspector metadata and read-only Space close with unchanged sources','Complete256x256 native window ARGB icon matches the asset','Both normal product closes exit0,persist theme/view and remove owned profiles,processes,displays,buses and mounts'],'gui_acceptance':'GUI_ACCEPTANCE.json','indexd_binary_sha256':index,'html_binary_sha256':html,'indexd_resource_evidence':previous['indexd_resource_evidence'],'indexd_resource_scope':'Byte-identical daemon ELF;earlier CPU/startup/reconciliation observations retain original dates,fixtures and OS context. No new resource measurement claimed.','html_identity_scope':'Byte-identical release helper. Fresh complete hosted Linux3d469f31 setup/render/preload/namespace-capability/cleanup checks pass on Ubuntu26.04;no fresh personal privileged setup claimed.','hosted_source_evidence':{'run':41243997066,'source_revision':source,'linux_job':'success','performance_job':'success','windows_job_at_linux_promotion':next(j for j in hosted['jobs'] if j['name']=='verify (windows-latest)')['status'],'scope':'Full Linux workspace tests,strict lint,release and production HTML integration;Windows full CI remains separate. No complete all-platform green or release acceptance inferred.'},'native_windows_source':'preview-footer-layout-current/ACCEPTANCE.json;380 regular tests each ordinary/elevated,one thread;Rust source unchanged from71f0cdb0','scope':'Local usable Linux cohort,not a production release. Targeted software-X11100/150% preview/normal-close/icon acceptance;broader workflows retain their earlier artifact hashes. No new fresh-distro,physical GPU/device,Office provider,editing,installer,signing or final owner acceptance.','personal_service':'Configured enabled service remains on byte-identical daemon;current active runtime delivery will be checked separately.','cleanup':'Owned GUI profiles/processes/displays/buses/mounts removed;staging/extraction retired after publication. Shared current compiler outputs and compact evidence retained.'}
(p/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(previous,indent=2)+'\n')
for name,digest in content['artifacts'].items():
 subprocess.run(['bash','-c','set -euo pipefail;source "$1";ottrin_publish_artifact "$2" "$3"','publish',str(root/'packaging/package-lifecycle.sh'),str(stage/name),str(public/name)],check=True)
 for suffix in ['.sha256','.provenance.json']:
  f=stage/(name+suffix)
  if f.exists():shutil.move(f,public/f.name)
 assert sha(public/name)==digest
(public/'LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n');(p/'ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
assert not any(stage.iterdir());stage.rmdir();shutil.rmtree(extract)
build.update(pending_consumers=[],cleanup_complete=True,delivery_status='Accepted local Linux cohort,published;staging/extraction removed');(p/'BUILD_SOURCE.json').write_text(json.dumps(build,indent=2)+'\n')
print('Published four Linux formats;retired completed staging/extraction;Windows/full release acceptance remains separate')
