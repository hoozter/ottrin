"""Run the metadata barrier on the retained Windows guest, with owned teardown."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import signal
import subprocess
import sys
import time

sys.dont_write_bytecode = True
root = Path.cwd()
evidence = Path(__file__).resolve().parent
producer = root / '.hermes/evidence/windows-copy-cleanup-current'
sys.path.insert(0, str(producer))
import transport as native

parser = argparse.ArgumentParser()
parser.add_argument('--selection', choices=['group-metadata', 'configured-actions'], default='group-metadata')
options = parser.parse_args()
domain = 'ottrin-copy-cleanup-qa'
report = {'status': 'starting', 'scope': 'ordinary-user ' + options.selection}
started = False
registered = False


def virsh(*args):
    return subprocess.check_output(['virsh', '-c', 'qemu:///system', *args], text=True).strip()


def execute(script):
    pid = native.start(script)
    while True:
        try:
            return native.wait(pid, 45)
        except TimeoutError:
            print(f'Observing the same guest command {pid}', flush=True)


def cancelled(signum, _frame):
    raise SystemExit(128 + signum)


for sig in (signal.SIGTERM, signal.SIGINT):
    signal.signal(sig, cancelled)

source = json.loads((evidence / 'BUILD_SOURCE.json').read_text())
for path, digest in source['source_sha256'].items():
    assert hashlib.sha256((root / path).read_bytes()).hexdigest() == digest
assert json.loads((evidence / 'build.jsonl').read_text().splitlines()[-1])['success']
assert virsh('domstate', domain) == 'shut off'
definition = virsh('dumpxml', domain, '--inactive')
(evidence / 'DOMAIN_BEFORE.xml').write_text(definition + '\n')
(evidence / 'OWNERSHIP.json').write_text(json.dumps({
    'domain': domain, 'initial_state': 'shut off', 'new_domains_or_volumes': False,
    'cleanup': 'Remove only this dispatch executable, actor, private temp, transport and baseline; restore shut-off state and unchanged definition.'
}, indent=2) + '\n')
try:
    # Ownership/teardown is registered before starting the existing guest.
    started = True
    virsh('start', domain)
    end = time.monotonic() + 120
    while True:
        try:
            native.qga('guest-ping')
            break
        except (subprocess.SubprocessError, RuntimeError, AssertionError):
            assert time.monotonic() < end, 'Same retained guest did not expose its agent'
            time.sleep(1)
    execute(r'''
$r='C:\OttrinCopyCleanupQA'
$task=Get-ScheduledTask 'Ottrin Copy Cleanup QA'
if($task.State.ToString()-in@('Running','Queued')){throw 'Existing actor is live'}
if([int]$task.Principal.RunLevel-ne0){throw 'Expected existing limited actor'}
foreach($name in @('platform.exe','platform.gz','actor.ps1','temp','PreparationBaseline.json')){
 if(Test-Path (Join-Path $r $name)){throw ('Unconsumed owned path: '+$name)}
}
$b=@{}
foreach($path in @('C:\Users\campbell\AppData\Roaming\ottrin\config.json','C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe')){
 $b[$path]=$(if(Test-Path -LiteralPath $path){(Get-FileHash -LiteralPath $path).Hash}else{$null})
}
$b|ConvertTo-Json|Set-Content "$r\PreparationBaseline.json"
''')
    registered = True
    subprocess.run([sys.executable, str(producer / 'run-native.py'), '--' + options.selection,
                    '--watchdog-ms', '600000', '--evidence-dir', str(evidence)], check=True)
    environment = {**os.environ, 'OTTRIN_NATIVE_EVIDENCE_DIR': str(evidence)}
    args = [sys.executable, str(producer / 'observe.py')]
    while True:
        result = subprocess.run(args, env=environment)
        if result.returncode != 75:
            result.check_returncode()
            break
        args = [sys.executable, str(producer / 'observe.py'), 'resume']
    report['status'] = 'pass within native metadata scope'
except BaseException as error:
    report['status'] = 'failed'
    report['error'] = repr(error)
    raise
finally:
    try:
        try:
            if registered:
                result = execute(r'''
    $r='C:\OttrinCopyCleanupQA'
    $task=Get-ScheduledTask 'Ottrin Copy Cleanup QA'
    if($task.State.ToString()-in@('Running','Queued')){Stop-ScheduledTask $task.TaskName}
    foreach($process in @(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'})){
     & C:\Windows\System32\taskkill.exe /PID $process.ProcessId /T /F|Out-Null
    }
    if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned process teardown incomplete'}
    $b=Get-Content "$r\PreparationBaseline.json" -Raw|ConvertFrom-Json
    foreach($item in $b.PSObject.Properties){
     $actual=$(if(Test-Path -LiteralPath $item.Name){(Get-FileHash -LiteralPath $item.Name).Hash}else{$null})
     if($actual-ne$item.Value){throw ('Original path changed: '+$item.Name)}
    }
    foreach($name in @('platform.exe','platform.gz','actor.ps1','temp','PreparationBaseline.json')){
     $path=Join-Path $r $name
     if(Test-Path -LiteralPath $path){Remove-Item -LiteralPath $path -Recurse -Force}
    }
    foreach($name in @('RESULTS.json','RESULTS.json.tmp','platform.stdout','platform.stderr')){
     $path=Join-Path "$r\results" $name
     if(Test-Path -LiteralPath $path){Remove-Item -LiteralPath $path -Force}
    }
    @{status='pass';owned_processes=0;owned_temp_and_exports_removed=$true;original_preferences_and_uninstaller_preserved=$true}|ConvertTo-Json
    ''')
                report['guest_teardown'] = result
        finally:
            if started:
                if virsh('domstate', domain) != 'shut off':
                    virsh('shutdown', domain, '--mode', 'agent')
                    end = time.monotonic() + 90
                    while virsh('domstate', domain) != 'shut off':
                        assert time.monotonic() < end, 'Retained guest shutdown still pending'
                        time.sleep(1)
                assert virsh('dumpxml', domain, '--inactive') == definition
                report['retained_guest_shut_off_and_definition_preserved'] = True
    except BaseException as error:
        report['status'] = 'teardown incomplete'
        report['teardown_error'] = repr(error)
        raise
    finally:
        (evidence / 'CONTROLLER_RESULT.json').write_text(json.dumps(report, indent=2) + '\n')
