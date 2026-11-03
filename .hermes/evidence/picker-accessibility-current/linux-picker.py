"""Exact staged AppImage field names through the native AT-SPI provider."""
from pathlib import Path
import hashlib,importlib.util,json,os,subprocess,time
r=Path.cwd();e=Path(__file__).resolve().parent
build=json.loads((e/'BUILD_SOURCE.json').read_text());image=Path(build['stage'])/'Ottrin-0.1.0-x86_64.AppImage'
accepted=json.loads((e/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text());digest=hashlib.sha256(image.read_bytes()).hexdigest();assert accepted['artifacts'][image.name]==digest
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
probe=r'''import json,sys,time,pyatspi
pid=int(sys.argv[1]);expected=json.loads(sys.argv[2]);deadline=time.monotonic()+10
while True:
 fields=[]
 for app in pyatspi.Registry.getDesktop(0):
  if app is None or app.get_process_id()!=pid:continue
  todo=[app]
  while todo:
   n=todo.pop()
   if n.getRoleName()=='entry':
    text=n.queryText();fields.append({'name':n.name,'value':text.getText(0,-1)})
   todo.extend(child for child in n if child is not None)
 names=[f['name'] for f in fields]
 if all(names.count(n)==1 for n in expected) and len(fields)==len(expected):
  assert all(names),fields;print(json.dumps(fields));break
 assert time.monotonic()<deadline,fields
 time.sleep(.1)
'''
for theme,scale in [('Dark','1'),('Light','1.5')]:
 out=e/('linux-picker-'+theme.lower());assert not out.exists();report={'status':'running','source_revision':build['source_revision'],'artifact_sha256':digest,'theme':theme,'scale':scale,'cases':[]}
 try:
  with desktop.OwnedDesktop(out,scale=scale) as d:
   d.env.pop('NO_AT_BRIDGE',None)
   desktop.run(['gsettings','set','org.gnome.desktop.interface','toolkit-accessibility','true'],d.env)
   folder=d.profile/'sources';folder.mkdir();source=folder/'alpha.txt';source.write_text('owned source bytes');original=source.read_bytes()
   config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(theme=theme,bookmarks=[]);pref=d.profile/'config/ottrin/config.json';pref.parent.mkdir(parents=True);pref.write_text(json.dumps(config)+'\n');before=pref.read_bytes()
   for mode in ['open','save','folder']:
    response=out/(mode+'-response.json');app=d.spawn([str(image),'picker'],mode+'.log',stdin=True,output_name=response.name)
    request={'mode':mode,'title':'Native named fields '+mode,'initial_directory':{'encoding':'unix-bytes','units_hex':os.fsencode(folder).hex()},'filename':{'encoding':'unix-bytes','units_hex':b'suggested.txt'.hex()}}
    app.stdin.write(json.dumps(request));app.stdin.close();deadline=time.monotonic()+25
    while True:
     assert app.poll() is None
     windows=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','Native named fields'],d.env,False).stdout.split()
     if windows:break
     assert time.monotonic()<deadline;time.sleep(.1)
    window=windows[-1];desktop.run(['xdotool','windowactivate','--sync',window],d.env)
    expected=['Filter this folder']+([] if mode=='folder' else ['File name'])
    def fields(names):
     result=desktop.run(['/usr/bin/python3','-c',probe,str(app.pid),json.dumps(names)],d.env)
     return json.loads(result.stdout)
    initial=fields(expected)
    if mode=='save':assert next(f['value'] for f in initial if f['name']=='File name')=='suggested.txt'
    desktop.run(['xdotool','key','ctrl+l'],d.env);location=fields(expected+['Location']);assert next(f['value'] for f in location if f['name']=='Location')==str(folder)
    desktop.run(['import','-window',window,str(out/(mode+'-location.png'))],d.env)
    desktop.run(['xdotool','key','Escape'],d.env);time.sleep(.2);desktop.run(['xdotool','key','Escape'],d.env);assert app.wait(timeout=15)==0
    result=json.loads(response.read_text());assert not result['accepted'] and not result['paths'];assert pref.read_bytes()==before and source.read_bytes()==original;assert not (d.profile/'runtime/ottrin/index-v1.sock').exists()
    report['cases'].append({'mode':mode,'initial_fields':initial,'location_fields':location,'response':result,'normal_exit':app.returncode,'preferences_and_source_unchanged':True,'no_index_socket':True})
   report['status']='pass native AT-SPI field names, roles, values and cancellation; captures require review'
 except BaseException as error:report.update(status='failed',error=repr(error));raise
 finally:
  report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
  (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
