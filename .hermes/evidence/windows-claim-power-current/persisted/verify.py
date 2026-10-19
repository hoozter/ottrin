"""Verify this persisted-reservation observation, not all Copy power boundaries."""
from pathlib import Path
import gzip, hashlib, json

p = Path(__file__).resolve().parent

def read(name):
    return json.loads((p / (name + '.json')).read_text(encoding='utf-8-sig'))

def claim(name, expected_sha, transaction):
    data = gzip.decompress((p / (name + '.bin.gz')).read_bytes())
    assert hashlib.sha256(data).hexdigest() == expected_sha
    assert data[:8] == b'OTCWCL03'
    assert int.from_bytes(data[8:12], 'little') == len(data) - 84
    assert data[12:52] == bytes(40)  # First sequence, no preceding frame.
    assert hashlib.sha256(data[12:52] + data[8:12] + data[84:]).digest() == data[52:84]
    value = json.loads(data[84:])
    assert value['event'] == 'claim' and value['claim']['transaction'] == transaction * 32
    return value['claim']

x, before, live, after, recovered, cut, cleanup, teardown = [
    read(n) for n in ['DISPATCH', 'BEFORE', 'LIVE', 'AFTER', 'RECOVERY', 'CUT', 'CLEANUP', 'TEARDOWN']
]
assert x['status'] == 'inspected-recovered-cleaned' and x['phase'] == 'Persisted'
for receipt in [before, live, after, recovered, cut, cleanup]:
    assert receipt['dispatch_id'] == x['id']
for receipt in [before, after, recovered]:
    assert receipt['ordinary_user'] and receipt['binary_sha256'] == x['binary_sha256']
    assert receipt['root'] == x['root']
assert live['child_live'] and live['child_pid'] == before['child_pid']
assert cut['guest_state'] == 'shut off'
assert before['canonical_exists'] and after['canonical_exists'] and not after['preparation_exists']
assert set(after['claim_root_names']) == {'short-names.ottrin-claims', 'short-names.ottrin-claims.root', '.preparing-' + 'f' * 32}
assert before['canonical_sha256'] == after['canonical_sha256']
for field in ['source_sha256', 'public_sha256', 'foreign_sha256']:
    assert before[field] == after[field] == recovered[field]
assert recovered['status'] == 'recovered-name' and recovered['fresh_admission'] and recovered['owner_exit'] == 86
for stream in ['stdout', 'stderr']:
    raw = gzip.decompress((p / ('recover.' + stream + '.gz')).read_bytes())
    assert hashlib.sha256(raw).hexdigest() == recovered[stream + '_sha256']
old = claim('old-canonical', before['canonical_sha256'], '1')
new = claim('canonical', recovered['canonical_sha256'], '2')
assert old['file'] != new['file']
for field in ['parent', 'root', 'anchor', 'bucket', 'bucket_anchor', 'central', 'creator', 'security']:
    assert old[field] == new[field]
assert old['target'] == list(b'copy') and new['target'] == list(b'COPY')
assert cleanup['status'] == 'pass' and cleanup['root_absent']
assert teardown['status'] == 'pass' and teardown['owned_vm_state'] == 'shut off'
assert teardown['owned_temporary_roots_absent'] and teardown['owned_executable_processes'] == 0
assert teardown['temporary_login_credential_absent'] and teardown['winlogon_unchanged']
assert teardown['source_profile_sha256'] == live['profile_sha256']
assert teardown['source_installation_sha256'] == live['install_sha256']
print('PASS: one persisted reservation survives guest stop and is safely reclaimed by a fresh ordinary process; protected bytes and original installation/profile/login preserved')
