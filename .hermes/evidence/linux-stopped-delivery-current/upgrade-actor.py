"""Actual same-version DEB replacement with real user systemd and saved search data."""
from pathlib import Path
import hashlib,json,os,pwd,subprocess,sys,tarfile,time
base=Path('/home/ottrinqa');out=base/'results';out.mkdir()
assert os.geteuid()==0 and os.uname().nodename=='ottrin-native-ubuntu-desktop-qa'
source=sys.argv[1];spec=json.loads((base/'UPGRADE_INPUT.json').read_text());assert source==spec['new_source']
a=pwd.getpwnam('ottrinqa');uid=a.pw_uid
report={'status':'running','source':source,'previous_source':spec['old_source'],'scope':'Actual f463-to-061 same-package-version DEB replacement; enabled/running, disabled/running and disabled/stopped; first ensure, saved search/source/preferences. No semantic-version, reboot/login or GUI upgrade acceptance.','cases':[]}
def sha(p):
 with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def run(args,check=True,timeout=120):
 p=subprocess.run(list(map(str,args)),capture_output=True,text=True,timeout=timeout)
 if check and p.returncode:raise RuntimeError(f'{args}: {p.returncode}: {p.stdout[-800:]} {p.stderr[-1000:]}')
 return p
def user(*args,check=True):return run(['runuser','-u','ottrinqa','--','env','-i','PATH=/usr/sbin:/usr/bin:/sbin:/bin','HOME='+str(base),'USER=ottrinqa','XDG_RUNTIME_DIR=/run/user/'+str(uid),'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/'+str(uid)+'/bus',*args],check)
def ctl(*args,check=True):return user('systemctl','--user',*args,check=check)
def app(*args):return user('/usr/bin/ottrin',*args)
def state():return {'pid':int(ctl('show','ottrin-index.service','--property=MainPID','--value').stdout.strip() or 0),'enabled':ctl('is-enabled','ottrin-index.service',check=False).stdout.strip()=='enabled'}
def install(package,label,expected):
 p=run(['env','DEBIAN_FRONTEND=noninteractive','apt-get','-y','install','--reinstall',package],check=False,timeout=180)
 (out/(label+'.log')).write_text(p.stdout+p.stderr);assert p.returncode==0
 assert 'Git revision: '+expected in Path('/usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text()
try:
 previous=base/spec['old_deb'];assert previous.name==spec['old_deb'] and sha(previous)==spec['old_deb_sha256']
 corpus=base/'fixtures';corpus.mkdir()
 for i in range(1000):(corpus/f'original-{i:04}.txt').write_text(f'owned original {i}\n')
 pref=base/'.config/ottrin/config.json';pref.parent.mkdir(parents=True,exist_ok=True)
 pref.write_text(json.dumps({'theme':'Dark','list_columns':['Name','Kind','Size','Modified'],'command_frame_always_visible':False,'show_hidden_files':False,'target':{'current':None,'pinned':[],'has_ever_set':False,'recent':[]},'default_view_mode':'Miller','search_setup_completed':True,'search':{'include_roots':[str(corpus)],'content_indexing_enabled':False}}))
 for p in [corpus,*corpus.rglob('*'),pref,*pref.parents]:
  if p==base or base in p.parents:os.chown(p,uid,a.pw_gid)
 originals={p.name:sha(p) for p in corpus.iterdir()};pref_sha=sha(pref)
 run(['loginctl','enable-linger','ottrinqa']);run(['systemctl','start',f'user@{uid}.service'])
 for number,(enabled,running) in enumerate([(True,True),(False,True),(False,False)]):
  install(previous,f'{number}-previous-install',spec['old_source'])
  app('index','service-install');app('index','ensure')
  deadline=time.monotonic()+45
  while json.loads(app('search','original-0999','--json').stdout)['total']!=1:
   assert time.monotonic()<deadline;time.sleep(.2)
  ctl('enable' if enabled else 'disable','ottrin-index.service')
  if not running:ctl('stop','ottrin-index.service')
  before=state();assert before['enabled']==enabled and (before['pid']>0)==running
  install(base/'candidate.deb',f'{number}-candidate-install',source)
  after=state();assert after['enabled']==enabled and (after['pid']>0)==running
  if running:assert after['pid']!=before['pid']
  assert sha('/usr/bin/ottrin-indexd')==spec['new_indexd_sha256']
  started=time.monotonic();first=app('index','ensure');duration=time.monotonic()-started
  final=state();assert final['pid']>0 and final['enabled']==enabled
  if running:assert final['pid']==after['pid']
  assert json.loads(app('search','original-0999','--json').stdout)['total']==1
  assert sha(pref)==pref_sha and {p.name:sha(p) for p in corpus.iterdir()}==originals
  report['cases'].append({'enabled':enabled,'running':running,'before':before,'after_package_transaction':after,'after_first_ensure':final,'first_ensure_exit':first.returncode,'first_ensure_seconds':duration,'first_ensure_stdout':first.stdout,'source_and_preferences_preserved':True})
 app('index','service-remove');assert not (base/'.config/systemd/user/ottrin-index.service').exists()
 report['status']='pass'
except BaseException as error:report.update(status='fail',error=repr(error))
finally:
 journal=user('journalctl','--user','--no-pager','-u','ottrin-index.service',check=False);(out/'service-journal.log').write_text(journal.stdout+journal.stderr)
 ctl('stop','ottrin-index.service',check=False);run(['loginctl','disable-linger','ottrinqa'],check=False);run(['systemctl','stop',f'user@{uid}.service'],check=False)
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
 with tarfile.open(base/'results.tar.gz','w:gz') as archive:archive.add(out,arcname='results')
 os.chown(base/'results.tar.gz',uid,a.pw_gid)
print(json.dumps(report,indent=2));sys.exit(0 if report['status']=='pass' else 1)
