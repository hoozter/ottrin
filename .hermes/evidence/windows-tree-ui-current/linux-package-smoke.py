"""Bounded real-daemon smoke in an owned, disposable XDG profile."""
import hashlib,json,os,pathlib,subprocess,tempfile,time

evidence=pathlib.Path(__file__).resolve().parent
artifact=pathlib.Path.cwd()/'dist/packages/Ottrin-0.1.0-x86_64.AppImage'
with tempfile.TemporaryDirectory(prefix='ottrin-watch-') as temp:
    root=pathlib.Path(temp)
    subprocess.run([str(artifact),'--appimage-extract'],cwd=root,stdout=subprocess.DEVNULL,check=True)
    app=root/'squashfs-root/AppRun'
    env=os.environ.copy()
    for name,folder in [('CONFIG','config'),('DATA','data'),('CACHE','cache'),('STATE','state'),('RUNTIME','runtime')]:
        path=root/folder;path.mkdir(mode=0o700);env['XDG_'+name+'_HOME' if name!='RUNTIME' else 'XDG_RUNTIME_DIR']=str(path)
    watched=root/'files';(watched/'original/nested').mkdir(parents=True)
    (watched/'original/nested/first-probe.txt').write_text('preserved bytes')
    config=root/'config/ottrin';config.mkdir()
    # Supply all required application fields so a malformed partial fixture
    # cannot silently select the default Home indexing policy.
    settings=dict(theme='Dark',default_view_mode='Miller',list_columns=[],
                  command_frame_always_visible=False,restore_session=False,
                  show_hidden_files=False,target={'current':None},
                  last_session_dirs=[],search={})
    settings['search'].update(include_roots=[str(watched)],exclude_roots=[],use_system_locate=False,manage_locate_db=False,refresh_interval_hours=0)
    (config/'config.json').write_text(json.dumps(settings))
    def cli(*args):
        return subprocess.run([str(app),*args],env=env,check=True,capture_output=True,text=True,timeout=35).stdout
    def wait_paths(query,expected):
        deadline=time.monotonic()+20
        while True:
            paths=set(cli('search',query).splitlines())
            if paths==set(map(str,expected)):return
            if time.monotonic()>deadline:raise AssertionError((query,paths,expected))
            time.sleep(.25)
    with (root/'daemon.log').open('w') as log:
        daemon=subprocess.Popen([str(app),'--index-daemon'],env=env,stdout=log,stderr=log)
        try:
            deadline=time.monotonic()+10
            while not list((root/'runtime').rglob('*.sock')):
                assert daemon.poll() is None,'Owned daemon exited before readiness'
                if time.monotonic()>deadline:raise TimeoutError('Private daemon socket not ready')
                time.sleep(.05)
            status=json.loads(cli('index','update','--json'))
            assert status['status']=='ready' and status['configured_roots']==1,status
            wait_paths('first-probe',[watched/'original/nested/first-probe.txt'])
            (watched/'original').rename(watched/'renamed')
            wait_paths('first-probe',[watched/'renamed/nested/first-probe.txt'])
            (watched/'new/deep').mkdir(parents=True)
            (watched/'new/deep/second-probe.txt').write_text('new tree')
            wait_paths('second-probe',[watched/'new/deep/second-probe.txt'])
            (watched/'new').rename(watched/'moved')
            wait_paths('second-probe',[watched/'moved/deep/second-probe.txt'])
            assert (watched/'renamed/nested/first-probe.txt').read_text()=='preserved bytes'
            assert daemon.poll() is None
            receipt={'status':'pass','artifact':artifact.name,'sha256':hashlib.file_digest(artifact.open('rb'),'sha256').hexdigest(),'checks':['Private configured root ready','Existing populated tree rename followed without stale path','New nested subtree created and watched','New populated subtree rename followed without stale path','Contents preserved; daemon remained running'],'scope':'Linux packaged CLI and real filesystem watching; no GUI or distro lifecycle claim'}
        finally:
            daemon.terminate()
            try:daemon.wait(timeout=10)
            except subprocess.TimeoutExpired:
                daemon.kill();daemon.wait();raise
            assert daemon.poll() is not None
            (evidence/'LINUX_DAEMON.log').write_text((root/'daemon.log').read_text())
    receipt['cleanup']='Owned daemon exited; private extraction/profile/fixtures removed'
(evidence/'LINUX_DAEMON_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt,indent=2))
