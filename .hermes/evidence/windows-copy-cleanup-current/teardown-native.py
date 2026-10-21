from pathlib import Path
import argparse, json, re, subprocess, sys, time
parser = argparse.ArgumentParser()
parser.add_argument('--evidence-dir', type=Path, required=True)
parser.add_argument('--case')
parser.add_argument('--baseline-name', required=True)
args = parser.parse_args()
assert re.fullmatch(r'[A-Za-z0-9_-]+\.json', args.baseline_name)
assert args.case is None or re.fullmatch(r'[A-Za-z0-9_-]+', args.case)
p = args.evidence_dir.resolve()
case = p / args.case if args.case else p
sys.path.insert(0, str(p.parent / 'windows-copy-cleanup-current'))
import transport as native
owner = json.loads((p / 'OWNERSHIP.json').read_text())
assert owner['domain'] == 'ottrin-copy-cleanup-qa' and owner['initial_state'] == 'shut off'
result = json.loads((case / 'RESULTS.json').read_text(encoding='utf-8-sig'))
binary = json.loads((case / 'TEST_BINARY.json').read_text())
dispatch = json.loads((case / 'DISPATCH.json').read_text())
assert result['status'] == 'pass' and result['exit'] == 0
assert result['dispatch_id'] == dispatch['id']
assert result['test_binary_hash'] == binary['transferred_sha256'] == dispatch['transferred_sha256']
script = r'''
if(Test-Path C:\OttrinCopyCleanupQA\NativeTeardown.json){throw 'Owned teardown export already exists'}
$t=Get-ScheduledTask 'Ottrin Copy Cleanup QA'
if($t.State.ToString()-ne'Ready'-or(Get-ScheduledTaskInfo $t).LastTaskResult-ne0){throw 'Owned actor did not finish successfully'}
if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned native process remains'}
if(Test-Path C:\OttrinCopyCleanupQA\temp){throw 'Owned native temporary root remains'}
$baseline=Get-Content C:\OttrinCopyCleanupQA\BASELINE_NAME -Raw|ConvertFrom-Json
foreach($item in $baseline.PSObject.Properties){$actual=$(if(Test-Path -LiteralPath $item.Name){(Get-FileHash -LiteralPath $item.Name).Hash}else{$null});if($actual-ne$item.Value){throw ('Original path changed: '+$item.Name)}}
if((Get-FileHash C:\OttrinCopyCleanupQA\platform.exe).Hash.ToLowerInvariant()-ne'EXPECTED_HASH'){throw 'Owned native executable changed'}
Remove-Item C:\OttrinCopyCleanupQA\platform.exe,C:\OttrinCopyCleanupQA\actor.ps1,C:\OttrinCopyCleanupQA\BASELINE_NAME -Force
@{status='pass';ordinary_actor_state=$t.State.ToString();owned_native_processes=0;temporary_entries=0;exported_executable_and_script_removed=$true;baseline_preferences_and_uninstaller_hashes_unchanged=$true}|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\NativeTeardown.json
'''.replace('EXPECTED_HASH', binary['transferred_sha256']).replace('BASELINE_NAME', args.baseline_name)
fixture = case / 'FIXTURE_BINARY.json'
if fixture.exists():
    fixture_hash = json.loads(fixture.read_text())['transferred_sha256']
    assert re.fullmatch(r'[0-9a-f]{64}', fixture_hash)
    script = script.replace('Remove-Item C:\\OttrinCopyCleanupQA\\platform.exe,',
        "if((Get-FileHash C:\\OttrinCopyCleanupQA\\platform-fixture.exe).Hash.ToLowerInvariant()-ne'" + fixture_hash + "'){throw 'Owned fixture executable changed'};Remove-Item C:\\OttrinCopyCleanupQA\\platform-fixture.exe -Force;Remove-Item C:\\OttrinCopyCleanupQA\\platform.exe,")
pid = native.start(script)
(p / 'teardown.pid').write_text(str(pid))
while True:
    try:
        native.wait(pid, 45)
        break
    except TimeoutError:
        print('Observing the same native teardown PID', flush=True)
native.download('NativeTeardown.json', p / 'TEARDOWN_GUEST.json')
native.wait(native.start('Remove-Item C:\\OttrinCopyCleanupQA\\NativeTeardown.json -Force'), 45)
run = lambda *args: subprocess.check_output(['virsh', *args], text=True).strip()
run('shutdown', owner['domain'], '--mode', 'agent')
end = time.monotonic() + 90
while run('domstate', owner['domain']) != 'shut off':
    assert time.monotonic() < end, 'Guest shutdown pending; inspect same domain'
    time.sleep(1)
assert run('dumpxml', owner['domain'], '--inactive') == (p / 'DOMAIN_BEFORE.xml').read_text().strip()
receipt = {'status': 'pass', 'guest': 'TEARDOWN_GUEST.json', 'domain_state': 'shut off', 'persistent_domain_definition_unchanged': True, 'retained_guest_preserved': True, 'additional_hypervisor_disks_or_guests_created': False}
(p / 'TEARDOWN.json').write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps(receipt))
