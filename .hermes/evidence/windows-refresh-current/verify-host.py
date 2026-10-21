from pathlib import Path
import hashlib,json,os,subprocess,tempfile,zipfile
tool_dir=Path(__file__).resolve().parent
p=Path(os.environ.get("OTTRIN_WINDOWS_PACKAGE_EVIDENCE_DIR",str(tool_dir))).resolve()
assert p.parent==tool_dir.parent,"Evidence must have a workspace-owned family"
root=tool_dir.parents[2]
o=json.loads((p/'OWNERSHIP.json').read_text());stage=Path(o['stage']);payload=stage/'windows-payload/Ottrin-0.1.0-windows-x64'
sha=lambda path:hashlib.sha256(path.read_bytes()).hexdigest()
m=json.loads((p/'PAYLOAD.json').read_text());expected={key.split('/',1)[1]:value for key,value in m['entries'].items()}
actual={f.relative_to(payload).as_posix():sha(f) for f in payload.rglob('*') if f.is_file()}
assert actual==expected
with zipfile.ZipFile(stage/'Ottrin-0.1.0-windows-x64-portable.zip') as z:
 files=[entry for entry in z.infolist() if not entry.is_dir()];assert len(files)==len(expected)
 assert {e.filename.split('/',1)[1]:hashlib.sha256(z.read(e)).hexdigest() for e in files}==expected
for name,relative in [('ottrin-app.exe','bin/ottrin.exe'),('ottrin-indexd.exe','bin/ottrin-indexd.exe'),('ottrin-priv-helper.exe','bin/ottrin-priv-helper.exe'),('ottrin-office-render.exe','bin/ottrin-office-render.exe'),('ottrin-legacy-doc-render.exe','bin/ottrin-legacy-doc-render.exe')]:
 assert sha(root/'target/x86_64-pc-windows-gnu/release'/name)==expected[relative]
with tempfile.TemporaryDirectory(prefix='windows-refresh-nsis-',dir=root/'.hermes/packaging-tmp') as temporary:
 t=Path(temporary);subprocess.run(['7z','x','-y','-o'+temporary,str(stage/'Ottrin-0.1.0-windows-x64-setup.exe')],check=True,stdout=subprocess.DEVNULL)
 extracted=t/'$INSTDIR.ottrin-setup/new';actual={f.relative_to(extracted).as_posix():f for f in extracted.rglob('*') if f.is_file()}
 assert actual.keys()==expected.keys()|{'Uninstall.exe'}
 for name,digest in expected.items():assert sha(actual[name])==digest,name
 assert 'WriteUninstaller "$INSTDIR.ottrin-setup\\new\\Uninstall.exe"' in (root/'packaging/windows/installer.nsi').read_text()
 subprocess.run(['bash',str(root/'scripts/windows-gui-subsystem-smoke.sh'),str(actual['Uninstall.exe'])],check=True)
 assert sha(t/'$PLUGINSDIR/index-controller.exe')==expected['bin/ottrin-indexd.exe']
 assert (t/'$PLUGINSDIR/payload-transaction.ps1').read_bytes()==(root/'packaging/windows/payload-transaction.ps1').read_bytes()
 uninstaller=sha(actual['Uninstall.exe'])
artifacts={}
for name in ['Ottrin-0.1.0-windows-x64-portable.zip','Ottrin-0.1.0-windows-x64-setup.exe','Ottrin-0.1.0-windows-x64.msi']:
 checksum,filename=(stage/(name+'.sha256')).read_text().split();assert filename==name and checksum==sha(stage/name);artifacts[name]=checksum
msi=json.loads((p/'MSI_PAYLOAD.json').read_text(encoding='utf-8-sig'));assert msi['status']=='pass' and msi['entries_checked']==len(expected) and msi['source_commit']==o['source_commit'] and msi['msi_sha256']==artifacts['Ottrin-0.1.0-windows-x64.msi']
receipt={'status':'pass','source_commit':o['source_commit'],'artifacts':artifacts,'payload_files':len(expected),'all_three_payloads_match':True,'five_executables_match_release_outputs':True,'nsis_generated_uninstaller_sha256':uninstaller,'nsis_index_controller_matches':True,'nsis_transaction_script_matches_source':True,'host_nsis_extraction_removed':True,'scope':'Package content and native MSI administrative extraction only; no installation, GUI or daily-driver acceptance'}
(p/'PACKAGE_IDENTITIES.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt,indent=2))
