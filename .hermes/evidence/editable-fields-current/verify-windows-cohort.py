"""Verify current portable and NSIS payload identity, retiring exact extraction."""
from pathlib import Path
import hashlib, json, subprocess, tempfile, zipfile
r=Path.cwd(); e=Path(__file__).resolve().parent
build=json.loads((e/'windows/BUILD_RESULT.json').read_text()); stage=Path(build['stage'])
assert stage.parent==r/'.hermes/packaging-tmp'
assert build['status'].startswith('portable and NSIS build pass')
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
payload=stage/'windows-payload/Ottrin-0.1.0-windows-x64'
manifest={str(p.relative_to(payload)):sha(p) for p in payload.rglob('*') if p.is_file()}
assert len(manifest)==58
provenance=(payload/'SOURCE_REVISION.txt').read_text()
assert f"Git revision: {build['source_revision']}\nWorking tree: clean\n" in provenance
for name,digest in build['artifacts'].items():
 assert sha(stage/name)==digest
 assert (stage/(name+'.sha256')).read_text().split()[0]==digest
with zipfile.ZipFile(stage/'Ottrin-0.1.0-windows-x64-portable.zip') as z:
 zipped={str(Path(p.filename).relative_to('Ottrin-0.1.0-windows-x64')):hashlib.sha256(z.read(p)).hexdigest() for p in z.infolist() if not p.is_dir()}
assert zipped==manifest
with tempfile.TemporaryDirectory(prefix='.windows-picker-nsis-',dir=r/'.hermes/packaging-tmp') as temp:
 t=Path(temp);subprocess.run(['7z','x','-bd','-y','-o'+temp,str(stage/'Ottrin-0.1.0-windows-x64-setup.exe')],check=True,stdout=subprocess.DEVNULL)
 roots=list(t.rglob('bin/ottrin.exe'));assert len(roots)==1
 root=roots[0].parent.parent
 actual={str(p.relative_to(root)):sha(p) for p in root.rglob('*') if p.is_file()}
 assert actual.keys()==manifest.keys()|{'Uninstall.exe'}
 assert {k:actual[k] for k in manifest}==manifest
 assert sha(t/'$PLUGINSDIR/index-controller.exe')==manifest['bin/ottrin-indexd.exe']
 assert (t/'$PLUGINSDIR/payload-transaction.ps1').read_bytes()==(r/'packaging/windows/payload-transaction.ps1').read_bytes()
 subprocess.run(['bash','scripts/windows-gui-subsystem-smoke.sh',str(root/'Uninstall.exe')],check=True)
 uninstaller=actual['Uninstall.exe']
assert not t.exists()
(e/'PAYLOAD.json').write_text(json.dumps(manifest,indent=2)+'\n')
report={'status':'pass within host archive/content scope','source_revision':build['source_revision'],'payload_files':len(manifest),'artifacts':build['artifacts'],'portable_nsis_payloads_identical':True,'uninstaller_sha256':uninstaller,'index_controller_matches_daemon':True,'transaction_script_matches_source':True,'owned_extraction_removed':True,'scope_limit':'Host archive/content verification; native lifecycle/rendered scopes have separate receipts'}
(e/'HOST_PAYLOAD.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
