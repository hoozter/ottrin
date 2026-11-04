"""Accept the complete native result only after direct original-capture review."""
from pathlib import Path
import hashlib, json, subprocess

r = Path.cwd()
e = Path(__file__).resolve().parent
f = e / 'native'
read = lambda p: json.loads(p.read_text())
v, owner = read(f / 'GUEST_RESULT.json'), read(f / 'OWNER.json')
source = read(e / 'BUILD_SOURCE.json')['source_revision']
assert v['status'].startswith('pass') and v['package']['source_revision'] == source
assert v['source_preserved'] and v['preferences_preserved']
assert v['remaining_owned_installed_processes'] == 0
assert v['dpi_setup']['actual_settings_dpi'] == 144
assert len(v['cases']) == 34
actions = v['numeric_actions']
assert len(actions) == 8 and all(a['keyboard_focus_acknowledged'] for a in actions)
assert [a['expected'] for a in actions] == [24, 0, 24, 24] * 2
assert [a['before'] for a in actions] == [0, 24, 0, 0] * 2
expected = {
    ('editor-live', 24), ('editor-live', 0),
    ('settings-live', 24), ('settings-live', 0),
    ('main-live', 24), ('main-live', 0),
    ('editor-after-restore', 0), ('settings-after-restore', 0),
    ('main-after-restore', 0), ('editor-before-cancel', 24),
    ('settings-after-cancel', 0), ('main-after-cancel', 0),
    ('main-saved-startup', 24), ('settings-saved-startup', 24),
    ('inspector-scaled', 24), ('picker-scaled', 24), ('properties-scaled', 24),
}
for renderer in ['Native', 'OpenGL']:
    cases = [c for c in v['cases'] if c['renderer'] == renderer]
    assert len(cases) == 17
    assert {(c['case'], c['logical_radius']) for c in cases} == expected
for c in v['cases']:
    assert c['dpi'] == 144 and c['physical_radius'] == c['logical_radius'] * 1.5
    assert c['native_expected_region_match'] and c['window_fits_work_area']
    assert bool(c['capture']) == c['foreground_capture_requested']
captures = {c['capture']: hashlib.sha256((f / 'results' / c['capture']).read_bytes()).hexdigest()
            for c in v['cases'] if c['capture']}
assert len(captures) == 22
review = read(e / 'WINDOWS_ORIGINAL_REVIEW.json')
assert review['captures'] == captures and review['directly_reviewed_originals'] == 22
assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed']
assert len(owner['cleanup']['owned_volumes_removed']) == 2
assert subprocess.run(['virsh', '-c', 'qemu:///system', 'domuuid', owner['domain']], capture_output=True).returncode != 0
assert v['package']['files'][0]['sha256'] == read(e / 'HOST_PAYLOAD.json')['artifacts']['Ottrin-0.1.0-windows-x64-setup.exe']
receipt = {
    'status': 'pass complete scoped Windows native numeric and 144DPI window matrix',
    'source_revision': source, 'installer_sha256': v['package']['files'][0]['sha256'],
    'cases': 34, 'full_window_captures': 22, 'captures': captures,
    'checks': ['Typed0/24 and property-specific restore through the actual named native control',
               'Live complete native regions on main/Settings/editor, including background owners',
               'Cancel rollback and saved-theme preservation',
               'Actual144DPI and complete work-area containment for every region observation',
               'Separate unobscured full-frame captures for all six window types, Native/OpenGL',
               'Normal close, supported uninstall, source and preference preservation'],
    'limits': ['Background owners have native-region observations while the editor owns input; no foreground capture is claimed for those12 observations',
               'Software VM with signed VirtIO display driver; physical graphics and human usability separate',
               'Dark Windows144DPI scope; not all appearance settings or platform combinations',
               'Separate fresh GUI idle baseline is recorded below; no performance fix, daemon CPU, Store/signing or fresh MSIX runtime/update/login acceptance'],
    'gui_idle_baseline': v.get('gui_idle_baseline', []),
    'cleanup': owner['cleanup'], 'result': str((f / 'GUEST_RESULT.json').relative_to(r)),
}
(e / 'WINDOWS_NATIVE_ACCEPTANCE.json').write_text(json.dumps(receipt, indent=2) + '\n')
print('Accepted34native cases and22directly reviewed complete-window captures')
