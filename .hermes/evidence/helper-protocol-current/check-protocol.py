"""Exercise the real helper's nonmutating handshake and request boundary."""
from pathlib import Path
import hashlib, importlib.util, json, os, subprocess, sys
sys.dont_write_bytecode = True
root = Path.cwd()
evidence = Path(__file__).resolve().parent
binary = root / 'target/debug/ottrin-priv-helper'
assert os.geteuid() != 0
spec = importlib.util.spec_from_file_location('workspace', root / 'scripts/test-workspace.py')
workspace = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workspace)
cases = []
with workspace.isolated_environment() as (profile, env):
    target = profile / 'protected-by-protocol'
    request = {'command': {'File': {'CreateFile': {'parent': str(profile), 'name': target.name}}}, 'context': {'reason': 'Owned protocol check', 'cwd': str(profile)}}
    def run(args, payload):
        return subprocess.run([str(binary), *args], input=json.dumps(payload), text=True, capture_output=True, env=env, timeout=10)
    for args, code in [(['--check-protocol', '1'], 0), (['--check-protocol', '1', '--copy-recovery'], 0), (['--check-protocol', '2'], 78), (['--check-protocol'], 78), (['--check-protocol', '1', '--unknown'], 69)]:
        result = run(args, request)
        assert result.returncode == code, result
        assert not target.exists()
        cases.append({'arguments': args, 'exit': code, 'no_mutation': True})
    for version in [None, 0, 2, '1', -1]:
        payload = dict(request)
        if version is not None:
            payload['protocol_version'] = version
        result = run(['--stdio-json'], payload)
        assert result.returncode == 0, result
        response = json.loads(result.stdout)
        assert response['status'] == 'Failed', response
        assert not target.exists()
        cases.append({'invalid_version': version, 'response': response, 'no_mutation': True})
    for args in [['--stdio-json', '--check-protocol', '1'], ['--stdio-json', '--unknown']]:
        result = run(args, dict(request, protocol_version=1))
        assert result.returncode == 2 and not target.exists(), result
        cases.append({'arguments': args, 'exit': 2, 'no_mutation': True})
    result = run(['--stdio-json'], dict(request, protocol_version=1))
    response = json.loads(result.stdout)
    assert result.returncode == 0 and response['status'] == 'Success' and target.is_file(), result
    cases.append({'valid_version': 1, 'created_requested_file': True})
report = {'status': 'pass', 'scope': 'Actual unprivileged debug helper; not packaged, elevated or native UI acceptance', 'helper_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'cases': cases, 'owned_profile_removed': not profile.exists()}
(evidence / 'PROTOCOL_ACCEPTANCE.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))
