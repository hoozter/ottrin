"""Read a small atomic progress receipt from the exact live owned guest."""
from pathlib import Path
import json,importlib.util,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent/'native-visible';owner=json.loads((e/'OWNER.json').read_text());assert subprocess.check_output(['virsh','-c','qemu:///system','domuuid',owner['domain']],text=True).strip()==owner['uuid']
preparation=e/'GUEST_PREPARATION.json'
if not preparation.exists():print(json.dumps({'host_stage':owner['status']}));raise SystemExit(0)
root=json.loads(preparation.read_text())['root'];spec=importlib.util.spec_from_file_location('guest',r/'.hermes/evidence/windows-20261011/guest_exec.py');g=importlib.util.module_from_spec(spec);spec.loader.exec_module(g);g.DOMAIN=owner['domain']
code="$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$p='"+root+"\\results\\RESULT.json';if(Test-Path $p){$s=[IO.File]::Open($p,'Open','Read',([IO.FileShare]::ReadWrite-bor[IO.FileShare]::Delete));$reader=[IO.StreamReader]::new($s);try{$v=$reader.ReadToEnd()|ConvertFrom-Json}finally{$reader.Dispose()};@{status=$v.status;phase=$v.phase;error=$v.error;cases=@($v.cases).Count;display_mode=$v.display_mode}|ConvertTo-Json -Compress}else{'not yet published'}"
raise SystemExit(g.execute(code,20))
