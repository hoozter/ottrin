"""Observe the actual corrected AppImage spreadsheet fallback, with owned teardown."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,shutil,subprocess,time,tarfile,tempfile,signal
r=Path.cwd();e=Path(__file__).resolve().parent
parser=argparse.ArgumentParser();parser.add_argument('--portable',type=Path);args=parser.parse_args()
def sha(p):
 with p.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def cancelled(signum,frame):raise SystemExit(128+signum)
for signum in [signal.SIGTERM,signal.SIGINT]:signal.signal(signum,cancelled)
b=json.loads((e/('BUILD_SOURCE.json' if args.portable else 'BUILD_RESULT.json')).read_text());stage=Path(b['stage'])
spec=importlib.util.spec_from_file_location('owned_desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
fixture_source=r/'crates/ottrin-preview/tests/fixtures/office-fidelity.xlsx';source_hash=sha(fixture_source);binary_hash=sha(r/'target/release/ottrin-app')
def observe(image,kind,artifact_hash):
 for theme,scale in [('Dark','1'),('Light','1.5')]:
  out=e/('linux-'+kind+'-'+theme.lower());assert not out.exists();result={'status':'running','source_revision':b['source_revision'],'artifact_sha256':artifact_hash,'artifact_kind':kind,'scope':'Actual packaged native spreadsheet fallback, main Miller and Inspector; private X11 software desktop','theme':theme,'scale':scale,'captures':{}}
  try:
   with desktop.OwnedDesktop(out/'desktop',scale=scale) as d:
    folder=d.profile/'files/owned/documents';folder.mkdir(parents=True);file=folder/'worksheet.xlsx';shutil.copy2(fixture_source,file)
    config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(theme=theme,default_view_mode='Miller',window_size=[1000,720],bookmarks=[],installed_office_previews_enabled=False);config['search'].update(include_roots=[str(folder)],content_indexing_enabled=False,refresh_interval_hours=0)
    cp=d.profile/'config/ottrin/config.json';cp.parent.mkdir(parents=True);cp.write_text(json.dumps(config)+'\n')
    for surface,arguments in [('main',['--select',str(file)]),('inspector',['preview',str(file)])]:
     app=d.spawn([str(image),*arguments,'--renderer','opengl'],surface+'.log');deadline=time.monotonic()+30;window=None
     while time.monotonic()<deadline:
      assert app.poll() is None,'AppImage exited before rendering'
      q=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid)],d.env,False)
      if q.returncode==0:
       for candidate in q.stdout.split():
        title=desktop.run(['xdotool','getwindowname',candidate],d.env).stdout.strip()
        if (surface=='main' and title=='Ottrin') or (surface=='inspector' and title.endswith('worksheet.xlsx')):window=candidate;break
      if window:
       shot=out/(surface+'.png');desktop.run(['import','-window',window,str(shot)],d.env);text=subprocess.check_output(['tesseract',str(shot),'stdout','--psm','11'],text=True,stderr=subprocess.DEVNULL)
       if 'FIDELITY' in text and 'Budget' in text:break
      time.sleep(.2)
     else:raise AssertionError(('Worksheet fallback did not paint',surface))
     assert sha(Path('/proc')/str(app.pid)/'exe')==binary_hash,'Mapped GUI differs from built package input'
     time.sleep(.5);desktop.run(['import','-window',window,str(shot)],d.env)
     result['captures'][surface]={'path':str(shot.relative_to(r)),'sha256':sha(shot),'mapped_gui_sha256':binary_hash,'ocr':text}
     desktop.run(['xdotool','windowactivate','--sync',window,'key','--clearmodifiers','alt+F4'],d.env);assert app.wait(timeout=15)==0
     assert sha(file)==source_hash,'Read-only native worksheet preview changed source'
    result.update(status='pass native capture and read-only checks; direct typography review required',source_bytes_preserved=True)
  finally:
   result['teardown']=json.loads((out/'desktop/TEARDOWN.json').read_text()) if (out/'desktop/TEARDOWN.json').exists() else None
   (out/'RESULT.json').write_text(json.dumps(result,indent=2)+'\n')

if args.portable:
 archive=args.portable.resolve();assert archive.is_relative_to(stage) and archive.name=='Ottrin-0.1.0-linux-x86_64-portable.tar.gz'
 digest=sha(archive);assert archive.with_name(archive.name+'.sha256').read_text().split()[0]==digest
 with tempfile.TemporaryDirectory(prefix='.worksheet-portable-',dir=r/'.hermes/packaging-tmp') as temporary:
  temporary=Path(temporary)
  with tarfile.open(archive) as bundle:bundle.extractall(temporary,filter='data')
  root=temporary/'Ottrin-0.1.0-linux-x86_64';notes=(root/'SOURCE_REVISION.txt').read_text();assert 'Git revision: '+b['source_revision'] in notes and 'Working tree: clean' in notes
  assert sha(root/'libexec/ottrin/ottrin')==binary_hash
  observe(root/'bin/ottrin','portable',digest)
 assert not temporary.exists()
 (e/'PORTABLE_EXTRACTION_CLEANUP.json').write_text(json.dumps({'path':str(temporary),'removed':True,'artifact_sha256':digest})+'\n')
else:
 image=stage/'Ottrin-0.1.0-x86_64.AppImage';assert b['status'].startswith('four-format build pass') and sha(image)==b['artifacts'][image.name]
 observe(image,'appimage',sha(image))
