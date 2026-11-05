"""Fresh exact-package daemon checks, with disposable state and registered teardown."""
from pathlib import Path
import hashlib, importlib.util, json, os, signal, socket, struct, subprocess, tarfile, time, sys
sys.dont_write_bytecode = True

r = Path.cwd()
e = Path(__file__).resolve().parent
def read(p): return json.loads(p.read_text())
def sha(p):
    with p.open('rb') as f: return hashlib.file_digest(f, 'sha256').hexdigest()
def cancelled(signum, _): raise SystemExit(128 + signum)
for sig in (signal.SIGINT, signal.SIGTERM): signal.signal(sig, cancelled)
spec = importlib.util.spec_from_file_location('workspace', r / 'scripts/test-workspace.py')
w = importlib.util.module_from_spec(spec); spec.loader.exec_module(w)
build = read(e / 'BUILD_SOURCE.json')
archive = Path(build['stage']) / 'Ottrin-0.1.0-linux-x86_64-portable.tar.gz'
assert sha(archive) == read(e / 'PACKAGE_CONTENT_ACCEPTANCE.json')['artifacts'][archive.name]
report = {'status': 'running', 'source_revision': build['source_revision'],
          'archive_sha256': sha(archive), 'scope': 'Exact portable daemon, private1000-file root, content indexing disabled; real filesystem watcher create/rename/delete, queries, persistence/restart, idle CPU and protocol shutdown. No personal service, distro installation, large corpus or GUI performance claim.'}
appimage_mode = sys.argv[1:] == ['--appimage']
assert not sys.argv[1:] or appimage_mode
image = Path(build['stage']) / 'Ottrin-0.1.0-x86_64.AppImage'
owned_mounts = []
if appimage_mode:
    assert sha(image) == read(e / 'PACKAGE_CONTENT_ACCEPTANCE.json')['artifacts'][image.name]
    report.update(artifact_sha256=sha(image), scope=report['scope'].replace('Exact portable daemon', 'Actual current AppImage/FUSE daemon and CLI'))
def cleanup_mounts(_profile):
    if not appimage_mode: return
    deadline = time.monotonic() + 10
    while True:
        mounts = {line.split()[4] for line in Path('/proc/self/mountinfo').read_text().splitlines()}
        consumers = subprocess.run(['fuser', str(image)], capture_output=True)
        assert consumers.returncode in (0, 1), consumers.stderr
        if not (set(owned_mounts) & mounts) and consumers.returncode == 1: break
        assert time.monotonic() < deadline, 'Owned AppImage mount/consumer cleanup remains pending'
        time.sleep(.1)
    report['owned_fuse_mounts_removed'] = True
