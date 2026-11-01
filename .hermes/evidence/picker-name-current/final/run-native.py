"""Bounded native preview/footer regression; own guest/root/actors and always request teardown."""
from pathlib import Path
import sys, os, json, subprocess, hashlib, tempfile, gzip, shutil, struct, time, signal, uuid, re
base = Path(__file__).resolve().parent
assert not sys.argv[1:], 'No arguments: complete native UI scope'
templates = Path.cwd() / '.hermes/evidence/preview-footer-layout-current'
p=base
p.mkdir(exist_ok=True)
root = Path.cwd()
sys.path.insert(0, str(Path.cwd() / '.hermes/evidence/windows-office-contract-current'))
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
    raise KeyboardInterrupt('native preview QA cancelled')
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
    owner = json.loads((templates / 'GUEST_OWNER.json').read_text())
    xml = subprocess.check_output(['virsh', '-c', 'qemu:///system', 'dumpxml', '--inactive', domain])
    assert hashlib.sha256(xml).hexdigest() == owner['definition_sha256']
    build_record = base.parent / 'windows-final2-build.jsonl'
    build_text = build_record.read_text() if build_record.exists() else gzip.decompress(build_record.with_name(build_record.name+'.gz').read_bytes()).decode()
    records = [json.loads(line) for line in build_text.splitlines() if line.startswith('{')]
    assert records[-1] == {'reason': 'build-finished', 'success': True}
    inputs = {r['target']['name']: Path(r['executable']) for r in records if r.get('reason') == 'compiler-artifact' and r.get('executable') and r['profile']['test']}
    assert 'ottrin_ui' in inputs
    assert (root / 'dist/windows-candidate/windows-payload/Ottrin-0.1.0-windows-x64/bin/rg.exe').is_file()
    state['source'] = {'base_revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
        'working_tree': subprocess.check_output(['git','status','--short'],text=True).splitlines(),
        'files': {name: hashlib.sha256((root / name).read_bytes()).hexdigest() if (root / name).is_file() else None
            for name in sorted(set(subprocess.check_output(['git', 'diff', '--name-only'], text=True).splitlines())
                | set(subprocess.check_output(['git','ls-files','--others','--exclude-standard','crates'],text=True).splitlines()) | {'crates/ottrin-ui/src/copy_activity_work.rs','crates/ottrin-core/src/picker.rs'}) if name.startswith('crates/')}}
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
    for crate, name in [('ottrin_ui', 'fixture-ui.exe')]:
        binary = inputs[crate]
        with binary.open('rb') as stream:
            stream.seek(0x3c); pe = struct.unpack('<I', stream.read(4))[0]
            stream.seek(pe + 8); epoch = struct.unpack('<I', stream.read(4))[0]
            stream.seek(0); original = hashlib.file_digest(stream, 'sha256').hexdigest()
        with tempfile.TemporaryDirectory(prefix='ui-miller-runtime-', dir=root / '.hermes/packaging-tmp') as directory:
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
    provider = root / 'dist/windows-candidate/windows-payload/Ottrin-0.1.0-windows-x64/bin/rg.exe'
    n.upload(provider, 'rg.exe')
    with provider.open('rb') as stream:
        provider_sha = hashlib.file_digest(stream, 'sha256').hexdigest()
    n.execute(r'''if((Get-FileHash 'C:\OttrinJobFixtureQA\rg.exe').Hash.ToLowerInvariant()-ne'HASH'){throw 'Provider changed'}'''.replace('HASH', provider_sha))
    for role, crate in [('ordinary-ui','ui'),('elevated-ui','ui')]:
        actor_role = 'elevated' if role.startswith('elevated') else 'ordinary'
        executable = 'fixture-' + crate + '.exe'
        text = (templates / (actor_role + '-actor.ps1')).read_text().replace('fixture-fixed.exe', executable)
        if crate != 'ui':
            text = '\n'.join(line for line in text.splitlines() if not line.lstrip().startswith("foreach($name in @('windows_move_history")) + '\n'
        inventory_command = r''' $inventory=Start-Process "$root\EXECUTABLE" -ArgumentList '--list' -WorkingDirectory $homeRoot -PassThru -RedirectStandardOutput "$root\results\$phase.list" -RedirectStandardError "$root\results\$phase-list.stderr";$null=$inventory.Handle;if(-not$inventory.WaitForExit(30000)){Stop-Process -Id $inventory.Id -Force;throw 'Inventory timed out'};if($inventory.ExitCode-ne0){throw "Test inventory failed $($inventory.ExitCode)"}'''
        text = text.replace(' $p=Start-Process ', inventory_command.replace('EXECUTABLE', executable) + '\n $p=Start-Process ')
        text = text.replace('WaitForExit(180000)', 'WaitForExit(300000)')
        controller = (templates / (actor_role + '-controller.ps1')).read_text()
        phase = 'short-temp-' + role + '-runtime'
        controller = controller.replace('short-temp-' + actor_role + '-fixed', phase)
        text=text.replace('$phase=(Get-Content "$root\\phase" -Raw).Trim()', '$phase=(Get-Content "$root\\phase" -Raw).Trim()\nStart-Transcript -Path "$root\\results\\$phase.transcript"|Out-Null')
        result = actor(role, text, controller)
        transcript=n.execute(r'$p="C:\OttrinJobFixtureQA\results\PHASE.transcript";if(Test-Path $p){Get-Content $p -Raw}'.replace('PHASE',phase))
        (p/(phase+'.transcript')).write_text(transcript['out-data'])
        assert result['exit']==0, result
        for suffix in ['.stdout', '.stderr', '.json', '-alias.json', '.list']:
            n.download('results/' + phase + suffix, p / (phase + suffix))
        assert result['exit'] == 0, result
        output = (p / (phase + '.stdout')).read_text()
        summary = re.search(r'test result: ok[.] (\d+) passed; 0 failed; (\d+) ignored; 0 measured; 0 filtered out;', output)
        assert summary, output[-3000:]
        inventory = (p / (phase + '.list')).read_text()
        count = len([line for line in inventory.splitlines() if line.endswith(': test')])
        assert int(summary[1]) + int(summary[2]) == count and count > 0
        required = {'ui': ['remote_index::tests::stalled_configuration_cannot_block_ui_snapshot_reads_or_new_settings','remote_index::tests::closing_the_ui_owner_cancels_active_queries_even_when_a_handle_is_retained','remote_index::tests::long_maintenance_does_not_hold_the_status_and_query_worker','remote_index::tests::ready_connections_have_no_periodic_idle_rpc','picker::tests::preview_keeps_its_column_when_grid_contents_are_sparse_or_empty','tests::index_service_changes_wait_for_startup_before_stopping_or_replacing_it','picker::tests::native_save_name_survives_refresh_and_navigation_without_a_selection','picker::tests::confirming_an_unedited_location_preserves_native_directory_units'],
            'app': ['tests::slow_index_startup_returns_control_before_the_daemon_is_ready','tests::index_startup_failure_is_a_ui_result_instead_of_a_process_exit','tests::index_startup_broker_is_headless_and_rejects_ambiguous_arguments']}[crate]
        for name in required: assert 'test ' + name + ' ... ok' in output, name
        assert not json.loads((p / (phase + '.json')).read_text())['uiRemainingEntries']
        print(role + ': complete ' + summary[1] + ' passed, ' + summary[2] + ' ignored, zero filtered; inventory matched', flush=True)
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
            controller = (templates / 'elevated-controller.ps1').read_text().replace('short-temp-elevated-fixed', 'owner-cleanup')
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
