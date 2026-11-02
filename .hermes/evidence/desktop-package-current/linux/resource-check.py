"""Finite exact-AppImage cancel/repeat/concurrent and quiet-idle observations."""
from pathlib import Path
import hashlib,importlib.util,json,os,time
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'resources';assert not out.exists()
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
build=json.loads((e/'BUILD_SOURCE.json').read_text());content=json.loads((e/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text());image=Path(build['stage'])/'Ottrin-0.1.0-x86_64.AppImage'
with image.open('rb') as f:digest=hashlib.file_digest(f,'sha256').hexdigest()
assert digest==content['artifacts'][image.name]
report={'artifact_sha256':digest,'source_revision':build['source_revision'],'scope':'Actual AppImage; private X11/software rendering; window mapping timings with warm shared OS caches, not whole-machine cold-cache claims'}
try:
 with desktop.OwnedDesktop(out) as d:
  fixture=d.profile/'fixture';fixture.mkdir();original=fixture/'original.txt';original.write_bytes(b'preserve original\n')
  request={'mode':'open','initial_directory':{'encoding':'unix-bytes','units_hex':os.fsencode(fixture).hex()}}
  def launch(name):
   started=time.monotonic();p=d.spawn([str(image),'picker'],name+'.stderr',stdin=True,output_name=name+'.response.json');p.stdin.write(json.dumps(request));p.stdin.close();deadline=time.monotonic()+15
   while True:
    assert p.poll() is None,'AppImage exited before its chooser mapped'
    windows=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(p.pid),'--name','Ottrin'],d.env,False).stdout.strip().splitlines()
    if windows:break
    assert time.monotonic()<deadline;time.sleep(.05)
   return p,windows[-1],{'name':name,'window_mapped_ms':round((time.monotonic()-started)*1000,2)}
  def cancel(p,w,name):
   desktop.run(['xdotool','windowactivate','--sync',w],d.env);desktop.run(['xdotool','key','--clearmodifiers','Escape'],d.env);p.wait(timeout=5);assert p.returncode==0
   response=json.loads((out/(name+'.response.json')).read_text());assert not response['accepted'] and not response['paths'];return response
  def sample(p):
   values=Path(f'/proc/{p.pid}/stat').read_text().rsplit(') ',1)[1].split();return {'time':time.monotonic(),'ticks':int(values[11])+int(values[12]),'rss_kib':int(Path(f'/proc/{p.pid}/status').read_text().split('VmRSS:')[1].split()[0])}
  observations=[]
  for name in ['first','repeat']:
   p,w,data=launch(name);time.sleep(2)
   if name=='first':
    first=sample(p);time.sleep(10);last=sample(p);data['quiet_idle']={'seconds':last['time']-first['time'],'cpu_percent_one_core':100*(last['ticks']-first['ticks'])/os.sysconf('SC_CLK_TCK')/(last['time']-first['time']),'rss_kib':last['rss_kib']};assert data['quiet_idle']['cpu_percent_one_core']<2,'Investigate sustained chooser CPU before delivery'
    desktop.run(['import','-window',w,str(out/'first.png')],d.env)
    icon=desktop.run(['xprop','-id',w,'_NET_WM_ICON'],d.env).stdout;assert 'not found' not in icon;data['window_icon_present']=True
   data['cancel_response']=cancel(p,w,name);observations.append(data)
  a=launch('concurrent-a');b=launch('concurrent-b');assert a[0].pid!=b[0].pid and a[1]!=b[1]
  for p,w,data in [a,b]:data['cancel_response']=cancel(p,w,data['name']);observations.append(data)
  assert original.read_bytes()==b'preserve original\n';assert not (d.profile/'runtime/ottrin/index-v1.sock').exists()
  report.update(status='pass',observations=observations,fixture_bytes_unchanged=True,index_not_started=True)
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