process = None
try:
    with w.isolated_environment(prefix='ot-daemon-', before_remove=cleanup_mounts) as (profile, env):
        assert os.geteuid() != 0, 'Never use administrator daemon namespace for this fixture'
        runtime = profile / 'runtime'; runtime.mkdir(mode=0o700)
        env['XDG_RUNTIME_DIR'] = str(runtime)
        if appimage_mode:
            binary = cli = image
            env.pop('APPIMAGE_EXTRACT_AND_RUN', None)
            env.pop('APPDIR', None)
            env.pop('APPIMAGE', None)
            report['binary_sha256'] = read(e / 'PACKAGE_CONTENT_ACCEPTANCE.json')['runtime_sha256']['index_daemon']
        else:
            payload = profile / 'payload'; payload.mkdir()
            with tarfile.open(archive) as t: t.extractall(payload, filter='data')
            root = payload / 'Ottrin-0.1.0-linux-x86_64'
            binary = root / 'libexec/ottrin/ottrin-indexd'
            cli = root / 'bin/ottrin'
            report['binary_sha256'] = sha(binary)
            assert report['binary_sha256'] == read(e / 'PACKAGE_CONTENT_ACCEPTANCE.json')['runtime_sha256']['index_daemon']
            env['LD_LIBRARY_PATH'] = str(root / 'lib')
        corpus = profile / 'home/Documents'; corpus.mkdir()
        for i in range(1000): (corpus / f'original-{i:04}.txt').write_text(f'owned original{i}\n')
        originals = {p.name: sha(p) for p in corpus.iterdir()}
        config = read(r / '.hermes/evidence/windows-20261011/package-ui-config.json')
        config['search'].update(include_roots=[str(corpus)], exclude_roots=[], indexed_volumes=[],
                                content_indexing_enabled=False, use_system_locate=False, manage_locate_db=False,
                                default_scope='Global', include_globs=[], exclude_globs=[])
        pref = profile / 'config/ottrin/config.json'; pref.parent.mkdir(); pref.write_text(json.dumps(config))
        pref_hash = sha(pref)
        endpoint = runtime / 'ottrin/index-v1.sock'
        def request(kind):
            data = json.dumps({'version': 2, 'request': {'type': kind}}).encode()
            with socket.socket(socket.AF_UNIX) as client:
                client.settimeout(5); client.connect(str(endpoint))
                client.sendall(struct.pack('!I', len(data)) + data)
                def exact(n):
                    result = b''
                    while len(result) < n:
                        part = client.recv(n - len(result)); assert part, 'Incomplete daemon reply'; result += part
                    return result
                size = struct.unpack('!I', exact(4))[0]; assert size <= 1024 * 1024
                frame = json.loads(exact(size)); assert frame['version'] == 2
                response = frame['response']; assert response['type'] != 'error', response
                return response
        def ready():
            if not endpoint.exists(): return None
            response = request('status'); assert response['type'] == 'status'
            d = response['diagnostics']
            return d if d['status'] == 'Ready' and d['watcher_ready'] and d['indexed_items'] >= 1000 else None
        def await_value(fn, label):
            deadline = time.monotonic() + 45
            while True:
                assert process.poll() is None, 'Owned daemon exited'
                value = fn()
                if value: return value
                assert time.monotonic() < deadline, label
                time.sleep(.2)
        def search(text):
            p = subprocess.run([str(cli), 'search', text, '--json'], env=env, capture_output=True, text=True, timeout=15)
            assert p.returncode == 0, p.stderr
            return json.loads(p.stdout)['items']
        def start(log):
            p = subprocess.Popen([str(binary)] + (['--index-daemon'] if appimage_mode else []), env=env, stdout=log, stderr=subprocess.STDOUT)
            return p
        def assert_identity():
            mapped = Path(f'/proc/{process.pid}/exe').resolve()
            assert sha(Path(f'/proc/{process.pid}/exe')) == report['binary_sha256']
            if appimage_mode:
                mount = next((line for line in Path('/proc/self/mountinfo').read_text().splitlines() if ' - fuse.' in line and str(mapped).startswith(line.split()[4] + '/')), None)
                assert mount, 'Current AppImage is not actually FUSE-mounted'
                owned_mounts.append(mount.split()[4])
                report['fuse_mounts'] = owned_mounts.copy()
            else:
                assert mapped == binary.resolve()
        def stop():
            assert request('shutdown')['type'] == 'complete'
            assert process.wait(timeout=10) == 0
            assert not endpoint.exists(), 'Socket left after protocol shutdown'
        with (e / 'linux-daemon-runtime.log').open('w') as log:
            try:
                process = start(log)
                report['initial_ready'] = await_value(ready, 'Initial readiness/watcher deadline')
                assert_identity()
                assert Path(f'/proc/{process.pid}/comm').read_text().strip() == 'ottrin-indexd'
                assert len(search('original-0999')) == 1
                needle = corpus / 'watchneedle.txt'; needle.write_text('owned mutation\n')
                await_value(lambda: len(search('watchneedle')) == 1, 'Create watcher did not publish')
                renamed = corpus / 'renamedneedle.txt'; needle.rename(renamed)
                await_value(lambda: not search('watchneedle') and len(search('renamedneedle')) == 1, 'Rename watcher did not publish')
                renamed.unlink()
                await_value(lambda: not search('renamedneedle'), 'Delete watcher did not publish')
                report['watcher_mutations'] = ['create', 'rename', 'delete']
                await_value(ready, 'Final ready deadline'); time.sleep(2)
                def counters():
                    fields = Path(f'/proc/{process.pid}/stat').read_text().rsplit(')', 1)[1].split()
                    return int(fields[11]) + int(fields[12])
                before = counters(); start_time = time.monotonic(); time.sleep(30)
                seconds = time.monotonic() - start_time
                cpu = 100 * (counters() - before) / os.sysconf('SC_CLK_TCK') / seconds
                report['idle'] = {'seconds': seconds, 'cpu_one_core_percent': cpu,
                                  'criterion_below_one_percent': cpu < 1, 'no_clients_during_sample': True}
                assert cpu < 1, report['idle']
                report['before_shutdown'] = request('status')['diagnostics']; stop()
                process = start(log); report['restart_ready'] = await_value(ready, 'Restart readiness deadline'); assert_identity()
                assert len(search('original-0999')) == 1
                assert not search('watchneedle') and not search('renamedneedle')
                stop()
                assert sha(pref) == pref_hash
                assert {p.name: sha(p) for p in corpus.iterdir()} == originals
                report.update(status='pass scoped packaged daemon runtime and idle sample',
                              source_and_preferences_preserved=True, protocol_shutdowns=2)
            finally:
                if process is not None and process.poll() is None:
                    process.terminate()
                    try: process.wait(timeout=10)
                    except subprocess.TimeoutExpired: process.kill(); process.wait()
        report['owned_daemon_exited'] = process.poll() is not None
    report['private_profile_removed'] = not profile.exists()
except BaseException as error:
    report.update(status='failed', error=repr(error)); raise
finally:
    (e / 'LINUX_DAEMON_RUNTIME.json').write_text(json.dumps(report, indent=2) + '\n')
