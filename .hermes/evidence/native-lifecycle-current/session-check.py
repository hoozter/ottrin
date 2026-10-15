import os,socket,pathlib,subprocess,pwd,json,shutil,time,hashlib,sys,re
if os.geteuid()!=0 or socket.gethostname()!='ottrin-native-lifecycle-qa':raise RuntimeError('Owned QA guest only')
home=pathlib.Path('/home/ottrinqa');out=home/'session-results';out.mkdir(exist_ok=True);uid=pwd.getpwnam('ottrinqa').pw_uid;result={'status':'running','scope':'Native session daemon and foreign session daemon without service registration'}
def run(a,check=True,timeout=180):
 r=subprocess.run(a,capture_output=True,text=True,timeout=timeout)
 if check and r.returncode:raise RuntimeError(f'{a}: {r.returncode}: {r.stderr[-1800:]} {r.stdout[-500:]}')
 return r

def user(*a,check=True):return run(['runuser','-u','ottrinqa','--','env','-i','PATH=/usr/sbin:/usr/bin:/sbin:/bin','HOME='+str(home),'USER=ottrinqa','XDG_RUNTIME_DIR=/run/user/'+str(uid),'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/'+str(uid)+'/bus',*a],check=check)
def pids():
 r=run(['pgrep','-u',str(uid),'-x','ottrin-indexd'],check=False);return [int(p) for p in r.stdout.split()]
def exe(pid):
 try:return os.readlink(f'/proc/{pid}/exe')
 except FileNotFoundError:return '<exited>'
def install():
 r=run(['dnf','-y','install',str(home/'candidate.rpm')]);(out/'install.log').write_text(r.stdout+r.stderr);run(['rpm','-V','ottrin'])
def remove(name):
 r=run(['dnf','-y','remove','ottrin']);(out/name).write_text(r.stdout+r.stderr)
expected=sys.argv[1];assert re.fullmatch('[a-f0-9]{40}',expected)
foreign=home/'session-portable/bin';child=None
try:
 install();provenance=pathlib.Path('/usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text();assert 'Git revision: '+expected in provenance and 'Tracked tree: clean' in provenance
 result['source_revision']=expected
 user('/usr/bin/ottrin','index','update','--json');prior=pids();assert len(prior)==1
 binary=pathlib.Path('/usr/bin/ottrin-indexd');data=binary.read_bytes();info=binary.stat();binary.unlink();binary.write_bytes(data);binary.chmod(info.st_mode & 0o7777);os.chown(binary,info.st_uid,info.st_gid);os.utime(binary,ns=(info.st_atime_ns,info.st_mtime_ns))
 old={p:exe(p) for p in prior};assert all(path=='/usr/bin/ottrin-indexd (deleted)' for path in old.values())
 run(['rpm','-V','ottrin']);result['candidate_sha256']=hashlib.file_digest((home/'candidate.rpm').open('rb'),'sha256').hexdigest();result['helper_sha256']=hashlib.file_digest(pathlib.Path('/usr/libexec/ottrin/native-package-lifecycle').open('rb'),'sha256').hexdigest()
 user('/usr/bin/ottrin-indexd','--native-package-stop');assert all(exe(p)=='<exited>' for p,path in old.items() if path.startswith('/usr/bin/ottrin-indexd'));result['previous_deleted_daemon']={'before':old,'stopped':True}
 assert not (home/'.config/systemd/user/ottrin-index.service').exists();user('/usr/bin/ottrin','index','update','--json');native=pids();assert len(native)==1 and exe(native[0])=='/usr/bin/ottrin-indexd';remove('native-remove.log');assert exe(native[0])=='<exited>';result['native_session_remove']={'pid':native[0],'status':'pass'}
 install();foreign.mkdir(parents=True,exist_ok=True)
 for name in ['ottrin','ottrin-indexd']:
  shutil.copy2('/usr/bin/'+name,foreign/name);os.chown(foreign/name,uid,uid)
 with (out/'foreign.log').open('w') as log:
  child=subprocess.Popen(['runuser','-u','ottrinqa','--','env','-i','PATH=/usr/sbin:/usr/bin:/sbin:/bin','HOME='+str(home),'USER=ottrinqa','XDG_RUNTIME_DIR=/run/user/'+str(uid),str(foreign/'ottrin-indexd')],stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT)
  deadline=time.monotonic()+10
  while time.monotonic()<deadline:
   r=user('/usr/bin/ottrin','index','status','--json',check=False)
   if r.returncode==0 and len(pids())==1:break
   time.sleep(.1)
  other=pids();assert len(other)==1 and exe(other[0])==str(foreign/'ottrin-indexd');remove('foreign-remove.log');assert pids()==other and exe(other[0])==str(foreign/'ottrin-indexd');result['foreign_session_preserved']={'pid':other[0],'status':'pass'}
  user(str(foreign/'ottrin'),'index','service-remove');assert child.wait(timeout=10)==0;child=None
 result['status']='pass'
except Exception as e:result['status']='fail';result['error']=str(e)
finally:
 if child and child.poll() is None:
  user(str(foreign/'ottrin'),'index','service-remove',check=False)
  try:child.wait(timeout=10)
  except subprocess.TimeoutExpired:child.terminate();child.wait(timeout=5)
 result['remaining_primary_daemons']={p:exe(p) for p in pids()}
 (out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2));raise SystemExit(0 if result['status']=='pass' else 1)
