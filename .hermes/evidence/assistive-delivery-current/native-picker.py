"""Bounded native packaged chooser selection; owned X11 desktop and teardown."""
from pathlib import Path
import csv, hashlib, importlib.util, io, json, os, subprocess, time

r = Path.cwd()
e = Path(__file__).resolve().parent
build = json.loads((e / 'BUILD_SOURCE.json').read_text())
image = Path(build['stage']) / 'Ottrin-0.1.0-x86_64.AppImage'
digest = hashlib.sha256(image.read_bytes()).hexdigest()
assert digest == json.loads((e / 'PACKAGE_CONTENT_ACCEPTANCE.json').read_text())['artifacts'][image.name]
spec = importlib.util.spec_from_file_location('desktop', r / 'scripts/linux-desktop-qa/session.py')
desktop = importlib.util.module_from_spec(spec)
spec.loader.exec_module(desktop)

for theme, scale in [('Dark', '1'), ('Light', '1.5')]:
    out = e / ('picker-' + theme.lower())
    assert not out.exists()
    report = {'status': 'running', 'source_revision': build['source_revision'],
              'artifact_sha256': digest, 'theme': theme, 'scale': scale, 'cases': [],
              'scope': 'Native X11/software AppImage chooser selection and accepted paths; same-frame modifier regression separately verified through actual egui event frames'}
    try:
        with desktop.OwnedDesktop(out, scale=scale) as d:
            folder = d.profile / 'selection'
            folder.mkdir()
            for name in ['alpha.txt', 'beta.txt', 'gamma.txt']:
                (folder / name).write_text(name)
            initial = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in folder.iterdir()}
            config = json.loads((r / '.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config']
            config.update(theme=theme, default_view_mode='List', bookmarks=[])
            config_path = d.profile / 'config/ottrin/config.json'
            config_path.parent.mkdir(parents=True)
            config_path.write_text(json.dumps(config) + '\n')
            config_bytes = config_path.read_bytes()
            def run(args, check=True):
                return desktop.run(args, d.env, check)
            for direction in ['Up', 'Down']:
                for shift in [True, False]:
                    case = direction.lower() + ('-range' if shift else '-single')
                    response_path = out / (case + '-response.json')
                    started = time.monotonic()
                    app = d.spawn([str(image), 'picker'], case + '.log', stdin=True,
                                  output_name=response_path.name)
                    request = {'mode': 'open', 'multiple': True, 'title': 'Selection proof',
                               'initial_directory': {'encoding': 'unix-bytes', 'units_hex': os.fsencode(folder).hex()}}
                    app.stdin.write(json.dumps(request))
                    app.stdin.close()
                    deadline = time.monotonic() + 20
                    while True:
                        assert app.poll() is None
                        windows = run(['xdotool', 'search', '--all', '--onlyvisible', '--pid', str(app.pid), '--name', 'Selection proof'], False).stdout.split()
                        if windows:
                            break
                        assert time.monotonic() < deadline, 'Owned picker window timed out'
                        time.sleep(.1)
                    window = windows[-1]
                    mapped_ms = round((time.monotonic() - started) * 1000, 2)
                    run(['xdotool', 'windowactivate', '--sync', window])
                    time.sleep(.6)
                    if not report['cases']:
                        # Process-cold, fresh profile; do not claim dropped OS disk caches.
                        def ticks():
                            fields = Path('/proc', str(app.pid), 'stat').read_text().rsplit(') ', 1)[1].split()
                            return int(fields[11]) + int(fields[12])
                        cpu_before = ticks()
                        idle_started = time.monotonic()
                        time.sleep(30)
                        seconds = time.monotonic() - idle_started
                        cpu = (ticks() - cpu_before) / os.sysconf('SC_CLK_TCK') / seconds * 100
                        memory = dict(line.split(':', 1) for line in Path('/proc', str(app.pid), 'smaps_rollup').read_text().splitlines()[1:] if ':' in line)
                        report['idle'] = {'seconds': seconds, 'cpu_percent_one_core': cpu,
                                          'rss_kib': int(memory['Rss'].split()[0]),
                                          'pss_kib': int(memory['Pss'].split()[0])}
                    before = out / (case + '-before.png')
                    run(['import', '-window', window, str(before)])
                    tsv = subprocess.check_output(['tesseract', str(before), 'stdout', 'tsv'], stderr=subprocess.DEVNULL, text=True, timeout=20)
                    rows = list(csv.DictReader(io.StringIO(tsv), delimiter='\t'))
                    matches = [row for row in rows if row.get('text', '').strip().casefold() == 'beta.txt']
                    assert len(matches) == 1, ('Expected one visible beta filename', matches)
                    row = matches[0]
                    x, y = int(row['left']) + int(row['width']) // 2, int(row['top']) + int(row['height']) // 2
                    run(['xdotool', 'mousemove', '--window', window, str(x), str(y), 'click', '1'])
                    time.sleep(.2)
                    keys = ['keydown', 'Shift_L', 'key', direction, 'keyup', 'Shift_L'] if shift else ['key', direction]
                    run(['xdotool', *keys])
                    time.sleep(.2)
                    run(['import', '-window', window, str(out / (case + '-selected.png'))])
                    run(['xdotool', 'key', 'Return'])
                    assert app.wait(timeout=15) == 0
                    response = json.loads(response_path.read_text())
                    next_name = 'alpha.txt' if direction == 'Up' else 'gamma.txt'
                    expected = {folder / next_name, *([folder / 'beta.txt'] if shift else [])}
                    assert response['accepted']
                    assert all(path['encoding'] == 'unix-bytes' for path in response['paths'])
                    actual = {Path(os.fsdecode(bytes.fromhex(path['units_hex']))) for path in response['paths']}
                    assert actual == expected and len(response['paths']) == len(expected), response
                    report['cases'].append({'case': case, 'normal_exit': app.returncode, 'response': response,
                                            'window_mapped_ms_upper_bound': mapped_ms,
                                            'launch_scope': 'Fresh-profile process-cold' if not report['cases'] else 'Repeated process-cold; existing profile and OS caches retained'})
            concurrent = []
            for number in range(2):
                response_path = out / f'concurrent-{number}-response.json'
                app = d.spawn([str(image), 'picker'], f'concurrent-{number}.log', stdin=True,
                              output_name=response_path.name)
                app.stdin.write(json.dumps({'mode': 'open', 'title': f'Concurrent selection {number}',
                                            'initial_directory': {'encoding': 'unix-bytes', 'units_hex': os.fsencode(folder).hex()}}))
                app.stdin.close()
                deadline = time.monotonic() + 20
                while True:
                    assert app.poll() is None
                    windows = run(['xdotool', 'search', '--all', '--onlyvisible', '--pid', str(app.pid), '--name', f'Concurrent selection {number}'], False).stdout.split()
                    if windows:
                        break
                    assert time.monotonic() < deadline, 'Concurrent chooser window timed out'
                    time.sleep(.1)
                concurrent.append((app, windows[-1], response_path))
            assert all(app.poll() is None for app, _, _ in concurrent)
            for number, (app, window, response_path) in enumerate(concurrent):
                run(['xdotool', 'windowactivate', '--sync', window])
                run(['import', '-window', window, str(out / f'concurrent-{number}.png')])
                run(['xdotool', 'key', 'Escape'])
                assert app.wait(timeout=15) == 0
                response = json.loads(response_path.read_text())
                assert not response['accepted'] and not response['paths']
            report['two_concurrent_requests_cancel_independently'] = True
            assert initial == {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in folder.iterdir()}
            assert config_path.read_bytes() == config_bytes
            assert not (d.profile / 'runtime/ottrin/index-v1.sock').exists()
            report.update(status='pass', source_bytes_preserved=True, main_config_preserved=True, no_index_started=True)
    except BaseException as error:
        report.update(status='failed', error=repr(error))
        raise
    finally:
        report['teardown'] = json.loads((out / 'TEARDOWN.json').read_text()) if (out / 'TEARDOWN.json').exists() else None
        (out / 'RESULT.json').write_text(json.dumps(report, indent=2) + '\n')
