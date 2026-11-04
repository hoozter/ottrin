"""Actual packaged startup errors, theme-save refusal/retry and Search status."""
from pathlib import Path
import hashlib
import importlib.util
import json
import os
import signal
import subprocess
import sys
import time

def cancelled(signum, _):
    raise SystemExit(128 + signum)
for sig in (signal.SIGINT, signal.SIGTERM):
    signal.signal(sig, cancelled)
r = Path.cwd()
binary, out = Path(sys.argv[1]), Path(sys.argv[2])
docs = Path.home() / 'Documents'
docs.mkdir()
source = docs / 'preserve.txt'
source.write_bytes(b'owned original')
source_hash = hashlib.sha256(source.read_bytes()).hexdigest()
pref = Path(os.environ['XDG_CONFIG_HOME']) / 'ottrin/config.json'
config = json.loads(pref.read_text())
config.update(theme='Dark', bookmarks=[], default_view_mode='List', window_size=[1000, 700], graphics_renderer='Native', active_custom_theme='Settings audit', custom_themes=[{'name': 'Settings audit', 'base_preset': 'Ottrin', 'dark': {'app_radius': 0}, 'light': {'app_radius': 0}}])
config['search']['include_roots'] = [str(docs)]
config['search']['content_indexing_enabled'] = False
(out / 'CONFIG_INPUT.json').write_text(json.dumps(config, indent=2) + '\n')
spec = importlib.util.spec_from_file_location('qa', r / 'scripts/linux-desktop-qa.py')
q = importlib.util.module_from_spec(spec)
spec.loader.exec_module(q)
report = {'status': 'running', 'cases': [], 'scope': 'Actual AppImage/private X11/software Dark150; invalid main configuration never replaced, actual native theme draft retained on failed save, repaired retry persisted, and actual Search labels. Not full appearance/physical/platform acceptance.'}
app = None
permissions = None
def windows(name):
    return q.run('xdotool', 'search', '--all', '--onlyvisible', '--pid', str(app.pid), '--name', '^' + name + '$', capture=True, check=False).stdout.split()
def window(name):
    return q.wait_for(lambda: windows(name) or None, 'owned ' + name)[0]
def controls(name, role):
    return [n for n in q.all_nodes(q.ottrin_app()) if n.name == name and n.getRoleName() == role and q.pyatspi.STATE_SHOWING in n.getState().getStates()]
def named(name, role):
    return q.wait_for(lambda: (v[0] if len(v := controls(name, role)) == 1 else None), 'unique ' + name)
def geometry(w):
    data = dict(line.split('=', 1) for line in q.run('xdotool', 'getwindowgeometry', '--shell', w, capture=True).stdout.splitlines() if '=' in line)
    return [int(data[k]) for k in ['X', 'Y', 'WIDTH', 'HEIGHT']]
def inside(node, w):
    b = node.queryComponent().getExtents(q.pyatspi.DESKTOP_COORDS)
    x, y, width, height = geometry(w)
    return b.x >= x and b.x + b.width <= x + width and b.y > y + 70 and b.y + b.height < y + height - 70
