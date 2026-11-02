"""Observe only the owned servicing clone through a separate private display."""
from pathlib import Path
import importlib.util,json,time,subprocess,os
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'console-probe';assert not out.exists();name='ottrin-msi-updated-client'
owner=json.loads((r/'.hermes/evidence/windows-msi-updated-client-current/OWNER.json').read_text());assert owner['domain']==name and owner['network_interfaces']==0
assert subprocess.check_output(['virsh','-c','qemu:///system','domuuid',name],text=True).strip()==owner['uuid']
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
with desktop.OwnedDesktop(out) as d:
 d.env.update(GDK_BACKEND='x11',XDG_SESSION_TYPE='x11')
 viewer=d.spawn(['virt-viewer','--connect','qemu:///system','--attach','--domain-name','--auto-resize=never','--spice-disable-audio','--spice-disable-usbredir',name],'viewer.log')
 time.sleep(10)
 assert viewer.poll() is None,(out/'viewer.log').read_text()
 desktop.run(['import','-window','root',str(out/'private-console.png')],d.env)
 report={'viewer_alive':True,'owned_domain':name,'uuid':owner['uuid'],'scope':'Read-only observation of owned servicing clone; no guest input or host desktop capture'}
 (out/'OBSERVATION.json').write_text(json.dumps(report,indent=2)+'\n')
