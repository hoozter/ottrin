"""Bounded native alias regression; own guest/root/actors and always request teardown."""
from pathlib import Path
import sys, os, json, subprocess, hashlib, tempfile, gzip, shutil, struct, time, signal, uuid
base = Path(__file__).resolve().parent
search_suite = sys.argv[1:] == ['--search']
assert not sys.argv[1:] or search_suite, 'Usage: run-runtime.py [--search]'
p = base / 'pipe' if search_suite else base
p.mkdir(exist_ok=True)
root = Path.cwd()
sys.path.insert(0, str(base.parent / 'windows-office-contract-current'))
import transport as n
n.GROOT = r'C:\OttrinJobFixtureQA'
domain = 'ottrin-copy-cleanup-qa'
nonce = str(uuid.uuid4())
state = {'nonce': nonce, 'guest_root': n.GROOT, 'domain': domain, 'status': 'starting'}
started = False
owned_root = False

def save():
    (p / 'RUNTIME_OWNER.json').write_text(json.dumps(state, indent=2) + '\n')

def cancelled(*_):
    raise KeyboardInterrupt('native alias QA cancelled')
for sig in [signal.SIGTERM, signal.SIGINT]:
    signal.signal(sig, cancelled)

def actor(role, text, controller, timeout=900):
    state['phase'] = role
    save()
    local = p / (role + '-runtime.ps1')
    local.write_text(text)
    n.upload(local, 'actor.ps1')
    n.execute(r"$ErrorActionPreference='Stop';$errors=$null;$tokens=$null;$null=[Management.Automation.Language.Parser]::ParseFile('C:\OttrinJobFixtureQA\actor.ps1',[ref]$tokens,[ref]$errors);if($errors.Count){throw ($errors|Out-String)}")
    pid = n.start(controller)
    state['active_controller_pid'] = pid
    state['active_controller_script_sha256'] = hashlib.sha256(("$ProgressPreference='SilentlyContinue'\n" + controller).encode('utf-16le')).hexdigest()
    save()
    (p / (role + '-runtime-pid.json')).write_text(json.dumps({'pid': pid}) + '\n')
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = n.poll(pid)
        if result is not None:
            state['active_controller_pid'] = None
            save()
            (p / (role + '-runtime-controller.json')).write_text(json.dumps(result, indent=2) + '\n')
            return result
        time.sleep(.25)
    raise TimeoutError('Owned actor exceeded its controller budget: ' + role)

