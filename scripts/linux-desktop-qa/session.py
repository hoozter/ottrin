"""Owned Linux desktop QA with teardown registered before resource allocation."""
from pathlib import Path
import importlib.util, json, os, select, signal, subprocess, time
ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('test_workspace', ROOT / 'scripts/test-workspace.py')
workspace = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workspace)

def run(command, env, check=True):
    return subprocess.run(command, env=env, check=check, text=True, capture_output=True, timeout=15)

def cleanup_owned_mounts(profile):

    def mounts():
        return [line for line in Path('/proc/self/mountinfo').read_text().splitlines() if line.split()[4].startswith(str(profile) + '/')]
    for _ in range(20):
        if not mounts():
            return []
        time.sleep(0.1)
    removed = []
    for line in mounts():
        target = Path(line.split()[4])
        tail = line.split(' - ', 1)[1].split()
        fstype = tail[0]
        portal = target == profile / 'runtime/doc' and fstype == 'fuse.portal'
        appimage = target.parent == profile / 't' and target.name.startswith('.mount_Ottrin') and fstype.startswith('fuse.')
        assert (portal or appimage) and 'user_id=' + str(os.getuid()) in line, ('Unclassified owned mount', line)
        users = run(['fuser', '-m', str(target)], os.environ.copy(), False)
        assert users.returncode == 1, ('Owned mount still has consumers', str(target), users.stdout, users.stderr)
        tool = __import__('shutil').which('fusermount3') or __import__('shutil').which('fusermount')
        assert tool
        subprocess.run([tool, '-u', str(target)], check=True)
        removed.append(line)
    assert not mounts()
    return removed

def cleanup_profile_children(profile):
    stopped = []
    unreadable = 0
    for path in Path('/proc').iterdir():
        if not path.name.isdigit() or int(path.name) == os.getpid():
            continue
        fd = None
        try:
            if path.stat().st_uid != os.getuid():
                continue
            fd = os.pidfd_open(int(path.name))
            entries = (path / 'environ').read_bytes().split(b'\x00')
            if not any((os.fsencode(k + '=' + str(profile / v)) in entries for k, v in [('HOME', 'home'), ('XDG_RUNTIME_DIR', 'runtime')])):
                continue
            signal.pidfd_send_signal(fd, signal.SIGTERM)
            if not select.select([fd], [], [], 5)[0]:
                signal.pidfd_send_signal(fd, signal.SIGKILL)
            assert select.select([fd], [], [], 5)[0], f'owned child {path.name} did not exit'
            stopped.append(int(path.name))
        except PermissionError:
            unreadable += 1
        except (ProcessLookupError, FileNotFoundError):
            continue
        finally:
            if fd is not None:
                os.close(fd)
    return {'stopped': stopped, 'unreadable_foreign_processes_preserved': unreadable}

class OwnedDesktop:

    def __init__(self, evidence, scale='1'):
        self.evidence = Path(evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.scale = scale
        self.processes = []
        self.streams = []
        self.report = {'status': 'starting'}
        self.isolation = None
        self.profile = None
        self.env = None

    def spawn(self, command, name, *, pipe=False, stdin=False, output_name=None):
        stream = (self.evidence / name).open('w')
        self.streams.append(stream)
        output = stream
        if output_name:
            output = (self.evidence / output_name).open('w')
            self.streams.append(output)
        process = subprocess.Popen(command, env=self.env, stdout=subprocess.PIPE if pipe else output, stdin=subprocess.PIPE if stdin else None, stderr=stream, text=True, start_new_session=True)
        self.processes.append(process)
        return process

    def ready_line(self, process):
        assert select.select([process.stdout], [], [], 15)[0], 'Owned desktop readiness timed out'
        value = process.stdout.readline().strip()
        assert value, 'Owned desktop helper exited'
        return value

    def __enter__(self):
        assert callable(cleanup_profile_children) and callable(cleanup_owned_mounts)
        for sig in [signal.SIGTERM, signal.SIGINT]:
            signal.signal(sig, workspace.cancelled)
        self.isolation = workspace.isolated_environment(directory='/var/tmp', prefix='.ottrin-picker-qa-', before_remove=self.cleanup_profile)
        self.profile, self.env = self.isolation.__enter__()
        try:
            self.report['profile'] = str(self.profile)
            runtime = self.profile / 'runtime'
            runtime.mkdir(mode=448)
            self.env.pop('XAUTHORITY', None)
            self.env.pop('SESSION_MANAGER', None)
            self.env.pop('WAYLAND_DISPLAY', None)
            self.env.pop('WAYLAND_SOCKET', None)
            self.env.update(XDG_RUNTIME_DIR=str(runtime), XDG_SESSION_TYPE='x11', GDK_BACKEND='x11', EGL_PLATFORM='x11', NO_AT_BRIDGE='1', LIBGL_ALWAYS_SOFTWARE='1', WINIT_UNIX_BACKEND='x11', WINIT_X11_SCALE_FACTOR=self.scale, KWIN_COMPOSE='N', QT_QPA_PLATFORM='xcb', OTTRIN_DEV_HELPER_AUTOBUILD='0')
            xvfb = self.spawn(['Xvfb', '-displayfd', '1', '-screen', '0', '1920x1440x24', '-nolisten', 'tcp', '-noreset'], 'xvfb.log', pipe=True)
            self.env['DISPLAY'] = ':' + self.ready_line(xvfb)
            bus = self.spawn(['dbus-daemon', '--session', '--nofork', '--print-address=1'], 'dbus.log', pipe=True)
            self.env['DBUS_SESSION_BUS_ADDRESS'] = self.ready_line(bus)
            self.spawn(['kwin_x11', '--replace'], 'wm.log')
            time.sleep(1)
            self.report['status'] = 'active'
            return self
        except BaseException as error:
            self.__exit__(type(error), error, error.__traceback__)
            raise

    def cleanup_profile(self, profile):
        self.report['owned_children'] = cleanup_profile_children(profile)
        self.report['owned_mounts_unmounted'] = cleanup_owned_mounts(profile)

    def __exit__(self, kind, error, trace):
        try:
            for process in reversed(self.processes):
                workspace.stop_tests(process)
            self.report['owned_processes_reaped'] = [
                {'pid': process.pid, 'exit': process.poll()} for process in self.processes
            ]
            if self.isolation:
                self.isolation.__exit__(kind, error, trace)
            self.report['status'] = 'closed' if kind is None else 'failed'
            if error:
                self.report['error'] = str(error)
        except BaseException as failure:
            self.report['status'] = 'failed teardown'
            self.report['teardown_error'] = str(failure)
            raise
        finally:
            for stream in self.streams:
                stream.close()
            self.report['profile_removed'] = self.profile is None or not self.profile.exists()
            (self.evidence / 'TEARDOWN.json').write_text(json.dumps(self.report, indent=2) + '\n')
        return False
