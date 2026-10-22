#!/usr/bin/env python3
"""Exact packaged index: populated idle, metadata/content freshness and restart."""
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
parser.add_argument('--offline-changes', action='store_true')
parser.add_argument('--content', action='store_true', help='Enable plain-text body indexing and verify persisted bodies plus content queries')
parser.add_argument('--quiet-seconds', type=float, default=30)
args = parser.parse_args()
assert args.quiet_seconds >= 0
artifact = args.appimage.resolve()
receipt = args.receipt.resolve()
receipt.parent.mkdir(parents=True, exist_ok=True)
provenance = json.loads(Path(str(artifact)+'.provenance.json').read_text())
assert hashlib.sha256(artifact.read_bytes()).hexdigest() == provenance['sha256']
assert provenance['built_from_tracked_clean_source']
result = {'artifact': str(artifact), 'provenance': provenance,
          'driver_sha256': hashlib.file_digest(Path(__file__).open('rb'), 'sha256').hexdigest(),
          'scope': 'Private 10,000-file '+('plain-text content-enabled' if args.content else 'metadata-only')+' corpus; no GUI, login unit or personal files'}
process = None
root = None
body_tokens = ['qaoriginalbodytoken', 'qamodifiedbodytoken', 'qacreatedbodytoken',
               'qaoffline1bodytoken', 'qaofflinecreatedbodytoken']
def body(text, token):
    return text + ((' ' + token).encode() if args.content else b'')

def cancelled(signum, _frame):
    raise SystemExit(128+signum)

for sig in (signal.SIGINT, signal.SIGTERM):
    signal.signal(sig, cancelled)