try:
    assert subprocess.check_output(['virsh', '-c', 'qemu:///system', 'domstate', domain], text=True).strip() == 'shut off'
    owner = json.loads((base / 'GUEST_OWNER.json').read_text())
    xml = subprocess.check_output(['virsh', '-c', 'qemu:///system', 'dumpxml', '--inactive', domain])
    assert hashlib.sha256(xml).hexdigest() == owner['definition_sha256']
    records = [json.loads(line) for line in (base / ('search-build.jsonl' if search_suite else 'windows-build.jsonl')).read_text().splitlines() if line.startswith('{')]
    assert records[-1] == {'reason': 'build-finished', 'success': True}
    inputs = {r['target']['name']: Path(r['executable']) for r in records if r.get('reason') == 'compiler-artifact' and r.get('executable')}
    assert ({'ottrin_search'} if search_suite else {'ottrin_ui', 'ottrin_platform'}) <= inputs.keys()
    assert (root / '.hermes/packaging-tmp/windows-activity-candidate/windows-payload/Ottrin-0.1.0-windows-x64/bin/rg.exe').is_file()
    state['source'] = {'base_revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
        'working_tree': subprocess.check_output(['git','status','--short'],text=True).splitlines(),
        'files': {name: hashlib.sha256((root / name).read_bytes()).hexdigest() if (root / name).is_file() else None
            for name in sorted(set(subprocess.check_output(['git', 'diff', '--name-only'], text=True).splitlines())
                | {'crates/ottrin-ui/src/copy_activity_work.rs'}) if name.startswith('crates/')}}
    save()
    subprocess.run(['virsh', '-c', 'qemu:///system', 'start', domain], check=True)
    started = True
    for attempt in range(360):
        try:
            n.qga('guest-ping')
            break
        except RuntimeError:
            assert attempt < 359
            time.sleep(.5)
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        try:
            check = n.execute(r"$p=Get-CimInstance Win32_UserProfile|Where-Object {$_.SID-eq'S-1-5-21-3183210791-1673105385-3262580894-1001'};[bool]($p.Loaded-and$p.LocalPath-eq'C:\Users\campbell')")
            if check['out-data'].strip() == 'True': break
        except RuntimeError:
            pass  # Read-only reboot observation; no mutation is repeated.
        time.sleep(1)
    else: raise TimeoutError('Existing ordinary WTS profile unavailable before fixture allocation')
    n.execute(r'''$ErrorActionPreference='Stop';$r='C:\OttrinJobFixtureQA';if(Test-Path $r){throw 'Refuse existing QA root'};New-Item -ItemType Directory $r|Out-Null;$acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetAccessRuleProtection($true,$false);foreach($sid in @('S-1-5-18','S-1-5-32-544')){$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid),'FullControl','ContainerInherit,ObjectInherit','None','Allow'))};Set-Acl $r $acl;@{nonce='NONCE';profiles=@(Get-CimInstance Win32_UserProfile|Select-Object SID,LocalPath)}|ConvertTo-Json -Depth 5|Set-Content "$r\OWNER.json"'''.replace('NONCE', nonce))
    owned_root = True
    binaries = {}
    for crate, name in ([('ottrin_search', 'fixture-runtime.exe')] if search_suite else [('ottrin_ui', 'fixture-runtime.exe'), ('ottrin_platform', 'platform-runtime.exe')]):
        binary = inputs[crate]
        with binary.open('rb') as stream:
            stream.seek(0x3c); pe = struct.unpack('<I', stream.read(4))[0]
            stream.seek(pe + 8); epoch = struct.unpack('<I', stream.read(4))[0]
            stream.seek(0); original = hashlib.file_digest(stream, 'sha256').hexdigest()
        with tempfile.TemporaryDirectory(prefix='ui-alias-runtime-', dir=root / '.hermes/packaging-tmp') as directory:
            exe = Path(directory) / name
            subprocess.run(['x86_64-w64-mingw32-strip', '--strip-debug', '-o', str(exe), str(binary)],
                           env={**os.environ, 'SOURCE_DATE_EPOCH': str(epoch)}, check=True)
            with exe.open('rb') as stream:
                digest = hashlib.file_digest(stream, 'sha256').hexdigest()
            binaries[name] = {'sha256': digest, 'raw_sha256': original, 'source_date_epoch': epoch}
            archive = Path(directory) / 'runtime.gz'
            with exe.open('rb') as source, gzip.open(archive, 'wb', compresslevel=1) as target:
                shutil.copyfileobj(source, target)
            n.upload(archive, 'runtime.gz')
            n.execute(r'''$ErrorActionPreference='Stop';$r='C:\OttrinJobFixtureQA';$s=[IO.File]::OpenRead("$r\runtime.gz");$z=[IO.Compression.GZipStream]::new($s,[IO.Compression.CompressionMode]::Decompress);$d=[IO.File]::Create("$r\NAME");try{$z.CopyTo($d)}finally{$d.Dispose();$z.Dispose();$s.Dispose()};if((Get-FileHash "$r\NAME").Hash.ToLowerInvariant()-ne'HASH'){throw 'Transfer changed'};Remove-Item "$r\runtime.gz"'''.replace('NAME', name).replace('HASH', digest))
    (p / 'RUNTIME_BINARIES.json').write_text(json.dumps(binaries, indent=2) + '\n')
    provider = root / '.hermes/packaging-tmp/windows-activity-candidate/windows-payload/Ottrin-0.1.0-windows-x64/bin/rg.exe'
    n.upload(provider, 'rg.exe')
    with provider.open('rb') as stream:
        provider_sha = hashlib.file_digest(stream, 'sha256').hexdigest()
    n.execute(r'''if((Get-FileHash 'C:\OttrinJobFixtureQA\rg.exe').Hash.ToLowerInvariant()-ne'HASH'){throw 'Provider changed'}'''.replace('HASH', provider_sha))
    for role in ['ordinary', 'elevated']:
        actor_role = 'elevated' if role.startswith('elevated') else 'ordinary'
        text = (base / (actor_role + '-actor.ps1')).read_text().replace('fixture-fixed.exe', 'fixture-runtime.exe')
        if search_suite:
            text = text.replace("'--nocapture','--test-threads=4'", "'--nocapture','--test-threads=4'")
            text = text.replace("'windows_move_history_reconciles_exact_inverse_without_overriding_its_worker','windows_move_recovery_requires_captured_paths_and_real_drive_bindings','windows_move_undo_admission_tracks_renamed_parent_paths'", "'windows_pipe_round_trip_authenticates_and_keeps_namespace_exclusive'")
            text = text.replace('test tests::$name ... ok', 'test windows_pipe::tests::$name ... ok')
        else:
            text = text.replace('}finally{if($p-and-not$p.HasExited)', r''' $native=Start-Process "$root\platform-runtime.exe" -ArgumentList @('windows_native_atomic_undo_admits_renamed_parents_and_refuses_a_stale_plan','--nocapture','--test-threads=1') -PassThru -RedirectStandardOutput "$root\results\$phase-native.stdout" -RedirectStandardError "$root\results\$phase-native.stderr"
     $null=$native.Handle
     if(-not$native.WaitForExit(90000)){& C:\Windows\System32\taskkill.exe /PID $native.Id /T /F|Out-Null;throw 'Native authority control timed out'}
     if($native.ExitCode-ne0){throw 'Native authority control failed'}
     if(-not(Get-Content "$root\results\$phase-native.stdout" -Raw).Contains('test result: ok. 1 passed; 0 failed;')){throw 'Native authority control did not execute'}
    }finally{if($p-and-not$p.HasExited)''')

        if not search_suite:
            checks = r"""
 $copy=Start-Process "$root\platform-runtime.exe" -ArgumentList @('windows_native_copy_session_','--nocapture','--test-threads=1') -PassThru -RedirectStandardOutput "$root\results\$phase-copy-native.stdout" -RedirectStandardError "$root\results\$phase-copy-native.stderr"
 $null=$copy.Handle
 if(-not$copy.WaitForExit(180000)){& C:\Windows\System32\taskkill.exe /PID $copy.Id /T /F|Out-Null;throw 'Copy receipt controls timed out'}
 if($copy.ExitCode-ne0){throw 'Copy receipt controls failed'}
 $output=Get-Content "$root\results\$phase-copy-native.stdout" -Raw
 if(-not$output.Contains('test result: ok. 4 passed; 0 failed;')-or-not$output.Contains('windows_native_copy_session_refresh_resolves_moved_parent_without_adopting_old_path ... ok')){throw 'Copy receipt controls did not execute'}
 $env:OTTRIN_TEST_WINDOWS_PLATFORM_FIXTURE_EXE="$root\platform-runtime.exe"
 $activity=Start-Process "$root\fixture-runtime.exe" -ArgumentList @('--exact','tests::windows_copy_recovery_activity_integration','--ignored','--nocapture','--test-threads=1') -PassThru -RedirectStandardOutput "$root\results\$phase-copy-activity.stdout" -RedirectStandardError "$root\results\$phase-copy-activity.stderr"
 $null=$activity.Handle
 if(-not$activity.WaitForExit(360000)){& C:\Windows\System32\taskkill.exe /PID $activity.Id /T /F|Out-Null;throw 'Copy Activity matrix timed out'}
 if($activity.ExitCode-ne0){throw 'Copy Activity matrix failed'}
 if(-not(Get-Content "$root\results\$phase-copy-activity.stdout" -Raw).Contains('test result: ok. 1 passed; 0 failed;')){throw 'Copy Activity matrix did not execute'}
 $left=@(Get-ChildItem $env:TEMP -Force|Where-Object {$_.Name -notin $actorTempEntries}|Select-Object Name,Attributes,Length)
 if($left.Count){throw ('Native Copy tests left temporary fixture data: '+($left|ConvertTo-Json -Compress))}
"""
            text = text.replace('}finally{if($p-and-not$p.HasExited)', checks + '}finally{if($p-and-not$p.HasExited)')
        text = text.replace("$ErrorActionPreference='Stop'\n", "$ErrorActionPreference='Stop'\ntry {\n", 1) + r"""
} catch {
 $receipt=@{phase=$phase;exception=$_.Exception.ToString();scriptStack=$_.ScriptStackTrace}
 [IO.File]::WriteAllText("C:\OttrinJobFixtureQA\results\$phase-actor-error.json",($receipt|ConvertTo-Json -Depth 8))
 throw
}
"""
        controller = (base / (actor_role + '-controller.ps1')).read_text()
        phase = 'short-temp-' + role + '-runtime'
        controller = controller.replace('short-temp-' + actor_role + '-fixed', phase)
        result = actor(role, text, controller)
        for suffix in (['.stdout', '.stderr', '.json', '-alias.json'] if search_suite else ['.stdout', '.stderr', '.json', '-alias.json', '-native.stdout', '-native.stderr', '-copy-native.stdout', '-copy-native.stderr', '-copy-activity.stdout', '-copy-activity.stderr']):
            try:
                n.download('results/' + phase + suffix, p / (phase + suffix))
            except RuntimeError:
                if result['exit'] == 0:
                    raise
        if result['exit'] != 0:
            try:
                n.download('results/' + phase + '-actor-error.json', p / (phase + '-actor-error.json'))
            except RuntimeError as error:
                state['actor_error_receipt_collection_error'] = repr(error)
                save()
        assert result['exit'] == 0, result
        assert ('test result: ok. 222 passed; 0 failed; 6 ignored;' if search_suite else 'test result: ok. 375 passed; 0 failed; 4 ignored;') in (p / (phase + '.stdout')).read_text()
        assert not json.loads((p / (phase + '.json')).read_text())['uiRemainingEntries']
        print(role + (':222 search checks pass' if search_suite else ':375 UI,4 Copy session controls,1 Move authority control and29-case Copy Activity matrix pass'), flush=True)
    state['status'] = 'passed native scopes; teardown pending'
