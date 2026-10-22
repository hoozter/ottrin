"""Fresh processes across actual detach/reconnect, before and after retirement."""
from pathlib import Path
import argparse, json, re, runpy, signal, sys
q = runpy.run_path(str(Path(__file__).with_name('run-cases.py')))
TOOLS, native_case = q['TOOLS'], q['case']
parser = argparse.ArgumentParser()
parser.add_argument('--evidence-root', type=Path, required=True)
parser.add_argument('--binary-receipt', type=Path, required=True)
parser.add_argument('--prefix', required=True)
args = parser.parse_args()
ROOT = args.evidence_root.resolve()
def cancelled(number, _frame):
    raise SystemExit(128 + number)
signal.signal(signal.SIGTERM, cancelled)
assert re.fullmatch(r'[A-Za-z0-9_-]+', args.prefix)
def case(name, arguments):
    native_case(ROOT, args.prefix + '-' + name, arguments, reuse_receipt=args.binary_receipt)

sys.path.insert(0, str(TOOLS))
import transport as n

def volume(action, label):
    label = args.prefix + '-' + label
    pid = n.start(r"& ([ScriptBlock]::Create((Get-Content C:\OttrinCopyCleanupQA\cross-volume-control.ps1 -Raw))) -Action " + action)
    (ROOT / (label + '.pid')).write_text(str(pid))
    while True:
        try:
            n.wait(pid, 45)
            break
        except TimeoutError:
            print('Continuing volume controller', pid, flush=True)
    export = 'CrossMoveVolume' + action.title() + '.json'
    n.download(export, ROOT / (label + '.json'))

def observed(label):
    label = args.prefix + '-' + label
    pid = n.start(r"@{attached=[bool](Get-DiskImage -ImagePath C:\OttrinCopyCleanupQA\cross-move-volume\owned.vhdx).Attached;fixture=[bool](Test-Path C:\OttrinCopyCleanupQA\completion-offline\fixture.json)}|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\CompletionVolumeObserved.json")
    (ROOT / (label + '.pid')).write_text(str(pid))
    while True:
        try:
            n.wait(pid, 45)
            break
        except TimeoutError:
            print('Continuing volume observation', pid, flush=True)
    target = ROOT / (label + '.json')
    n.download('CompletionVolumeObserved.json', target)
    return json.loads(target.read_text(encoding='utf-8-sig'))

assert not observed('offline-initial')['fixture']
case('offline-failure-teardown', ['--move-completion-offline', 'failure_teardown'])
for retired in (False, True):
    prefix = 'offline-retired' if retired else 'offline-original'
    try:
        case(prefix + '-prepare', ['--move-completion-offline', 'prepare_retired' if retired else 'prepare'])
        n.download(r'completion-offline\fixture.json', ROOT / (args.prefix + '-' + prefix + '-fixture.json'))
        volume('detach', prefix + '-detach')
        case(prefix + '-verify', ['--move-completion-offline', 'verify'])
    finally:
        # A controller interruption must not race a still-live scheduled actor.
        # Its own watchdog tears down its process/profile; wait for that actor
        # before touching the owned VHD or the persistent fixture.
        pid = n.start(r"$deadline=(Get-Date).AddSeconds(930);while((Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-in@('Running','Queued')){if((Get-Date)-gt$deadline){throw 'Owned actor still live; preserve its volume'};Start-Sleep -Milliseconds 500}")
        (ROOT / (args.prefix + '-' + prefix + '-actor-terminal.pid')).write_text(str(pid))
        while True:
            try:
                n.wait(pid, 45)
                break
            except TimeoutError:
                print('Waiting for the same scheduled actor', pid, flush=True)
        current = observed(prefix + '-observed')
        if not current['attached']:
            volume('attach', prefix + '-attach')
        if current['fixture']:
            case(prefix + '-cleanup', ['--move-completion-offline', 'cleanup'])
    assert not observed(prefix + '-final')['fixture']
