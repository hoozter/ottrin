"""Assess immutable native MSI state and logs, without relabelling the original run."""
from pathlib import Path
import argparse, hashlib, json, re, zipfile

SUCCESS_LOG_REFERENCE = 'https://learn.microsoft.com/en-us/windows/win32/msi/logging-of-action-return-values'


def rollback_lines(text):
    """Only the exact success-coded terminal End marker is separated from failures."""
    lines = text.splitlines()
    genuine, terminal = [], []
    for number, line in enumerate(lines):
        if 'Error in rollback skipped' not in line:
            continue
        match = re.search(r'Error in rollback skipped\.\s*Return:\s*(\d+)\s*$', line)
        if (match and match[1] == '5' and number > 0
                and re.fullmatch(r'.*Executing op: End\(Checksum=0,ProgressTotalHDWord=0,ProgressTotalLDWord=0\)', lines[number - 1])):
            terminal.append({'line': number + 1, 'text': line})
        else:
            # Unknown formatting and all failed/other operations remain failures.
            genuine.append({'line': number + 1, 'text': line})
    return genuine, terminal


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--case', type=Path, required=True)
    args = parser.parse_args()
    root = args.case.resolve()
    assert root.is_relative_to(Path.cwd() / '.hermes/evidence')
    original = root / 'GUEST_RESULT.json'
    result = json.loads(original.read_text())
    assert result['status'] in ['pass', 'failed native rollback or supported cleanup']
    assert [case['kind'] for case in result['cases']] == ['install', 'uninstall']
    cases = []
    archive = zipfile.ZipFile(root / 'RESULTS.zip') if (root / 'RESULTS.zip').exists() else None
    try:
        for case in result['cases']:
            name = 'failure-' + case['kind'] + '.log'
            raw = archive.read(name) if archive else (root / 'results' / name).read_bytes()
            text = raw.decode('utf-16') if raw.startswith(b'\xff\xfe') else raw.decode('utf-8-sig')
            errors, terminal = rollback_lines(text)
            file_op = 'Executing op: FileCopy' if case['kind'] == 'install' else 'Executing op: FileRemove'
            product_op = 'Executing op: ProductRegister(' if case['kind'] == 'install' else 'Executing op: ProductUnpublish('
            fault = text.find('Executing op: CustomActionSchedule(Action=DeliberateFailure')
            if fault < 0:
                fault = text.find('Doing action: DeliberateFailure')
            assert fault >= 0, 'Deliberate failure missing from native log'
            executed = 0 <= text.find(file_op) < fault and 0 <= text.find(product_op) < fault
            expected_state = -1 if case['kind'] == 'install' else 5
            state_ok = (case['state_before'] == -1 and case['failure_exit'] == 1603
                        and case['state_after_failure'] == expected_state
                        and case['payload_after_failure'] == (case['kind'] == 'uninstall')
                        and case['actual_file_operation_before_failure'] and executed)
            cleanup_ok = (case['recovery_install_exit'] == 0 and case['state_after_recovery'] == 5
                          and case['cleanup_exit'] == 0 and case['state_after_cleanup'] == -1
                          and not case['payload_after_cleanup'])
            cases.append({'kind': case['kind'], 'log_sha256': hashlib.sha256(raw).hexdigest(),
                          'raw_skipped_lines': len(errors) + len(terminal),
                          'successful_terminal_end_markers': terminal, 'failed_or_unknown_undo_lines': errors,
                          'file_and_registration_executed_before_fault': executed,
                          'rollback_state_correct': state_ok, 'supported_recovery_and_cleanup_correct': cleanup_ok,
                          'pass': state_ok and cleanup_ok and not errors})
    finally:
        if archive:
            archive.close()
    passed = all(case['pass'] for case in cases)
    elevated = result['identity']['administrator']
    report = {'status': 'pass within diagnostic scope' if passed else 'failed native rollback/cleanup gate',
              'scope': 'Elevated file-only diagnostic; cannot accept an ordinary-user or actual Ottrin installer' if elevated else 'Ordinary-user native file-only control; actual Ottrin installer remains separate',
              'original_actor_status': result['status'], 'original_actor_result_sha256': hashlib.sha256(original.read_bytes()).hexdigest(),
              'source_reference': SUCCESS_LOG_REFERENCE,
              'interpretation': 'Logged action return5 is documented success. Only the adjacent exact terminal End(0) marker is separated; all other skipped lines, failed state, missing executed operations and cleanup residue still fail. Original results/logs are unchanged.',
              'cases': cases}
    (root / 'CONTROL_ASSESSMENT.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'status': report['status'], 'scope': report['scope'],
                      'cases': [{'kind': case['kind'], 'genuine_lines': len(case['failed_or_unknown_undo_lines']), 'pass': case['pass']} for case in cases]}))


if __name__ == '__main__':
    main()
