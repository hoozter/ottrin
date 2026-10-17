from pathlib import Path
import sys
ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT.parent / 'windows-copy-cleanup-current'))
import transport as native
expected, capture = sys.argv[1:3]
assert expected.isalpha() and all(c.isalnum() or c in '-.' for c in capture)
previous = sys.argv[3] if len(sys.argv) > 3 else None
if previous:
    assert previous.isalpha()
    native.wait(native.start(r'''$r='C:\OttrinCopyCleanupQA\office-ui';if((Get-Content "$r\PHASE.txt" -Raw)-ne'PREVIOUS'){throw 'Unexpected native phase'};[IO.File]::WriteAllText("$r\continue-PREVIOUS",'inspected')'''.replace('PREVIOUS', previous)), 30)
script = r'''$r='C:\OttrinCopyCleanupQA\office-ui';$until=[DateTime]::UtcNow.AddSeconds(65);while((-not(Test-Path "$r\PHASE.txt"))-or(Get-Content "$r\PHASE.txt" -Raw)-ne'EXPECTED'){if(Test-Path "$r\RESULTS.json"){Get-Content "$r\RESULTS.json";Get-Content "$r\main.stderr";Get-Content "$r\inspector.stderr" -ErrorAction SilentlyContinue;throw 'UI actor ended before capture'};if([DateTime]::UtcNow-gt$until){throw 'Capture deadline'};Start-Sleep -Milliseconds 250};Get-Content "$r\PHASE.txt"'''.replace('EXPECTED', expected)
pid = native.start(script)
(ROOT / 'phase.pid').write_text(str(pid))
while True:
    try:
        native.wait(pid, 45)
        break
    except TimeoutError:
        print('Observing the same native phase PID', flush=True)
native.GROOT = r'C:\OttrinCopyCleanupQA\office-ui'
native.download(capture, ROOT / capture)
