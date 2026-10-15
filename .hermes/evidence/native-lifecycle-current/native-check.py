import os,socket,pathlib,subprocess,pwd,json,hashlib,shutil,time,sqlite3,sys,re
if os.geteuid()!=0 or socket.gethostname()!='ottrin-native-lifecycle-qa':raise RuntimeError('Owned QA guest only')
base=pathlib.Path('/home/ottrinqa');out=base/'native-results';out.mkdir(exist_ok=True)
expected=sys.argv[1];assert re.fullmatch('[a-f0-9]{40}',expected)
provenance=pathlib.Path('/usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text();assert 'Git revision: '+expected in provenance and 'Tracked tree: clean' in provenance
result={'status':'running','scope':'Clean RPM native package lifecycle; multi-user Fedora 44','source_revision':expected,'rpm_sha256':hashlib.file_digest((base/'candidate.rpm').open('rb'),'sha256').hexdigest()};users=['ottrinqa','secondqa','offlineqa','portableqa'];admin='ottrin-administrator-index.service'
def run(args,check=True,timeout=120):
 r=subprocess.run(args,capture_output=True,text=True,timeout=timeout)
 if check and r.returncode:raise RuntimeError(f'{args}: {r.returncode}: {r.stderr[-1800:]} {r.stdout[-1200:]}')
 return r

def user(name,*args,check=True):
 info=pwd.getpwnam(name);return run(['runuser','-u',name,'--','env','-i','PATH=/usr/sbin:/usr/bin:/sbin:/bin','HOME='+info.pw_dir,'USER='+name,'XDG_RUNTIME_DIR=/run/user/'+str(info.pw_uid),'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/'+str(info.pw_uid)+'/bus',*args],check=check)
def write(name,p,data,mode=0o600):
 info=pwd.getpwnam(name);p=pathlib.Path(p);p.parent.mkdir(parents=True,exist_ok=True)
 for d in [p.parent,p.parent.parent]:
  if str(d).startswith(info.pw_dir+'/'):os.chown(d,info.pw_uid,info.pw_gid)
 p.write_text(data);p.chmod(mode);os.chown(p,info.pw_uid,info.pw_gid)
def home(name):return pathlib.Path(pwd.getpwnam(name).pw_dir)
def unit(name):return home(name)/'.config/systemd/user/ottrin-index.service'
def pid(name):return int(user(name,'systemctl','--user','show','ottrin-index.service','--property=MainPID','--value').stdout.strip())
def adminpid():return int(run(['systemctl','show',admin,'--property=MainPID','--value']).stdout.strip())
def sha(p):return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
try:
 for name in users:
  if name!='ottrinqa' and run(['id',name],check=False).returncode!=0:run(['useradd','--create-home','--shell','/bin/bash',name])
  assert pwd.getpwnam(name).pw_dir=='/home/'+name
  write(name,home(name)/'fixtures/source.txt','Native cleanup preserves '+name+'\n')
  write(name,home(name)/'.config/ottrin/config.json',json.dumps({'default_view_mode':'Miller','search':{'include_roots':[str(home(name)/'fixtures')],'content_indexing_enabled':False},'search_setup_completed':True}))
  if name!='offlineqa':
   run(['loginctl','enable-linger',name]);run(['systemctl','start','user@'+str(pwd.getpwnam(name).pw_uid)+'.service'])
 for name in ['ottrinqa','secondqa']:user(name,'/usr/bin/ottrin','index','service-install')
 user('portableqa','systemctl','--user','stop','ottrin-index.service',check=False)
 portable=home('portableqa')/'portable/bin';portable.mkdir(parents=True,exist_ok=True)
 for name in ['ottrin','ottrin-indexd']:
  shutil.copy2('/usr/bin/'+name,portable/name);os.chown(portable/name,pwd.getpwnam('portableqa').pw_uid,pwd.getpwnam('portableqa').pw_gid)
 user('portableqa',str(portable/'ottrin'),'index','service-install')
 text='[Unit]\nDescription=Offline native fixture\n[Service]\nExecStart="/usr/bin/ottrin-indexd" --index-daemon\n[Install]\nWantedBy=default.target\n'
 write('offlineqa',unit('offlineqa'),text,0o644);wants=unit('offlineqa').parent/'default.target.wants';wants.mkdir(exist_ok=True);os.chown(wants,pwd.getpwnam('offlineqa').pw_uid,pwd.getpwnam('offlineqa').pw_gid);(wants/'ottrin-index.service').unlink(missing_ok=True);(wants/'ottrin-index.service').symlink_to('../ottrin-index.service')
 pathlib.Path('/etc/ottrin-administrator').mkdir(mode=0o700,exist_ok=True);pathlib.Path('/root/admin-fixtures').mkdir(exist_ok=True);pathlib.Path('/root/admin-fixtures/source.txt').write_text('Protected original\n');pathlib.Path('/etc/ottrin-administrator/search.json').write_text(json.dumps({'include_roots':['/root/admin-fixtures'],'content_indexing_enabled':False}))
 pathlib.Path('/etc/systemd/system/'+admin).write_text('[Unit]\nDescription=Owned native Administrator fixture\n[Service]\nExecStart="/usr/bin/ottrin-indexd" --administrator-index-daemon\nRuntimeDirectory=ottrin-administrator\nRuntimeDirectoryMode=0700\nStateDirectory=ottrin-administrator\nStateDirectoryMode=0700\nConfigurationDirectory=ottrin-administrator\nConfigurationDirectoryMode=0700\n[Install]\nWantedBy=multi-user.target\n');run(['systemctl','daemon-reload']);run(['systemctl','enable','--now',admin]);time.sleep(1)
 before={n:pid(n) for n in ['ottrinqa','secondqa','portableqa']};before_admin=adminpid();assert all(before.values()) and before_admin>0
 preferences={n:sha(home(n)/'.config/ottrin/config.json') for n in users};sources={n:sha(home(n)/'fixtures/source.txt') for n in users};portable_unit=unit('portableqa').read_bytes()
 upgrade=run(['dnf','-y','reinstall',str(base/'candidate.rpm')],timeout=180);(out/'reinstall.log').write_text(upgrade.stdout+upgrade.stderr);run(['rpm','-V','ottrin'])
 after={n:pid(n) for n in before};assert all(after[n]>0 and after[n]!=before[n] for n in ['ottrinqa','secondqa']);assert after['portableqa']==before['portableqa'];assert adminpid()>0 and adminpid()!=before_admin
 for name in ['ottrinqa','secondqa']:
  assert json.loads(user(name,'/usr/bin/ottrin','search','source','--json').stdout)['total']==1
 assert unit('offlineqa').read_text()==text;assert all(sha(home(n)/'.config/ottrin/config.json')==preferences[n] for n in users)
 result['same_version_reinstall']={'status':'pass','before':before,'after':after,'administrator_restarted':True,'offline_registration_preserved':True,'portable_preserved':True}
 drop=unit('offlineqa').parent/'ottrin-index.service.d/override.conf';write('offlineqa',drop,'[Service]\nMemoryMax=128M\n')
 reject=run(['dnf','-y','remove','ottrin'],check=False,timeout=180);(out/'rejected-remove.log').write_text(reject.stdout+reject.stderr);assert reject.returncode!=0;assert pathlib.Path('/usr/bin/ottrin').exists();assert all(pid(n)==after[n] for n in after);assert adminpid()>0;result['ambiguous_offline_override']='refused before any native or portable daemon stopped';drop.unlink();drop.parent.rmdir()
 remove=run(['dnf','-y','remove','ottrin'],timeout=180);(out/'remove.log').write_text(remove.stdout+remove.stderr)
 assert not pathlib.Path('/usr/bin/ottrin-indexd').exists();assert all(not unit(n).exists() for n in ['ottrinqa','secondqa','offlineqa']);assert not (pathlib.Path('/etc/systemd/system')/admin).exists();assert unit('portableqa').read_bytes()==portable_unit and pid('portableqa')==before['portableqa']
 assert all(sha(home(n)/'.config/ottrin/config.json')==preferences[n] and sha(home(n)/'fixtures/source.txt')==sources[n] for n in users)
 assert pathlib.Path('/etc/ottrin-administrator/search.json').exists() and pathlib.Path('/root/admin-fixtures/source.txt').read_text()=='Protected original\n'
 db=pathlib.Path('/var/lib/ottrin-administrator/search.db');assert db.exists()
 with sqlite3.connect('file:'+str(db)+'?mode=ro',uri=True) as conn:assert conn.execute('PRAGMA quick_check').fetchone()[0]=='ok'
 result['direct_remove']='pass; active native users, offline registration and Administrator service removed; portable unit/PID and all preferences/source preserved';result['status']='pass'
except Exception as e:result['status']='fail';result['error']=str(e)
finally:
 for name in users:
  if run(['id',name],check=False).returncode==0:
   user(name,'systemctl','--user','stop','ottrin-index.service',check=False)
   run(['loginctl','disable-linger',name],check=False)
 run(['systemctl','stop',admin],check=False)
 (out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2));raise SystemExit(0 if result['status']=='pass' else 1)
