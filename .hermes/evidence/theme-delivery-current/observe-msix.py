"""Observe the exact owned live MSIX fixture without restarting its actor."""
from pathlib import Path
import argparse,importlib.util,json,subprocess
parser=argparse.ArgumentParser();parser.add_argument('case',choices=['runtime17-reader','runtime17-trace','runtime17-completion','runtime17-wake']);args=parser.parse_args()
e=Path(__file__).resolve().parent/args.case;o=json.loads((e/'OWNER.json').read_text())
identity=subprocess.run(['virsh','-c','qemu:///system','domuuid',o['domain']],text=True,capture_output=True)
if identity.returncode or identity.stdout.strip()!=o['uuid']:
 assert o.get('cleanup',{}).get('domain_removed'),'Missing fixture without terminal teardown receipt'
 v=json.loads((e/'RUNTIME_RESULT.json').read_text());print(json.dumps({'terminal':True,'status':v.get('status'),'error':v.get('error'),'actor':v.get('actor',{}).get('status')}));raise SystemExit(0)
s=importlib.util.spec_from_file_location('guest',Path.cwd()/'.hermes/evidence/windows-20261011/guest_exec.py');g=importlib.util.module_from_spec(s);s.loader.exec_module(g);g.DOMAIN=o['domain']
code=r"$ProgressPreference='SilentlyContinue';foreach($name in @('ACTOR','RESULT','LOGIN','MSIX_CHECKBOX_TRACE','STATE_READER_SMOKE')){$p='C:\OttrinOwnedMsixRuntime\results\'+$name+'.json';if(Test-Path $p){$s=[IO.File]::Open($p,'Open','Read',([IO.FileShare]::ReadWrite-bor[IO.FileShare]::Delete));$reader=[IO.StreamReader]::new($s);try{$v=$reader.ReadToEnd()|ConvertFrom-Json}finally{$reader.Dispose()};@{name=$name;status=$v.status;phase=$v.phase;error=$v.error;gui_error=$v.gui_error;checkbox=$v.msix_startup_checkbox;active_cli=$v.active_cli;cli_exits=$v.cli_exits;trace_stage=$v.stage;trace_fields=$v.fields;transient_milliseconds=$v.transient_milliseconds;persistent_lock_seconds=$v.persistent_lock_seconds;private_fixture_removed=$v.private_fixture_removed}|ConvertTo-Json -Depth 7 -Compress}}"
raise SystemExit(g.execute(code,20))
