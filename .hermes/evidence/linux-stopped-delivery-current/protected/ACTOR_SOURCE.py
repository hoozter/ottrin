"""Owned Ubuntu graphical-session bootstrap; real polkit policy remains intact."""
import hashlib,json,os,pathlib,pwd,secrets,signal,subprocess,sys,tarfile,time
base=pathlib.Path('/home/ottrinqa');out=base/'results';out.mkdir()
assert os.geteuid()==0 and os.uname().nodename=='ottrin-native-ubuntu-desktop-qa'
source=sys.argv[1];account=pwd.getpwnam('ottrinqa');uid=account.pw_uid
report={'status':'running','source':source,'scope':'Real graphical polkit setup/cancel/reinstall in disposable Ubuntu login session'}
xvfb=None
protected=None
try:
 assert 'Git revision: '+source in pathlib.Path('/usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text()
 removed=subprocess.run(['env','DEBIAN_FRONTEND=noninteractive','apt-get','-y','remove','ottrin'],capture_output=True,text=True,timeout=120)
 (out/'native-package-remove.log').write_text(removed.stdout+removed.stderr);assert removed.returncode==0
 assert not pathlib.Path('/usr/libexec/ottrin/ottrin-priv-helper').exists()
 password=secrets.token_urlsafe(24)
 subprocess.run(['chpasswd'],input='ottrinqa:'+password+'\n',text=True,check=True)
 secret=base/'.qa-password';secret.write_text(password);secret.chmod(0o600);os.chown(secret,uid,account.pw_gid)
 image_input=base/'QA_APPIMAGE_INPUT.json'
 if image_input.exists():
  image_config=json.loads(image_input.read_text());assert pathlib.Path(image_config['name']).name==image_config['name']
  image=base/image_config['name'];assert hashlib.sha256(image.read_bytes()).hexdigest()==image_config['sha256']
  image.chmod(0o755);os.chown(image,uid,account.pw_gid)
  fuse=subprocess.run(['env','DEBIAN_FRONTEND=noninteractive','apt-get','-y','install','libfuse2t64'],capture_output=True,text=True,timeout=180)
  (out/'fuse-install.log').write_text(fuse.stdout+fuse.stderr);assert fuse.returncode==0
  subprocess.run(['modprobe','fuse'],check=True);assert pathlib.Path('/dev/fuse').exists()
  report['launch_kind']='AppImage/FUSE'
 else:
  portable=base/'portable';portable.mkdir()
  with tarfile.open(base/'portable.tar.gz') as archive:archive.extractall(portable,filter='data')
  for p in [portable,*portable.rglob('*')]:os.chown(p,uid,account.pw_gid,follow_symlinks=False)
  report['launch_kind']='portable'
 os.chown(out,uid,account.pw_gid)
 fixture=base/'fixtures';fixture.mkdir();(fixture/'preserve.txt').write_text('Original owned fixture.\n')
 if (base/'QA_PROTECTED_INPUT.json').exists():
  protected=pathlib.Path('/var/lib/ottrin-owned-protected-qa');assert not protected.exists();protected.mkdir(mode=0o700)
  (fixture/'cancel-copy.txt').write_text('Cancelled privileged copy must not appear.\n')
 pref=base/'.config/ottrin/config.json';pref.parent.mkdir(parents=True,exist_ok=True)
 pref.write_text(json.dumps({'theme':'Dark','list_columns':['Name','Kind','Size','Modified'],'command_frame_always_visible':False,'show_hidden_files':False,'target':{'current':None,'pinned':[],'has_ever_set':False,'recent':[]},'default_view_mode':'Miller','search_setup_completed':True,'window_size':[1100,800],'search':{'include_roots':[str(fixture)],'content_indexing_enabled':False}}))
 for p in [fixture,*fixture.rglob('*'),pref,*pref.parents]:
  if p==base or base in p.parents:os.chown(p,uid,account.pw_gid)
 xvfb=subprocess.Popen(['Xvfb',':77','-screen','0','1440x1000x24','-nolisten','tcp','-ac'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 time.sleep(1);assert xvfb.poll() is None
 subprocess.run(['loginctl','enable-linger','ottrinqa'],check=True)
 subprocess.run(['systemctl','start',f'user@{uid}.service'],check=True)
 env=['DISPLAY=:77','XDG_RUNTIME_DIR=/run/user/'+str(uid),'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/'+str(uid)+'/bus','HOME='+str(base),'USER=ottrinqa','PATH=/usr/sbin:/usr/bin:/sbin:/bin','LIBGL_ALWAYS_SOFTWARE=1','XDG_SESSION_TYPE=x11','XDG_CURRENT_DESKTOP=OttrinQA','GTK_MODULES=gail:atk-bridge','NO_AT_BRIDGE=0']
 result=subprocess.run(['systemd-run','--wait','--pipe','--collect','--unit=ottrin-qa-desktop','--uid=ottrinqa','--property=PAMName=login','/usr/bin/env',*env,'/usr/bin/python3',str(base/'prompt-worker.py')],capture_output=True,text=True,timeout=360)
 (out/'desktop-worker.log').write_text(result.stdout+result.stderr)
 report['worker_exit']=result.returncode
 if (out/'WORKER_RESULT.json').exists():report['worker']=json.loads((out/'WORKER_RESULT.json').read_text())
 assert result.returncode==0,report.get('worker',result.stderr[-1500:])
 if protected is not None:
  copied=protected/'preserve.txt';assert copied.is_file() and copied.stat().st_uid==0
  assert copied.read_bytes()==(fixture/'preserve.txt').read_bytes()
  assert not (protected/'cancel-copy.txt').exists()
  report['protected_copy']={'root_owned_copy_matches_source':True,'cancelled_destination_absent':True}
 report['status']='pass'
except BaseException as error:report.update(status='fail',error=repr(error))
finally:
 subprocess.run(['systemctl','stop','ottrin-qa-desktop.service'],capture_output=True)
 subprocess.run(['loginctl','disable-linger','ottrinqa'],capture_output=True)
 subprocess.run(['systemctl','stop',f'user@{uid}.service'],capture_output=True)
 if xvfb is not None:
  xvfb.terminate();xvfb.wait(timeout=10)
 if protected is not None:
  report['protected_outputs_before_cleanup']={'cancel_copy_exists':(protected/'cancel-copy.txt').exists(),'authenticated_copy_exists':(protected/'preserve.txt').exists()}
  import shutil
  shutil.rmtree(protected)
  report['owned_protected_directory_removed']=not protected.exists()
 secret=base/'.qa-password'
 if secret.exists():secret.unlink()
 report['private_password_removed']=not secret.exists()
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
 with tarfile.open(base/'results.tar.gz','w:gz') as archive:archive.add(out,arcname='results')
 os.chown(base/'results.tar.gz',uid,account.pw_gid)
print(json.dumps(report,indent=2));sys.exit(0 if report['status']=='pass' else 1)
