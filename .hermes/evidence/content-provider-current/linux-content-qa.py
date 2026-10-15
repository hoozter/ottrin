"""Exercise public CLI in exact disposable Linux package extractions."""
import hashlib,json,os,pathlib,shutil,subprocess,tempfile,sys,signal,time
def cancel(signum,frame):raise SystemExit('Cancelled package QA')
signal.signal(signal.SIGTERM,cancel)
base=pathlib.Path.cwd(); evidence=pathlib.Path(__file__).resolve().parent
artifact=pathlib.Path(sys.argv[1]).resolve()
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
with tempfile.TemporaryDirectory(prefix='ottrin-content-package-') as name:
 root=pathlib.Path(name)
 if artifact.suffix=='.AppImage':
  subprocess.run([str(artifact),'--appimage-extract'],cwd=root,stdout=subprocess.DEVNULL,check=True)
  stage=root/'squashfs-root'; app=stage/'AppRun'; provider=stage/'usr/libexec/ottrin/rg'; notice=stage/'usr/share/doc/ottrin/third-party-notices/ripgrep'
 elif artifact.name.endswith('.tar.gz'):
  subprocess.run(['tar','-xzf',str(artifact),'-C',str(root)],check=True)
  stage=root/'Ottrin-0.1.0-linux-x86_64';app=stage/'bin/ottrin';provider=stage/'libexec/ottrin/rg';notice=stage/'third-party-notices/ripgrep'
 else:
  stage=root/'native';stage.mkdir()
  if artifact.suffix=='.deb':subprocess.run(['dpkg-deb','-x',str(artifact),str(stage)],check=True)
  else:
   rpm_env=os.environ.copy();rpm_env['LD_LIBRARY_PATH']=str(base/'.hermes/tools/rpm/usr/lib/x86_64-linux-gnu')
   archive=subprocess.run([str(base/'.hermes/tools/rpm/usr/bin/rpm2cpio'),str(artifact)],env=rpm_env,capture_output=True,check=True).stdout
   subprocess.run(['cpio','-idm','--quiet'],input=archive,cwd=stage,check=True)
  app=stage/'usr/bin/ottrin';provider=stage/'usr/libexec/ottrin/rg';notice=stage/'usr/share/doc/ottrin/third-party-notices/ripgrep'
 manifest=json.loads((base/'packaging/search-runtime.json').read_text())
 provider_identity='whole-file pinned hash'
 if artifact.suffix=='.rpm':
  # RPM removes ELF comment/note sections. Compare executable/data sections
  # against the independently hash-verified AppImage provider.
  control=root/'control';control.mkdir()
  subprocess.run([str(base/'dist/packages/Ottrin-0.1.0-x86_64.AppImage'),'--appimage-extract'],cwd=control,stdout=subprocess.DEVNULL,check=True)
  original=control/'squashfs-root/usr/libexec/ottrin/rg';assert sha(original)==manifest['platforms']['linux-x86_64']['binary_sha256']
  for section in ['.text','.rodata']:
   for label,source in [('original',original),('rpm',provider)]:
    subprocess.run(['objcopy','--dump-section',section+'='+str(root/label),str(source),str(root/'section-copy')],check=True)
   assert (root/'original').read_bytes()==(root/'rpm').read_bytes(),section
  provider_identity='RPM stripped ELF: .text and .rodata equal pinned provider'
 else:assert sha(provider)==manifest['platforms']['linux-x86_64']['binary_sha256']
 assert {p.name for p in notice.iterdir()}==set(manifest['licenses'])|set(manifest['source_notices'])|{'MANIFEST.txt'}
 # Broken /usr/bin sibling proves private libexec takes precedence.
 if (stage/'usr/bin').is_dir():
  sibling=stage/'usr/bin/rg';sibling.write_text('#!/bin/sh\nexit 99\n');sibling.chmod(0o755)
 tools=root/'tools';tools.mkdir()
 for tool in ['dirname','readlink']:(tools/tool).symlink_to(shutil.which(tool))
 env=os.environ.copy()
 for key in ['LD_LIBRARY_PATH','OTTRIN_PRIV_HELPER','OTTRIN_INDEX_SOCKET','DBUS_SESSION_BUS_ADDRESS']:env.pop(key,None)
 for key,folder in [('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache'),('XDG_STATE_HOME','state'),('XDG_RUNTIME_DIR','runtime')]:
  p=root/folder;p.mkdir(mode=0o700);env[key]=str(p)
 env['PATH']=str(tools)
 rgconfig=root/'rg-config';rgconfig.write_text('--glob=!*.txt\n');env['RIPGREP_CONFIG_PATH']=str(rgconfig)
 meta=root/'metadata';meta.mkdir();body=root/'bodies';body.mkdir();other=root/'other';other.mkdir()
 marker='ottrinproviderneedle';hit=body/'r\u00e9sum\u00e9.txt';hit.write_text(marker)
 (body/'large.txt').write_text(marker+'x'*100);(body/'excluded.txt').write_text(marker);(body/'wrong.bin').write_text(marker)
 (meta/(marker+'.txt')).write_text('filename match')
 config=root/'config/ottrin/config.json';config.parent.mkdir()
 settings=dict(theme='Dark',default_view_mode='Miller',list_columns=[],command_frame_always_visible=False,restore_session=False,show_hidden_files=False,target={'current':None},last_session_dirs=[],search=dict(include_roots=[str(meta)],exclude_roots=[],content_indexing_enabled=True,content_include_roots=[str(body)],content_exclude_roots=[str(body/'excluded.txt')],content_file_groups=['TextDocuments'],content_max_file_size_bytes=64,use_system_locate=False,manage_locate_db=False,refresh_interval_hours=0,default_scope='Global'))
 provider_hash=sha(provider)
 def own_daemons():
  found=[]
  for proc in pathlib.Path('/proc').iterdir():
   try:
    if not proc.name.isdigit() or proc.stat().st_uid!=os.getuid():continue
    values=dict(x.split(b'=',1) for x in (proc/'environ').read_bytes().split(b'\0') if b'=' in x)
    if values.get(b'XDG_STATE_HOME')==os.fsencode(root/'state') and b'ottrin-indexd' in (proc/'comm').read_bytes():found.append(int(proc.name))
   except OSError:continue
  return found
 def stop_daemons():
  for pid in own_daemons():os.kill(pid,signal.SIGTERM)
  until=time.monotonic()+10
  while own_daemons():
   if time.monotonic()>until:raise RuntimeError('Owned daemon cleanup timed out')
   time.sleep(.05)
 def save():config.write_text(json.dumps(settings))
 save()
 def cli(*args,cwd=other,ok=0):
  p=subprocess.run([str(app),*args],cwd=cwd,env=env,capture_output=True,text=True,timeout=35)
  assert p.returncode==ok,(args,p.returncode,p.stdout,p.stderr)
  return json.loads(p.stdout) if p.stdout.strip().startswith('{') else p.stderr
 def paths(r):return {pathlib.Path(i['path']).resolve() for i in r['items']}
 try:
  cli('index','update','--json')
  stop_daemons()
  filenames=cli('search',marker,'--json');assert paths(filenames)=={meta/(marker+'.txt')},filenames
  both=cli('search',marker,'--content','--json');assert paths(both)=={hit,meta/(marker+'.txt')},both
  assert both['error'] is None
  settings['search']['default_scope']='CurrentFolder';save()
  scoped=cli('search',marker,'--content','--json',cwd=body);assert paths(scoped)=={hit},scoped
  empty=cli('search',marker,'--content','--json');assert not empty['items'],empty
  settings['search']['default_scope']='Global';save()
  if (stage/'usr/bin/rg').is_file():(stage/'usr/bin/rg').unlink()
  provider.rename(provider.with_name('rg-held'))
  failure=cli('search',marker,'--content','--json',ok=1);assert 'Live content search' in failure['error'] and paths(failure)=={meta/(marker+'.txt')},failure
  assert paths(cli('search',marker,'--json'))=={meta/(marker+'.txt')}
  settings['search']['content_indexing_enabled']=False;save()
  disabled=cli('search',marker,'--content','--json',ok=1);assert 'Settings' in disabled,disabled
  result={'status':'pass','artifact':artifact.name,'sha256':sha(artifact),'provider_identity':provider_identity,'provider_sha256':provider_hash,'checks':['Pinned private provider and all notices','PATH contains only wrapper tools; external RG config ignored','Metadata-only and live content-only roots remain distinct','Size/type/exclusion policy and Unicode paths','Current-folder traversal restriction','Missing provider is a nonzero explicit error preserving filename hits','Disabled content policy is actionable'],'cleanup':'Disposable extraction and XDG profile removed; headless commands all terminal','scope':'Exact extracted-package CLI on host Linux; no distro lifecycle or GUI claim'}
 finally:stop_daemons()

(evidence/('LINUX_CONTENT_'+artifact.suffix.lstrip('.')+'.json')).write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
