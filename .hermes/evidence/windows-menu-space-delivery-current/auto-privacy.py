"""Drive only recognized native first-login pages for one owned live runtime."""
from pathlib import Path
import argparse,importlib.util,json,signal,subprocess,time
r=Path.cwd();parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True);args=parser.parse_args();e=args.output.resolve();assert e.is_relative_to(r/'.hermes/evidence')
spec=importlib.util.spec_from_file_location('workspace',r/'scripts/test-workspace.py');workspace=importlib.util.module_from_spec(spec);spec.loader.exec_module(workspace)
for sig in [signal.SIGINT,signal.SIGTERM]:signal.signal(sig,workspace.cancelled)
child=None
try:
 deadline=time.monotonic()+180
 while True:
  p=e/'OWNER.json'
  if p.exists():
   a=json.loads(p.read_text())
   if 'cleanup' in a:raise RuntimeError('Native consumer already terminal')
   if 'private_console' in a:
    assert subprocess.check_output(['virsh','-c','qemu:///system','domuuid',a['domain']],text=True).strip()==a['uuid'];break
  assert time.monotonic()<deadline,'Owned private console did not become available';time.sleep(.5)
 child=subprocess.Popen(['python3',str(r/'.hermes/evidence/windows-startup-lifetime-current/privacy-control.py'),str(e),'finish-msix'],start_new_session=True)
 code=child.wait(timeout=360);assert code==0,code
 (e/'AUTO_PRIVACY.json').write_text(json.dumps({'status':'pass','domain_uuid':a['uuid'],'scope':'Recognized normal first-login UI, then existing observed-desktop SID/nonce guard; no security/OOBE policy changes'},indent=2)+'\n')
finally:
 if child:workspace.stop_tests(child)
