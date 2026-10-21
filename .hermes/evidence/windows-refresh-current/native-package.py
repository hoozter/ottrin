#!/usr/bin/env python3
"""Owned native WiX compilation and payload checks for this staged candidate."""
from pathlib import Path
import base64, hashlib, json, os, subprocess, sys, time, zipfile

p = Path(__file__).resolve().parent
root = p.parents[2]
owner = json.loads((p / 'OWNERSHIP.json').read_text())
assert owner['domain'] == 'ottrin-copy-cleanup-qa'
stage = Path(owner['stage'])
assert stage == root / '.hermes/packaging-tmp/.windows-refresh-stage'
sys.path.insert(0, str(p.parent / 'windows-copy-cleanup-current'))
import transport as native
native.GROOT = r'C:\OttrinCopyCleanupQA\package-refresh'
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()

def observe(script, phase):
    pid = native.start(script)
    (p / (phase + '.pid')).write_text(str(pid))
    while True:
        try:
            return native.wait(pid, 45)
        except TimeoutError:
            print('Still observing the same native ' + phase + ' PID', flush=True)

def write(name, data):
    (p / name).write_text(json.dumps(data, indent=2) + '\n')

mode = sys.argv[1]
if mode == 'prepare':
    pins = json.loads((p.parent / 'windows-20261011/windows-msi-compiler-downloads.json').read_text())
    cache = root / '.hermes/tools/windows-msi-native'
    assert hashlib.sha512((cache / 'runtime.zip').read_bytes()).hexdigest() == pins['runtime_sha512']
    assert sha(cache / 'wix.nupkg') == pins['wix_sha256']
    observe(r'''$r='C:\OttrinCopyCleanupQA\package-refresh';if(Test-Path $r){throw 'Owned compiler root exists'}
$t=Get-ScheduledTask 'Ottrin Copy Cleanup QA';if($t.State.ToString()-ne'Ready'){throw 'Ordinary QA actor still active'}
if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned QA process remains'}
New-Item -ItemType Directory $r|Out-Null
$b=@{};foreach($path in @('C:\Users\campbell\AppData\Roaming\ottrin\config.json','C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe')){$b[$path]=$(if(Test-Path -LiteralPath $path){(Get-FileHash -LiteralPath $path).Hash}else{$null})};$b|ConvertTo-Json|Set-Content "$r\BASELINE.json"
''', 'prepare-root')
    native.download('BASELINE.json', p / 'BASELINE_GUEST.json')
    native.upload(cache / 'runtime.zip', 'runtime.zip')
    native.upload(cache / 'wix.nupkg', 'wix.zip')
    observe(r'''$r='C:\OttrinCopyCleanupQA\package-refresh';Expand-Archive "$r\runtime.zip" "$r\dotnet";Expand-Archive "$r\wix.zip" "$r\wix";& "$r\dotnet\dotnet.exe" "$r\wix\tools\net6.0\any\wix.dll" --version;if($LASTEXITCODE-ne0){throw 'Native compiler failed'};Remove-Item "$r\runtime.zip","$r\wix.zip"''', 'prepare-compiler')
elif mode == 'deploy':
    archive = stage / 'Ottrin-0.1.0-windows-x64-portable.zip'
    with zipfile.ZipFile(archive) as z:
        entries = {i.filename: hashlib.sha256(z.read(i)).hexdigest() for i in z.infolist() if not i.is_dir()}
        revision = z.read('Ottrin-0.1.0-windows-x64/SOURCE_REVISION.txt').decode()
        assert 'Git revision: ' + owner['source_commit'] in revision and 'Working tree: clean' in revision
        assert all(name.startswith('Ottrin-0.1.0-windows-x64/') and '..' not in Path(name).parts for name in entries)
    manifest = {'source_commit': owner['source_commit'], 'archive_sha256': sha(archive), 'entries': entries}
    write('PAYLOAD.json', manifest)
    native.upload(archive, 'candidate.zip')
    native.upload(p / 'PAYLOAD.json', 'PAYLOAD.json')
    observe(r'''$r='C:\OttrinCopyCleanupQA\package-refresh';$m=Get-Content "$r\PAYLOAD.json" -Raw|ConvertFrom-Json;if((Get-FileHash "$r\candidate.zip").Hash.ToLowerInvariant()-ne$m.archive_sha256){throw 'Archive changed'};if(Test-Path "$r\payload"){throw 'Payload extraction exists'};Expand-Archive "$r\candidate.zip" "$r\payload";foreach($e in $m.entries.PSObject.Properties){if((Get-FileHash (Join-Path "$r\payload" $e.Name)).Hash.ToLowerInvariant()-ne$e.Value){throw ('Payload mismatch: '+$e.Name)}};$count=@(Get-ChildItem "$r\payload" -Recurse -File).Count;if($count-ne@($m.entries.PSObject.Properties).Count){throw 'Extra payload files'};Remove-Item "$r\candidate.zip";@{status='pass';source_commit=$m.source_commit;entries_checked=$count;archive_sha256=$m.archive_sha256}|ConvertTo-Json|Set-Content "$r\DEPLOYMENT.json"''', 'deploy')
    native.download('DEPLOYMENT.json', p / 'DEPLOYMENT.json')
