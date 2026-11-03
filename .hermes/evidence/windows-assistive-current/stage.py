"""Build one private UIAutomation input, retaining explicit runtime provenance."""
from pathlib import Path
import argparse
import hashlib
import json
import subprocess
import zipfile

parser = argparse.ArgumentParser()
parser.add_argument("--renderer", choices=["native", "opengl"], default="native")
parser.add_argument("--evidence", type=Path, required=True)
args = parser.parse_args()
root = Path.cwd()
stage = root / '.hermes/packaging-tmp/.windows-assistive-stage'
evidence = args.evidence.resolve()
assert evidence.is_relative_to(root / '.hermes/evidence')
baseline = json.loads((evidence / 'BUILD_BASELINE.json').read_text())
revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
assert revision == baseline['source'] and not baseline['tracked_dirty']
observer_changes = subprocess.check_output(['git', 'diff', '--name-only'], text=True).splitlines()
assert set(observer_changes) <= {'.hermes/evidence/windows-assistive-current/actor.ps1'}, observer_changes
assert not stage.exists(), 'Existing stage must finish its consumers before replacement'
archive = root / 'dist/windows-candidate/Ottrin-0.1.0-windows-x64-portable.zip'
app = root / 'target/x86_64-pc-windows-gnu/release/ottrin-app.exe'
digest = lambda b: hashlib.sha256(b).hexdigest()
borrowed_hash = 'e11cb37ad662d79c991d86c6a623e1e8aeaebf9db49283d600e3d529fc007863'
assert digest(archive.read_bytes()) == borrowed_hash
app_bytes = app.read_bytes()
assert app_bytes[:2] == b'MZ'
build = json.loads((evidence / 'BUILD_RESULT.json').read_text())
assert build['status'] == 'standard Windows GNU release build passed'
assert build['source'] == revision and build['application_sha256'] == digest(app_bytes)
prefix = 'Ottrin-0.1.0-windows-x64/'
changed = [prefix + 'SOURCE_REVISION.txt', prefix + 'bin/ottrin.exe']
members = {}
stage.mkdir(parents=True)
try:
    with zipfile.ZipFile(archive) as original, zipfile.ZipFile(
        stage / 'uia.zip', 'w', compression=zipfile.ZIP_DEFLATED
    ) as candidate:
        files = [n for n in original.namelist() if not n.endswith('/')]
        assert len(files) == len(set(files)) == 58
        assert all(n.startswith(prefix) and '..' not in Path(n).parts for n in files)
        assert all(n in files for n in changed)
        for name in files:
            raw = original.read(name)
            if name == changed[0]:
                raw = (f'Private native UIAutomation GUI source {revision}\n'
                       'Borrowed accepted runtime source 656768852a930e4e69dcf37e901d5bf00c6b6175; '
                       'not a rebuilt package cohort.\n').encode()
            elif name == changed[1]:
                raw = app_bytes
            candidate.writestr(name, raw)
            members[name] = {'sha256': digest(raw), 'bytes': len(raw)}
    with zipfile.ZipFile(archive) as original, zipfile.ZipFile(stage / 'uia.zip') as candidate:
        assert sorted(n for n in files if original.read(n) != candidate.read(n)) == changed
        assert all(digest(candidate.read(n)) == v['sha256'] for n, v in members.items())
    receipt = {
        'source_revision': revision, 'application_sha256': digest(app_bytes),
        'observer_only_changes': observer_changes, 'renderer': args.renderer, 'flow': 'all',
        'borrowed_runtime_source': '656768852a930e4e69dcf37e901d5bf00c6b6175',
        'borrowed_archive_sha256': borrowed_hash,
        'archive_sha256': digest((stage / 'uia.zip').read_bytes()),
        'scope': 'Standard GNU release GUI plus exact accepted borrowed runtime; private native UIAutomation input, not refreshed delivery',
        'changed_members': changed, 'payload_members': members,
        'cleanup_owner': 'Roadmap task, after owned native UIAutomation consumer is terminal and volumes are removed',
    }
    raw = json.dumps(receipt, indent=2) + '\n'
    (stage / 'uia-input.json').write_text(raw)
    (evidence / 'INPUT.json').write_text(raw)
except BaseException:
    # No guest can consume a producer that has not returned a complete receipt.
    for name in ['uia.zip', 'uia-input.json']:
        (stage / name).unlink(missing_ok=True)
    stage.rmdir()
    raise
print(json.dumps({'source': revision, 'application_sha256': digest(app_bytes), 'members': len(members)}))
