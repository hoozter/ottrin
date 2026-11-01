"""Actual GIO default-directory dispatch; all preferences, data and UI are private."""
from pathlib import Path
import importlib.util,json,hashlib,os,time,urllib.parse,argparse
r=Path.cwd();e=Path(__file__).resolve().parent;parser=argparse.ArgumentParser();parser.add_argument('--case',choices=['initial','final','index','index-fixed','query-control'],default='initial');case=parser.parse_args().case;out=e/('gui-'+case if case!='initial' else 'gui');assert not out.exists();binary=r/'target/debug/ottrin-app'
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');dsk=importlib.util.module_from_spec(spec);spec.loader.exec_module(dsk)
report={'scope':'Source GIO default-handler resolution,native URI-to-argv dispatch,actual Services UI and reversible preferences in owned X11','binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest()}
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def wait(test):
 deadline=time.monotonic()+20
 while not test():assert time.monotonic()<deadline,'Owned folder integration readiness timeout';time.sleep(.05)
try:
 with dsk.OwnedDesktop(out) as d:
  fixture=d.profile/'fixture';fixture.mkdir();folder=fixture/os.fsdecode(b'source\x80 100% & `literal`');folder.mkdir();source=folder/'note.txt';source.write_bytes(b'preserve directory handler fixture\n');initial=sha(source)
  appconfig=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];appconfig.update(theme='Dark',default_view_mode='List',restore_session=False,last_session_dirs=[],installed_office_previews_enabled=False);appconfig['bookmarks']=[];appconfig['search'].update(include_roots=[str(fixture)],content_indexing_enabled=False)
  config=d.profile/'config/ottrin/config.json';config.parent.mkdir();config.write_text(json.dumps(appconfig)+'\n')
  data=d.profile/'data/applications';data.mkdir(parents=True);previous=data/'org.example.Previous.desktop';previous.write_text('[Desktop Entry]\nType=Application\nName=Previous fixture handler\nExec=/usr/bin/true %f\nNoDisplay=true\nMimeType=inode/directory;\n');previous_sha=sha(previous)
  mime=d.profile/'config/mimeapps.list';original='[Default Applications]\ninode/directory=org.example.Previous.desktop;\napplication/pdf=reader.desktop;\n';mime.write_text(original)
  def cli(*args):return dsk.run([str(binary),'folders',*args],d.env).stdout
  def default():return dsk.run(['/usr/bin/python3','-c',"from gi.repository import Gio; a=Gio.AppInfo.get_default_for_type('inode/directory',False);print(a.get_id() if a else '')"],d.env).stdout.strip()
  assert default()=='org.example.Previous.desktop'
  assert not json.loads(cli('status','--json'))['enabled'];cli('enable');status=json.loads(cli('status','--json'));assert status['enabled'] and status['ready'],status
  assert default()=='org.ottrin.Ottrin.Folders.desktop';report['enabled_status']=status
  uri='file://'+urllib.parse.quote_from_bytes(os.fsencode(folder),safe='/');launch=d.spawn(['gio','open',uri],'launcher.log');windows=[]
  def found():windows[:]=dsk.run(['xdotool','search','--onlyvisible','--name','^Ottrin$'],d.env,False).stdout.split();return bool(windows)
  wait(found);window=windows[-1];pid=int(dsk.run(['xdotool','getwindowpid',window],d.env).stdout.strip());proc=Path('/proc')/str(pid);assert (proc/'exe').resolve()==binary.resolve();arguments=(proc/'cmdline').read_bytes().split(b'\0');assert os.fsencode(folder) in arguments and b'--' in arguments,arguments
  launch.wait(timeout=5);assert launch.returncode==0;time.sleep(.5);dsk.run(['import','-window',window,str(out/'native-folder.png')],d.env)
  if case in ['index','index-fixed','query-control']:
   import socket,struct
   samples=[]
   for delay in [.1,1,3,5]:
    time.sleep(delay);path=d.profile/'runtime/ottrin/index-v1.sock';sample={'delay':delay,'socket_exists':path.exists()}
    if path.exists():
     try:
      with socket.socket(socket.AF_UNIX) as connection:
       connection.settimeout(3);connection.connect(str(path));payload=json.dumps({'version':1,'request':{'type':'status'}}).encode();connection.sendall(struct.pack('>I',len(payload))+payload);header=connection.recv(4);sample['header_hex']=header.hex()
       if len(header)==4:
        size=struct.unpack('>I',header)[0];body=b''
        while len(body)<size:
         block=connection.recv(size-len(body));assert block,'Truncated status payload';body+=block
        sample['response']=json.loads(body)
     except Exception as error:sample['error']=repr(error)
    samples.append(sample)
   report['index_status_samples']=samples
   if case=='index-fixed':
    assert samples[-1].get('response',{}).get('response',{}).get('diagnostics',{}).get('status')=='Ready',samples
    assert all(sample.get('header_hex')!='' for sample in samples if sample.get('socket_exists')),samples
  if case=='query-control':
   import socket,struct
   with socket.socket(socket.AF_UNIX) as connection:
    connection.settimeout(5);connection.connect(str(d.profile/'runtime/ottrin/index-v1.sock'))
    payload=json.dumps({'version':1,'request':{'type':'query','query':{'text':'note','scope':'Global','root_path':None,'include_hidden_system':False,'sort':'Name','limit':10,'offset':0,'content_search':False}}}).encode()
    connection.sendall(struct.pack('>I',len(payload))+payload);header=connection.recv(4);report['native_query_header_hex']=header.hex()
    if len(header)==4:
     size=struct.unpack('>I',header)[0];body=b''
     while len(body)<size:
      part=connection.recv(size-len(body));assert part;body+=part
     report['native_query_response']=json.loads(body)
  dsk.run(['xdotool','windowactivate','--sync',window,'mousemove','--window',window,'1074','786','click','1'],d.env);time.sleep(.5)
  settings=dsk.run(['xdotool','search','--onlyvisible','--name','^Ottrin Settings$'],d.env).stdout.split()[-1];dsk.run(['import','-window',settings,str(out/'services.png')],d.env)
  if case=='final':
   dsk.run(['xdotool','mousemove','--window',settings,'800','540','click','--repeat','14','--delay','30','5'],d.env);time.sleep(.3);dsk.run(['import','-window',settings,str(out/'desktop-integration.png')],d.env)
  dsk.run(['xdotool','mousemove','--window',settings,'898','15','click','1'],d.env);time.sleep(.2);dsk.run(['xdotool','windowactivate','--sync',window,'mousemove','--window',window,'1072','18','click','1'],d.env);wait(lambda:not proc.exists())
  mime.write_text(mime.read_text().replace('reader.desktop','new-reader.desktop'));cli('enable');assert json.loads(cli('status','--json'))['ready'];cli('disable');off=json.loads(cli('status','--json'));assert not off['enabled'] and not off['ready'];assert default()=='org.example.Previous.desktop';assert 'application/pdf=new-reader.desktop;' in mime.read_text();assert sha(previous)==previous_sha;assert sha(source)==initial
  assert not (data/'org.ottrin.Ottrin.Folders.desktop').exists();assert not (d.profile/'data/ottrin/integration/icon.png').exists();assert not (d.profile/'state/ottrin/folder-registration.json').exists()
  report.update(status='pass',actual_default_dispatch=True,native_folder_argv_hex=os.fsencode(folder).hex(),original_file_preserved=True,previous_handler_restored=True,unrelated_pdf_change_preserved=True,foreign_desktop_entry_preserved=True,managed_assets_removed=True,off_status=off,main_closed_via_visible_button=True)
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None;(out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
