"""Finish this owned candidate cohort after the portable builder succeeds."""
from pathlib import Path
import gzip,hashlib,json,os,subprocess,sys
tools=Path(__file__).resolve().parent
p=Path(os.environ.get("OTTRIN_WINDOWS_PACKAGE_EVIDENCE_DIR",str(tools))).resolve();assert p.parent==tools.parent,"Evidence must have a workspace-owned family"
root=p.parents[2]
owner=json.loads((p/'OWNERSHIP.json').read_text());stage=Path(owner['stage'])
env=dict(os.environ,OTTRIN_WINDOWS_PACKAGE_EVIDENCE_DIR=str(p),OTTRIN_DIST_DIR=str(stage))
sha=lambda f:hashlib.sha256(f.read_bytes()).hexdigest()
write=lambda name,data:(p/name).write_text(json.dumps(data,indent=2)+'\n')
def run(args,log):
 with (p/log).open('w') as out:subprocess.run(args,env=env,cwd=root,stdout=out,stderr=subprocess.STDOUT,check=True)
 print(log+': pass',flush=True)
def native(mode):run([sys.executable,str(tools/'native-package.py'),mode],mode+'.log')
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==owner['source_commit']
assert not subprocess.check_output(['git','diff','--name-only'],text=True).strip()
assert not subprocess.check_output(['git','diff','--cached','--name-only'],text=True).strip()
payload=stage/'windows-payload/Ottrin-0.1.0-windows-x64'
assert 'Git revision: '+owner['source_commit'] in (payload/'SOURCE_REVISION.txt').read_text()
assert 'Working tree: clean' in (payload/'SOURCE_REVISION.txt').read_text()
native('deploy')
run(['bash','packaging/windows/package-installer.sh'],'nsis-build.log')
env['WIX']=str(tools/'native-package.py')
run(['bash','packaging/windows/package-msi.sh'],'msi-build.log')
native('verify');native('cli-smoke')
sys.path.insert(0,str(p.parent/'windows-copy-cleanup-current'));import transport as n
n.GROOT=r'C:\OttrinCopyCleanupQA\package-refresh'
n.download('MSI_SERVICE_BASELINE.json',p/'MSI_SERVICE_BASELINE.json')
cli=json.loads((p/'CLI_SMOKE.json').read_text(encoding='utf-8-sig'))
for case in cli['runs']:
 for stream in ('stdout','stderr'):
  name=case['executable']+'.'+stream;dest=p/name
  n.download(name,dest);data=dest.read_bytes();assert hashlib.sha256(data).hexdigest()==case[stream+'_sha256']
  (p/(name+'.gz')).write_bytes(gzip.compress(data,mtime=0));dest.unlink()
run([sys.executable,str(tools/'verify-host.py')],'verify-host.log')
old=json.loads((p/'OLD_PAYLOAD.json').read_text());current={str(f.relative_to(payload)):sha(f) for f in payload.rglob('*') if f.is_file()}
assert old.keys()==current.keys()
write('PAYLOAD_DELTA.json',{f:{'old_sha256':old[f],'new_sha256':current[f],'changed':old[f]!=current[f]} for f in sorted(old)})
review=json.loads((p/'SOURCE_REVIEW.json').read_text())
for path,digest in review['files'].items():
 if path.startswith('crates/'):assert sha(root/path)==digest,path
assert all(not x['changed'] for f,x in json.loads((p/'PAYLOAD_DELTA.json').read_text()).items() if f.startswith(('bin/pdfium/','third-party-notices/')) or f in ('bin/opengl32.dll','bin/libEGL.dll','bin/libGLESv2.dll','bin/libgallium_wgl.dll','bin/dxil.dll','bin/rg.exe','bin/ottrin-office-render.exe','bin/ottrin-legacy-doc-render.exe'))
native('teardown')
run([sys.executable,str(tools/'promote.py')],'promote.log')
checks=json.loads((p/'PACKAGE_IDENTITIES.json').read_text())
baseline=json.loads((p/'HOST_BASELINE.json').read_text());public={f:h for f,h in baseline.items() if Path(f).parent.resolve()!=Path(owner['candidate'])}
assert all(sha(Path(f))==h for f,h in public.items())
for f in p.rglob('*.json'):f.write_text(json.dumps(json.loads(f.read_text(encoding='utf-8-sig')),indent=2)+'\n')
receipt={'status':'pass within package content and native loader/CLI scopes','source_commit':owner['source_commit'],'working_tree_at_build':'clean','candidate':owner['candidate'],'artifacts':checks['artifacts'],'payload_files':checks['payload_files'],'package_identity':'PACKAGE_IDENTITIES.json','native_deployment':'DEPLOYMENT.json','native_msi_administrative_extraction':'MSI_PAYLOAD.json','native_loader_cli':cli,'source_checks':{'evidence':'SOURCE_REVIEW.json','production_files_byte_identical_to_recorded_source':True,'scope':review['scope']},'payload_delta':'PAYLOAD_DELTA.json','unchanged_office_and_graphics_bytes':'Both Office helpers and all bundled runtimes match the preceding candidate; this is byte identity, not fresh main-window/Office GUI acceptance','promotion':'PROMOTION.json','cleanup':json.loads((p/'TEARDOWN.json').read_text()),'guest_preservation_scope':'Original config.json and Uninstall.exe hashes plus prior MSI service state; not whole profile/installation inventory','storage':'STORAGE.json','public_windows_files_unchanged':True,'linux_packages_unchanged':True,'tools_sha256':{name:sha(tools/name) for name in ('native-package.py','verify-host.py','promote.py')},'limits':['Native CLI ran as SYSTEM, not ordinary-account GUI or service-lifecycle acceptance','Native MSI compilation/administrative extraction is not installation, upgrade, repair, rollback or uninstall acceptance','Recorded source checks remain separate from packaged window/OS clipboard acceptance','Schema 18 preserves older recovery bytes for review without conversion','Incomplete-payload reclamation and remaining transaction/storage/interruption/resource/workflow gates remain; public Windows Copy stays disabled','No user acceptance, release readiness or daily-driver readiness claim']}
delta=json.loads((p/'PAYLOAD_DELTA.json').read_text());changed=[f for f,x in delta.items() if x['changed']]
receipt['changed_payload_files']=changed
write('ACCEPTANCE.json',receipt)
print(json.dumps({'status':'pass','source_commit':owner['source_commit'],'artifacts':checks['artifacts'],'changed_payload_files':changed}),flush=True)
