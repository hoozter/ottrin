"""Owned real desktop: observe cold startup, then compare exact Grid GUI bytes."""
from pathlib import Path
import hashlib,json,os,pwd,secrets,shutil,subprocess,sys,tarfile,time,traceback
base=Path('/home/ottrinqa');out=base/'results';out.mkdir()
assert os.geteuid()==0 and os.uname().nodename=='ottrin-native-ubuntu-desktop-qa'
account=pwd.getpwnam('ottrinqa');uid=account.pw_uid
report={'status':'running','package_source':sys.argv[1]}
try:
 assert 'Git revision: '+sys.argv[1] in Path('/usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text()
 source=json.loads((base/'GUI_INPUT.json').read_text())
 assert hashlib.sha256((base/'grid-ottrin-app').read_bytes()).hexdigest()==source['gui_sha256']
 (base/'grid-ottrin-app').chmod(0o755)
 (base/'ottrin-indexd').symlink_to('/usr/bin/ottrin-indexd')
 subprocess.run(['chpasswd'],input='ottrinqa:'+secrets.token_urlsafe(24)+'\n',text=True,check=True)
 fixture=base/'fixtures';fixture.mkdir()
 for i in range(1000):(fixture/f'owned-{i:04}.txt').write_text(f'owned{i}\n')
 (fixture/'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW.txt').write_text('wide glyphs\n')
 (fixture/'iiiiiiiiiiiiiiiiiiii.txt').write_text('narrow glyphs\n')
 pref=base/'.config/ottrin/config.json';pref.parent.mkdir(parents=True,exist_ok=True)
 pref.write_text(json.dumps({'theme':'Dark','list_columns':['Name','Kind','Size','Modified'],'command_frame_always_visible':False,'show_hidden_files':False,'target':{'current':None,'pinned':[],'has_ever_set':False,'recent':[]},'default_view_mode':'Miller','search_setup_completed':True,'window_size':[1100,800],'search':{'include_roots':[str(fixture)],'content_indexing_enabled':False}}))
 for p in [base,out,fixture,*fixture.iterdir(),pref,*pref.parents]:
  if p==base or base in p.parents:os.chown(p,uid,account.pw_gid)
 subprocess.run(['systemctl','stop','lightdm'],check=True)
 Path('/usr/share/xsessions/ottrin-qa.desktop').write_text('[Desktop Entry]\nName=Owned Ottrin UI QA\nType=Application\nExec=/usr/bin/env LIBGL_ALWAYS_SOFTWARE=1 GTK_MODULES=gail:atk-bridge NO_AT_BRIDGE=0 /usr/bin/python3 /home/ottrinqa/worker.py\n')
 config=Path('/etc/lightdm/lightdm.conf.d/90-ottrin-owned-qa.conf');config.parent.mkdir(parents=True,exist_ok=True)
 config.write_text('[Seat:*]\nautologin-user=ottrinqa\nautologin-user-timeout=0\nuser-session=ottrin-qa\nautologin-session=ottrin-qa\n')
 subprocess.run(['systemctl','start','lightdm'],check=True)
 deadline=time.monotonic()+240
 while time.monotonic()<deadline and not (out/'WORKER_RESULT.json').exists():time.sleep(.5)
 worker=json.loads((out/'WORKER_RESULT.json').read_text());report['worker']=worker
 assert worker['status']=='pass',worker
 report.update(status='pass',gui_input=source)
except BaseException as error:report.update(status='fail',error=repr(error),traceback=traceback.format_exc())
finally:
 subprocess.run(['systemctl','stop','lightdm'],capture_output=True)
 subprocess.run(['systemctl','stop',f'user@{uid}.service'],capture_output=True)
 (out/'lightdm.log').write_text(subprocess.run(['journalctl','-u','lightdm','--no-pager'],capture_output=True,text=True).stdout)
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
 with tarfile.open(base/'results.tar.gz','w:gz') as archive:archive.add(out,arcname='results')
 os.chown(base/'results.tar.gz',uid,account.pw_gid)
print(json.dumps(report,indent=2));sys.exit(0 if report['status']=='pass' else 1)
