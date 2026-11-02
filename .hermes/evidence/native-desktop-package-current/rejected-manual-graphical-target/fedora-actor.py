"""Exact RPM optional desktop lifecycle in one disposable Fedora VM."""
import hashlib,json,os,pathlib,pwd,socket,subprocess,sys,tarfile,time
assert os.geteuid()==0 and socket.gethostname()=='ottrin-native-desktop-qa'
base=pathlib.Path('/home/ottrinqa');out=base/'results';out.mkdir(exist_ok=True)
source=sys.argv[1];assert 'Git revision: '+source in pathlib.Path('/usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text()
report={'source':source,'scope':'Exact RPM same-version reinstall/removal; native online/offline folder/picker and independent portable choice','status':'running'}
users=['ottrinqa','offlineqa','portableqa']
def run(args,check=True,timeout=120):
 p=subprocess.run(list(map(str,args)),text=True,capture_output=True,timeout=timeout)
 if check and p.returncode:raise RuntimeError(f'{args}: {p.returncode}: {p.stdout[-1000:]} {p.stderr[-1500:]}')
 return p
def home(name):return pathlib.Path(pwd.getpwnam(name).pw_dir)
def user(name,*args,check=True):
 account=pwd.getpwnam(name)
 return run(['runuser','-u',name,'--','env','-i','PATH=/usr/sbin:/usr/bin:/sbin:/bin','HOME='+str(home(name)),'USER='+name,'XDG_CURRENT_DESKTOP=OttrinQA','XDG_RUNTIME_DIR=/run/user/'+str(account.pw_uid),'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/'+str(account.pw_uid)+'/bus',*args],check=check)
def write(name,path,data):
 account=pwd.getpwnam(name);path=pathlib.Path(path);path.parent.mkdir(parents=True,exist_ok=True)
 for directory in [path.parent,*path.parent.parents]:
  if directory==home(name) or home(name) in directory.parents:os.chown(directory,account.pw_uid,account.pw_gid)
 path.write_bytes(data if isinstance(data,bytes) else data.encode());os.chown(path,account.pw_uid,account.pw_gid)
def sha(path):return hashlib.file_digest(pathlib.Path(path).open('rb'),'sha256').hexdigest()
def portal_pid(name):return int(user(name,'systemctl','--user','show','ottrin-portal.service','--property=MainPID','--value').stdout.strip())
try:
 for name in users:
  if run(['id',name],check=False).returncode:run(['useradd','--create-home','--shell','/bin/bash',name])
  assert str(home(name))=='/home/'+name
  write(name,home(name)/'fixtures/source.txt','Preserved '+name+'\n')
  write(name,home(name)/'.config/ottrin/config.json',json.dumps({'default_view_mode':'Miller','search_setup_completed':True,'search':{'include_roots':[str(home(name)/'fixtures')],'content_indexing_enabled':False}}))
  write(name,home(name)/'.config/mimeapps.list','[Default Applications]\ninode/directory=org.example.Files.desktop;\n')
  write(name,home(name)/'.config/xdg-desktop-portal/portals.conf','[preferred]\ndefault=gtk\norg.freedesktop.impl.portal.FileChooser=gtk\n')
  run(['loginctl','enable-linger',name]);run(['systemctl','start','user@'+str(pwd.getpwnam(name).pw_uid)+'.service'])
  display=':'+str(77+users.index(name))
  user(name,'systemd-run','--user','--unit=ottrin-qa-xvfb','--property=Type=exec','/usr/bin/Xvfb',display,'-screen','0','1024x768x24','-nolisten','tcp','-ac')
  user(name,'env','DISPLAY='+display,'XDG_SESSION_TYPE=x11','systemctl','--user','import-environment','DISPLAY','XDG_SESSION_TYPE','XDG_CURRENT_DESKTOP')
  user(name,'systemctl','--user','start','graphical-session.target')

 portable_root=home('portableqa')/'portable';portable_root.mkdir()
 with tarfile.open(base/'portable.tar.gz') as archive:archive.extractall(portable_root,filter='data')
 account=pwd.getpwnam('portableqa')
 for path in [portable_root,*portable_root.rglob('*')]:os.chown(path,account.pw_uid,account.pw_gid,follow_symlinks=False)
 portable=next(portable_root.glob('*/bin/ottrin'))
 for name in users:
  exe=str(portable) if name=='portableqa' else '/usr/bin/ottrin'
  user(name,'systemctl','--user','import-environment','XDG_CURRENT_DESKTOP')
  user(name,exe,'folders','enable');user(name,exe,'portal','enable');user(name,exe,'portal','apply')
 report['configured']={name:portal_pid(name) for name in users};assert all(report['configured'].values())
 keep={name:{str(path.relative_to(home(name))):sha(path) for path in [home(name)/'fixtures/source.txt',home(name)/'.config/ottrin/config.json']} for name in users}
 portable_before={str(path):path.read_bytes() for path in (home('portableqa')/'.local/state/ottrin').glob('*registration.json')}
 portable_pid=portal_pid('portableqa')
 # Leave one configured user with no manager at package-maintenance entry.
 run(['loginctl','disable-linger','offlineqa']);run(['systemctl','stop','user@'+str(pwd.getpwnam('offlineqa').pw_uid)+'.service'])
 reinstall=run(['dnf','-y','reinstall',base/'candidate.rpm'],check=False,timeout=180);(out/'reinstall.log').write_text(reinstall.stdout+reinstall.stderr);assert reinstall.returncode==0
 run(['rpm','-V','ottrin']);assert portal_pid('ottrinqa')==report['configured']['ottrinqa'];assert portal_pid('portableqa')==portable_pid
 report['reinstall']='pass; selected integrations retained; active picker PIDs unchanged'
 altered=home('ottrinqa')/'.local/share/applications/org.ottrin.Ottrin.Folders.desktop';original=altered.read_bytes();write('ottrinqa',altered,original+b'# user edit\n')
 reject=run(['dnf','-y','remove','ottrin'],check=False,timeout=180);(out/'rejected-remove.log').write_text(reject.stdout+reject.stderr)
 assert reject.returncode!=0 and pathlib.Path('/usr/bin/ottrin').exists();assert portal_pid('ottrinqa')==report['configured']['ottrinqa'];assert portal_pid('portableqa')==portable_pid
 report['modified_native_asset']='refused before native or portable picker stopped';write('ottrinqa',altered,original)
 removal=run(['dnf','-y','remove','ottrin'],check=False,timeout=180);(out/'remove.log').write_text(removal.stdout+removal.stderr);assert removal.returncode==0
 assert not pathlib.Path('/usr/bin/ottrin').exists()
 for name in ['ottrinqa','offlineqa']:
  assert not (home(name)/'.local/state/ottrin/folder-registration.json').exists()
  assert not (home(name)/'.local/state/ottrin/portal-registration.json').exists()
  assert (home(name)/'.config/mimeapps.list').read_text()=='[Default Applications]\ninode/directory=org.example.Files.desktop;\n'
  assert (home(name)/'.config/xdg-desktop-portal/portals.conf').read_text()=='[preferred]\ndefault=gtk\norg.freedesktop.impl.portal.FileChooser=gtk\n'
 for name in users:
  assert all(sha(home(name)/path)==digest for path,digest in keep[name].items())
 report['native_removal']='pass; online/offline preferences restored and native receipts removed; app preferences/source retained'
 assert all(pathlib.Path(path).read_bytes()==data for path,data in portable_before.items())
 assert portal_pid('portableqa')==portable_pid
 definition=pathlib.Path('/usr/share/xdg-desktop-portal/portals/ottrin.portal')
 report['portable_registration_and_process']='preserved'
 report['portable_system_definition_present']=definition.exists()
 assert definition.exists(),'Native package removal deleted the shared portal definition still needed by the portable picker'
 user('portableqa',str(portable),'portal','apply')
 report['portable_picker_after_native_removal']='pass; real frontend restart and backend readiness'
 report['status']='pass'
except Exception as error:
 report.update(status='fail',error=repr(error))
 for name in users:
  if run(['id',name],check=False).returncode==0:
   journal=user(name,'journalctl','--user','--no-pager','-n','100',check=False)
   (out/(name+'-failure-journal.log')).write_text(journal.stdout+journal.stderr)
   state=user(name,'systemctl','--user','show','xdg-desktop-portal.service','ottrin-portal.service','graphical-session.target',check=False)
   (out/(name+'-failure-units.log')).write_text(state.stdout+state.stderr)

finally:
 for name in users:
  if run(['id',name],check=False).returncode==0:
   user(name,'systemctl','--user','stop','ottrin-portal.service','graphical-session.target','ottrin-qa-xvfb.service',check=False)
   run(['loginctl','disable-linger',name],check=False)
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
 with tarfile.open(base/'results.tar.gz','w:gz') as archive:archive.add(out,arcname='results')
 os.chown(base/'results.tar.gz',pwd.getpwnam('ottrinqa').pw_uid,pwd.getpwnam('ottrinqa').pw_gid)
print(json.dumps(report,indent=2));raise SystemExit(0 if report['status']=='pass' else 1)
