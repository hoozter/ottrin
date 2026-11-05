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

def enabled():
    value=subprocess.run(['systemctl','--user','is-enabled','ottrin-index.service'],capture_output=True,text=True,timeout=30)
    state=value.stdout.strip()
    assert value.returncode in (0,1) and state in {'enabled','disabled'},(value.returncode,state,value.stderr)
    return state

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
import argparse
parser=argparse.ArgumentParser();parser.add_argument('--publish',action='store_true');args=parser.parse_args()
build=read(E/'BUILD_SOURCE.json');source=build['source_revision'];stage=Path(build['stage']);dest=R/'dist/packages';authority=dest/'LINUX_PACKAGES_ACCEPTANCE.json'
assert stage==R/'.hermes/packaging-tmp/.linux-native-desktop-stage'
content=passed('PACKAGE_CONTENT_ACCEPTANCE.json');previous=read(E/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json')
native=passed('NATIVE_UI_ACCEPTANCE.json');identity=passed('DAEMON_IDENTITY_ACCEPTANCE.json')
assert content['source_revision']==native['source_revision']==identity['source_revision']==source
changed=run(['git','diff','--name-only',previous['source_revision'],source,'--','crates']).stdout.splitlines()
assert set(changed)=={'crates/ottrin-ui/src/file_row.rs','crates/ottrin-ui/src/lib.rs','crates/ottrin-ui/src/theme_colors.rs'}
save('SOURCE_SCOPE.json',{'source_revision':source,'previous_source_revision':previous['source_revision'],'changed_crate_files':changed,'scope':'Current row/icon/thumbnail spacing, selected footer text color and Ready status appearance. Full interface and roadmap acceptance remain open.'})
assert native['appimage_input']['sha256']==content['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
assert native['appimage_input']['gui_sha256']==content['runtime_sha256']['application']
assert native['appimage_input']['daemon_sha256']==identity['current_binary_sha256']==content['runtime_sha256']['index_daemon']==previous['runtime_sha256']['index_daemon']
assert not identity['freshly_rerun'] and identity['historical_runtime_source']==previous['source_revision']
historical=read(R/identity['historical_runtime_receipt']);assert historical['status'].startswith('pass') and historical['binary_sha256']==identity['current_binary_sha256']
assert read(E/identity['retained_copy'])==historical
for name,digest in native['original_captures_directly_reviewed'].items():assert sha(E/'native/ubuntu/results'/name)==digest
assert len(native['original_captures_directly_reviewed'])==16 and native['consumed_transport_removed'] and native['owned_domain_and_volumes_removed']
assert native['sources_preserved'] and native['database_identity_preserved']
assert content['verification_head']==source and content['content_extraction_removed']
assert not (R/'.hermes/packaging-tmp/.linux-desktop-extract').exists()
assert hashlib.sha256(subprocess.check_output(['git','show',source+':packaging/verify-release-artifacts.sh'])).hexdigest()==content['verifier_sha256']
for name in ['privileged_helper','html_renderer']:assert previous['runtime_sha256'][name]==content['runtime_sha256'][name]
daemon={'binary_sha256':identity['current_binary_sha256']}
ci=json.loads(run(['gh','run','view','41966597535','--json','status,conclusion,headSha,jobs']).stdout);ci['run_id']=41966597535;save('CI_CURRENT.json',ci)
assert ci['headSha']==source and len(ci['jobs'])==4
ci_ready=ci['status']=='completed' and ci['conclusion']=='success' and all(job['conclusion']=='success' for job in ci['jobs'])
old_bytes=authority.read_bytes();assert old_bytes==(E/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').read_bytes();old=read(authority);artifacts=content['artifacts'];assert len(artifacts)==len(old['artifacts'])==4
windows=R/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json';windows_bytes=windows.read_bytes();assert windows_bytes==(E/'PREVIOUS_WINDOWS_CANDIDATE_ACCEPTANCE.json').read_bytes()
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
retired=R/'.hermes/packaging-tmp/.linux-current-retired';temporary=authority.with_suffix('.json.next');assert not retired.exists() and not temporary.exists()
before=unit();was_running=before['ActiveState']=='active' and int(before['MainPID'])>0
assert was_running,'Inspect a changed/stopped managed service before this owned runtime update'
for pid in consumers(old_files):assert Path(f'/proc/{pid}/cgroup').read_text().strip().endswith(before['ControlGroup']), 'Unrelated package consumer: preserve it'
config=Path.home()/'.config/ottrin/config.json';database=Path.home()/'.cache/ottrin/search.db'
preferences_sha=sha(config);store=(database.stat().st_dev,database.stat().st_ino);startup=enabled()
preflight={'status':'ready for guarded publication' if ci_ready else 'scoped preflight passes; exact-source CI still pending','source_revision':source,'ci_run':ci['run_id'],'ci_ready':ci_ready,'stage':str(stage),'current_managed_unit':before,'mutation_performed':False}
save('PROMOTION_PREFLIGHT.json',preflight)
if not args.publish:
    print(json.dumps(preflight,indent=2));raise SystemExit(0)
assert ci_ready,'Do not publish before all four exact-source hosted jobs pass'
intent={'status':'preflight passed; managed service will be quiesced for exact package replacement','source_revision':source,'before_unit':before,'startup_before':startup,'preferences_sha256':preferences_sha,'database_identity':store,'stage':str(stage),'retired':str(retired)};save('PROMOTION_INTENT.json',intent)
manifest={
 'status':'pass scoped row/thumbnail/footer/status appearance delivery; full roadmap and UI audit incomplete',
 'source_revision':source,'artifacts':artifacts,'evidence':str(E.relative_to(R)),
 'checks':content['checks'],'runtime_sha256':content['runtime_sha256'],
 'hosted_source':{'run':ci['run_id'],'tested_source_revision':source,'all_four_jobs_success':True},
 'native_current_ui':str((E/'NATIVE_UI_ACCEPTANCE.json').relative_to(R)),
 'native_services':previous['native_services'],
 'native_captions_and_footer':previous['native_captions_and_footer'],
 'native_saved_theme_name':previous['native_saved_theme_name'],
 'source_scope':str((E/'SOURCE_SCOPE.json').relative_to(R)),
 'daemon_identity':str((E/'DAEMON_IDENTITY_ACCEPTANCE.json').relative_to(R)),
 'daemon_runtime':identity['historical_runtime_receipt'],
 'daemon_runtime_scope':{'freshly_rerun':False,'original_source_revision':identity['historical_runtime_source'],'exact_current_binary_identity':True},
 'native_privileged_copy':previous['native_privileged_copy'],
 'retained_native_scope_source':previous['retained_native_scope_source'],
 'historical_daemon_evidence':previous['historical_daemon_evidence'],
 'limits':[
  'Actual AppImage software-X11 Ubuntu guest only; no physical graphics, Wayland or full interface acceptance.',
  'All16 current original row/thumbnail/footer/Services frames directly reviewed. Prior captions/startup toggles keep original source scope.',
  'Exact unchanged daemon bytes retain prior private watcher/query/restart/shutdown/idle evidence; no new sustained or large-corpus sample.',
  'Unchanged helper/HTML bytes retain original protected-Copy scope. Personal helper setup requires owner OS authentication.',
  'No public release, signing, Store or MSI acceptance.',
  f'Packaged manuals are build-time snapshots at {source[:8]}; repository metadata records later delivery.'
 ]
}

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
