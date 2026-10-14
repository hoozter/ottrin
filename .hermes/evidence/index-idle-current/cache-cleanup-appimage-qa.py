"""Exact AppImage CLI cleanup failure/retry, using only a disposable profile."""
import hashlib, json, os, pathlib, signal, subprocess, tempfile, socket, struct, sys, time
repo=pathlib.Path('/home/campbell/Projects/Ottrin')
artifact=pathlib.Path(sys.argv[1]).resolve()
receipt_path=pathlib.Path(sys.argv[2]).resolve()
provenance_record=json.loads(pathlib.Path(str(artifact)+'.provenance.json').read_text())
assert provenance_record['built_from_tracked_clean_source']
assert provenance_record['sha256']==hashlib.sha256(artifact.read_bytes()).hexdigest()
expected=provenance_record['revision']
def cancelled(signum, _frame):raise SystemExit(128+signum)
for sig in (signal.SIGINT,signal.SIGTERM):signal.signal(sig,cancelled)
receipt={'artifact_sha256':hashlib.sha256(artifact.read_bytes()).hexdigest(),'expected_revision':expected}

def run(args, env, cwd):
    child=subprocess.Popen(args,env=env,cwd=cwd,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,start_new_session=True)
    try:
        out,err=child.communicate(timeout=30)
        return child.returncode,out,err
    finally:
        if child.poll() is None:
            os.killpg(child.pid,signal.SIGTERM)
            try:child.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid,signal.SIGKILL);child.communicate()

with tempfile.TemporaryDirectory(prefix='ottrin-cleanup-package-') as name:
    root=pathlib.Path(name)
    env=os.environ.copy()
    for key,relative in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_DATA_HOME','data'),('XDG_STATE_HOME','state'),('XDG_RUNTIME_DIR','runtime')]:
        directory=root/relative;directory.mkdir(mode=0o700);env[key]=str(directory)
    env['DBUS_SESSION_BUS_ADDRESS']='unix:path='+str(root/'nonexistent-bus')
    env['OTTRIN_DEV_HELPER_AUTOBUILD']='0'
    code,out,err=run([str(artifact),'--appimage-extract','usr/share/doc/ottrin/SOURCE_REVISION.txt'],env,root)
    assert code==0,(code,err)
    provenance=(root/'squashfs-root/usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text()
    assert 'Git revision: '+expected in provenance,provenance
    assert 'Working tree: clean' in provenance,provenance
    receipt['source_provenance']=provenance
    config=root/'config/ottrin/config.json';config.parent.mkdir();config.write_text('{"show_hidden_files":true}\n')
    before=config.read_bytes()
    cache=root/'cache/ottrin/preview_cache';blocked=cache/'blocked';blocked.mkdir(parents=True)
    source=root/'source.txt';source.write_bytes(b'PRESERVE USER SOURCE')
    nested=blocked/'nested.txt';nested.write_bytes(b'PRESERVE NESTED DATA')
    (cache/'thumbnail').write_bytes(b'cached bytes')
    endpoint=root/'runtime/ottrin/index-v1.sock'
    database=root/'cache/ottrin/search.db'
    assert not (root/'config/systemd/user/ottrin-index.service').exists()
    def request(kind):
        payload=json.dumps({'version':1,'request':{'type':kind}}).encode()
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(3);client.connect(str(endpoint))
            client.sendall(struct.pack('!I',len(payload))+payload)
            def exact(count):
                data=b''
                while len(data)<count:
                    part=client.recv(count-len(data));assert part
                    data+=part
                return data
            size=struct.unpack('!I',exact(4))[0];assert size<=1024*1024
            return json.loads(exact(size))['response']
    daemon=subprocess.Popen([str(artifact),'--index-daemon'],env=env,cwd=root,
                            stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True)
    try:
        deadline=time.monotonic()+15
        while True:
            assert daemon.poll() is None,'daemon exited before readiness'
            diagnostics=request('status').get('diagnostics',{}) if endpoint.exists() else {}
            if diagnostics.get('status')=='Ready':break
            assert time.monotonic()<deadline,'daemon readiness timeout: '+repr(diagnostics)
            time.sleep(.05)
        database_before=hashlib.sha256(database.read_bytes()).hexdigest()
        code,out,err=run([str(artifact),'--uninstall'],env,root)
        assert code==1,(code,out,err)
        assert str(blocked) in err,(out,err)
        assert nested.read_bytes()==b'PRESERVE NESTED DATA'
        assert source.read_bytes()==b'PRESERVE USER SOURCE' and config.read_bytes()==before
        assert daemon.poll() is None and request('ping')['type']=='pong'
        assert hashlib.sha256(database.read_bytes()).hexdigest()==database_before
        receipt['same_live_daemon_after_failure']=True
        nested.unlink();blocked.rmdir()
        (cache/'retry-thumbnail').write_bytes(b'retry cache bytes')
        code2,out2,err2=run([str(artifact),'--uninstall'],env,root)
        assert code2==0,(code2,out2,err2)
        assert daemon.wait(timeout=5)==0
        assert not endpoint.exists() and not database.exists()
        receipt['session_daemon_stopped']=True
        assert not list(cache.iterdir())
        assert source.read_bytes()==b'PRESERVE USER SOURCE' and config.read_bytes()==before
        receipt['checks']=['Exact AppImage cleanup exits 1 and identifies blocked entry','Nested data, user source and preferences preserved','Failed cleanup retains the same live session daemon and database bytes','Retry exits 0, stops unscheduled session daemon and removes generated index/cache files']
        receipt['exit_codes']=[code,code2]
    finally:
        if daemon.poll() is None:
            os.killpg(daemon.pid,signal.SIGTERM)
            try:daemon.wait(timeout=5)
            except subprocess.TimeoutExpired:os.killpg(daemon.pid,signal.SIGKILL);daemon.wait()
receipt['cleanup']={'private_profile_removed':not root.exists(),'owned_commands_exited':True}
receipt_path.write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt,indent=2))
