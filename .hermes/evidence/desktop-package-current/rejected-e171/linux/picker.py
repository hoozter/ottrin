"""Execute the shared-contract Rust example against actual packaged chooser windows."""
from pathlib import Path
import importlib.util,json,hashlib,time,os
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'picker';assert not out.exists();binary=Path(json.loads((e/'BUILD_SOURCE.json').read_text())['stage'])/'Ottrin-0.1.0-x86_64.AppImage';caller=r/'target/debug/examples/picker_client'
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');dsk=importlib.util.module_from_spec(spec);spec.loader.exec_module(dsk)
report={'scope':'Actual native-byte Rust cooperating example/packaged chooser; Save/folder/cancel and abrupt caller loss','caller_sha256':hashlib.sha256(caller.read_bytes()).hexdigest(),'app_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'cases':[]}
def wait(predicate):
 deadline=time.monotonic()+20
 while not predicate():assert time.monotonic()<deadline,'Example caller QA readiness timeout';time.sleep(.05)
try:
 with dsk.OwnedDesktop(out) as d:
  token='ottrin-owned-picker_TIME0';d.env['DESKTOP_STARTUP_ID']=token
  folder=d.profile/os.fsdecode(b'caller\x80');folder.mkdir();original=folder/'original.txt';original.write_bytes(b'preserve cooperating example fixture\n');before=original.read_bytes()
  for case in ['save','folder','cancel','caller-loss']:
   mode='save' if case=='save' else 'folder' if case=='folder' else 'open';title={'save':'Save file','folder':'Choose folder','open':'Open file'}[mode]
   process=d.spawn([str(caller),str(binary),mode,str(folder),*(['chosen.txt'] if mode=='save' else [])],case+'.log',pipe=True);windows=[]
   def ready():
    assert process.poll() is None,(out/(case+'.log')).read_text();windows[:]=dsk.run(['xdotool','search','--onlyvisible','--name','^Ottrin.*'+title+'$'],d.env,False).stdout.split();return bool(windows)
   wait(ready);window=windows[-1];assert token in dsk.run(['xprop','-id',window,'_NET_STARTUP_ID'],d.env).stdout;pid=int(dsk.run(['xdotool','getwindowpid',window],d.env).stdout);time.sleep(.3);dsk.run(['import','-window',window,str(out/(case+'.png'))],d.env)
   if case=='caller-loss':
    process.kill();process.wait(timeout=5);wait(lambda:not Path('/proc',str(pid)).exists());report['cases'].append({'case':case,'caller_exit':process.returncode,'owned_chooser_exited':True});continue
   if case=='cancel':dsk.run(['xdotool','windowactivate','--sync',window,'key','Escape'],d.env)
   else:dsk.run(['xdotool','windowactivate','--sync',window,'mousemove','--window',window,'900','606','click','1'],d.env)
   process.wait(timeout=15);raw=process.stdout.read();assert process.returncode==0,((out/(case+'.log')).read_text(),raw);response=json.loads(raw);assert response['accepted']==(case!='cancel'),response
   if response['accepted']:
    expected=folder/'chosen.txt' if case=='save' else folder;assert response['paths']==[{'encoding':'unix-bytes','units_hex':os.fsencode(expected).hex()}],response
   assert not (folder/'chosen.txt').exists();report['cases'].append({'case':case,'exit':process.returncode,'response':response})
  assert original.read_bytes()==before;assert not (d.profile/'runtime/ottrin/index-v1.sock').exists();assert not (d.profile/'config/ottrin/config.json').exists();report.update(startup_property_matches_request=True,status='pass',original_preserved=True,chooser_created_no_file=True,no_main_config_written=True,no_index_started=True)
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None;(out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
