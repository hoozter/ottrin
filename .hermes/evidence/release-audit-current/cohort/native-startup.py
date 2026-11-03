from pathlib import Path
import importlib.util,json,time,subprocess,hashlib,sys,csv,io,os
r=Path.cwd();e=Path(__file__).resolve().parent;theme=sys.argv[1];scale=sys.argv[2];case=e/('startup-'+theme.lower()+'-'+scale);case.mkdir(exist_ok=True)
build=json.loads((e/'BUILD_RESULT.json').read_text());content=json.loads((e/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text());b=Path(build['stage'])/'Ottrin-0.1.0-x86_64.AppImage';assert build['source_revision']==content['source_revision'];digest=hashlib.file_digest(b.open('rb'),'sha256').hexdigest();assert digest==build['artifacts'][b.name]
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
report={'status':'running','artifact_sha256':digest,'source_revision':build['source_revision'],'theme':theme,'desktop_scale':scale,'launch_arguments':[],'scope':'Exact staged AppImage default renderer and no-location startup; current Home contents and normal native Close on private X11/software desktop'}
def save():(case/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
save()
try:
 with m.OwnedDesktop(case,scale=scale) as d:
  for key in ['NO_AT_BRIDGE','OTTRIN_RENDERER','OTTRIN_PDFIUM_DIR','OTTRIN_OFFICE_HELPER','LD_LIBRARY_PATH']:d.env.pop(key,None)
  home=d.profile/'home';(home/'Documents').mkdir(parents=True);proof=home/'Documents/proof.txt';proof.write_text('Private startup source must remain unchanged.\n');original=proof.read_bytes()
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(default_view_mode='Miller',theme=theme,bookmarks=[],graphics_renderer='Automatic');config['search'].update(include_roots=[str(home)],content_indexing_enabled=False);p=d.profile/'config/ottrin/config.json';p.parent.mkdir(parents=True);p.write_text(json.dumps(config))
  subprocess.run(['gdbus','call','--session','--dest','org.a11y.Bus','--object-path','/org/a11y/bus','--method','org.freedesktop.DBus.Properties.Set','org.a11y.Status','IsEnabled','<true>'],env=d.env,check=True,capture_output=True,timeout=15)
  app=d.spawn([str(b)],'app.log');deadline=time.monotonic()+30;w=None
  while time.monotonic()<deadline:
   q=subprocess.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin$'],env=d.env,capture_output=True,text=True,timeout=10)
   if q.returncode==0:w=q.stdout.split()[-1];break
   assert app.poll() is None;time.sleep(.1)
  assert w;time.sleep(1);report['gui_binary_sha256']=hashlib.file_digest((Path('/proc')/str(app.pid)/'exe').open('rb'),'sha256').hexdigest();expected=Path(content['extraction'])/'squashfs-root/usr/bin/ottrin';assert report['gui_binary_sha256']==hashlib.file_digest(expected.open('rb'),'sha256').hexdigest()
  shot=case/'startup.png';subprocess.run(['import','-window',w,str(shot)],env=d.env,check=True,timeout=15)
  tsv=subprocess.check_output(['tesseract',str(shot),'stdout','--psm','11','tsv'],stderr=subprocess.DEVNULL,text=True,timeout=15);words=list(csv.DictReader(io.StringIO(tsv),delimiter='\t'));matches=[x for x in words if x['text'].casefold()=='documents'];assert len(matches)==1,'Current Home contents are not visibly readable in actual startup capture';report['visible_documents_word']={k:matches[0][k] for k in ['left','top','width','height','conf']};save()
  code="import importlib.util,sys;spec=importlib.util.spec_from_file_location('qa',sys.argv[1]);q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q);q.target_process_id=int(sys.argv[2]);q.click_accessible_name('Close window','push button')"
  closer=d.spawn(['/usr/bin/python3','-c',code,str(r/'scripts/linux-desktop-qa.py'),str(app.pid)],'close.log');assert closer.wait(timeout=20)==0;assert app.wait(timeout=15)==0;assert proof.read_bytes()==original;report.update(status='pass within staged default startup/native Close scope; visual review remains required',normal_exit=0,source_unchanged=True,default_renderer=True)
except BaseException as error:report.update(status='failed staged native startup',error=repr(error));raise
finally:
 if (case/'TEARDOWN.json').exists():report['teardown']=json.loads((case/'TEARDOWN.json').read_text())
 save()
