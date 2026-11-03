"""Actual packaged main/chooser directory-link navigation using owned synthetic files."""
from pathlib import Path
import hashlib,importlib.util,json,time,subprocess,io,csv
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'symlink-gui-final';assert not out.exists()
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
build=json.loads((e/'BUILD_RESULT.json').read_text());image=Path(build['stage'])/'Ottrin-0.1.0-x86_64.AppImage'
with image.open('rb') as f:digest=hashlib.file_digest(f,'sha256').hexdigest()
assert digest==build['artifacts'][image.name]
report={'status':'observing','source_revision':build['source_revision'],'artifact_sha256':digest,'scope':'Actual AppImage main List and chooser directory/file/broken symlink rendering and directory navigation, private native X11 software desktop'}
try:
 with desktop.OwnedDesktop(out) as d:
  fixture=d.profile/'fixture';fixture.mkdir();source=fixture/'source';source.mkdir();target=fixture/'target';target.mkdir();payload=b'Directory link chooser source remains unchanged.\n';(target/'InsideProof.txt').write_bytes(payload);(source/'DirShortcut').symlink_to('../target',target_is_directory=True);(source/'FileShortcut.txt').symlink_to('../target/InsideProof.txt');(source/'BrokenShortcut').symlink_to('../missing')
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(theme='Dark',default_view_mode='List',bookmarks=[],installed_office_previews_enabled=False);config['search'].update(include_roots=[str(fixture)],content_indexing_enabled=False)
  cp=d.profile/'config/ottrin/config.json';cp.parent.mkdir(parents=True);cp.write_text(json.dumps(config)+'\n')
  def run(args,check=True):return desktop.run(args,d.env,check)
  def window(app,pattern):
   until=time.monotonic()+20
   while time.monotonic()<until:
    assert app.poll() is None
    ids=run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name',pattern],False).stdout.split()
    if ids:run(['xdotool','windowactivate','--sync',ids[-1]]);time.sleep(.8);return ids[-1]
    time.sleep(.1)
   raise AssertionError('Owned app window unavailable')
  def capture(w,name):
   path=out/(name+'.png');run(['import','-window',w,str(path)]);return path
  def find(w,name,shot):
   path=capture(w,shot);text=subprocess.check_output(['tesseract',str(path),'stdout','tsv'],text=True,stderr=subprocess.DEVNULL,timeout=20);rows=list(csv.DictReader(io.StringIO(text),delimiter='\t'));matches=[x for x in rows if x.get('text','').strip().casefold()==name.casefold()];assert matches,(name,[x.get('text') for x in rows if x.get('text','').strip()]);a=matches[0];return int(a['left'])+int(a['width'])//2,int(a['top'])+int(a['height'])//2
  def double(w,xy):
   run(['xdotool','windowactivate','--sync',w,'mousemove','--window',w,str(xy[0]),str(xy[1])]);time.sleep(.2);run(['xdotool','click','--repeat','2','--delay','100','1']);time.sleep(.7)
  app=d.spawn([str(image),str(source),'--renderer','opengl'],'main.log');w=window(app,'^Ottrin$');xy=find(w,'DirShortcut','main-links');double(w,xy);find(w,'InsideProof.txt','main-directory-link-open');report['main_directory_link_navigates']=True
  geometry=dict(line.split('=',1) for line in run(['xdotool','getwindowgeometry','--shell',w]).stdout.splitlines() if '=' in line);run(['xdotool','windowactivate','--sync',w,'mousemove','--window',w,str(int(geometry['WIDTH'])-22),'18']);time.sleep(.2);run(['xdotool','click','1']);assert app.wait(timeout=15)==0
  chooser=d.spawn([str(image),'picker'],'picker.log',stdin=True,output_name='picker-response.json');chooser.stdin.write(json.dumps({'mode':'open','title':'Link navigation proof','initial_directory':{'encoding':'unix-bytes','units_hex':str(source).encode().hex()}}));chooser.stdin.close();w=window(chooser,'Link navigation proof');xy=find(w,'DirShortcut','chooser-links');double(w,xy);assert chooser.poll() is None,'Chooser accepted a directory link as a file';xy=find(w,'InsideProof.txt','chooser-directory-link-open');double(w,xy);assert chooser.wait(timeout=15)==0
  response=json.loads((out/'picker-response.json').read_text());report['picker_response']=response
  assert response['accepted'] is True,response
  selected=response['paths'][0];assert selected['encoding']=='unix-bytes';selected_path=Path(bytes.fromhex(selected['units_hex']).decode());assert selected_path.resolve()==(target/'InsideProof.txt').resolve();assert selected_path.read_bytes()==payload;report['chooser_directory_link_navigates_and_selects_child']=True
  assert (target/'InsideProof.txt').read_bytes()==payload;assert (source/'DirShortcut').is_symlink() and (source/'FileShortcut.txt').is_symlink() and (source/'BrokenShortcut').is_symlink();assert not (source/'BrokenShortcut').exists();report['fixtures_preserved']=True;report['status']='pass'
except BaseException as error:
 report.update(status='failed',error=repr(error));raise
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
