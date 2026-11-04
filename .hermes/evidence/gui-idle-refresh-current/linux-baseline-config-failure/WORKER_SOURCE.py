"""Measure a private packaged main window, then observe actual native status updates."""
from pathlib import Path
import hashlib, importlib.util, json, os, signal, subprocess, sys, time

def cancelled(signum, _): raise SystemExit(128 + signum)
for sig in (signal.SIGTERM, signal.SIGINT): signal.signal(sig, cancelled)

r = Path.cwd(); binary = Path(sys.argv[1]); out = Path(sys.argv[2])
docs = Path.home() / 'Documents'; docs.mkdir()
source = docs / 'preserve.txt'; original = b'owned original'; source.write_bytes(original)
pref = Path(os.environ['XDG_CONFIG_HOME']) / 'ottrin/config.json'
config = json.loads(pref.read_text())
config.update(theme='Dark', bookmarks=[], default_view_mode='List', window_size=[1000, 700],
              graphics_renderer='Native', active_custom_theme='Radius audit',
              custom_themes=[{'name':'Radius audit','dark':{'app_radius':0},'light':{'app_radius':0}}])
config['search']['include_roots'] = [str(docs)]
config['search']['content_indexing_enabled'] = False
pref.write_text(json.dumps(config) + '\n')
(out / 'CONFIG_INPUT.json').write_text(json.dumps(config,indent=2)+'\n')
spec = importlib.util.spec_from_file_location('qa', r / 'scripts/linux-desktop-qa.py')
q = importlib.util.module_from_spec(spec); spec.loader.exec_module(q)
report = {'status':'running','scope':'Actual AppImage, private X11/software Dark150, one-file root. GUI process CPU only; native Settings index-count update without injected input, source/preferences and normal close. No physical graphics/large corpus/full appearance acceptance.'}

def status():
    result = subprocess.run([str(binary), 'index', 'status', '--json'], text=True, capture_output=True, timeout=20)
    assert result.returncode == 0, result.stderr
    return json.loads(result.stdout)

def ready():
    value = status()
    report['last_index_status'] = value
    return value if value['status'] == 'ready' and value['watcher_ready'] and value['indexed_items'] == 1 else None

def window(name):
    return q.wait_for(lambda: q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^'+name+'$',capture=True,check=False).stdout.split() or None, 'owned '+name)[0]

def text(name):
    return [node for node in q.all_nodes(q.ottrin_app())
            if node.name == name and q.pyatspi.STATE_SHOWING in node.getState().getStates()]

def visible_text(name):
    bounds = q.run('xdotool','getwindowgeometry','--shell',settings,capture=True).stdout
    geometry = dict(line.split('=',1) for line in bounds.splitlines() if '=' in line)
    y, height = int(geometry['Y']), int(geometry['HEIGHT'])
    return [node for node in text(name) if (b := node.queryComponent().getExtents(q.pyatspi.DESKTOP_COORDS)).y > y+100 and b.y+b.height < y+height-60]

def cpu():
    fields = Path(f'/proc/{app.pid}/stat').read_text().rsplit(')',1)[1].split()
    return int(fields[11]) + int(fields[12])

with (out / 'app.log').open('w') as log:
    app = subprocess.Popen([str(binary), str(docs)], stderr=log, start_new_session=True)
    q.target_process_id = app.pid
    try:
        q.wait_for(q.ottrin_app, 'owned packaged accessibility tree')
        main = window('Ottrin')
        q.wait_for(lambda: text('Menu'), 'main menu')
        report['index_before'] = q.wait_for(ready, 'quiet one-file index ready', timeout=60)
        q.run('xdotool','windowmove','--sync',main,'0','0')
        q.run('xdotool','windowactivate','--sync',main)
        q.run('xdotool','mousemove','--sync','1918','1438')
        time.sleep(5)
        executable = Path(f'/proc/{app.pid}/exe')
        report['gui_binary_sha256'] = hashlib.sha256(executable.read_bytes()).hexdigest()
        report['process_comm'] = Path(f'/proc/{app.pid}/comm').read_text().strip()
        report['vulkan_driver_maps'] = sorted({Path(line.split()[-1]).name for line in Path(f'/proc/{app.pid}/maps').read_text().splitlines() if 'libvulkan_' in line})
        before = cpu(); started = time.monotonic(); time.sleep(30)
        seconds = time.monotonic() - started
        report['idle'] = {'seconds':seconds,'cpu_one_core_percent':100*(cpu()-before)/os.sysconf('SC_CLK_TCK')/seconds,
                          'no_clients_or_injected_input_during_sample':True}
        report['index_after'] = status()
        assert report['index_after']['status'] == 'ready' and report['index_after']['watcher_ready'] and report['index_after']['indexed_items'] == 1
        q.screenshot(out, 'main-idle')
        q.click_named_control(main, 'Menu')
        q.click_accessible_name('Settings'+chr(0x2026), 'push button')
        settings = window('Ottrin Settings')
        q.click_accessible_name('Search', 'radio button')
        for _ in range(16):
            if visible_text('1 files indexed'): break
            q.run('xdotool','mousemove','--sync','1000','700','click','--repeat','4','--delay','100','5')
            time.sleep(.2)
        else: raise AssertionError('Initial index count is not visible')
        q.run('xdotool','mousemove','--sync','1918','1438')
        needle = docs / 'observed-needle.txt'; needle.write_text('owned external change')
        q.wait_for(lambda: visible_text('2 files indexed'), 'native index count2 without input', timeout=15)
        q.screenshot(out, 'index-updated')
        needle.unlink()
        q.wait_for(lambda: visible_text('1 files indexed'), 'native index count1 after delete without input', timeout=15)
        # Select the close belonging to the Settings window, not the main owner.
        general = q.wait_for(lambda: next((n for n in q.all_nodes(q.ottrin_app()) if n.name == 'General' and n.getRoleName() == 'radio button'),None), 'Settings General')
        parent = general
        for _ in range(12):
            parent = parent.parent
            assert parent is not None
            close = [n for n in q.all_nodes(parent) if n.name == 'Close window' and n.getRoleName() == 'push button']
            if len(close) == 1: break
            assert len(close) < 2
        else: raise AssertionError('Settings close is not unique')
        assert close[0].queryAction().doAction(0)
        q.wait_for(lambda: not q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin Settings$',capture=True,check=False).stdout.split(), 'Settings closed')
        q.click_accessible_name('Close window','push button')
        assert app.wait(timeout=15) == 0
        saved = json.loads(pref.read_text())
        assert saved['default_view_mode'] == 'List' and saved['search']['include_roots'] == [str(docs)] and not saved['search']['content_indexing_enabled']
        assert source.read_bytes() == original
        report.update(status='pass native idle workflow; CPU criterion assessed separately',index_counts=[1,2,1],
                      source_and_preferences_preserved=True,normal_exit=0)
    except BaseException as error:
        report.update(status='failed',error=repr(error)); raise
    finally:
        if app.poll() is None:
            os.killpg(app.pid, signal.SIGTERM)
            try: app.wait(timeout=10)
            except subprocess.TimeoutExpired: os.killpg(app.pid,signal.SIGKILL); app.wait()
        (out / 'FUNCTIONAL_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
