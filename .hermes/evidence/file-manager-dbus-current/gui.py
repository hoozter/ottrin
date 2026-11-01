"""Real D-Bus activation and GUI requests on a private desktop; no host defaults."""
from pathlib import Path
import importlib.util,json,hashlib,os,time,urllib.parse,re
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'gui-accepted';assert not out.exists();b=r/'target/debug/ottrin-app';bus='org.freedesktop.FileManager1';obj='/org/freedesktop/FileManager1'
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
report={'binary_sha256':hashlib.sha256(b.read_bytes()).hexdigest(),'cases':[]}
def wait(test):
 end=time.monotonic()+25
 while not test():assert time.monotonic()<end,'D-Bus GUI readiness timeout';time.sleep(.05)
def close_window(d,w,properties):
 if properties:m.run(['xdotool','windowactivate','--sync',w,'key','Escape'],d.env)
 else:
  geometry=m.run(['xdotool','getwindowgeometry','--shell',w],d.env).stdout;values=dict(line.split('=',1) for line in geometry.splitlines() if '=' in line);x=int(values['WIDTH'])-22
  m.run(['xdotool','windowactivate','--sync',w,'mousemove','--window',w,str(x),'16'],d.env);time.sleep(.1);m.run(['xdotool','mousedown','1'],d.env);time.sleep(.06);m.run(['xdotool','mouseup','1'],d.env)
def uri(path):return 'file://'+urllib.parse.quote_from_bytes(os.fsencode(path),safe='/')
try:
 with m.OwnedDesktop(out) as d:
  fixture=d.profile/'fixture';fixture.mkdir();folder=fixture/os.fsdecode(b'folder\x80 space %');folder.mkdir();source=folder/'preserve.txt';source.write_bytes(b'FileManager1 must preserve this file.\n');link=folder/'broken-link';link.symlink_to('missing')
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.pop('restore_session',None);config.pop('last_session_dirs',None);config.update(theme='Dark',default_view_mode='List',bookmarks=[],installed_office_previews_enabled=False);config['search'].update(include_roots=[str(fixture)],content_indexing_enabled=False)
  cp=d.profile/'config/ottrin/config.json';cp.parent.mkdir();cp.write_text(json.dumps(config)+'\n');before=source.read_bytes()
  def cli(*args):return m.run([str(b),'folders',*args],d.env)
  def call(interface,*args,check=True):return m.run(['gdbus','call','--session','--dest',bus,'--object-path',obj,'--method',interface,*args],d.env,check)
  cli('enable');registration=d.profile/'data/dbus-1/services'/f'{bus}.service';assert registration.is_file()
  m.run(['gdbus','call','--session','--dest','org.freedesktop.DBus','--object-path','/org/freedesktop/DBus','--method','org.freedesktop.DBus.ReloadConfig'],d.env)
  def windows():return m.run(['xdotool','search','--onlyvisible','--name','^Ottrin'],d.env,False).stdout.split()
  def request(method,paths,title,keep_open=False):
   old=set(windows());result=call(bus+'.'+method,json.dumps([uri(p) for p in paths]),'');assert result.returncode==0
   found=[]
   def ready():found[:]=[w for w in windows() if w not in old];return bool(found)
   wait(ready);w=found[-1];pid=int(m.run(['xdotool','getwindowpid',w],d.env).stdout);assert (Path('/proc')/str(pid)/'exe').resolve()==b.resolve()
   argv=(Path('/proc')/str(pid)/'cmdline').read_bytes().split(b'\0');assert os.fsencode(paths[0]) in argv,argv
   time.sleep(.6);m.run(['import','-window',w,str(out/(title+'.png'))],d.env)
   if method!='ShowItemProperties':
    m.run(['xdotool','windowactivate','--sync',w,'key','ctrl+l'],d.env);time.sleep(.2);m.run(['import','-window',w,str(out/(title+'-location.png'))],d.env);m.run(['xdotool','key','Escape'],d.env)
   if not keep_open:
    close_window(d,w,method=='ShowItemProperties');wait(lambda:not (Path('/proc')/str(pid)).exists())
   report['cases'].append({'method':method,'native_argv_preserved':True,'window_pid':pid,'normal_close':not keep_open})
   return w,pid
  request('ShowItemProperties',[link,source],'properties')
  owner=int(re.search(r'uint32 (\d+)',m.run(['gdbus','call','--session','--dest','org.freedesktop.DBus','--object-path','/org/freedesktop/DBus','--method','org.freedesktop.DBus.GetConnectionUnixProcessID',bus],d.env).stdout).group(1))
  proc=Path('/proc')/str(owner);assert (proc/'comm').read_text().strip()=='ottrin-filemgr';assert (proc/'exe').resolve()==b.resolve()
  assert not list((d.profile/'runtime').rglob('index*.sock'))
  xml=m.run(['gdbus','introspect','--session','--dest',bus,'--object-path',obj,'--xml'],d.env).stdout;(out/'interface.xml').write_text(xml)
  import xml.etree.ElementTree as ET
  face=ET.fromstring(xml).find("interface[@name='org.freedesktop.FileManager1']");assert face is not None
  for method in ['ShowFolders','ShowItems','ShowItemProperties']:assert [x.attrib['type'] for x in face.find("method[@name='"+method+"']").findall('arg')]==['as','s']
  prior=set(windows());bad=call(bus+'.ShowItems',json.dumps([uri(source),'https://example.com/a']),'',check=False);assert bad.returncode!=0;assert set(windows())==prior
  conflict=m.run([str(b),'file-manager'],d.env,False);assert conflict.returncode!=0;assert proc.exists()
  def ticks():return sum(map(int,(proc/'stat').read_text().split(') ')[1].split()[11:13]))
  first=ticks();time.sleep(5);last=ticks();report['backend_smaps_rollup']={line.split(':')[0]:line.split(':',1)[1].strip() for line in (proc/'smaps_rollup').read_text().splitlines() if line.startswith(('Rss:','Pss:','Private_Clean:','Private_Dirty:'))};report['backend_memory_kib']={line.split(':')[0]:line.split(':')[1].strip() for line in (proc/'status').read_text().splitlines() if line.startswith(('VmRSS:','VmSize:'))};report['idle']={'duration_seconds':5,'cpu_ticks':last-first,'clock_ticks_per_second':os.sysconf('SC_CLK_TCK')};assert last-first<=2
  request('ShowFolders',[folder],'folder');request('ShowItems',[link],'reveal-link')
  # Repair stops only our activation backend; next request starts the current image.
  active,active_pid=request('ShowItemProperties',[source],'kept-open',keep_open=True)
  cli('enable');wait(lambda:not proc.exists());assert (Path('/proc')/str(active_pid)).exists();assert active in windows()
  close_window(d,active,True);wait(lambda:not (Path('/proc')/str(active_pid)).exists());report['active_window_survived_backend_restart']=True
  request('ShowItemProperties',[source],'reactivated')
  cli('disable');assert not registration.exists();assert source.read_bytes()==before;assert not (d.profile/'state/ottrin/folder-registration.json').exists();assert "thread 'main'" not in (out/'dbus.log').read_text();report.update(status='pass',source_preserved=True,registration_removed=True,foreign_owner_not_replaced=True)
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None;(e/'GUI_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
