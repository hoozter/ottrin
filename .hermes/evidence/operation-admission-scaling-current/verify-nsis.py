from pathlib import Path
import tempfile,subprocess,json,hashlib
r=Path.cwd();e=Path(__file__).resolve().parent;s=r/'.hermes/packaging-tmp/windows-activity-candidate';m=json.loads((e/'PAYLOAD.json').read_text());sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();expected={k.split('/',1)[1]:v for k,v in m['entries'].items()}
with tempfile.TemporaryDirectory(prefix='ottrin-nsis-check-') as tmp:
 t=Path(tmp);subprocess.run(['7z','x','-y','-o'+tmp,str(s/'Ottrin-0.1.0-windows-x64-setup.exe')],check=True,stdout=subprocess.DEVNULL)
 p=t/'$INSTDIR.ottrin-setup/new';actual={str(f.relative_to(p)).replace('\\','/'):f for f in p.rglob('*') if f.is_file()};assert actual.keys()==expected.keys()|{'Uninstall.exe'},actual.keys()-expected.keys()
 for k,h in expected.items():assert sha(actual[k])==h,k
 author=(r/'packaging/windows/installer.nsi').read_text();assert 'WriteUninstaller "$INSTDIR.ottrin-setup\\new\\Uninstall.exe"' in author
 subprocess.run(['bash','scripts/windows-gui-subsystem-smoke.sh',str(actual['Uninstall.exe'])],check=True)
 assert sha(t/'$PLUGINSDIR/index-controller.exe')==expected['bin/ottrin-indexd.exe']
 assert (t/'$PLUGINSDIR/payload-transaction.ps1').read_bytes()==(r/'packaging/windows/payload-transaction.ps1').read_bytes()
 result={'status':'pass','entries_checked':len(expected),'source_revision':m['source_commit'],'nsis_sha256':sha(s/'Ottrin-0.1.0-windows-x64-setup.exe'),'generated_uninstaller':{'sha256':sha(actual['Uninstall.exe']),'declared_in_authoring':True,'gui_pe_header_verified':True},'controller_matches_indexd':True,'transaction_script_matches_source':True,'installer_authoring_sha256':sha(r/'packaging/windows/installer.nsi'),'extraction_removed':True}
(e/'NSIS_PAYLOAD.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
