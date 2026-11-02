"""Replay the previously accepted native appearance input against this exact candidate."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,signal,subprocess
r=Path.cwd();parser=argparse.ArgumentParser();parser.add_argument('--case',choices=['dark100','light150'],required=True);case=parser.parse_args().case;e=Path(__file__).resolve().parent;prior=r/'.hermes/evidence/menu-space-delivery-current'/('appearance-'+case)/'RESULT.json';actions=json.loads(prior.read_text())['actions'];assert actions[-1]['op']=='finish'
spec=importlib.util.spec_from_file_location('workspace',r/'scripts/test-workspace.py');workspace=importlib.util.module_from_spec(spec);spec.loader.exec_module(workspace)
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,workspace.cancelled)
source=r/'.hermes/evidence/menu-space-delivery-current/appearance-session.py';commands=e/('appearance-'+case+'-input.jsonl');commands.write_text(''.join(json.dumps(a)+'\n' for a in actions));child=None
try:
 with commands.open('r') as stdin,(e/('APPEARANCE_'+case.upper()+'.log')).open('w') as log:
  child=subprocess.Popen(['python3',str(source),'--case',case,'--output',str(e)],stdin=stdin,stdout=log,stderr=log,start_new_session=True);code=child.wait(timeout=420);assert code==0,code
 (e/('APPEARANCE_'+case.upper()+'_INPUT.json')).write_text(json.dumps({'status':'executed; rendered review still required','prior_accepted_actions':str(prior.relative_to(r)),'input_sha256':hashlib.sha256(commands.read_bytes()).hexdigest(),'driver_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'count':len(actions)},indent=2)+'\n')
finally:
 if child:workspace.stop_tests(child)
