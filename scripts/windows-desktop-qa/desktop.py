"""Owned Windows desktop input; observer timeouts resume the same guest PID."""
import hashlib
import argparse
import json
from pathlib import Path
import sys
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / '.hermes/evidence/windows-copy-cleanup-current'))
import transport as n

n.GROOT = r'C:\OttrinWindowQA'
HERE = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--evidence-dir', required=True, type=Path)
parser.add_argument('operation', help='prepare, cleanup, retire, resume, or a JSON desktop operation')
arguments = parser.parse_args()
E = arguments.evidence_dir.resolve()
if E.parent != ROOT / '.hermes/evidence':
    raise SystemExit('Evidence must be one owned directory under .hermes/evidence')


def wait(pid):
    (E / 'observer.pid').write_text(str(pid))
    try:
        n.wait(pid, 45)
    except TimeoutError:
        print('Observer live; use resume, never dispatch the operation again.')
        raise SystemExit(75)


operation = arguments.operation
if operation in ['cleanup', 'retire']:
    if operation == 'cleanup':
        wait(n.start((HERE / 'cleanup.ps1').read_text()))
        n.download('cleanup-result.json', E / 'TEARDOWN_GUEST.json')
        for name in ['app.stderr', 'app.stdout']:
            n.download('results\\' + name, E / name)
    wait(n.start((HERE / 'retire-owned-root.ps1').read_text()))
    subprocess.run(['virsh', '-c', 'qemu:///system', 'shutdown', 'ottrin-copy-cleanup-qa', '--mode', 'agent'], check=True)
    print('Owned fixtures/account removed; guest shutdown requested. Verify domain off and unchanged XML.')
    raise SystemExit()
if operation == 'prepare':
    owner = json.loads((E / 'OWNERSHIP.json').read_text())
    archive = Path(owner['archive'])
    assert hashlib.sha256(archive.read_bytes()).hexdigest() == owner['archive_sha256']
    manifest = Path(owner['payload_manifest']).resolve()
    manifest.relative_to(ROOT / '.hermes/evidence')
    payload = json.loads(manifest.read_text())
    assert payload['archive_sha256'] == owner['archive_sha256']
    assert payload['source_commit'] == owner['package_source']
    wait(n.start((HERE / 'prepare.ps1').read_text()))
    for name in ['interop.ps1', 'actor.ps1', 'cleanup.ps1']:
        n.upload(HERE / name, name)
    n.upload(ROOT / '.hermes/evidence/address-selection-current/VALID_CONFIG_TEMPLATE.json', 'config-template.json')
    n.upload(manifest, 'payload-manifest.json')
    n.upload(archive, 'payload.zip')
    wait(n.start(r"""
$root='C:\OttrinWindowQA'
$tokens=$null;$errors=$null
foreach($f in @('actor.ps1','interop.ps1','cleanup.ps1')){
 $null=[Management.Automation.Language.Parser]::ParseFile("$root\$f",[ref]$tokens,[ref]$errors)
 if($errors.Count){throw ($errors|Out-String)}
}
if((Get-FileHash "$root\payload.zip" -Algorithm SHA256).Hash.ToLower()-ne'""" + owner['archive_sha256'] + r"""'){throw 'ZIP identity mismatch'}
Expand-Archive "$root\payload.zip" "$root\payload"
Remove-Item "$root\payload.zip"
$m=Get-Content "$root\payload-manifest.json" -Raw|ConvertFrom-Json
$files=@(Get-ChildItem "$root\payload" -File -Recurse)
if($files.Count-ne@($m.entries.PSObject.Properties).Count){throw 'Payload count mismatch'}
foreach($e in $m.entries.PSObject.Properties){if((Get-FileHash (Join-Path "$root\payload" $e.Name)).Hash.ToLower()-ne$e.Value){throw 'Payload hash mismatch'}}
@{status='pass';count=$files.Count;source=$m.source_commit;archive_sha256=$m.archive_sha256}|ConvertTo-Json|Set-Content "$root\results\payload.json"
Write-Output 'Scripts parsed; exact candidate extracted.'
"""))
    print('Prepared. Reboot once to enter the owned console.')
    raise SystemExit()
if operation == 'resume':
    wait(int((E / 'observer.pid').read_text()))
    command = json.loads((E / 'command.json').read_text())
else:
    command = json.loads(operation)
    command.update(id=uuid.uuid4().hex, case='ordinary')
    (E / 'command.json').write_text(json.dumps(command))
    n.upload(E / 'command.json', 'command.json')
    script = r"""
if((Get-CimInstance Win32_ComputerSystem).UserName-notlike'*\OttrinWindowQA'){throw 'Wrong console user'}
$t=Get-ScheduledTask 'Ottrin Window Input QA'
if($t.State.ToString()-in@('Running','Queued')){throw 'Owned actor already active'}
Remove-Item C:\OttrinWindowQA\results\actor-error.txt -ErrorAction SilentlyContinue
Start-ScheduledTask $t.TaskName
$deadline=[DateTime]::UtcNow.AddSeconds(90)
do{
 Start-Sleep -Milliseconds 150;$t=Get-ScheduledTask $t.TaskName
 $done=$false
 if(Test-Path C:\OttrinWindowQA\results\desktop.json){$r=Get-Content C:\OttrinWindowQA\results\desktop.json -Raw|ConvertFrom-Json;$done=$r.id-eq'__ID__'}
 if($t.State.ToString()-notin@('Running','Queued')-and$done){break}
 if($t.State.ToString()-notin@('Running','Queued')-and(Test-Path C:\OttrinWindowQA\results\actor-error.txt)){throw 'Owned actor failed'}
 if([DateTime]::UtcNow-gt$deadline){throw 'Actor did not publish matching result'}
}while($true)
if((Get-ScheduledTaskInfo $t.TaskName).LastTaskResult-ne0){throw 'Owned actor failed'}
""".replace('__ID__', command['id'])
    try:
        wait(n.start(script))
    except AssertionError:
        for name in ['actor-error.txt', 'app.stderr', 'app.stdout']:
            try:
                n.download('results\\' + name, E / name)
                print((E / name).read_text(encoding='utf-8-sig'))
            except Exception as error:
                print(type(error).__name__, name)
        raise
for name in ['desktop.json', 'desktop.png']:
    n.download('results\\' + name, E / name)
result = json.loads((E / 'desktop.json').read_text(encoding='utf-8-sig'))
assert result['id'] == command['id']
with (E / 'commands.jsonl').open('a') as log:
    log.write(json.dumps({'command': command, 'result': result}) + '\n')
print(json.dumps(result))