try:
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
                p.write_bytes(body(b'original metadata fixture', body_tokens[0]))
                files.append(p)
        config = root/'config/ottrin'
        config.mkdir()
        (config/'config.json').write_text(json.dumps({'theme': 'Dark', 'default_view_mode': 'Miller',
            'list_columns': ['Name', 'Kind', 'Size', 'Modified'],
            'command_frame_always_visible': False, 'restore_session': False,
            'show_hidden_files': False, 'target': {'current': None},
            'last_session_dirs': [], 'search_setup_completed': True,
            'search': {'include_roots': [str(corpus)], 'exclude_roots': [],
                       'refresh_interval_hours': 0, 'content_indexing_enabled': args.content,
                       'content_include_roots': [str(corpus)] if args.content else [],
                       'content_file_groups': ['TextDocuments'],
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

        def search_result(text, *, content_search=False):
            query = {'text': text, 'scope': 'Global', 'root_path': None,
                     'include_hidden_system': False, 'sort': 'Name', 'limit': 500,
                     'offset': 0, 'content_search': content_search}
            return request('query', query=query)['result']

        def search_response(text):
            return search_result(text)['response']

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

        def content_matches(expected_content):
            if not args.content:
                return True
            generation, _ = rows()
            content_database = Path(str(database)+f'.content-{generation}.db')
            try:
                with closing(sqlite3.connect(content_database.as_uri()+'?mode=ro', uri=True)) as conn:
                    cached = {os.fsdecode(path): (state, content) for path, state, content in conn.execute(
                        'SELECT path,state,content FROM search_generation_content_state WHERE generation_id=?', (generation,)
                    ) if os.fsdecode(path).endswith('.txt')}
            except sqlite3.OperationalError as error:
                result['last_content_observation_error'] = str(error)
                return False
            if cached != {path: (0, content) for path, content in expected_content.items()}:
                result['last_content_observation_error'] = 'persisted body rows have not converged'
                return False
            for token in body_tokens:
                matching = sorted((path for path, content in expected_content.items() if token in content), key=lambda path: Path(path).name)
                queried = search_result(token, content_search=True)
                response = queried['response']
                if response.get('error') or queried.get('warning') or response['total'] != len(matching) or {item['path'] for item in response['items']} != set(matching[:500]):
                    result['last_content_observation_error'] = {'query': token, 'expected': len(matching), 'response_total': response['total'], 'error': response.get('error'), 'warning': queried.get('warning')}
                    return False
            result.pop('last_content_observation_error', None)
            return True

        def expected_bodies():
            return {str(p): p.read_text() for p in corpus.rglob('*.txt')} if args.content else {}

        def counters():
            fields = Path(f'/proc/{process.pid}/stat').read_text().rsplit(')', 1)[1].split()
            memory = dict(line.split(':', 1) for line in Path(f'/proc/{process.pid}/status').read_text().splitlines() if ':' in line)
            return int(fields[11])+int(fields[12]), int(memory['VmRSS'].split()[0])

        def quiet_sample(label):
            if args.quiet_seconds == 0:
                result[label] = {'skipped': True}
                return
            print(label+f': measuring {args.quiet_seconds:g} seconds without IPC or file changes', flush=True)
            before, _ = counters()
            started = time.monotonic()
            while time.monotonic()-started < args.quiet_seconds:
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
                initial_content = expected_bodies()
                process = subprocess.Popen([str(binary)], env=env, stdout=log,
                                           stderr=subprocess.STDOUT, start_new_session=True)
                deadline = time.monotonic()+60
                while True:
                    assert process.poll() is None, 'daemon startup exited'
                    diagnostics = status() if endpoint.exists() else {}
                    if diagnostics.get('status') == 'Ready' and diagnostics.get('watcher_ready'):
                        generation, initial = rows()
                        if len(initial) == 10000 and content_matches(initial_content):
                            break
                    assert time.monotonic() < deadline, diagnostics
                    time.sleep(.1)
                assert initial == {str(p): p.stat().st_size for p in files}
                assert query_count('seed-') == 10000
                result['initial_diagnostics'] = diagnostics
                result['initial_generation'] = generation
                if args.content: result['initial_content'] = {'persisted_bodies_match': True, 'query_groups_verified': len(body_tokens), 'files': len(initial_content)}
                time.sleep(2)
                quiet_sample('populated_idle')
                started = time.monotonic()
                for p in files[:500]:
                    p.rename(p.with_name(p.name.replace('seed-', 'renamed-')))
                for p in files[500:1000]:
                    p.unlink()
                for p in files[1000:1500]:
                    p.write_bytes(body(b'modified metadata fixture with different length', body_tokens[1]))
                if args.content:
                    unchanged = files[1800].stat()
                    before = files[1800].read_bytes()
                    after = before.replace(body_tokens[0].encode(), body_tokens[1].encode())
                    assert after != before and len(after) == len(before)
                    files[1800].write_bytes(after)
                    os.utime(files[1800], ns=(unchanged.st_atime_ns, unchanged.st_mtime_ns))
                    result['live_same_size_edit'] = {'bytes': len(after), 'mtime_ns_preserved': files[1800].stat().st_mtime_ns == unchanged.st_mtime_ns}
                for item in range(500):
                    (corpus/'bucket-099'/f'born-{item:05}.txt').write_bytes(body(b'new fixture', body_tokens[2]))
                mutation_ms = (time.monotonic()-started)*1000
                expected = {str(p): p.stat().st_size for p in corpus.rglob('*.txt')}
                assert len(expected) == 10000
                changed_content = expected_bodies()
                deadline = time.monotonic()+30
                while True:
                    assert process.poll() is None, 'daemon exited after burst'
                    after_generation, indexed = rows()
                    diagnostics = status()
                    if indexed == expected and diagnostics['status'] == 'Ready' and queries_match(expected) and content_matches(changed_content):
                        break
                    assert time.monotonic() < deadline, {'indexed_count': len(indexed), 'status': diagnostics}
                    time.sleep(.1)
                result['burst'] = {'renamed': 500, 'deleted': 500, 'modified': 501 if args.content else 500, 'created': 500,
                                   'mutation_ms': round(mutation_ms, 3),
                                   'all_metadata_and_query_results_match_ms': round((time.monotonic()-started)*1000, 3),
                                   'generation_unchanged': after_generation == generation,
                                   'scan_started_unchanged': diagnostics['last_scan_started_unix_secs'] == result['initial_diagnostics']['last_scan_started_unix_secs']}
                assert result['burst']['generation_unchanged'] and result['burst']['scan_started_unchanged'], result['burst']
                result['after_burst_diagnostics'] = diagnostics
                if args.content: result['after_burst_content'] = {'persisted_bodies_match': True, 'query_groups_verified': len(body_tokens), 'files': len(changed_content)}
                time.sleep(3)
                quiet_sample('after_burst_idle')
                assert request('shutdown')['type'] == 'complete'
                assert process.wait(timeout=10) == 0
                assert not endpoint.exists()
                if args.offline_changes:
                    if args.content:
                        unchanged = files[2000].stat()
                        before = files[2000].read_bytes()
                        after = before.replace(body_tokens[0].encode(), body_tokens[3].encode())
                        assert after != before and len(after) == len(before)
                        files[2000].write_bytes(after)
                        os.utime(files[2000], ns=(unchanged.st_atime_ns, unchanged.st_mtime_ns))
                        result['offline_same_size_edit'] = {'bytes': len(after), 'mtime_ns_preserved': files[2000].stat().st_mtime_ns == unchanged.st_mtime_ns}
                    else:
                        files[2000].write_bytes(b'changed while daemon was stopped, with a new size')
                    files[3000].rename(files[3000].with_name('renamed-offline.txt'))
                    files[4000].unlink()
                    (corpus/'bucket-099'/'born-offline.txt').write_bytes(body(b'created offline', body_tokens[4]))
                    expected = {str(p): p.stat().st_size for p in corpus.rglob('*.txt')}
                    result['offline_changes'] = {'modified': 1, 'renamed': 1, 'deleted': 1, 'created': 1}
                restarted_content = expected_bodies()
                started = time.monotonic()
                process = subprocess.Popen([str(binary)], env=env, stdout=log,
                                           stderr=subprocess.STDOUT, start_new_session=True)
                deadline = time.monotonic()+15
                while True:
                    assert process.poll() is None, 'restart exited before readiness'
                    restored = status() if endpoint.exists() else {}
                    if restored.get('status') == 'Ready' and restored.get('watcher_ready'):
                        restored_generation, restored_rows = rows()
                        if restored_rows == expected and queries_match(expected) and content_matches(restarted_content) and restored.get('last_scan_started_unix_secs') is not None and (restored.get('detail') or '').startswith('Ready'):
                            break
                    assert time.monotonic() < deadline, {'diagnostics': restored, 'missing': sorted(set(expected)-set(restored_rows)), 'unexpected': sorted(set(restored_rows)-set(expected)), 'wrong_sizes': [p for p in expected.keys() & restored_rows.keys() if expected[p] != restored_rows[p]]}
                    time.sleep(.05)
                result['restart'] = {
                    'ready_with_complete_metadata_and_query_results_ms': round((time.monotonic()-started)*1000, 3),
                    'generation_unchanged': restored_generation == generation,
                    'startup_validation_completed': restored['last_scan_started_unix_secs'] is not None and (restored.get('detail') or '').startswith('Ready'),
                    'watcher_ready': restored['watcher_ready']}
                result['restored_diagnostics'] = restored
                if args.content: result['after_restart_content'] = {'persisted_bodies_match': True, 'query_groups_verified': len(body_tokens), 'files': len(restarted_content)}
                assert result['restart']['startup_validation_completed'], result['restart']
                time.sleep(2)
                quiet_sample('after_restart_idle')
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
    result['status'] = 'pass'
except BaseException as error:
    result['status'] = 'fail'
    result['failure'] = {'type': type(error).__name__, 'message': str(error)}
    raise
finally:
    result['cleanup'] = {'daemon_exited': process is None or process.poll() is not None, 'private_profile_removed': root is None or not root.exists()}
    receipt.write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(result, indent=2), flush=True)
