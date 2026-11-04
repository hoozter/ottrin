"""Bind current native workflows, matched CPU samples and reviewed full captures."""
from pathlib import Path
import hashlib
import json
import subprocess

r = Path.cwd()
e = Path(__file__).resolve().parent
f = e / 'native'
read = lambda p: json.loads(p.read_text())
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
v = read(f / 'GUEST_RESULT.json')
owner = read(f / 'OWNER.json')
source = read(e / 'BUILD_SOURCE.json')['source_revision']
previous = read(r / '.hermes/evidence/appearance-numeric-values-current/WINDOWS_NATIVE_ACCEPTANCE.json')
assert v['status'].startswith('pass native idle observation workflow')
assert v['package']['source_revision'] == source
assert v['package']['actor_sha256'] == sha(f / 'actor.ps1') == sha(e / 'native-actor.ps1')
assert v['package']['files'][0]['sha256'] == read(e / 'HOST_PAYLOAD.json')['artifacts']['Ottrin-0.1.0-windows-x64-setup.exe']
assert v['source_preserved'] and v['preferences_preserved']
assert v['remaining_owned_installed_processes'] == 0
assert v['dpi_setup']['actual_settings_dpi'] == 144
assert len(v['cases']) == 4 and len(v['gui_idle_baseline']) == 2
captures = {}
comparison = []
for renderer in ['Native', 'OpenGL']:
    cases = [c for c in v['cases'] if c['renderer'] == renderer]
    assert len(cases) == 2 and all(c['dpi'] == 144 for c in cases)
    assert cases[0]['case'] == 'native index label follows external create/delete without input'
    assert cases[0]['counts'] == [1, 2, 1]
    assert cases[1]['case'] == 'new DOS device root visible without injected input'
    assert 'Indexed search: disabled' in cases[1]['label']
    current = next(c for c in v['gui_idle_baseline'] if c['renderer'] == renderer)
    baseline = next(c for c in previous['gui_idle_baseline'] if c['renderer'] == renderer)
    assert current['status'] == 'pass current-scope criterion'
    assert current['cpu_one_core_percent'] < 2 and current['seconds'] >= 30
    for key in ['dpi', 'bounds', 'work_area']:
        assert current[key] == baseline[key]
    for key in ['index_before', 'index_after']:
        assert current[key]['status'] == 'ready' and current[key]['watcher_ready']
        assert current[key]['indexed_items'] == baseline[key]['indexed_items'] == 1
    comparison.append({'renderer': renderer, 'baseline_source': previous['source_revision'], 'baseline_cpu_one_core_percent': baseline['cpu_one_core_percent'], 'current_cpu_one_core_percent': current['cpu_one_core_percent'], 'matched_dpi': current['dpi'], 'matched_bounds': current['bounds'], 'matched_work_area': current['work_area']})
    for suffix, note in [('idle-main', 'Reviewed complete original: intact main frame and one-file list.'), ('index-updated', 'Reviewed complete original: themed Search page visibly reports 2 files indexed and active watcher.'), ('storage-updated', 'Reviewed complete original: additional locations-rail drive button appears, main frame/list intact; label verified through native accessibility result.')]:
        name = renderer + '-' + suffix + '.png'
        captures[name] = {'sha256': sha(f / 'results' / name), 'review': note}
assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed']
assert len(owner['cleanup']['owned_volumes_removed']) == 2
assert subprocess.run(['virsh', '-c', 'qemu:///system', 'domuuid', owner['domain']], capture_output=True).returncode != 0
assert not Path(owner['private_console']['home']).parent.exists()
receipt = {'status': 'pass scoped Windows native idle and background observation', 'source_revision': source, 'installer_sha256': v['package']['files'][0]['sha256'], 'cases': 4, 'full_window_captures': 6, 'captures': captures, 'gui_idle_baseline': v['gui_idle_baseline'], 'matched_comparison': comparison, 'cleanup': owner['cleanup'], 'checks': ['Ordinary-user exact NSIS installation and full 58-file verification', 'Both software renderers meet GUI CPU below 2 percent of one core over 30 seconds', 'Native index count 1/2/1 after external mutations without injected input', 'Actual OS DOS-drive insertion reaches sleeping locations rail without injected input', 'Normal close, supported uninstall, source and saved-preference preservation'], 'limits': ['Zero observed processor-time delta is limited by counter resolution and the bounded sample; no promise of universal zero CPU', 'Private signed-VirtIO software VM, Dark144DPI, one-file corpus; sustained, physical and personal-machine acceptance separate', 'DOS-drive insertion is not physical-volume removal/reconnect', 'Current check covers changed observation responsibilities; preceding numeric/shape/mapping scopes retain their exact historical artifacts', 'No Store/signing or fresh MSIX deployment/update/login acceptance']}
(e / 'WINDOWS_NATIVE_ACCEPTANCE.json').write_text(json.dumps(receipt, indent=2) + '\n')
print('Accepted both strict GUI CPU samples, four native workflows and six reviewed originals')
