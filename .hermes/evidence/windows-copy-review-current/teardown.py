from pathlib import Path
import json, subprocess, sys, time
p = Path(__file__).resolve().parent
sys.path.insert(0, str(p.parent / 'windows-copy-cleanup-current'))
import transport as native
owner = json.loads((p / 'OWNERSHIP.json').read_text())
assert owner['domain'] == 'ottrin-copy-cleanup-qa' and owner['initial_state'] == 'shut off'
result = json.loads((p / 'RESULTS.json').read_text(encoding='utf-8-sig'))
binary = json.loads((p / 'TEST_BINARY.json').read_text())
dispatch = json.loads((p / 'DISPATCH.json').read_text())
assert result['status'] == 'pass' and result['exit'] == 0
assert result['dispatch_id'] == dispatch['id']
assert result['test_binary_hash'] == binary['transferred_sha256'] == dispatch['transferred_sha256']
script = r'''
$t=Get-ScheduledTask 'Ottrin Copy Cleanup QA'
if($t.State.ToString()-ne'Ready'-or(Get-ScheduledTaskInfo $t).LastTaskResult-ne0){throw 'Owned actor did not finish successfully'}
if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned native process remains'}
if(Test-Path C:\OttrinCopyCleanupQA\temp){throw 'Owned native temporary root remains'}
$baseline=Get-Content C:\OttrinCopyCleanupQA\ReviewBaseline.json -Raw|ConvertFrom-Json
foreach($item in $baseline.PSObject.Properties){$actual=$(if(Test-Path -LiteralPath $item.Name){(Get-FileHash -LiteralPath $item.Name).Hash}else{$null});if($actual-ne$item.Value){throw ('Original path changed: '+$item.Name)}}
if((Get-FileHash C:\OttrinCopyCleanupQA\platform.exe).Hash.ToLowerInvariant()-ne'EXPECTED_HASH'){throw 'Owned native executable changed'}
Remove-Item C:\OttrinCopyCleanupQA\platform.exe,C:\OttrinCopyCleanupQA\actor.ps1,C:\OttrinCopyCleanupQA\ReviewBaseline.json -Force
@{status='pass';ordinary_actor_state=$t.State.ToString();owned_native_processes=0;temporary_entries=0;exported_executable_and_script_removed=$true;baseline_preferences_and_uninstaller_hashes_unchanged=$true}|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\ReviewTeardown.json
'''.replace('EXPECTED_HASH', binary['transferred_sha256'])
pid = native.start(script)
(p / 'teardown.pid').write_text(str(pid))
native.wait(pid, 45)
native.download('ReviewTeardown.json', p / 'TEARDOWN_GUEST.json')
native.wait(native.start('Remove-Item C:\\OttrinCopyCleanupQA\\ReviewTeardown.json -Force'), 45)
run = lambda *args: subprocess.check_output(['virsh', *args], text=True).strip()
run('shutdown', owner['domain'], '--mode', 'agent')
end = time.monotonic() + 90
while run('domstate', owner['domain']) != 'shut off':
    assert time.monotonic() < end, 'Guest shutdown pending; inspect same domain'
    time.sleep(1)
assert run('dumpxml', owner['domain'], '--inactive') == (p / 'DOMAIN_BEFORE.xml').read_text().strip()
receipt = {'status': 'pass', 'guest': 'TEARDOWN_GUEST.json', 'domain_state': 'shut off', 'persistent_domain_definition_unchanged': True, 'retained_guest_preserved': True, 'additional_disks_or_guests_created': False}
(p / 'TEARDOWN.json').write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps(receipt))
