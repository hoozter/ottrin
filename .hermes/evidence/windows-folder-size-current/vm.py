from pathlib import Path
import subprocess,xml.etree.ElementTree as E,uuid,json,sys
r=Path(__file__).parent.resolve()
def run(*args):return subprocess.check_output(['virsh','-c','qemu:///system',*args],text=True).strip()
def create(name,backing):
 assert name.startswith('ottrin-init-')
 assert 'shut off'==run('domstate','ottrin-qa-20261011')
 if 'ottrin-msi-phase-compile.qcow2' in backing:assert 'shut off'==run('domstate','ottrin-msi-phase-compile')
 # Refuse an existing domain/volume; no automatic adoption.
 names=run('list','--all','--name').splitlines();assert name not in names
 volumes=[line.split()[0] for line in run('vol-list','default').splitlines()[2:] if line.strip()];assert name+'.qcow2' not in volumes
 print(run('vol-create-as','default',name+'.qcow2','64G','--format','qcow2','--backing-vol',backing,'--backing-vol-format','qcow2'),flush=True)
 tree=E.parse(r/'PARENT.xml');root=tree.getroot();root.find('name').text=name;root.find('uuid').text=str(uuid.uuid4())
 if root.find('genid') is not None:root.find('genid').text=str(uuid.uuid4())
 root.find('./os/nvram').text='/var/lib/libvirt/qemu/nvram/'+name+'_VARS.fd'
 disks=root.findall('./devices/disk');assert len(disks)==1;disks[0].find('source').set('file','/var/lib/libvirt/images/'+name+'.qcow2')
 for interface in root.findall('./devices/interface'):root.find('devices').remove(interface)
 path=r/(name+'.xml');tree.write(path,encoding='unicode');print(run('define',str(path)),flush=True);print(run('start',name),flush=True)
 (r/(name+'-OWNERSHIP.json')).write_text(json.dumps({'domain':name,'volume':name+'.qcow2','backing_volume':backing,'scope':'Task-owned disposable overlay; original QA parent remains shut off/read-only as backing','cleanup':'Stop domain, undefine with owned NVRAM/TPM, delete only this exact owned volume'},indent=2)+'\n')
if __name__=='__main__':create(sys.argv[1],sys.argv[2])
