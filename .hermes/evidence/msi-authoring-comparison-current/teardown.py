from pathlib import Path
import json,sys,subprocess,time,hashlib,shutil
p=Path(__file__).resolve().parent;owner=json.loads((p/'OWNERSHIP.json').read_text());assert owner['domain']=='ottrin-copy-cleanup-qa' and owner['initial_state']=='shut off'
sys.path.insert(0,str(p.parent/'windows-copy-cleanup-current'));import transport as n
n.GROOT=owner['guest_root']
script=r'''$r='C:\OttrinCopyCleanupQA\msi-authoring-compare';if((Get-Item $r).Attributes-band[IO.FileAttributes]::ReparsePoint){throw 'Comparison root is redirected'}
if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like"$r\*"}).Count){throw 'Unexpected sample process exists'}
$inputs=Get-Content "$r\INPUTS.json" -Raw|ConvertFrom-Json
$expected=@('INPUTS.json','BASELINE.json','TABLES.json');foreach($item in $inputs.PSObject.Properties){$name=$item.Name+'.msi';$expected+=,$name;if((Get-FileHash (Join-Path $r $name)).Hash.ToLowerInvariant()-ne$item.Value.msi_sha256){throw 'Read-only MSI input changed'}}
$actual=@(Get-ChildItem -LiteralPath $r -Force);if($actual.Count-ne$expected.Count-or@($actual|Where-Object{$_.PSIsContainer-or$_.Name-notin$expected}).Count){throw 'Unknown comparison content retained'}
$b=Get-Content "$r\BASELINE.json" -Raw|ConvertFrom-Json;foreach($item in $b.PSObject.Properties){$hash=$(if(Test-Path -LiteralPath $item.Name){(Get-FileHash -LiteralPath $item.Name).Hash}else{$null});if($hash-ne$item.Value){throw 'Original profile or uninstaller changed'}}
$service=(Get-Service msiserver).Status.ToString();Remove-Item -LiteralPath $r -Recurse -Force
@{status='pass';inputs_byte_identical_after_read=$true;owned_comparison_directory_removed=$true;original_preferences_and_uninstaller_hashes_unchanged=$true;sample_executable_processes=0;msi_service_state=$service}|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\MsiComparisonTeardown.json
'''
pid=n.start(script);(p/'teardown.pid').write_text(str(pid));n.wait(pid,45)
n.GROOT=r'C:\OttrinCopyCleanupQA';n.download('MsiComparisonTeardown.json',p/'TEARDOWN_GUEST.json');n.wait(n.start(r'Remove-Item C:\OttrinCopyCleanupQA\MsiComparisonTeardown.json -Force'),45)
run=lambda *args:subprocess.check_output(['virsh',*args],text=True).strip()
run('shutdown',owner['domain'],'--mode','agent');end=time.monotonic()+90
while run('domstate',owner['domain'])!='shut off':
 assert time.monotonic()<end,'Guest shutdown pending; inspect same domain';time.sleep(1)
assert run('dumpxml',owner['domain'],'--inactive')==(p/'DOMAIN_BEFORE.xml').read_text().strip()
inputs=json.loads((p/'INPUTS.json').read_text());host=p/'inputs'
assert {f.name for f in host.iterdir()}=={'advancedinstaller.msi','wix.msi'}
assert all(hashlib.sha256((host/(name+'.msi')).read_bytes()).hexdigest()==inputs[name]['msi_sha256'] for name in ['advancedinstaller','wix'])
shutil.rmtree(host)
(p/'TEARDOWN.json').write_text(json.dumps({'status':'pass','guest':'TEARDOWN_GUEST.json','domain_state':'shut off','persistent_domain_definition_unchanged':True,'host_downloaded_sample_inputs_removed':True,'current_Ottrin_deliverables_preserved':True},indent=2)+'\n')
