"""Bind the native theme editor exercise to the exact staged AppImage."""
import hashlib
import json
from pathlib import Path
import subprocess

evidence = Path(__file__).resolve().parent

def read(path):
    return json.loads(path.read_text())

def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

build = read(evidence / 'BUILD_SOURCE.json')
content = read(evidence / 'PACKAGE_CONTENT_ACCEPTANCE.json')
image = Path(build['stage']) / 'Ottrin-0.1.0-x86_64.AppImage'
assert content['source_revision'] == build['source_revision']
assert digest(image) == content['artifacts'][image.name]

expected = {
    'Window corners': 24,
    'Surface corners': 14,
    'Text size': 1.2,
    'Button corners': 14,
    'Window border thickness': 3.25,
    'Neutral text blend': 0.75,
}
cases = {}
for case, theme, scale in [('candidate-dark100', 'Dark', 1.0), ('candidate-light150', 'Light', 1.5)]:
    directory = evidence / case
    control = read(directory / 'CONTROL.json')
    teardown = read(directory / 'TEARDOWN.json')
    result = read(directory / 'FUNCTIONAL_RESULT.json')
    assert control['status'] == 'functional checks pass; native controls exercised'
    assert control['source_revision'] == content['source_revision']
    assert control['artifact_sha256'] == content['artifacts'][image.name]
    assert control['worker_sha256'] == digest(directory / 'WORKER_SOURCE.py')
    assert result['theme'] == theme and result['scale'] == scale
    assert result['named_selector_used'] and result['source_preserved']
    assert result['unsaved_numeric_edit_not_persisted']
    assert len(result['cases']) == len(expected)
    assert {entry['name']: entry['typed'] for entry in result['cases']} == expected
    assert teardown['profile_removed'] and not Path(teardown['profile']).exists()
    assert not any(teardown['profile'] in mount for mount in subprocess.check_output(
        ['findmnt', '-rn', '-o', 'TARGET'], text=True).splitlines())
    cases[case] = result | {'profile_removed': True, 'worker_sha256': control['worker_sha256']}

receipt = {
    'status': 'pass',
    'source_revision': content['source_revision'],
    'appimage_sha256': content['artifacts'][image.name],
    'app_binary_sha256': content['runtime_sha256']['application'],
    'cases': cases,
    'scope': 'Mounted AppImage in private X11. Named AT-SPI component selector, typed values, property-specific Restore, Cancel and normal closure. No broad color or visual effect claim.',
}
(evidence / 'THEME_COMPONENT_ACCEPTANCE.json').write_text(json.dumps(receipt, indent=2) + '\n')
