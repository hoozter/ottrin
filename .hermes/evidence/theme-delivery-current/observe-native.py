"""Read a small atomic progress receipt from the exact live owned guest."""
from pathlib import Path
import argparse,json,importlib.util,subprocess
parser=argparse.ArgumentParser();parser.add_argument('--case',choices=['native-observed-controls','native-toggle-pointer','native-pointer','native','services','registration-trace','services-launch-observation','services-refresh-ambiguity','services-reader-sharing'],default='native');args=parser.parse_args();r=Path.cwd();e=Path(__file__).resolve().parent/args.case;owner=json.loads((e/'OWNER.json').read_text())
identity=subprocess.run(['virsh','-c','qemu:///system','domuuid',owner['domain']],capture_output=True,text=True)
if identity.returncode or identity.stdout.strip()!=owner['uuid']:
 assert owner.get('cleanup',{}).get('domain_removed'), 'Owned guest is unavailable or identity differs; inspect the producer handle before deciding its state'
 result=e/'GUEST_RESULT.json';value=json.loads(result.read_text()) if result.exists() else owner
 print(json.dumps({'terminal_clone_removed':True,'status':value.get('status'),'phase':value.get('phase'),'error':value.get('error'),'cases':len(value.get('cases',[])),'scope':value.get('scope')}));raise SystemExit(0)

preparation=e/'GUEST_PREPARATION.json'
if not preparation.exists():print(json.dumps({'host_stage':owner['status']}));raise SystemExit(0)
root=json.loads(preparation.read_text())['root'];spec=importlib.util.spec_from_file_location('guest',r/'.hermes/evidence/windows-20261011/guest_exec.py');g=importlib.util.module_from_spec(spec);spec.loader.exec_module(g);g.DOMAIN=owner['domain']
code="$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$p='"+root+"\\results\\RESULT.json';if(Test-Path $p){$s=[IO.File]::Open($p,'Open','Read',([IO.FileShare]::ReadWrite-bor[IO.FileShare]::Delete));$reader=[IO.StreamReader]::new($s);try{$v=$reader.ReadToEnd()|ConvertFrom-Json}finally{$reader.Dispose()};@{status=$v.status;phase=$v.phase;error=$v.error;cases=@($v.cases).Count;display_mode=$v.display_mode;preview_controls=@($v.preview_controls|Where-Object {$_}).Count;preview_target=$v.preview_target;pointer_target=$v.pointer_target;service_states=$(if($v.service_matrix){@($v.service_matrix.states).Count}else{0});index_refresh_selector=$v.index_refresh_selector;timings=$v.service_timings;pending=@($v.service_pending|Where-Object {$_}|ForEach-Object {@{pid=$_.pid;parent=$_.parent;seconds=$_.seconds;stage=$_.stage;expected=$_.expected;script_characters=$_.fixed_script.Length}})}|ConvertTo-Json -Depth 6 -Compress}else{'not yet published'}"
raise SystemExit(g.execute(code,20))
