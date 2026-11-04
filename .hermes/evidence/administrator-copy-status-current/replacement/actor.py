"""Owned actual desktop bootstrap for stable-path binary replacement."""
from pathlib import Path
import hashlib,json,os,pwd,secrets,shutil,subprocess,sys,tarfile,time,traceback
base=Path('/home/ottrinqa');out=base/'results';out.mkdir();source=sys.argv[1];account=pwd.getpwnam('ottrinqa');uid=account.pw_uid
assert os.geteuid()==0 and os.uname().nodename=='ottrin-native-ubuntu-desktop-qa'
report={'status':'running','source_revision':source}
try:
 assert 'Git revision: '+source in Path('/usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text()
 subprocess.run(['env','DEBIAN_FRONTEND=noninteractive','apt-get','-y','remove','ottrin'],check=True,capture_output=True,text=True,timeout=120)
 subprocess.run(['chpasswd'],input='ottrinqa:'+secrets.token_urlsafe(24)+'\n',text=True,check=True)
 subprocess.run(['env','DEBIAN_FRONTEND=noninteractive','apt-get','-y','install','libfuse2t64'],check=True,capture_output=True,text=True,timeout=120)
 subprocess.run(['modprobe','fuse'],check=True)
 data=json.loads((base/'REPLACEMENT_INPUT.json').read_text());assets=base/'assets';assets.mkdir()
 for tag,name in [('old','old-portable.tar.gz'),('new','portable.tar.gz')]:
  archive=base/name;assert hashlib.sha256(archive.read_bytes()).hexdigest()==data[tag]['portable_sha256']
  destination=assets/(tag+'-portable');destination.mkdir()
  with tarfile.open(archive) as t:t.extractall(destination,filter='data')
 for tag,name in [('old','old.AppImage'),('new','Ottrin-0.1.0-x86_64.AppImage')]:
  image=base/name;assert hashlib.sha256(image.read_bytes()).hexdigest()==data[tag]['appimage_sha256']
  shutil.copyfile(image,assets/(tag+'.AppImage'));(assets/(tag+'.AppImage')).chmod(0o755)
 fixture=base/'fixtures';fixture.mkdir()
 for i in range(100):(fixture/f'owned-{i:04}.txt').write_text(f'owned{i}\n')
 pref=base/'.config/ottrin/config.json';pref.parent.mkdir(parents=True,exist_ok=True)
 pref.write_text(json.dumps({'theme':'Dark','list_columns':['Name','Kind','Size','Modified'],'command_frame_always_visible':False,'show_hidden_files':False,'target':{'current':None,'pinned':[],'has_ever_set':False,'recent':[]},'default_view_mode':'Grid','search_setup_completed':True,'window_size':[1100,800],'grid_show_thumbnails':False,'search':{'include_roots':[str(fixture)],'content_indexing_enabled':False}}))
 for root in [assets,fixture,pref.parent,out]:
  for p in [root,*root.rglob('*')]:os.chown(p,uid,account.pw_gid,follow_symlinks=False)
 os.chown(pref.parent.parent,uid,account.pw_gid)
 subprocess.run(['systemctl','stop','lightdm'],check=True)
 Path('/usr/share/xsessions/ottrin-qa.desktop').write_text('[Desktop Entry]\nName=Owned Ottrin replacement QA\nType=Application\nExec=/usr/bin/env LIBGL_ALWAYS_SOFTWARE=1 GTK_MODULES=gail:atk-bridge NO_AT_BRIDGE=0 /usr/bin/python3 /home/ottrinqa/worker.py\n')
 configuration=Path('/etc/lightdm/lightdm.conf.d/90-ottrin-owned-qa.conf');configuration.parent.mkdir(parents=True,exist_ok=True);configuration.write_text('[Seat:*]\nautologin-user=ottrinqa\nautologin-user-timeout=0\nuser-session=ottrin-qa\nautologin-session=ottrin-qa\n')
 subprocess.run(['systemctl','start','lightdm'],check=True)
 deadline=time.monotonic()+450
 while time.monotonic()<deadline and not (out/'WORKER_RESULT.json').exists():time.sleep(.5)
 worker=json.loads((out/'WORKER_RESULT.json').read_text());assert worker['status']=='pass',worker
 assert len(worker['cases'])==4
 report.update(status='pass',worker=worker)
except BaseException as error:report.update(status='fail',error=repr(error),traceback=traceback.format_exc())
finally:
 subprocess.run(['systemctl','stop','lightdm'],capture_output=True)
 subprocess.run(['systemctl','stop',f'user@{uid}.service'],capture_output=True)
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
 with tarfile.open(base/'results.tar.gz','w:gz') as t:t.add(out,arcname='results')
print(json.dumps(report,indent=2));raise SystemExit(0 if report['status']=='pass' else 1)
