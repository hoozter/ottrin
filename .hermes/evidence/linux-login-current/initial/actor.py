"""Prepare/assess an owned LightDM desktop; the SSH control account is separate."""
from pathlib import Path
import hashlib,json,os,pwd,secrets,shutil,subprocess,sys,tarfile,time,traceback
control=Path('/home/ottrincontrol');base=Path('/home/ottrinqa');out=base/'results'
source=sys.argv[1];phase=int(sys.argv[3]) if len(sys.argv)>3 else 0
assert os.geteuid()==0 and os.uname().nodename=='ottrin-native-ubuntu-desktop-qa'
account=pwd.getpwnam('ottrinqa');uid=account.pw_uid
report={'status':'running','phase':phase,'source_revision':source,'control_account':'ottrincontrol','desktop_account':'ottrinqa'}
try:
 assert 'Git revision: '+source in Path('/usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text()
 if phase==0:
  out.mkdir();os.chown(out,uid,account.pw_gid)
  subprocess.run(['chpasswd'],input='ottrinqa:'+secrets.token_urlsafe(24)+'\n',text=True,check=True)
  for name in ['worker.py','linux-desktop-qa.py']:
   shutil.copyfile(control/name,base/name);os.chown(base/name,uid,account.pw_gid)
  fixture=base/'fixtures';fixture.mkdir()
  for i in range(1000):(fixture/f'owned-{i:04}.txt').write_text(f'owned{i}\n')
  pref=base/'.config/ottrin/config.json';pref.parent.mkdir(parents=True,exist_ok=True)
  pref.write_text(json.dumps({'theme':'Dark','default_view_mode':'Miller','search_setup_completed':True,'window_size':[1100,800],'search':{'include_roots':[str(fixture)],'content_indexing_enabled':False}}))
  for p in [fixture,*fixture.iterdir(),pref,*pref.parents]:
   if p==base or base in p.parents:os.chown(p,uid,account.pw_gid)
  subprocess.run(['systemctl','stop','lightdm'],check=True)
  Path('/usr/share/xsessions/ottrin-qa.desktop').write_text('[Desktop Entry]\nName=Owned Ottrin login QA\nType=Application\nExec=/usr/bin/env LIBGL_ALWAYS_SOFTWARE=1 GTK_MODULES=gail:atk-bridge NO_AT_BRIDGE=0 /usr/bin/python3 /home/ottrinqa/worker.py\n')
  config=Path('/etc/lightdm/lightdm.conf.d/90-ottrin-owned-qa.conf');config.parent.mkdir(parents=True,exist_ok=True)
  config.write_text('[Seat:*]\nautologin-user=ottrinqa\nautologin-user-timeout=0\nuser-session=ottrin-qa\nautologin-session=ottrin-qa\n')
  subprocess.run(['systemctl','set-default','graphical.target'],check=True)
  subprocess.run(['systemctl','enable','lightdm'],check=True)
  subprocess.run(['systemctl','start','lightdm'],check=True)
 deadline=time.monotonic()+120
 result=out/('PHASE'+str(phase)+'.json')
 while time.monotonic()<deadline and not result.exists():time.sleep(.5)
 worker=json.loads(result.read_text());assert worker['status']=='pass',worker
 assert worker['boot_id']==Path('/proc/sys/kernel/random/boot_id').read_text().strip()
 pid=worker['after_normal_gui_close_unit']['MainPID'];exe=Path('/proc')/pid/'exe'
 assert hashlib.sha256(exe.read_bytes()).hexdigest()==hashlib.sha256(Path('/usr/bin/ottrin-indexd').read_bytes()).hexdigest()
 assert int((Path('/proc')/pid/'status').read_text().split('Uid:')[1].split()[0])==uid
 report.update(status='pass',worker=worker,daemon_sha256=hashlib.sha256(exe.read_bytes()).hexdigest())
except BaseException as error:report.update(status='fail',error=repr(error),traceback=traceback.format_exc())
finally:
 out.mkdir(exist_ok=True)
 (out/('ROOT_PHASE'+str(phase)+'.json')).write_text(json.dumps(report,indent=2)+'\n')
 with tarfile.open(control/'results.tar.gz','w:gz') as archive:archive.add(out,arcname='results')
print(json.dumps(report,indent=2));raise SystemExit(0 if report['status']=='pass' else 1)