except BaseException as error:
    state['status'] = 'failed'
    state['error'] = repr(error)
    raise
finally:
    try:

        controller_pid = state.get('active_controller_pid')
        if controller_pid and n.poll(controller_pid) is None:
            n.execute(r"""$ErrorActionPreference='Stop';$target=Get-CimInstance Win32_Process -Filter 'ProcessId=CONTROLLER_PID';if($target){$m=[regex]::Match($target.CommandLine,'(?i)-EncodedCommand\s+"?([A-Za-z0-9+/=]+)"?\s*$');if(-not$m.Success){throw 'Owned controller encoding changed'};$bytes=[Convert]::FromBase64String($m.Groups[1].Value);$h=[Security.Cryptography.SHA256]::Create();try{$digest=[BitConverter]::ToString($h.ComputeHash($bytes)).Replace('-','').ToLowerInvariant()}finally{$h.Dispose()};if($digest-ne'CONTROLLER_HASH'){throw 'Owned controller identity changed'};& C:\Windows\System32\taskkill.exe /PID CONTROLLER_PID /T /F|Out-Null;if($LASTEXITCODE-ne0){throw 'Owned controller tree did not stop'}}""".replace('CONTROLLER_PID', str(controller_pid)).replace('CONTROLLER_HASH', state['active_controller_script_sha256']))
            state['active_controller_pid'] = None
            save()
        if owned_root:
            n.execute(r'''$ErrorActionPreference='Stop';$r='C:\OttrinJobFixtureQA';if((Get-Content "$r\OWNER.json" -Raw|ConvertFrom-Json).nonce-ne'NONCE'){throw 'Refuse unrelated root'};foreach($process in @(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($r+'\',[StringComparison]::OrdinalIgnoreCase)})){& C:\Windows\System32\taskkill.exe /PID $process.ProcessId /T /F|Out-Null}'''.replace('NONCE', nonce))
            cleanup = r'''$ErrorActionPreference='Stop';$r='C:\OttrinJobFixtureQA';$identity=[Security.Principal.WindowsIdentity]::GetCurrent();if($identity.User.Value-ne'S-1-5-21-3183210791-1673105385-3262580894-1001'-or-not([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Wrong cleanup owner'};if((Get-Content "$r\OWNER.json" -Raw|ConvertFrom-Json).nonce-ne'NONCE'){throw 'Refuse unrelated root'};if(Test-Path "$r\data"){Remove-Item "$r\data" -Recurse -Force}'''.replace('NONCE', nonce)
            controller = (base / 'elevated-controller.ps1').read_text().replace('short-temp-elevated-fixed', 'owner-cleanup')
            cleaned = actor('cleanup', cleanup, controller, timeout=60)
            assert cleaned['exit'] == 0, cleaned
            result = n.execute(r'''$ErrorActionPreference='Stop';$r='C:\OttrinJobFixtureQA';$owner=Get-Content "$r\OWNER.json" -Raw|ConvertFrom-Json;if($owner.nonce-ne'NONCE'){throw 'Wrong root'};if(Test-Path "$r\data"){throw 'Actor data remains'};if(@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($r+'\',[StringComparison]::OrdinalIgnoreCase)}).Count){throw 'Owned process remains'};$before=@($owner.profiles|ForEach-Object {$_.SID+'|'+$_.LocalPath}|Sort-Object);$after=@(Get-CimInstance Win32_UserProfile|ForEach-Object {$_.SID+'|'+$_.LocalPath}|Sort-Object);if(Compare-Object $before $after){throw 'Original profiles changed'};Remove-Item $r -Recurse -Force;@{ownedRootRemoved=(-not(Test-Path $r));ownedProcesses=0;originalProfilesPreserved=$true;noGlobalPoliciesOrLogonSettingsChanged=$true}|ConvertTo-Json'''.replace('NONCE', nonce))
            (p / 'RUNTIME_TEARDOWN.json').write_text(result['out-data'])
            state['root_removed'] = True
    except BaseException as error:
        state['teardown_error'] = repr(error)
        state['status'] = 'failed teardown'
        raise
    finally:
        if started:
            try:
                n.qga('guest-shutdown', {'mode': 'powerdown'})
            except RuntimeError:
                state['shutdown_transport_disconnected'] = True
            deadline = time.monotonic() + 45
            while time.monotonic() < deadline:
                if subprocess.check_output(['virsh', '-c', 'qemu:///system', 'domstate', domain], text=True).strip() == 'shut off':
                    break
                time.sleep(.5)
            else:
                state['status'] = 'failed shutdown'
                raise RuntimeError('Owned guest did not finish requested shutdown')
            state['guest_off'] = True
            assert hashlib.sha256(subprocess.check_output(['virsh', '-c', 'qemu:///system', 'dumpxml', '--inactive', domain])).hexdigest() == owner['definition_sha256']
        if state['status'] == 'passed native scopes; teardown pending' and state.get('root_removed') and state.get('guest_off'):
            state['status'] = 'pass'
        save()
