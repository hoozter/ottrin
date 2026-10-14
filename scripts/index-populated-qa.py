#!/usr/bin/env python3
"""Exact packaged metadata index: populated idle, burst freshness and recovery."""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import sqlite3
import struct
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('appimage', type=Path)
parser.add_argument('receipt', type=Path)
args = parser.parse_args()
artifact = args.appimage.resolve()
receipt = args.receipt.resolve()
receipt.parent.mkdir(parents=True, exist_ok=True)
provenance = json.loads(Path(str(artifact)+'.provenance.json').read_text())
assert hashlib.sha256(artifact.read_bytes()).hexdigest() == provenance['sha256']
assert provenance['built_from_tracked_clean_source']
result = {'artifact': str(artifact), 'provenance': provenance,
          'scope': 'Private 10,000-file metadata-only corpus; no GUI, login unit or content extraction'}
process = None

def cancelled(signum, _frame):
    raise SystemExit(128+signum)

for sig in (signal.SIGINT, signal.SIGTERM):
    signal.signal(sig, cancelled)

with tempfile.TemporaryDirectory(prefix='ottrin-populated-') as temporary:
    root = Path(temporary)
    env = os.environ.copy()
    for key, name in [('HOME', 'home'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_DATA_HOME', 'data'),
                      ('XDG_STATE_HOME', 'state'), ('XDG_RUNTIME_DIR', 'runtime')]:
        p = root/name
        p.mkdir(mode=0o700)
        env[key] = str(p)
    env['DBUS_SESSION_BUS_ADDRESS'] = 'unix:path='+str(root/'absent-bus')
    env['OTTRIN_DEV_HELPER_AUTOBUILD'] = '0'
    corpus = root/'corpus'
    corpus.mkdir()
    files = []
    for bucket in range(100):
        directory = corpus/f'bucket-{bucket:03}'
        directory.mkdir()
        for item in range(100):
            p = directory/f'seed-{bucket*100+item:05}.txt'
            p.write_bytes(b'original metadata fixture')
            files.append(p)
    config = root/'config/ottrin'
    config.mkdir()
    (config/'config.json').write_text(json.dumps({'theme': 'Dark', 'default_view_mode': 'Miller',
        'list_columns': ['Name', 'Kind', 'Size', 'Modified'],
        'command_frame_always_visible': False, 'restore_session': False,
        'show_hidden_files': False, 'target': {'current': None},
        'last_session_dirs': [], 'search_setup_completed': True,
        'search': {'include_roots': [str(corpus)], 'exclude_roots': [],
                   'refresh_interval_hours': 0, 'content_indexing_enabled': False,
                   'use_system_locate': False, 'manage_locate_db': False}}))
    subprocess.run([str(artifact), '--appimage-extract'], cwd=root,
                   stdout=subprocess.DEVNULL, check=True, timeout=30)
    extracted = root/'squashfs-root'
    source = (extracted/'usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text()
    assert 'Git revision: '+provenance['revision'] in source and 'Working tree: clean' in source
    binary = extracted/'usr/bin/ottrin-indexd'
    env['LD_LIBRARY_PATH'] = str(extracted/'usr/lib')
    endpoint = root/'runtime/ottrin/index-v1.sock'
    database = root/'cache/ottrin/search.db'

    def request(kind, **extra):
        payload = json.dumps({'version': 1, 'request': {'type': kind, **extra}}).encode()
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(3)
            client.connect(str(endpoint))
            client.sendall(struct.pack('!I', len(payload))+payload)
            def exact(count):
                data = bytearray()
                while len(data) < count:
                    part = client.recv(count-len(data))
                    assert part, 'incomplete response'
                    data.extend(part)
                return data
            size = struct.unpack('!I', exact(4))[0]
            assert size <= 1024*1024
            response = json.loads(exact(size))
            assert response['version'] == 1
            assert response['response']['type'] != 'error', response
            return response['response']

    def status():
        return request('status')['diagnostics']

    def rows():
        with closing(sqlite3.connect(database.as_uri()+'?mode=ro', uri=True)) as conn:
            generation = conn.execute('SELECT active_generation_id FROM search_store_state WHERE singleton=1').fetchone()[0]
            entries = conn.execute('SELECT path,size_bytes FROM search_generation_items WHERE generation_id=?', (generation,)).fetchall()
            return generation, {os.fsdecode(path): size for path, size in entries if os.fsdecode(path).endswith('.txt')}

    def search_response(text):
        query = {'text': text, 'scope': 'Global', 'root_path': None,
                 'include_hidden_system': False, 'sort': 'Name', 'limit': 500,
                 'offset': 0, 'content_search': False}
        return request('query', query=query)['result']['response']

    def query_count(text):
        return search_response(text)['total']

    def queries_match(expected):
        for prefix in ('renamed-', 'born-', 'seed-'):
            matching = sorted((p for p in expected if Path(p).name.startswith(prefix)),
                              key=lambda p: Path(p).name)
            response = search_response(prefix)
            returned = {item['path']: item['size_bytes'] for item in response['items']}
            if response['total'] != len(matching) or returned != {p: expected[p] for p in matching[:500]}:
                return False
        return True

    def counters():
        fields = Path(f'/proc/{process.pid}/stat').read_text().rsplit(')', 1)[1].split()
        memory = dict(line.split(':', 1) for line in Path(f'/proc/{process.pid}/status').read_text().splitlines() if ':' in line)
        return int(fields[11])+int(fields[12]), int(memory['VmRSS'].split()[0])

    def quiet_sample(label):
        print(label+': measuring 30 seconds without IPC or file changes', flush=True)
        before, _ = counters()
        started = time.monotonic()
        while time.monotonic()-started < 30:
            assert process.poll() is None, 'daemon exited during quiet sample'
            time.sleep(.1)
        after, rss = counters()
        elapsed = time.monotonic()-started
        measurement = {'seconds': round(elapsed, 3), 'cpu_ticks': after-before,
                       'cpu_percent_one_core': round((after-before)/os.sysconf('SC_CLK_TCK')/elapsed*100, 3),
                       'rss_kib_at_end': rss}
        result[label] = measurement
        assert measurement['cpu_percent_one_core'] < .5, measurement

    try:
        with receipt.with_suffix('.log').open('w') as log:
            process = subprocess.Popen([str(binary)], env=env, stdout=log,
                                       stderr=subprocess.STDOUT, start_new_session=True)
            deadline = time.monotonic()+60
            while True:
                assert process.poll() is None, 'daemon startup exited'
                diagnostics = status() if endpoint.exists() else {}
                if diagnostics.get('status') == 'Ready' and diagnostics.get('watcher_ready'):
                    generation, initial = rows()
                    if len(initial) == 10000:
                        break
                assert time.monotonic() < deadline, diagnostics
                time.sleep(.1)
            assert initial == {str(p): p.stat().st_size for p in files}
            assert query_count('seed-') == 10000
            result['initial_diagnostics'] = diagnostics
            result['initial_generation'] = generation
            time.sleep(2)
            quiet_sample('populated_idle')
            started = time.monotonic()
            for p in files[:500]:
                p.rename(p.with_name(p.name.replace('seed-', 'renamed-')))
            for p in files[500:1000]:
                p.unlink()
            for p in files[1000:1500]:
                p.write_bytes(b'modified metadata fixture with different length')
            for item in range(500):
                (corpus/'bucket-099'/f'born-{item:05}.txt').write_bytes(b'new fixture')
            mutation_ms = (time.monotonic()-started)*1000
            expected = {str(p): p.stat().st_size for p in corpus.rglob('*.txt')}
            assert len(expected) == 10000
            deadline = time.monotonic()+30
            while True:
                assert process.poll() is None, 'daemon exited after burst'
                after_generation, indexed = rows()
                diagnostics = status()
                if indexed == expected and diagnostics['status'] == 'Ready' and queries_match(expected):
                    break
                assert time.monotonic() < deadline, {'indexed_count': len(indexed), 'status': diagnostics}
                time.sleep(.1)
            result['burst'] = {'renamed': 500, 'deleted': 500, 'modified': 500, 'created': 500,
                               'mutation_ms': round(mutation_ms, 3),
                               'all_metadata_and_query_results_match_ms': round((time.monotonic()-started)*1000, 3),
                               'generation_unchanged': after_generation == generation,
                               'scan_started_unchanged': diagnostics['last_scan_started_unix_secs'] == result['initial_diagnostics']['last_scan_started_unix_secs']}
            assert result['burst']['generation_unchanged'] and result['burst']['scan_started_unchanged'], result['burst']
            result['after_burst_diagnostics'] = diagnostics
            time.sleep(3)
            quiet_sample('after_burst_idle')
            assert request('shutdown')['type'] == 'complete'
            assert process.wait(timeout=10) == 0
            assert not endpoint.exists()
            started = time.monotonic()
            process = subprocess.Popen([str(binary)], env=env, stdout=log,
                                       stderr=subprocess.STDOUT, start_new_session=True)
            deadline = time.monotonic()+15
            while True:
                assert process.poll() is None, 'restart exited before readiness'
                restored = status() if endpoint.exists() else {}
                if restored.get('status') == 'Ready' and restored.get('watcher_ready'):
                    restored_generation, restored_rows = rows()
                    if restored_rows == expected and queries_match(expected):
                        break
                assert time.monotonic() < deadline, restored
                time.sleep(.05)
            result['restart'] = {
                'ready_with_complete_metadata_and_query_results_ms': round((time.monotonic()-started)*1000, 3),
                'generation_unchanged': restored_generation == generation,
                'no_scan_started_in_new_process': restored['last_scan_started_unix_secs'] is None,
                'watcher_ready': restored['watcher_ready']}
            result['restored_diagnostics'] = restored
            assert result['restart']['generation_unchanged'] and result['restart']['no_scan_started_in_new_process'], result['restart']
            assert request('shutdown')['type'] == 'complete'
            assert process.wait(timeout=10) == 0 and not endpoint.exists()
    finally:
        if process and process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
result['cleanup'] = {'daemon_exited': process.poll() is not None, 'private_profile_removed': not root.exists()}
receipt.write_text(json.dumps(result, indent=2)+'\n')
print(json.dumps(result, indent=2), flush=True)