elif mode == 'build':
    args = sys.argv[1:]
    assert len(args) == 10 and args[:3] == ['build', '-arch', 'x64']
    assert args[3] == args[5] == '-d' and args[4] == 'Version=0.1.0'
    assert args[6] == 'Stage=' + str(stage / 'windows-payload/Ottrin-0.1.0-windows-x64') and args[7] == '-o'
    assert Path(args[9]) == root / 'packaging/windows/installer.wxs'
    output = Path(args[8])
    assert output.parent.parent == stage and output.parent.name.startswith('.windows-installer.')
    native.upload(Path(args[9]), 'installer.wxs')
    result = observe(r'''$r='C:\OttrinCopyCleanupQA\package-refresh';$m=Get-Content "$r\PAYLOAD.json" -Raw|ConvertFrom-Json;foreach($e in $m.entries.PSObject.Properties){if((Get-FileHash (Join-Path "$r\payload" $e.Name)).Hash.ToLowerInvariant()-ne$e.Value){throw 'Compiler payload changed'}};& "$r\dotnet\dotnet.exe" "$r\wix\tools\net6.0\any\wix.dll" build -arch x64 -d Version=0.1.0 -d 'Stage=C:\OttrinCopyCleanupQA\package-refresh\payload\Ottrin-0.1.0-windows-x64' -o "$r\current.msi" "$r\installer.wxs";if($LASTEXITCODE-ne0){throw 'Native MSI compilation failed'};(Get-FileHash "$r\current.msi").Hash.ToLowerInvariant()''', 'compile')
    expected = base64.b64decode(result['out-data']).decode().strip().splitlines()[-1]
    native.download('current.msi', output)
    assert sha(output) == expected
    print('Native MSI SHA256 ' + expected, flush=True)
elif mode == 'cli-smoke':
    observe(r'''$r='C:\OttrinCopyCleanupQA\package-refresh';$bin="$r\payload\Ottrin-0.1.0-windows-x64\bin";$runs=@()
foreach($case in @(@('ottrin.exe',0,'Ottrin'),@('ottrin-indexd.exe',2,'usage: ottrin-indexd'),@('ottrin-priv-helper.exe',2,'Usage: ottrin-priv-helper'))){
 $stdout=Join-Path $r ($case[0]+'.stdout');$stderr=Join-Path $r ($case[0]+'.stderr');$process=$null
 try{$process=Start-Process (Join-Path $bin $case[0]) -ArgumentList '--help' -WorkingDirectory C:\Windows\System32 -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru;$null=$process.Handle;if(-not$process.WaitForExit(15000)){throw 'CLI smoke timed out'};if($process.ExitCode-ne$case[1]){throw ('Unexpected CLI exit for '+$case[0]+': '+$process.ExitCode)};$out=[IO.File]::ReadAllText($stdout);$err=[IO.File]::ReadAllText($stderr);if(($out+$err).Length-gt65536-or-not($out+$err).Contains($case[2])){throw ('Unexpected CLI output for '+$case[0])};$runs+=@{executable=$case[0];exit=$process.ExitCode;stdout_sha256=(Get-FileHash $stdout).Hash.ToLowerInvariant();stderr_sha256=(Get-FileHash $stderr).Hash.ToLowerInvariant()}}
 finally{if($process){if(-not$process.HasExited){$process.Kill();$null=$process.WaitForExit(5000)};$process.Dispose()}}
}
@{status='pass';identity=[Security.Principal.WindowsIdentity]::GetCurrent().Name;runs=$runs;scope='Native loader and CLI argument paths only; no GUI or service lifecycle'}|ConvertTo-Json -Depth 5|Set-Content "$r\CLI_SMOKE.json"''', 'cli-smoke')
    native.download('CLI_SMOKE.json', p / 'CLI_SMOKE.json')
