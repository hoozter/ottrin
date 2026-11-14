"""Publish the accepted Linux cohort, preserving Windows and managed user state."""
from pathlib import Path
import hashlib,json,os,shutil,signal,socket,struct,subprocess,time,sys
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
parser=argparse.ArgumentParser();parser.add_argument('--publish',action='store_true');parser.add_argument('--ci-run',type=int);args=parser.parse_args()
build=read(E/'BUILD_SOURCE.json');source=build['source_revision'];stage=Path(build['stage']);dest=R/'dist/packages';authority=dest/'LINUX_PACKAGES_ACCEPTANCE.json'
assert stage==R/'.hermes/packaging-tmp/.linux-pdf-concurrency-stage'
content=passed('PACKAGE_CONTENT_ACCEPTANCE.json')
previous=read(E/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json')
assert content['source_revision']==content['verification_head']==source
assert content['content_extraction_removed']
image_name='Ottrin-0.1.0-x86_64.AppImage'
desktop_passes = {}
for artifact, label in [(image_name, 'appimage'), ('Ottrin-0.1.0-linux-x86_64-portable.tar.gz', 'portable')]:
    path = E.parent / 'release-audit-current' / f'linux-{source[:8]}-{label}' / 'CONTROL.json'
    desktop = read(path)
    assert desktop['status'] == 'pass within actual workflow scope'
    assert desktop['runtime_source'] == source
    assert desktop['artifact_sha256'] == content['artifacts'][artifact]
    assert desktop['teardown']['profile_removed']
    assert sum(row['check'] == 'quick-inspector-pdf' for row in desktop['checks']) >= 2
    assert all(row['status'] == 'passed' for row in desktop['checks'])
    if label == 'portable':
        assert desktop['portable_extraction_removed']
    desktop_passes[label] = str(path.relative_to(R))
theme=passed('THEME_COMPONENT_ACCEPTANCE.json')
assert theme['source_revision']==source and theme['appimage_sha256']==content['artifacts'][image_name]
assert theme['app_binary_sha256']==content['runtime_sha256']['application']
fields={'Window corners','Surface corners','Text size','Button corners','Window border thickness','Neutral text blend'}
for case, palette, scale in [('candidate-dark100','Dark',1.0),('candidate-light150','Light',1.5)]:
    result=theme['cases'][case]
    assert result['theme']==palette and result['scale']==scale
    assert {field['name'] for field in result['cases']}==fields
    assert result['named_selector_used'] and result['unsaved_numeric_edit_not_persisted']
    assert result['source_preserved'] and result['profile_removed']
runtime=content['runtime_sha256']
assert all(runtime[key]==previous['runtime_sha256'][key] for key in ('index_daemon','privileged_helper','html_renderer'))
helper=passed('LINUX_HELPER_RUNTIME.json')
assert helper['sha256']==runtime['privileged_helper'] and len(helper['cases'])==10
if args.ci_run:
 ci=json.loads(run(['gh','run','view',str(args.ci_run),'--json','status,conclusion,headSha,jobs']).stdout);ci['run_id']=args.ci_run
 assert ci['headSha']==source
else:
 ci={'run_id':None,'headSha':None,'jobs':[],'status':'source-equivalent run not yet dispatched; preceding main run still active','conclusion':None}
save('CI_CURRENT.json',ci)
required_linux_jobs={'verify (ubuntu-latest)', 'search-performance (ubuntu-latest)'}
linux_jobs=[job for job in ci['jobs'] if job['name'] in required_linux_jobs]
ci_ready=(ci['headSha']==source and {job['name'] for job in linux_jobs}==required_linux_jobs
          and len(linux_jobs)==len(required_linux_jobs)
          and all(job['status']=='completed' and job['conclusion']=='success' for job in linux_jobs))
old_bytes=authority.read_bytes();assert old_bytes==(E/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').read_bytes();old=read(authority);artifacts=content['artifacts'];assert len(artifacts)==len(old['artifacts'])==4
windows=R/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json';windows_bytes=windows.read_bytes();assert windows_bytes==(E/'PREVIOUS_WINDOWS_CANDIDATE_ACCEPTANCE.json').read_bytes()
win=read(windows);assert all(sha(R/'dist/windows-candidate'/n)==d for n,d in win['artifacts'].items())
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
before=unit();was_running=before['ActiveState']=='active' and int(before['MainPID'])>0;assert was_running
image=dest/'Ottrin-0.1.0-x86_64.AppImage'
assert not consumers([path for path in old_files if path!=image]), 'Unexpected non-AppImage consumer'
atomic=passed('ATOMIC_APPIMAGE_QA.json')
assert atomic['old_window_operated_after_replacement'] and atomic['next_launch_new_binary']
assert atomic['previous_image_sha256']==old['artifacts'][image.name]
assert atomic['new_image_sha256']==artifacts[image.name]
assert stage.stat().st_dev==dest.stat().st_dev==retired.parent.stat().st_dev
assert sha(Path('/proc')/before['MainPID']/'exe')==runtime['index_daemon']
assert live_status()['status']=='Ready'
image_consumers=sorted(consumers([image]))
for pid in image_consumers:
    assert sha(Path('/proc')/str(pid)/'exe')==old['artifacts'][image.name], 'Unexpected AppImage holder'
config=Path.home()/'.config/ottrin/config.json';database=Path.home()/'.cache/ottrin/search.db'
preferences_sha=sha(config);store=(database.stat().st_dev,database.stat().st_ino);startup=enabled()
preflight={'status':'ready for guarded atomic publication' if ci_ready else 'scoped preflight passes; source-equivalent CI pending','source_revision':source,'ci_run':ci['run_id'],'ci_ready':ci_ready,'stage':str(stage),'current_managed_unit':before,'image_consumers':image_consumers,'mutation_performed':False}
save('PROMOTION_PREFLIGHT.json',preflight)
if not args.publish:print(json.dumps(preflight,indent=2));raise SystemExit(0)
assert ci_ready,'Both source-equivalent Linux CI jobs must pass before promotion'
save('PROMOTION_INTENT.json',{'status':'preflight passed; replace paths atomically while existing mounts retain their original inode','source_revision':source,'before_unit':before,'startup_before':startup,'preferences_sha256':preferences_sha,'database_identity':store,'stage':str(stage),'retired':str(retired)})
manifest=dict(
    status='pass Linux packaged desktop and focused theme controls delivery',
    desktop_workflows=desktop_passes,
    source_revision=source,artifacts=artifacts,evidence=str(E.relative_to(R)),
    checks=content['checks'],runtime_sha256=runtime,
    hosted_source={'run':ci['run_id'],'tested_source_revision':ci['headSha'],
                   'non_runtime_diff':[],'linux_jobs':{job['name']:job['conclusion'] for job in linux_jobs}},
    source_scope=str((E/'BUILD_SOURCE.json').relative_to(R)),
    mounted_theme_component=str((E/'THEME_COMPONENT_ACCEPTANCE.json').relative_to(R)),
    current_helper_read_only=[str((E/'LINUX_HELPER_RUNTIME.json').relative_to(R))],
    previous_cohort={'source_revision':previous['source_revision'],
                     'receipt':str((E/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').relative_to(R))},
    limits=[
        'Mounted X11 checks cover the named component selector and six numeric fields in Dark 100% and Light 150%, including typing, restore and Cancel.',
        'Other portal, theme and native distro receipts retain their recorded package scopes.',
        'Daemon, helper and HTML renderer bytes match the previous cohort.',
        f'Packaged manuals are build-time snapshots at {source[:8]}; repository metadata records later delivery.',
    ],
)

applied=[]
published=False
receipt={'status':'starting accepted Linux promotion','source_revision':source,'artifacts':artifacts,'windows_preserved':True}
retired.mkdir()
try:
    for path in old_files:
        os.link(path,retired/path.name)
    assert all((dest/p.name).stat().st_ino==p.stat().st_ino for p in retired.iterdir())
    assert {path.name for path in old_files}=={path.name for path in new_files}|{authority.name}
    for path in new_files:
        path.replace(dest/path.name)
        applied.append(path)
        save('PROMOTION_IN_FLIGHT.json',{'replaced':[p.name for p in applied],'managed_service_restart_required':False})
    temporary.write_text(json.dumps(manifest,indent=2)+'\n')
    temporary.replace(authority)
    published=True
    assert all(sha(dest/name)==digest for name,digest in artifacts.items()) and windows.read_bytes()==windows_bytes
    after=unit()
    diagnostics=live_status()
    assert after==before and diagnostics['status']=='Ready'
    assert sha(Path('/proc')/after['MainPID']/'exe')==runtime['index_daemon']
    assert enabled()==startup and (database.stat().st_dev,database.stat().st_ino)==store
    still_running=[]
    for pid in image_consumers:
        executable=Path('/proc')/str(pid)/'exe'
        if executable.exists():
            assert sha(executable)==old['artifacts'][image.name]
            still_running.append(pid)
    receipt.update(status='pass promoted exact four-format Linux cohort without restarting existing app or managed index',existing_appimage_holders_preserved=still_running,managed_service={'before':before,'after':after,'binary_sha256':runtime['index_daemon'],'ready':diagnostics['status'],'startup_preserved':True,'preferences_bytes_match_at_verification':sha(config)==preferences_sha,'database_identity_preserved':True})
except BaseException as error:
    receipt.update(status='promotion failed; restoring previous package paths',error=repr(error))
    if published:(retired/authority.name).replace(authority)
    temporary.unlink(missing_ok=True)
    for path in reversed(applied):
        os.link(dest/path.name,path)
        (retired/path.name).replace(dest/path.name)
    shutil.rmtree(retired)
    receipt['previous_artifacts_and_authority_restored']=authority.read_bytes()==old_bytes and all(sha(dest/name)==digest for name,digest in old['artifacts'].items())
    save('PROMOTION.json',receipt)
    raise
else:
    retired_files=list(retired.iterdir())
    receipt['superseded_files_removed']=[{'name':p.name,'bytes':p.stat().st_size,'sha256':sha(p)} for p in retired_files]
    shutil.rmtree(retired)
    stage.rmdir()
    receipt['stage_and_retired_removed']=True
    assert windows.read_bytes()==windows_bytes
    save('PROMOTION.json',receipt)
    (E/'PROMOTION_IN_FLIGHT.json').unlink()
    print(json.dumps({key:value for key,value in receipt.items() if key!='superseded_files_removed'},indent=2))
