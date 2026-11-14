"""Verify exact mounted AppImage replacement on an owned X11 desktop."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time

sys.dont_write_bytecode = True
root = Path.cwd()
evidence = Path(__file__).resolve().parent
read = lambda path: json.loads(path.read_text())


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


old = read(root / 'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json')
new = read(evidence / 'PACKAGE_CONTENT_ACCEPTANCE.json')
name = 'Ottrin-0.1.0-x86_64.AppImage'
old_file = root / 'dist/packages' / name
new_file = Path(read(evidence / 'BUILD_SOURCE.json')['stage']) / name
assert sha(old_file) == old['artifacts'][name] and sha(new_file) == new['artifacts'][name]
spec = importlib.util.spec_from_file_location('desktop', root / 'scripts/linux-desktop-qa/session.py')
desktop = importlib.util.module_from_spec(spec)
spec.loader.exec_module(desktop)


def prepare(session):
    template = root / '.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json'
    config = read(template)['persisted_config']
    config.update(theme='Dark', theme_preset='Ottrin', default_view_mode='Miller', window_size=[1024, 680])
    config['search'].update(include_roots=[str(session.profile / 'home')], content_indexing_enabled=False)
    path = session.profile / 'config/ottrin/config.json'
    path.parent.mkdir(parents=True)
    path.write_text(json.dumps(config))
    (session.profile / 'home/source.txt').write_text('Owned preview fixture\n')


def expire(*_):
    raise TimeoutError('Owned atomic AppImage verification lease expired')


with desktop.OwnedDesktop(evidence / 'atomic-appimage', prepare=prepare) as session:
    signal.signal(signal.SIGALRM, expire)
    signal.alarm(120)
    private = session.profile / 'images'
    private.mkdir()
    current = private / name
    incoming = private / ('incoming-' + name)
    shutil.copy2(old_file, current)
    shutil.copy2(new_file, incoming)
    backup = private / ('old-' + name)
    assert not session.env.get('APPIMAGE_EXTRACT_AND_RUN')

    def command(*args, owner=session):
        return subprocess.check_output(args, env=owner.env, timeout=10, text=True).strip()

    def find_window(title, owner=session):
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            result = subprocess.run(
                ['xdotool', 'search', '--onlyvisible', '--name', title],
                env=owner.env, capture_output=True, text=True, timeout=5,
            )
            if result.returncode == 0:
                window = result.stdout.split()[0]
                pid = int(command('xdotool', 'getwindowpid', window, owner=owner))
                proc = Path('/proc') / str(pid)
                if ('HOME=' + str(owner.profile / 'home')).encode() in (proc / 'environ').read_bytes().split(b'\0'):
                    return window, proc
            time.sleep(.15)
        raise AssertionError('Owned window did not appear: ' + title)

    def app(window, proc, expected, owner=session):
        executable = proc / 'exe'
        assert sha(executable) == expected
        assert str(owner.profile / 't/.mount_Ottrin') in os.readlink(executable)
        assert int(command('xdotool', 'getwindowpid', window, owner=owner)) == int(proc.name)

    def click(window, x, y):
        command('xdotool', 'windowactivate', '--sync', window)
        command('xdotool', 'mousemove', '--window', window, str(x), str(y), 'click', '1')
        time.sleep(.4)

    old_process = session.spawn([str(current), str(session.profile / 'home')], 'old-application.log')
    old_window, old_proc = find_window('^Ottrin$')
    app(old_window, old_proc, old['runtime_sha256']['application'])
    original_inode = current.stat().st_ino
    os.link(current, backup)
    os.replace(incoming, current)
    assert backup.stat().st_ino == original_inode and current.stat().st_ino != original_inode
    assert sha(backup) == old['artifacts'][name] and sha(current) == new['artifacts'][name]
    backup.unlink()
    app(old_window, old_proc, old['runtime_sha256']['application'])
    click(old_window, 996, 54)
    click(old_window, 870, 322)
    settings, settings_proc = find_window('^Ottrin Settings$')
    assert settings_proc == old_proc
    with desktop.OwnedDesktop(evidence / 'atomic-appimage-next', prepare=prepare) as next_session:
        next_session.spawn([str(current), str(next_session.profile / 'home')], 'new-application.log')
        new_window, new_proc = find_window('^Ottrin$', owner=next_session)
        app(new_window, new_proc, new['runtime_sha256']['application'], owner=next_session)
        assert read(next_session.profile / 'config/ottrin/config.json')['default_view_mode'] == 'Miller'
        app(old_window, old_proc, old['runtime_sha256']['application'])
        assert (session.profile / 'home/source.txt').read_text() == 'Owned preview fixture\n'
    result = {'status': 'pass atomic replacement preserved the existing mounted window and the next launch used the new AppImage',
              'previous_image_sha256': old['artifacts'][name], 'new_image_sha256': new['artifacts'][name],
              'old_window_operated_after_replacement': True, 'next_launch_new_binary': True,
              'scope': 'Private same-filesystem AppImage path replacement only. Multi-package promotion, concurrent settings writes, and managed service continuity are separate.'}
    signal.alarm(0)

teardown = read(evidence / 'atomic-appimage/TEARDOWN.json')
assert teardown['status'] == 'closed' and teardown['profile_removed']
next_teardown = read(evidence / 'atomic-appimage-next/TEARDOWN.json')
assert next_teardown['status'] == 'closed' and next_teardown['profile_removed']
(evidence / 'ATOMIC_APPIMAGE_QA.json').write_text(json.dumps(result, indent=2) + '\n')
print(result['status'])
