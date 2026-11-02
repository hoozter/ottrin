"""Bind actual-package MSI observations to native logs and completed fixture teardown."""
from pathlib import Path
import argparse, hashlib, json, runpy, zipfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--case', type=Path, required=True)
    args = parser.parse_args()
    root = args.case.resolve()
    assert root.is_relative_to(Path.cwd() / '.hermes/evidence')
    original = root / 'GUEST_RESULT.json'
    result = json.loads(original.read_text())
    assert result['status'] == 'observed' and result['desktop_ready']
    assert not result['identity']['administrator'] and result['identity']['session'] > 0
    expected = json.loads((root / 'APPLICATION_INPUT.json').read_text())
    assert result['package'] == expected
    assert result['installed_payload_files'] == expected['payload_files']
    assert isinstance(result['installed_payload_files'], int)
    assert result['cleanup_tool_available'] and result['service_cleanup_exit'] == 0
    archive = zipfile.ZipFile(root / 'RESULTS.zip') if (root / 'RESULTS.zip').exists() else None
    try:
        raw = archive.read('failure.log') if archive else (root / 'results/failure.log').read_bytes()
        text = raw.decode('utf-16') if raw.startswith(b'\xff\xfe') else raw.decode('utf-8-sig')
        assert 'CleanupBeforeUninstall' in text and 'returned actual error code 1' in text
        classify = runpy.run_path(str(Path(__file__).with_name('assess-control.py')))['rollback_lines']
        errors, terminal = classify(text)
        if archive:
            final_result = json.loads(archive.read('RESULT.json').decode('utf-8-sig'))
        else:
            final_result = json.loads((root / 'results/RESULT.json').read_text(encoding='utf-8-sig'))
        assert final_result == result, 'Parent snapshot preceded the final native cleanup acknowledgement'
    finally:
        if archive:
            archive.close()
    owner = json.loads((root / 'OWNER.json').read_text())
    viewer = json.loads((root / 'viewer/TEARDOWN.json').read_text())
    assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed']
    assert len(owner['cleanup']['owned_volumes_removed']) == 2
    assert viewer['profile_removed'] and not Path(viewer['profile']).exists()
    preservation = {key: result[key] for key in [
        'daemon_instance_unchanged', 'task_xml_unchanged', 'task_security_unchanged',
        'payload_intact', 'preferences_preserved', 'source_preserved',
        'database_identity_preserved', 'index_query_preserved',
        'preferences_after_retry', 'source_after_retry']}
    passed = result['state_gate_passed'] and not errors and all(preservation.values())
    report = {
        'status': 'pass within exact actual-MSI scope' if passed else 'failed ordinary-user actual MSI rollback/retry gate',
        'source_revision': expected['source_revision'], 'msi_sha256': expected['sha256'],
        'installed_payload_files_verified': result['installed_payload_files'],
        'native_identity': result['identity'],
        'original_actor_result_sha256': hashlib.sha256(original.read_bytes()).hexdigest(),
        'failure_log_sha256': hashlib.sha256(raw).hexdigest(),
        'fault': 'Unmodified production cleanup refuses an owned cache file held without delete sharing',
        'postconditions_preserved': preservation,
        'outcomes': {key: result[key] for key in [
            'install_exit', 'failure_exit', 'state_after_failure', 'retry_exit',
            'state_after_retry', 'payload_after_retry', 'state_gate_passed']},
        'raw_skipped_lines': len(errors) + len(terminal),
        'genuine_or_unknown_undo_failures': len(errors),
        'successful_terminal_end_markers': len(terminal),
        'native_database_identity': [result['database_file_id_before'], result['database_file_id_after']],
        'fixture_cleanup': 'Explicit service-remove exits0 before terminal native result; exact final snapshot agrees; guest, two volumes and private console/profile removed',
        'scope_limit': 'Real ordinary-user current package on the independent Microsoft client. Preserved data/task state does not accept broken package registration/retry. No elevation, registry repair or security-policy change.'}
    (root / 'ASSESSMENT.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'status': report['status'], 'preservation_checks': preservation,
                      'genuine_undo_failures': len(errors), 'fixture_cleanup_verified': True}))


if __name__ == '__main__':
    main()
