"""Owned Ubuntu graphical-session bootstrap; real polkit policy remains intact."""
import hashlib,json,os,pathlib,pwd,secrets,signal,subprocess,sys,tarfile,time
base=pathlib.Path('/home/ottrinqa');out=base/'results';out.mkdir()
assert os.geteuid()==0 and os.uname().nodename=='ottrin-native-ubuntu-desktop-qa'
source=sys.argv[1];account=pwd.getpwnam('ottrinqa');uid=account.pw_uid
report={'status':'running','source':source,'scope':'Real old-helper upgrade/cancel/reinstall and Copy recovery in disposable Ubuntu login session'}
xvfb=None
protected=None
try:
 assert 'Git revision: '+source in pathlib.Path('/usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text()
 policy_bytes=pathlib.Path('/usr/share/polkit-1/actions/org.ottrin.filesystem.policy').read_bytes()
 removed=subprocess.run(['env','DEBIAN_FRONTEND=noninteractive','apt-get','-y','remove','ottrin'],capture_output=True,text=True,timeout=120)
 (out/'native-package-remove.log').write_text(removed.stdout+removed.stderr);assert removed.returncode==0
 assert not pathlib.Path('/usr/libexec/ottrin/ottrin-priv-helper').exists()
 helper=pathlib.Path('/usr/libexec/ottrin/ottrin-priv-helper');helper.parent.mkdir(parents=True,exist_ok=True)
 old_helper=base/'old-helper';old_config=json.loads((base/'QA_OLD_HELPER_INPUT.json').read_text())
 assert hashlib.sha256(old_helper.read_bytes()).hexdigest()==old_config['sha256']
 helper.write_bytes(old_helper.read_bytes());helper.chmod(0o755)
 policy=pathlib.Path('/usr/share/polkit-1/actions/org.ottrin.filesystem.policy');policy.write_bytes(policy_bytes);policy.chmod(0o644)
 assert subprocess.run([str(helper),'--check-protocol','2','--copy-recovery'],capture_output=True).returncode==0
 report['old_helper_sha256']=old_config['sha256']
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
 # LightDM owns a real VT/seat0 login; retain stock polkit policy.
 subprocess.run(['systemctl','stop','lightdm'],check=True)
 session=pathlib.Path('/usr/share/xsessions/ottrin-qa.desktop')
 session.write_text('[Desktop Entry]\nName=Ottrin owned acceptance\nType=Application\nExec=/usr/bin/env LIBGL_ALWAYS_SOFTWARE=1 GTK_MODULES=gail:atk-bridge NO_AT_BRIDGE=0 /usr/bin/python3 /home/ottrinqa/prompt-worker.py\n')
 configuration=pathlib.Path('/etc/lightdm/lightdm.conf.d/90-ottrin-owned-qa.conf')
 configuration.parent.mkdir(parents=True,exist_ok=True)
 configuration.write_text('[Seat:*]\nautologin-user=ottrinqa\nautologin-user-timeout=0\nuser-session=ottrin-qa\nautologin-session=ottrin-qa\n')
 subprocess.run(['systemctl','start','lightdm'],check=True)
 deadline=time.monotonic()+450
 verified_phases=set()
 while time.monotonic()<deadline and not (out/'WORKER_RESULT.json').exists():
  for phase in ['HELPER_COPY','GUI_COPY']:
   ready=out/(phase+'_READY.json')
   if ready.exists() and phase not in verified_phases:
    copied=protected/'preserve.txt'
    assert copied.is_file() and copied.stat().st_uid==0
    assert copied.read_bytes()==(fixture/'preserve.txt').read_bytes()
    proof={'phase':phase,'root_owned':True,'exact_source_bytes':True,'sha256':hashlib.sha256(copied.read_bytes()).hexdigest()}
    temporary=out/(phase+'_VERIFIED.next');temporary.write_text(json.dumps(proof));temporary.replace(out/(phase+'_VERIFIED.json'))
    verified_phases.add(phase)
  time.sleep(1)
 worker=json.loads((out/'WORKER_RESULT.json').read_text()) if (out/'WORKER_RESULT.json').exists() else {'status':'timeout'}
 result=subprocess.CompletedProcess([],0 if worker['status']=='pass' else 1,json.dumps(worker),'')
 (out/'desktop-worker.log').write_text(result.stdout+result.stderr)
 report['worker_exit']=result.returncode
 if (out/'WORKER_RESULT.json').exists():report['worker']=json.loads((out/'WORKER_RESULT.json').read_text())
 assert result.returncode==0,report.get('worker',result.stderr[-1500:])
 if protected is not None:
  assert verified_phases=={'HELPER_COPY','GUI_COPY'}
  assert not (protected/'preserve.txt').exists() and not (protected/'cancel-copy.txt').exists()
  assert not list(protected.glob('.ottrin-txn-*'))
  report['protected_copy']={'root_owned_copies_match_source':sorted(verified_phases),'undo_removed_publications':True,'explicit_cleanup_removed_stages':True,'cancelled_destination_absent':True}
 report['status']='pass'
except BaseException as error:report.update(status='fail',error=repr(error))
finally:
 subprocess.run(['systemctl','stop','lightdm'],capture_output=True)
 (out/'lightdm.log').write_text(subprocess.run(['journalctl','-u','lightdm','--no-pager'],capture_output=True,text=True).stdout)
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
