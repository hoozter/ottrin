"""Real systemd acceptance of the published Linux index CLI; no GUI claims."""
import hashlib,json,os,pathlib,pwd,subprocess,sys,tarfile,time
base=pathlib.Path('/home/ottrinqa');out=base/'results';out.mkdir()
distro=sys.argv[2];source=sys.argv[1]
assert os.geteuid()==0 and os.uname().nodename=='ottrin-native-'+distro+'-desktop-qa'
assert 'Git revision: '+source in pathlib.Path('/usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text()
account=pwd.getpwnam('ottrinqa');uid=account.pw_uid
report={'source':source,'status':'running','scope':'Published native package CLI with real user systemd; no graphical controls, polkit prompts, reboot or binary upgrade acceptance','states':[]}
def run(args,check=True,timeout=90):
 p=subprocess.run(list(map(str,args)),capture_output=True,text=True,timeout=timeout)
 if check and p.returncode:raise RuntimeError(f'{args}: {p.returncode}: {p.stdout[-1000:]} {p.stderr[-1000:]}')
 return p
def user(*args,check=True,timeout=90):
 return run(['runuser','-u','ottrinqa','--','env','-i','PATH=/usr/sbin:/usr/bin:/sbin:/bin','HOME='+str(base),'USER=ottrinqa','XDG_RUNTIME_DIR=/run/user/'+str(uid),'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/'+str(uid)+'/bus',*args],check,timeout)
def ctl(*args,check=True):return user('systemctl','--user',*args,check=check)
def app(*args):return user('/usr/bin/ottrin',*args)
def write(path,data):
 path.parent.mkdir(parents=True,exist_ok=True);path.write_text(data)
 for p in [path,*path.parents]:
  if p==base or base in p.parents:os.chown(p,uid,account.pw_gid)
def sha(path):
 with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def state(label,enabled,running):
 registered=(base/'.config/systemd/user/ottrin-index.service').exists()
 active=ctl('is-active','ottrin-index.service',check=False).stdout.strip()
 on=ctl('is-enabled','ottrin-index.service',check=False).stdout.strip()
 pid=int(ctl('show','ottrin-index.service','--property=MainPID','--value').stdout.strip() or 0)
 actual={'label':label,'registered':registered,'enabled':on,'active':active,'pid':pid,'cli':app('index','service-status').stdout}
 report['states'].append(actual)
 assert (on=='enabled')==enabled,actual
 assert (active=='active')==running,actual
 assert (pid>0)==running,actual
 return pid
try:
 fixture=base/'fixtures/source.txt';config=base/'.config/ottrin/config.json'
 write(fixture,'Owned service fixture; preserve this file.\n')
 write(config,json.dumps({'theme':'Dark','list_columns':['Name','Kind','Size','Modified'],'command_frame_always_visible':False,'show_hidden_files':False,'target':{'current':None,'pinned':[],'has_ever_set':False,'recent':[]},'default_view_mode':'Miller','search_setup_completed':True,'search':{'include_roots':[str(fixture.parent)],'content_indexing_enabled':False}}))
 retained={str(p):sha(p) for p in [fixture,config]}
 run(['loginctl','enable-linger','ottrinqa']);run(['systemctl','start',f'user@{uid}.service'])
 app('index','service-install');app('index','ensure');first=state('installed',True,True)
 ctl('disable','ottrin-index.service');assert state('disabled while running',False,True)==first
 ctl('enable','ottrin-index.service');assert state('enabled while running',True,True)==first
 ctl('disable','ottrin-index.service');app('index','service-install');app('index','ensure')
 state('repair disabled running service',False,True)
 ctl('stop','ottrin-index.service');state('stopped disabled',False,False)
 app('index','service-install');state('repair disabled stopped service',False,False)
 # Guest-only unit drop-in delays actual service startup beyond the former 3s limit.
 delay=base/'.config/systemd/user/ottrin-index.service.d/qa-delay.conf'
 write(delay,'[Service]\nExecStartPre=/bin/sleep 7\n');ctl('daemon-reload')
 started=time.monotonic();app('index','ensure');elapsed=time.monotonic()-started
 assert elapsed>=7,elapsed
 report['delayed_readiness_seconds']=elapsed
 state('first ensure after delayed start',False,True)
 delay.unlink();delay.parent.rmdir();ctl('daemon-reload')
 # A client command exiting must leave the managed daemon alive.
 pid=state('client exited; daemon remains',False,True);time.sleep(2)
 assert state('daemon retained after client exit',False,True)==pid
 app('index','service-restart');app('index','ensure');state('explicit restart',False,True)
 app('index','service-remove');state('removed',False,False)
 assert not (base/'.config/systemd/user/ottrin-index.service').exists()
 assert all(sha(pathlib.Path(p))==digest for p,digest in retained.items())
 report['source_and_preferences_preserved']=True
 report['status']='pass'
except BaseException as error:
 report.update(status='fail',error=repr(error))
finally:
 journal=user('journalctl','--user','--no-pager','-u','ottrin-index.service',check=False)
 (out/'service-journal.log').write_text(journal.stdout+journal.stderr)
 ctl('stop','ottrin-index.service',check=False)
 run(['loginctl','disable-linger','ottrinqa'],check=False)
 run(['systemctl','stop',f'user@{uid}.service'],check=False)
 report['user_manager_stopped']=run(['systemctl','is-active',f'user@{uid}.service'],check=False).returncode!=0
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
 with tarfile.open(base/'results.tar.gz','w:gz') as archive:archive.add(out,arcname='results')
 os.chown(base/'results.tar.gz',uid,account.pw_gid)
print(json.dumps(report,indent=2));sys.exit(0 if report['status']=='pass' else 1)
