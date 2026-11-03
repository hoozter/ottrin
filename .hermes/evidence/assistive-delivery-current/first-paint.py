"""Measure actual chooser contents, separately from native window mapping."""
from pathlib import Path
import hashlib, importlib.util, io, json, os, subprocess, time
from PIL import Image

r = Path.cwd()
e = Path(__file__).resolve().parent
b = json.loads((e / 'BUILD_RESULT.json').read_text())
image = r / 'dist/packages/Ottrin-0.1.0-x86_64.AppImage'
assert hashlib.sha256(image.read_bytes()).hexdigest() == b['artifacts'][image.name]
spec = importlib.util.spec_from_file_location('desktop', r / 'scripts/linux-desktop-qa/session.py')
desktop = importlib.util.module_from_spec(spec)
spec.loader.exec_module(desktop)
out = e / 'first-paint'
assert not out.exists()
report = {'status': 'running', 'source_revision': b['source_revision'],
          'artifact_sha256': b['artifacts'][image.name], 'cases': [],
          'scope': 'Process-cold actual AppImage on private X11/software renderer. Content-ready upper bounds include screenshot/OCR observation overhead; OS disk caches are retained, not a disk-cold benchmark.'}
try:
    with desktop.OwnedDesktop(out) as d:
        folder = d.profile / 'files'
        folder.mkdir()
        names = ['alpha.txt', 'beta.txt', 'gamma.txt']
        for name in names:
            (folder / name).write_text(name)
        for number in range(3):
            started = time.monotonic()
            app = d.spawn([str(image), 'picker'], f'case-{number}.log', stdin=True,
                          output_name=f'response-{number}.json')
            app.stdin.write(json.dumps({'mode': 'open', 'title': 'First paint proof',
                                        'initial_directory': {'encoding': 'unix-bytes', 'units_hex': os.fsencode(folder).hex()}}))
            app.stdin.close()
            window = None
            while True:
                assert app.poll() is None
                if window is None:
                    windows = desktop.run(['xdotool', 'search', '--all', '--onlyvisible', '--pid', str(app.pid), '--name', 'First paint proof'], d.env, False).stdout.split()
                    if windows:
                        window = windows[-1]
                        mapped_ms = (time.monotonic() - started) * 1000
                if window is not None:
                    shot = out / f'content-{number}.png'
                    desktop.run(['import', '-window', window, str(shot)], d.env)
                    text = subprocess.check_output(['tesseract', str(shot), 'stdout', '--psm', '11'], text=True, stderr=subprocess.DEVNULL, timeout=10).casefold()
                    # Bounds were observed in the rejected full-window OCR run.
                    # Enlarge only the actual filename rows for exact text reading.
                    labels = io.BytesIO()
                    with Image.open(shot) as frame:
                        assert frame.width >= 500 and frame.height >= 240
                        frame.crop((166, 140, 500, 240)).resize((1002, 300)).save(labels, format='PNG')
                    label_text = subprocess.check_output(['tesseract', 'stdin', 'stdout', '--psm', '6'], input=labels.getvalue(), stderr=subprocess.DEVNULL, timeout=10).decode().casefold()
                    if all(name in label_text for name in names) and 'open' in text and 'cancel' in text:
                        content_ms = (time.monotonic() - started) * 1000
                        break
                assert time.monotonic() - started < 15, 'Actual chooser contents did not become ready'
                time.sleep(.02)
            desktop.run(['xdotool', 'windowactivate', '--sync', window, 'key', 'Escape'], d.env)
            assert app.wait(timeout=15) == 0
            response = json.loads((out / f'response-{number}.json').read_text())
            assert not response['accepted'] and not response['paths']
            report['cases'].append({'launch': number, 'fresh_profile': number == 0,
                                    'window_mapped_ms_upper_bound': mapped_ms,
                                    'contents_observed_ms_upper_bound': content_ms,
                                    'screenshot_sha256': hashlib.sha256(shot.read_bytes()).hexdigest(),
                                    'normal_cancel_exit': app.returncode})
        assert all((folder / name).read_text() == name for name in names)
        assert not (d.profile / 'runtime/ottrin/index-v1.sock').exists()
        report.update(status='pass', source_bytes_preserved=True, no_index_started=True)
finally:
    report['teardown'] = json.loads((out / 'TEARDOWN.json').read_text()) if (out / 'TEARDOWN.json').exists() else None
    (out / 'RESULT.json').write_text(json.dumps(report, indent=2) + '\n')
