"""Publish the accepted Linux cohort, preserving Windows and managed user state."""
from pathlib import Path
import hashlib,json,os,shutil,signal,socket,struct,subprocess,time
R=Path.cwd();E=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):
    with p.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def run(args,timeout=60):return subprocess.run(list(map(str,args)),check=True,capture_output=True,text=True,timeout=timeout)
def save(name,value):(E/name).write_text(json.dumps(value,indent=2)+'\n')
def passed(name):
    value=read(E/name);assert value['status'].startswith('pass'),name;return value

def consumers(paths):
    result=set()
    for start in range(0,len(paths),40):
        value=subprocess.run(['fuser',*map(str,paths[start:start+40])],capture_output=True,text=True)
        assert value.returncode in (0,1),value.stderr
        result.update(int(pid) for pid in value.stdout.split())
    return result

def unit():
    text=run(['systemctl','--user','show','ottrin-index.service','--property=MainPID','--property=ActiveState','--property=ControlGroup']).stdout
    return dict(line.split('=',1) for line in text.splitlines())

def enabled():return run(['systemctl','--user','is-enabled','ottrin-index.service']).stdout.strip()

def live_status():
    endpoint=Path(os.environ.get('XDG_RUNTIME_DIR',f'/run/user/{os.getuid()}'))/'ottrin/index-v1.sock'
    data=json.dumps({'version':2,'request':{'type':'status'}}).encode()
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(4);client.connect(str(endpoint));client.sendall(struct.pack('!I',len(data))+data)
        def exact(size):
            value=b''
            while len(value)<size:
                part=client.recv(size-len(value));assert part,'Incomplete status reply';value+=part
            return value
        size=struct.unpack('!I',exact(4))[0];assert size<=1024*1024
        value=json.loads(exact(size));assert value['version']==2 and value['response']['type']=='status'
        return value['response']['diagnostics']

for sig in (signal.SIGINT,signal.SIGTERM):signal.signal(sig,lambda *_:(_ for _ in ()).throw(KeyboardInterrupt('Linux promotion cancelled')))
build=read(E/'BUILD_SOURCE.json');source=build['source_revision'];stage=Path(build['stage']);dest=R/'dist/packages';authority=dest/'LINUX_PACKAGES_ACCEPTANCE.json'
assert stage==R/'.hermes/packaging-tmp/.linux-native-desktop-stage'
content=passed('PACKAGE_CONTENT_ACCEPTANCE.json');native=passed('LINUX_PRIVILEGED_COPY_ACCEPTANCE.json');daemon=passed('LINUX_DAEMON_RUNTIME.json')
login=passed('login/ACCEPTANCE.json');replacement=passed('REPLACEMENT_ACCEPTANCE.json')
assert login['source_revision']==replacement['source_revision']==source
assert login['daemon_sha256']==daemon['binary_sha256']
assert len(replacement['cases'])==4
assert all(value['source_revision']==source for value in [content,native,daemon])
assert native['artifact_sha256']==content['artifacts'][native['artifact']]
assert daemon['binary_sha256']==content['runtime_sha256']['index_daemon'] and daemon['idle']['cpu_one_core_percent']<1 and daemon['protocol_shutdowns']==2 and daemon['private_profile_removed']
for capture,digest in native['original_captures_directly_reviewed'].items():assert sha(R/capture)==digest
ci=read(E/'CI_CURRENT.json');assert ci['headSha']==source and ci['status']=='completed' and ci['conclusion']=='success'
assert len(ci['jobs'])==4 and all(job['conclusion']=='success' for job in ci['jobs'])
assert native['updated_helper_sha256']!=native['old_helper_sha256']
assert read(E/'NATIVE_UPGRADE_CONTROLLER.json')['owned_host_input_removed']
checks=passed('SOURCE_CHECKS.json')
# Publication consumes an immutable accepted package, not whatever source happens
# to be under development now. Bind checks to its actual committed Git objects.
assert all(hashlib.sha256(subprocess.check_output(['git','show',f'{source}:{name}'])).hexdigest()==digest for name,digest in checks['source_sha256'].items())
assert content['verification_head']==source
assert content['content_extraction_removed'] and not (R/'.hermes/packaging-tmp/.linux-desktop-extract').exists()
for case in ['native-upgrade','login','replacement']:
    out=E/case/'ubuntu';cleanup=read(out/'VM_CLEANUP.json');assert cleanup['domain_removed'] and len(cleanup['volumes_removed'])==3
    assert not any(out.glob('.native-vm-*')) and not (out/'VM_RESULTS.tar.gz').exists()