elif mode == 'verify':
    expected = sha(stage / 'Ottrin-0.1.0-windows-x64.msi')
    script = r'''$r='C:\OttrinCopyCleanupQA\package-refresh';if((Get-FileHash "$r\current.msi").Hash.ToLowerInvariant()-ne'EXPECTED'){throw 'MSI changed'};if(Test-Path "$r\msi-extract"){throw 'MSI extraction exists'};@{state=(Get-Service msiserver).Status.ToString()}|ConvertTo-Json|Set-Content "$r\MSI_SERVICE_BASELINE.json";$proc=Start-Process msiexec.exe -ArgumentList @('/a',"$r\current.msi",'/qn',"TARGETDIR=$r\msi-extract",'/l*v',"$r\extract.log") -Wait -PassThru;if($proc.ExitCode-ne0){throw ('Administrative extraction exit '+$proc.ExitCode)};$roots=@(Get-ChildItem "$r\msi-extract" -Recurse -Filter SOURCE_REVISION.txt);if($roots.Count-ne1){throw 'Ambiguous MSI payload'};$payload=$roots[0].Directory.FullName;$m=Get-Content "$r\PAYLOAD.json" -Raw|ConvertFrom-Json;foreach($e in $m.entries.PSObject.Properties){$relative=$e.Name.Substring($e.Name.IndexOf('/')+1);if((Get-FileHash (Join-Path $payload $relative)).Hash.ToLowerInvariant()-ne$e.Value){throw ('MSI payload mismatch: '+$relative)}};$count=@(Get-ChildItem $payload -Recurse -File).Count;if($count-ne@($m.entries.PSObject.Properties).Count){throw 'Extra MSI payload files'};@{status='pass';source_commit=$m.source_commit;entries_checked=$count;msi_sha256='EXPECTED'}|ConvertTo-Json|Set-Content "$r\MSI_PAYLOAD.json"'''.replace('EXPECTED', expected)
    observe(script, 'verify-msi')
    native.download('MSI_PAYLOAD.json', p / 'MSI_PAYLOAD.json')
elif mode == 'teardown':
    observe(r'''$r='C:\OttrinCopyCleanupQA\package-refresh';if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like"$r\*"}).Count){throw 'Owned package process remains'};$b=Get-Content "$r\BASELINE.json" -Raw|ConvertFrom-Json;foreach($item in $b.PSObject.Properties){$actual=$(if(Test-Path -LiteralPath $item.Name){(Get-FileHash -LiteralPath $item.Name).Hash}else{$null});if($actual-ne$item.Value){throw ('Original changed: '+$item.Name)}};if(Test-Path "$r\MSI_SERVICE_BASELINE.json"){$service=Get-Content "$r\MSI_SERVICE_BASELINE.json" -Raw|ConvertFrom-Json;if($service.state-eq'Stopped'-and(Get-Service msiserver).Status-ne'Stopped'){Stop-Service msiserver -ErrorAction Stop};if((Get-Service msiserver).Status.ToString()-ne$service.state){throw 'MSI service baseline not restored'}};Remove-Item -LiteralPath $r -Recurse -Force;@{status='pass';native_processes=0;msi_service_baseline_restored=$true;compiler_payload_and_extractions_removed=$true;original_preferences_and_uninstaller_hashes_unchanged=$true}|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\PackageRefreshTeardown.json''', 'teardown')
    native.GROOT = r'C:\OttrinCopyCleanupQA'
    native.download('PackageRefreshTeardown.json', p / 'TEARDOWN_GUEST.json')
    observe(r'Remove-Item C:\OttrinCopyCleanupQA\PackageRefreshTeardown.json -Force', 'teardown-receipt')
    run = lambda *args: subprocess.check_output(['virsh', *args], text=True).strip()
    run('shutdown', owner['domain'], '--mode', 'agent')
    end = time.monotonic() + 90
    while run('domstate', owner['domain']) != 'shut off':
        assert time.monotonic() < end, 'Guest shutdown pending; inspect same domain'
        time.sleep(1)
    assert run('dumpxml', owner['domain'], '--inactive') == (p / 'DOMAIN_BEFORE.xml').read_text().strip()
    write('TEARDOWN.json', {'status': 'pass', 'domain_state': 'shut off', 'persistent_domain_definition_unchanged': True, 'guest': 'TEARDOWN_GUEST.json', 'retained_guests_preserved': True})
else:
    raise RuntimeError('Unknown native package phase: ' + mode)