def reveal(name, role, w):
    for _ in range(16):
        nodes = controls(name, role)
        if len(nodes) == 1 and inside(nodes[0], w):
            return nodes[0]
        x, y, width, height = geometry(w)
        q.run('xdotool', 'mousemove', '--sync', str(x + 3 * width // 4), str(y + height // 2), 'click', '--repeat', '4', '--delay', '100', '5')
        time.sleep(.2)
    raise AssertionError('Control not visibly inside viewport: ' + name)
def close_settings(settings):
    q.run('xdotool', 'windowactivate', '--sync', settings)
    parent = named('General', 'radio button')
    for _ in range(12):
        parent = parent.parent
        assert parent is not None
        choices = [n for n in q.all_nodes(parent) if n.name == 'Close window' and n.getRoleName() == 'push button']
        if len(choices) == 1:
            assert choices[0].queryAction().doAction(0)
            q.wait_for(lambda: not windows('Ottrin Settings'), 'Settings closed')
            return
        assert len(choices) < 2
    raise AssertionError('Settings close is not unique')
try:
    malformed = b'{ invalid settings fixture'
    pref.write_bytes(malformed)
    for label, arguments in [('main', []), ('properties', ['--properties', str(source)]), ('inspector', ['preview', str(source)]), ('index-status', ['index', 'status', '--json'])]:
        result = subprocess.run([str(binary), *arguments], capture_output=True, text=True, timeout=25)
        assert result.returncode != 0 and str(pref) in result.stderr and 'preserved' in result.stderr, (label, result.returncode, result.stderr)
        assert pref.read_bytes() == malformed
        report['cases'].append({'case': 'invalid config refused: ' + label, 'exit': result.returncode, 'stderr': result.stderr, 'original_bytes_preserved': True})
    pref.write_text(json.dumps(config) + '\n')
    with (out / 'app.log').open('w') as log:
        app = subprocess.Popen([str(binary), str(docs)], stderr=log, start_new_session=True)
        q.target_process_id = app.pid
        q.wait_for(q.ottrin_app, 'owned packaged application')
        main = window('Ottrin')
        q.wait_for(lambda: q.visible_entry('preserve.txt'), 'source fixture')
        q.click_named_control(main, 'Menu')
        q.click_accessible_name('Settings' + chr(0x2026), 'push button')
        settings = window('Ottrin Settings')
        q.click_accessible_name('Appearance', 'radio button')
        edit = reveal('Edit Settings audit', 'push button', settings)
        assert edit.queryAction().doAction(0)
        editor = window('Ottrin Theme Editor')
        q.run('xdotool', 'windowmove', '--sync', editor, '0', '0')
        q.run('xdotool', 'windowsize', '--sync', editor, '1770', '1290')
        q.run('xdotool', 'windowactivate', '--sync', editor)
        number = reveal('Window corners', 'spin button', editor)
        assert number.queryComponent().grabFocus()
        q.run('xdotool', 'key', '--clearmodifiers', 'ctrl+a')
        q.run('xdotool', 'type', '--clearmodifiers', '24')
        q.run('xdotool', 'key', '--clearmodifiers', 'Return')
        q.wait_for(lambda: named('Window corners', 'spin button').queryValue().currentValue == 24, 'typed24')
        before = pref.read_bytes()
        permissions = pref.stat().st_mode & 0o777
        pref.chmod(0o400)
        q.click_accessible_name('Save changes', 'push button')
        q.wait_for(lambda: any('Could not save Ottrin settings' in n.name for n in q.all_nodes(q.ottrin_app())), 'visible save failure')
        assert windows('Ottrin Theme Editor') and named('Window corners', 'spin button').queryValue().currentValue == 24
        assert pref.read_bytes() == before
        q.screenshot(out, 'theme-save-refused')
        report['cases'].append({'case': 'native theme save refused', 'draft_radius': 24, 'original_config_bytes_preserved': True, 'editor_stays_open': True})
        pref.chmod(permissions)
        permissions = None
        q.click_accessible_name('Save changes', 'push button')
        q.wait_for(lambda: not windows('Ottrin Theme Editor'), 'successful retry closed editor')
        saved = json.loads(pref.read_text())
        assert saved['custom_themes'][0]['dark']['app_radius'] == 24
        assert saved['custom_themes'][0]['light']['app_radius'] == 0
        report['cases'].append({'case': 'native save retry persisted retained draft', 'dark_radius': 24, 'light_radius': 0})
        q.screenshot(out, 'theme-save-retried')
        q.click_accessible_name('Search', 'radio button')
        reveal('Start index at login', 'label', settings)
        named('active in the index daemon', 'label')
        assert not controls('active while Ottrin is running', 'label')
        q.screenshot(out, 'search-health')
        report['cases'].append({'case': 'Search shows independent daemon and separate login state', 'backend': 'active in the index daemon', 'login': 'Start index at login'})
        close_settings(settings)
        q.click_accessible_name('Close window', 'push button')
        assert app.wait(timeout=15) == 0
        assert hashlib.sha256(source.read_bytes()).hexdigest() == source_hash
        saved = json.loads(pref.read_text())
        assert saved['default_view_mode'] == 'List' and saved['custom_themes'][0]['dark']['app_radius'] == 24
        report.update(status='pass native settings error/preservation/save-retry and Search status workflow', source_preserved=True, normal_exit=0)
except BaseException as error:
    report.update(status='failed', error=repr(error))
    try:
        q.screenshot(out, 'failure')
    except Exception as capture_error:
        report['capture_error'] = repr(capture_error)
    raise
finally:
    if permissions is not None:
        pref.chmod(permissions)
    if app is not None and app.poll() is None:
        os.killpg(app.pid, signal.SIGTERM)
        try:
            app.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(app.pid, signal.SIGKILL)
            app.wait()
    (out / 'FUNCTIONAL_RESULT.json').write_text(json.dumps(report, indent=2) + '\n')