assert set(run(['virsh','-c','qemu:///system','list','--all','--name']).stdout.split())=={'win11ltsc','ottrin-copy-cleanup-qa','ottrin-index-qa','ottrin-qa-20261011'}
subprocess.run(['git','cat-file','-e',source+'^{commit}'],check=True,stdout=subprocess.PIPE)
assert hashlib.sha256(subprocess.check_output(['git','show',source+':packaging/verify-release-artifacts.sh'])).hexdigest()==content['verifier_sha256']
old_bytes=authority.read_bytes();assert old_bytes==(E/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').read_bytes();old=read(authority);artifacts=content['artifacts'];assert len(artifacts)==len(old['artifacts'])==4
windows=R/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json';windows_bytes=windows.read_bytes();assert windows_bytes==(E/'UNCHANGED_WINDOWS_CANDIDATE_ACCEPTANCE.json').read_bytes()
win=read(windows);assert all(sha(R/'dist/windows-candidate'/name)==digest for name,digest in win['artifacts'].items())
old_files=[authority];new_files=[]
for name,digest in artifacts.items():
    assert sha(stage/name)==digest and sha(dest/name)==old['artifacts'][name]
    assert (stage/(name+'.sha256')).read_text().split()[0]==digest
    for suffix in ['', '.sha256','.provenance.json']:
        if (dest/(name+suffix)).exists():old_files.append(dest/(name+suffix))
        if (stage/(name+suffix)).exists():new_files.append(stage/(name+suffix))
assert set(stage.iterdir())==set(new_files)
assert all(p.is_file() and not p.is_symlink() and p.stat().st_nlink==1 for p in old_files+new_files)
assert not consumers(new_files)
retired=R/'.hermes/packaging-tmp/.linux-helper-readiness-retired';temporary=authority.with_suffix('.json.next');assert not retired.exists() and not temporary.exists()
before=unit();was_running=before['ActiveState']=='active' and int(before['MainPID'])>0
assert was_running,'Inspect a changed/stopped managed service before this owned runtime update'
for pid in consumers(old_files):assert Path(f'/proc/{pid}/cgroup').read_text().strip().endswith(before['ControlGroup']), 'Unrelated package consumer: preserve it'
config=Path.home()/'.config/ottrin/config.json';database=Path.home()/'.cache/ottrin/search.db'
preferences_sha=sha(config);store=(database.stat().st_dev,database.stat().st_ino);startup=enabled()
intent={'status':'preflight passed; managed service will be quiesced for exact package replacement','source_revision':source,'before_unit':before,'startup_before':startup,'preferences_sha256':preferences_sha,'database_identity':store,'stage':str(stage),'retired':str(retired)};save('PROMOTION_INTENT.json',intent)
manifest={'status':'pass scoped Linux administrator Copy history recovery, helper readiness, native login/reboot and binary replacement delivery; full roadmap incomplete','source_revision':source,'artifacts':artifacts,'evidence':str(E.relative_to(R)),'checks':content['checks'],'hosted_source':{'run':ci['run_id'],'tested_source_revision':source,'all_four_jobs_success':True},'native_privileged_copy':str((E/'LINUX_PRIVILEGED_COPY_ACCEPTANCE.json').relative_to(R)),'daemon_runtime':str((E/'LINUX_DAEMON_RUNTIME.json').relative_to(R)),'runtime_sha256':content['runtime_sha256'],'native_login':str((E/'login/ACCEPTANCE.json').relative_to(R)),'binary_replacement':str((E/'REPLACEMENT_ACCEPTANCE.json').relative_to(R)),'historical_service_basis':'.hermes/evidence/linux-stopped-delivery-current/DELIVERY_VERIFICATION.json','limits':native['limits']+['Four-format content verification; current GUI protected Copy uses actual mounted AppImage. Prior native update/Services matrices retain original hashes/scopes','Installed portable/AppImage helper requires Update / reinstall helper to receive its new binary; no automatic privileged installation','Windows remains458/MSIX17; no signing, public release, Store submission or full-roadmap completion']}
manifest['limits'] = [
    'Current helper/Copy uses actual AppImage/FUSE; login/reboot uses native DEB; four-format contents/closure and namespace gates have their explicit scopes',
    'Actual f8-to-aa AppImage/portable replacement preserves enabled/disabled startup and Grid preference; future unknown schema/version transitions are not inferred',
    'Linux administrator history restart/load/Undo/cleanup passes the stated native scope; Windows privileged Copy remains unfinished',
    'Whole U08/theme audit, settled startup status, physical graphics, Wayland and final daily-driver acceptance remain open',
    'Private1000-file idle sample is not a personal-index or large-corpus resource measurement',
    'Personal helper installation requires owner authentication; no automatic privileged installation was performed',
    'Windows remains458/MSIX17; no signing, public release or Store submission',
    'Packaged development manuals are build-time snapshots ataa241633; repository delivery receipts are authoritative for subsequent acceptance/publication'
]
undo=[];stopped=False;published=False;receipt={'status':'starting accepted Linux promotion','source_revision':source,'artifacts':artifacts,'windows_preserved':True}
def move(a,b):
    a.replace(b);undo.append((b,a));save('PROMOTION_IN_FLIGHT.json',{'moves':[(str(a),str(b)) for a,b in undo],'managed_service_restart_required':True})
try:
    run(['systemctl','--user','stop','ottrin-index.service']);stopped=True
    assert unit()['ActiveState']=='inactive' and not consumers(old_files)
    assert sha(config)==preferences_sha and enabled()==startup
    retired.mkdir()
    for path in old_files:move(path,retired/path.name)
    for path in new_files:move(path,dest/path.name)
    temporary.write_text(json.dumps(manifest,indent=2)+'\n');temporary.replace(authority);published=True
    assert all(sha(dest/name)==digest for name,digest in artifacts.items()) and windows.read_bytes()==windows_bytes
    run(['systemctl','--user','start','ottrin-index.service'])
    deadline=time.monotonic()+60;last_error=None
    while time.monotonic()<deadline:
        try:
            after=unit();assert after['ActiveState']=='active' and int(after['MainPID'])>0
            assert sha(Path('/proc')/after['MainPID']/'exe')==daemon['binary_sha256']
            diagnostics=live_status()
            if diagnostics['status']=='Ready':break
        except (OSError,AssertionError,subprocess.SubprocessError) as error:last_error=repr(error)
        time.sleep(1)
    else:raise RuntimeError(f'Current managed daemon readiness failed: {last_error}')
    assert sha(config)==preferences_sha and enabled()==startup and (database.stat().st_dev,database.stat().st_ino)==store
    receipt.update(status='pass promoted exact four-format Linux cohort and restarted current managed daemon',managed_service={'before':before,'after':after,'binary_sha256':daemon['binary_sha256'],'ready':diagnostics['status'],'startup_preserved':True,'preferences_preserved':True,'database_identity_preserved':True})
except BaseException as error:
    receipt.update(status='promotion failed; restoring previous packages and managed runtime',error=repr(error))
    if stopped:run(['systemctl','--user','stop','ottrin-index.service'])
    if published:authority.unlink()
    temporary.unlink(missing_ok=True)
    for current,previous in reversed(undo):
        if current.exists():current.replace(previous)
    if retired.exists() and not any(retired.iterdir()):retired.rmdir()
    if stopped:run(['systemctl','--user','start','ottrin-index.service'])
    receipt['previous_artifacts_and_authority_restored']=authority.read_bytes()==old_bytes and all(sha(dest/name)==digest for name,digest in old['artifacts'].items())
    save('PROMOTION.json',receipt);raise
else:
    retired_files=[p for p in retired.iterdir() if p.is_file()]
    assert not consumers(retired_files),'A newly arrived old-package consumer must be recorded before retirement'
    receipt['superseded_files_removed']=[{'name':p.name,'bytes':p.stat().st_size,'sha256':sha(p)} for p in retired_files]
    shutil.rmtree(retired);stage.rmdir();receipt['stage_and_retired_removed']=True
    assert windows.read_bytes()==windows_bytes
    save('PROMOTION.json',receipt)
    (E/'PROMOTION_IN_FLIGHT.json').unlink()
    print(json.dumps({key:value for key,value in receipt.items() if key!='superseded_files_removed'},indent=2))
