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
  subprocess.run([str(base/'.hermes/packaging-tmp/preview-refresh/Ottrin-0.1.0-x86_64.AppImage'),'--appimage-extract'],cwd=control,stdout=subprocess.DEVNULL,check=True)
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
 def wait_saved(term, expected):
  until=time.monotonic()+15
  while True:
   answer=cli('search',term+' !neverpresentterm','--content','--json')
   assert answer['error'] is None,answer
   if paths(answer)==set(expected):return
   assert time.monotonic()<until,(term,answer,expected)
   time.sleep(.05)
 try:
  status=cli('index','update','--json');assert status['configured_roots']==2,status
  wait_saved(marker,[hit])
  hit.write_text('updatedneedle');wait_saved('updatedneedle',[hit]);wait_saved(marker,[])
  nested=body/'new/deep';nested.mkdir(parents=True);(nested/'document.txt').write_text('watcherneedle')
  wait_saved('watcherneedle',[nested/'document.txt'])
  (body/'new').rename(body/'moved');wait_saved('watcherneedle',[body/'moved/deep/document.txt'])
  shutil.rmtree(body/'moved');wait_saved('watcherneedle',[])
  hit.write_text(marker);wait_saved(marker,[hit])
  settings['search']['content_indexing_enabled']=False;save()
  status=cli('index','update','--json');assert status['configured_roots']==1 and status['watched_roots']==1,status
  settings['search']['content_indexing_enabled']=True;settings['search']['include_roots']=[];save()
  status=cli('index','update','--json');assert status['configured_roots']==1,status
  wait_saved(marker,[hit]);assert not cli('search',marker,'--json')['items']
  settings['search']['include_roots']=[str(meta)];save();cli('index','update','--json');wait_saved(marker,[hit])
  # Measure only this tiny fixture's settled daemon, never the operator service.
  time.sleep(2)
  daemons=own_daemons();assert len(daemons)==1,daemons
  daemon=daemons[0]
  def cpu_ticks():
   fields=pathlib.Path('/proc',str(daemon),'stat').read_text().rsplit(')',1)[1].split()
   return int(fields[11])+int(fields[12])
  before=cpu_ticks();started=time.monotonic();time.sleep(5)
  idle_cpu_percent=(cpu_ticks()-before)/os.sysconf('SC_CLK_TCK')/(time.monotonic()-started)*100
  assert idle_cpu_percent<1.0,('Tiny settled content fixture CPU',idle_cpu_percent)
  page_files=[]
  for index in range(137):
   folder=body/('group-'+str(index%3));folder.mkdir(exist_ok=True)
   file=folder/('document-%03d.txt'%(index//3));file.write_text('pagebodyneedle');page_files.append(file)
  settings['search']['default_sort']='Name';save();cli('index','update','--json')
  def check_pages(expected):
   expected=sorted(expected,key=lambda path:(path.name.lower(),os.fsencode(path)))
   all_hits=cli('search','pagebodyneedle','--content','--json','--limit','500','--offset','0')
   assert all_hits['error'] is None and all_hits['total']==len(expected),all_hits
   assert [pathlib.Path(item['path']) for item in all_hits['items']]==expected
   for offset in range(0,len(expected)+17,17):
    page=cli('search','pagebodyneedle','--content','--json','--limit','17','--offset',str(offset))
    assert page['error'] is None and page['total']==len(expected),page
    assert [pathlib.Path(item['path']) for item in page['items']]==expected[offset:offset+17],(offset,page)
  check_pages(page_files)
  assert '--limit' in cli('search','pagebodyneedle','--limit','0',ok=1)
  stop_daemons()
  # No daemon can index these new paths. The live source must merge all of them
  # with the saved matches, not truncate before common sorting/pagination.
  for index in range(23):
   file=body/('live-%02d.txt'%index);file.write_text('pagebodyneedle');page_files.append(file)
  check_pages(page_files)
  filenames=cli('search',marker,'--json');assert paths(filenames)=={meta/(marker+'.txt')},filenames
  both=cli('search',marker,'--content','--json');assert paths(both)=={hit,meta/(marker+'.txt')},both
  assert both['error'] is None
  settings['search']['default_scope']='CurrentFolder';save()
  scoped=cli('search',marker,'--content','--json',cwd=body);assert paths(scoped)=={hit},scoped
  empty=cli('search',marker,'--content','--json');assert not empty['items'],empty
  settings['search']['default_scope']='Global';save()
  if (stage/'usr/bin/rg').is_file():(stage/'usr/bin/rg').unlink()
  provider.rename(provider.with_name('rg-held'))
  failure=cli('search',marker,'--content','--json',ok=1);assert 'Live content search' in failure['error'] and paths(failure)=={meta/(marker+'.txt'),hit},failure
  assert paths(cli('search',marker,'--json'))=={meta/(marker+'.txt')}
  settings['search']['content_indexing_enabled']=False;save()
  disabled=cli('search',marker,'--content','--json',ok=1);assert 'Settings' in disabled,disabled
  result={'status':'pass','artifact':artifact.name,'sha256':sha(artifact),'provider_identity':provider_identity,'provider_sha256':provider_hash,'idle_cpu_percent_single_core':idle_cpu_percent,'idle_cpu_scope':'Five seconds after settling; tiny private two-root fixture only','checks':['Pinned private provider and all notices','PATH contains only wrapper tools; external RG config ignored','Metadata-only and live content-only roots remain distinct','Size/type/exclusion policy and Unicode paths','Current-folder traversal restriction','Missing provider is a nonzero explicit error preserving indexed filename/content hits','Disabled content policy is actionable','Saved-only negated queries verify content-root extraction and real watcher edits/moves/deletion','Disabling content detaches its watch; zero metadata roots retain content without filename membership'],'cleanup':'Disposable extraction and XDG profile removed; headless commands all terminal','pagination':{'saved_live_matches':137,'saved_plus_new_live_matches':160,'page_size':17,'order':'Name then path','all_pages_and_beyond_end':'pass','invalid_limit':'rejected'},'scope':'Exact extracted-package CLI on host Linux; no distro lifecycle or GUI claim'}
 finally:stop_daemons()

(evidence/('LINUX_CONTENT_'+artifact.suffix.lstrip('.')+'.json')).write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
