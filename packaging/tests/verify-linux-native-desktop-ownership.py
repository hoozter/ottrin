#!/usr/bin/env python3
"""Exercise real FileManager1 removal in owned user/mount namespaces and D-Bus.

This is a native executable ownership check, not package-manager acceptance.
All profiles, private buses, mount sources and owned children are torn down.
"""
import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time


def stop_children(children):
    for child in reversed(children):
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGTERM)
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=5)
    children.clear()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--in-user-namespace', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    if os.getuid() == 0:
        parser.error('Run as an ordinary user with enabled user/mount namespaces')
    if not args.in_user_namespace:
        os.execvp('unshare', ['unshare', '--user', '--map-current-user', '--keep-caps',
                              '/usr/bin/python3', str(Path(__file__).resolve()),
                              '--in-user-namespace', *os.sys.argv[1:]])

    def cancelled(*_):
        raise KeyboardInterrupt('Desktop ownership fixture cancelled')
    signal.signal(signal.SIGTERM, cancelled)
    wrapper = (
        "import os,ctypes,sys;libc=ctypes.CDLL('libc.so.6',use_errno=True);"
        "r=libc.mount(os.fsencode(sys.argv[1]),b'/usr/bin',None,4096,None);"
        "assert r==0,os.strerror(ctypes.get_errno());"
        "os.execv('/usr/bin/ottrin',['/usr/bin/ottrin']+sys.argv[2:])"
    )
    launcher = ['unshare', '--mount', '--propagation', 'private', '--keep-caps',
                '/usr/bin/python3', '-c', wrapper]
    report = {'scope': 'Native desktop ownership, private namespace and bus; not DPKG/RPM', 'cases': []}
    for case in ['native', 'foreign-mount', 'native-deleted', 'offline', 'portable', 'modified']:
        row = {'case': case, 'status': 'running'}
        children = []
        try:
            with tempfile.TemporaryDirectory(prefix='ot-desktop-') as directory, ExitStack() as teardown:
                teardown.callback(stop_children, children)
                root = Path(directory)
                env = os.environ.copy()
                for key, name in [('HOME', 'home'), ('XDG_CONFIG_HOME', 'config'),
                                  ('XDG_DATA_HOME', 'data'), ('XDG_STATE_HOME', 'state'),
                                  ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')]:
                    (root / name).mkdir(mode=0o700)
                    env[key] = str(root / name)
                env['XDG_CURRENT_DESKTOP'] = 'OttrinQA'
                native, foreign = root / 'native', root / 'foreign'
                for folder in [native, foreign]:
                    folder.mkdir()
                    shutil.copy2(binary, folder / 'ottrin')
                mime = root / 'config/mimeapps.list'
                original = b'[Default Applications]\ninode/directory=org.example.Files.desktop;\n'
                mime.write_bytes(original)
                source = root / 'source.txt'
                source.write_bytes(b'Preserved source\n')
                bus = subprocess.Popen(['dbus-daemon', '--session', '--nofork', '--print-address=1'],
                                       env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                       text=True, start_new_session=True)
                children.append(bus)
                env['DBUS_SESSION_BUS_ADDRESS'] = bus.stdout.readline().strip()
                assert env['DBUS_SESSION_BUS_ADDRESS']
                def invoke(*arguments, check=True):
                    result = subprocess.run(launcher + [str(native), *arguments], env=env,
                                            capture_output=True, text=True, timeout=12)
                    if check and result.returncode:
                        raise RuntimeError(result.stderr + result.stdout)
                    return result
                try:
                    if case == 'portable':
                        result = subprocess.run([str(binary), 'folders', 'enable'], env=env,
                                                capture_output=True, text=True, timeout=12)
                        assert result.returncode == 0, result.stderr
                    else:
                        invoke('folders', 'enable')
                    receipt = root / 'state/ottrin/folder-registration.json'
                    installed = mime.read_bytes()
                    before = receipt.read_bytes()
                    peer = None
                    if case in ['native', 'foreign-mount', 'native-deleted']:
                        peer = subprocess.Popen(launcher + [str(foreign if case == 'foreign-mount' else native),
                                                            'file-manager'], env=env, stdout=subprocess.DEVNULL,
                                                stderr=subprocess.DEVNULL, start_new_session=True)
                        children.append(peer)
                        deadline = time.monotonic() + 12
                        while time.monotonic() < deadline:
                            assert peer.poll() is None, 'FileManager1 exited before readiness'
                            ready = subprocess.run(['gdbus', 'call', '--session', '--dest', 'org.freedesktop.DBus',
                                                    '--object-path', '/org/freedesktop/DBus', '--method',
                                                    'org.freedesktop.DBus.NameHasOwner',
                                                    'org.freedesktop.FileManager1'], env=env,
                                                   capture_output=True, text=True, timeout=3)
                            if 'true' in ready.stdout:
                                break
                            time.sleep(0.05)
                        else:
                            raise RuntimeError('FileManager1 did not acquire the private bus name')
                        if case == 'native-deleted':
                            (native / 'ottrin').unlink()
                            shutil.copy2(binary, native / 'ottrin')
                    mode = 'offline' if case == 'offline' else 'online'
                    invoke('--native-package-desktop', 'check-' + mode)
                    assert mime.read_bytes() == installed and receipt.read_bytes() == before
                    service = root / 'data/dbus-1/services/org.freedesktop.FileManager1.service'
                    if case == 'modified':
                        service.write_text('User-owned modification\n')
                        rejected = invoke('--native-package-desktop', 'remove-' + mode, check=False)
                        assert rejected.returncode != 0 and mime.read_bytes() == installed
                        assert receipt.read_bytes() == before
                        row['modified_state_refused'] = True
                    else:
                        invoke('--native-package-desktop', 'remove-' + mode)
                        if case == 'portable':
                            assert receipt.read_bytes() == before and mime.read_bytes() == installed
                            assert service.exists()
                        else:
                            assert not receipt.exists() and mime.read_bytes() == original
                            assert not service.exists()
                        if peer:
                            if case == 'foreign-mount':
                                assert peer.poll() is None, 'Independent namespace peer was stopped'
                            else:
                                assert peer.wait(timeout=4) == 0
                    assert source.read_bytes() == b'Preserved source\n'
                    assert not (root / 'config/ottrin/config.json').exists()
                    assert not (root / 'runtime/ottrin/index-v1.sock').exists()
                    row['status'] = 'pass'
                finally:
                    stop_children(children)
                    row['owned_processes_reaped'] = True
            row['profile_and_mount_sources_removed'] = True
        except Exception as error:
            row.update(status='fail', error=str(error))
        finally:
            stop_children(children)
        report['cases'].append(row)
    report['status'] = 'pass' if all(row['status'] == 'pass' for row in report['cases']) else 'fail'
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    return 0 if report['status'] == 'pass' else 1


if __name__ == '__main__':
    raise SystemExit(main())
