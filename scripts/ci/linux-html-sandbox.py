#!/usr/bin/env python3
"""Install/retire production HTML integration only on a disposable hosted runner."""
from pathlib import Path
import hashlib,json,os,signal,subprocess,sys,tempfile

assert (os.environ.get('GITHUB_ACTIONS')=='true' and os.environ.get('RUNNER_OS')=='Linux'
        and os.environ.get('RUNNER_ENVIRONMENT')=='github-hosted'), 'Disposable hosted Linux runner required'
root=Path.cwd();out=root/'html-sandbox-evidence';out.mkdir(exist_ok=True)
state_path=Path(os.environ['RUNNER_TEMP'])/'ottrin-html-ci-state.json'
policy=Path('/etc/apparmor.d/ottrin-html-render')
def run(args,**kw):return subprocess.run(args,check=True,**kw)
def sha(path):return hashlib.file_digest(Path(path).open('rb'),'sha256').hexdigest()
def label_loaded():
    return any(line.startswith('ottrin-html-render ') for line in run(['sudo','cat','/sys/kernel/security/apparmor/profiles'],capture_output=True,text=True).stdout.splitlines())
def stop_units():
    listing=run(['systemctl','--user','list-units','--all','--plain','--no-legend','--no-pager','ottrin-render-*.service'],capture_output=True,text=True).stdout
    for line in listing.splitlines():run(['systemctl','--user','stop',line.split()[0]])
def cleanup():
    if not state_path.exists():return
    state=json.loads(state_path.read_text());stop_units()
    if label_loaded():
        # The state is written only after this profile was confirmed absent.
        run(['sudo','apparmor_parser','-R',str(root/'packaging/linux/ottrin-html-render.apparmor')])
    for name,expected in state['owned_files'].items():
        path=Path(name)
        if not path.exists():continue
        assert not path.is_symlink() and sha(path)==expected, 'Owned integration file changed: '+name
        run(['sudo','rm','--',name])
    for name in reversed(state['new_directories']):run(['sudo','rmdir','--',name])
    assert Path('/proc/sys/kernel/apparmor_restrict_unprivileged_userns').read_text().strip()==state['restriction']
    state['status']='cleaned';state['profile_unloaded']=not label_loaded();assert state['profile_unloaded']
    (out/'TEARDOWN.json').write_text(json.dumps(state,indent=2)+'\n');state_path.unlink()
def cancelled(*_):raise KeyboardInterrupt('HTML setup cancelled')
for signum in [signal.SIGINT,signal.SIGTERM]:signal.signal(signum,cancelled)
if sys.argv[1:]==['cleanup']:
    cleanup();sys.exit(0)
assert sys.argv[1:]==['prepare']
assert not state_path.exists() and not label_loaded()
restriction=Path('/proc/sys/kernel/apparmor_restrict_unprivileged_userns').read_text().strip();assert restriction=='1'
# Root-owned runner Chrome is shipped world-writable in this image. Repair only
# permissions on the disposable installation; never accept mutable browser code.
browser=Path('/opt/google/chrome/chrome');before=sha(browser)
for directory in ['/opt','/opt/google','/opt/google/chrome']:assert Path(directory).stat().st_uid==0
run(['sudo','chmod','go-w','/opt','/opt/google'])
run(['sudo','chmod','-R','go-w','/opt/google/chrome']);assert sha(browser)==before
try:
    run(['cargo','build','-p','ottrin-preview','--bin','ottrin-html-render'])
    run(['cargo','build','-p','ottrin-platform','--bin','ottrin-priv-helper'])
    files={
        '/usr/libexec/ottrin/ottrin-html-render':sha('target/debug/ottrin-html-render'),
        '/usr/libexec/ottrin/ottrin-priv-helper':sha('target/debug/ottrin-priv-helper'),
        '/usr/share/ottrin/ottrin-html-render.apparmor':sha('packaging/linux/ottrin-html-render.apparmor'),
        str(policy):sha('packaging/linux/ottrin-html-render.apparmor'),
        '/usr/share/polkit-1/actions/org.ottrin.filesystem.policy':sha('packaging/linux/org.ottrin.filesystem.policy'),
    }
    for name in files:assert not Path(name).exists() and not Path(name).is_symlink(),name
    existing=run(['systemctl','--user','list-units','--all','--plain','--no-legend','--no-pager','ottrin-render-*.service'],capture_output=True,text=True).stdout;assert not existing.strip()
    directories=['/usr/libexec','/usr/libexec/ottrin','/usr/share/ottrin','/usr/share/polkit-1','/usr/share/polkit-1/actions']
    state={'owned_files':files,'new_directories':[p for p in directories if not Path(p).exists()],
           'restriction':restriction,'browser_sha256':before,'status':'preparing'}
    state_path.write_text(json.dumps(state,indent=2)+'\n')
    with tempfile.TemporaryDirectory(prefix='ottrin-html-setup-',dir=os.environ['RUNNER_TEMP']) as temp:
        import shutil
        os.chmod(temp,0o700)
        for name in ['ottrin-html-render','ottrin-priv-helper']:
            shutil.copyfile(root/'target/debug'/name,Path(temp)/name);os.chmod(Path(temp)/name,0o755)
        run(['sudo',str(Path(temp)/'ottrin-priv-helper'),'--install'])
    run(['/usr/libexec/ottrin/ottrin-html-render','--check'])
    assert label_loaded()
    state['status']='ready';(out/'SETUP.json').write_text(json.dumps(state,indent=2)+'\n')
    with Path(os.environ['GITHUB_ENV']).open('a') as f:f.write('OTTRIN_REQUIRE_HTML_RENDERER=1\n')
except BaseException:
    cleanup();raise
