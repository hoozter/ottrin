import pathlib,tempfile,subprocess,hashlib,json,zipfile,sys
root=pathlib.Path.cwd();dist=pathlib.Path(sys.argv[1]).resolve() if len(sys.argv)>1 else root/'.hermes/packaging-tmp/windows-current';stage=dist/'windows-payload/Ottrin-0.1.0-windows-x64'
def sha(p):return hashlib.file_digest(p.open('rb'),'sha256').hexdigest()
manifest={str(p.relative_to(stage)).replace('\\','/'):sha(p) for p in stage.rglob('*') if p.is_file()}
revision=json.loads((pathlib.Path(__file__).parent/'WINDOWS_BUILD.json').read_text())['source_revision'];source=(stage/'SOURCE_REVISION.txt').read_text();assert 'Git revision: '+revision in source.splitlines() and 'Working tree: clean' in source.splitlines()
for name in ['LICENSE','README.md','docs/USER_GUIDE.md','docs/DEVELOPMENT.md','docs/ROADMAP.md','docs/RELEASE_NOTES.md','docs/RELEASE_CHECKLIST.md']:assert subprocess.check_output(['git','show',revision+':'+name])==(stage/name).read_bytes(),name
with zipfile.ZipFile(dist/'Ottrin-0.1.0-windows-x64-portable.zip') as z:
 names={x.removeprefix('Ottrin-0.1.0-windows-x64/') for x in z.namelist() if not x.endswith('/')};assert names==set(manifest),(names-set(manifest),set(manifest)-names)
 for name,h in manifest.items():assert hashlib.sha256(z.read('Ottrin-0.1.0-windows-x64/'+name)).hexdigest()==h,name
with tempfile.TemporaryDirectory(prefix='nsis-extract-',dir=dist) as d:
 subprocess.run(['7z','x','-y','-o'+d,str(dist/'Ottrin-0.1.0-windows-x64-setup.exe')],check=True,stdout=subprocess.DEVNULL)
 for name,h in manifest.items():assert sha(pathlib.Path(d)/'$INSTDIR.ottrin-setup/new'/name)==h,name
 assert sha(pathlib.Path(d)/'$PLUGINSDIR/index-controller.exe')==manifest['bin/ottrin-indexd.exe'],'Embedded independent controller differs'
 assert (pathlib.Path(d)/'$PLUGINSDIR/payload-transaction.ps1').read_bytes()==subprocess.check_output(['git','show',revision+':packaging/windows/payload-transaction.ps1']),'Embedded payload controller differs'

result={'status':'pass','source_revision':revision,'portable_and_nsis_payload_files':len(manifest),'payload_sha256':manifest,'artifacts':{p.name:sha(p) for p in [dist/'Ottrin-0.1.0-windows-x64-portable.zip',dist/'Ottrin-0.1.0-windows-x64-setup.exe']},'checks':['Every portable payload file exactly equals stage; no extra archive files','Every expected NSIS payload file exactly equals stage','Current packaged docs and clean source revision match'],'scope':'Package contents only; native lifecycle and current GUI acceptance remain separate'}
out=root/'.hermes/evidence/preview-package-current';out.mkdir(exist_ok=True);(out/'FINAL_ARCHIVE_CONTENTS.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k!='payload_sha256'},indent=2))
