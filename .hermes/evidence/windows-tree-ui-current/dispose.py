"""Dispose only this run's owned child VM after terminal actor and data checks."""
from pathlib import Path
import json, subprocess, sys, time, xml.etree.ElementTree as ET

root = Path(__file__).resolve().parent
sys.path.insert(0, str(root))
import transport as native

owner = json.loads((root / 'ottrin-init-tree-ui-OWNERSHIP.json').read_text())
assert owner['domain'] == 'ottrin-init-tree-ui'
assert owner['volume'] == owner['domain'] + '.qcow2'

def virsh(*args):
    return subprocess.check_output(['virsh', '-c', 'qemu:///system', *args], text=True).strip()

domain = ET.fromstring(virsh('dumpxml', owner['domain']))
assert domain.findtext('./os/nvram') == '/var/lib/libvirt/qemu/nvram/' + owner['domain'] + '_VARS.fd'
disks = domain.findall('./devices/disk')
assert len(disks) == 1
assert disks[0].find('source').get('file') == '/var/lib/libvirt/images/' + owner['volume']
volume = ET.fromstring(virsh('vol-dumpxml', '--pool', 'default', owner['volume']))
assert volume.findtext('./backingStore/path') == owner['backing_volume']
assert virsh('domstate', 'ottrin-qa-20261011') == 'shut off'
assert native.execute(r'''
$t=Get-ScheduledTask 'Ottrin Tree UI QA'
if($t.State-eq'Running'){throw 'Actor is not terminal'}
$baseline=Get-Content C:\OttrinTreeUIQA\baseline.json -Raw|ConvertFrom-Json
if((Get-FileHash C:\Users\campbell\AppData\Roaming\ottrin\config.json).Hash-ne$baseline.preference_hash){throw 'Original preferences changed'}
if((Get-FileHash C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe).Hash-ne$baseline.prior_uninstaller_hash){throw 'Original installation changed'}
$own=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-like'C:\OttrinTreeUIQA\*'-or$_.ExecutablePath-like'C:\Users\OttrinTreeUIQA\AppData\Local\Programs\Ottrin\*'})
if($own.Count){throw 'Owned product/test processes remain'}
@{original_profile_and_installation='unchanged';owned_product_processes=0;actor_state=$t.State.ToString();actor_result=(Get-ScheduledTaskInfo $t.TaskName).LastTaskResult}|ConvertTo-Json|Set-Content C:\OttrinTreeUIQA\results\FINAL_GUEST.json
''') == 0
native.download('results\\FINAL_GUEST.json', root / 'FINAL_GUEST.json')
assert native.execute(r"Start-Process shutdown.exe -ArgumentList '/s /t 2 /f'") == 0
deadline = time.monotonic() + 55
while virsh('domstate', owner['domain']) != 'shut off':
    if time.monotonic() > deadline:
        raise TimeoutError('Normal child shutdown pending; do not destroy or delete live disk')
    time.sleep(1)
print(virsh('undefine', owner['domain'], '--nvram', '--tpm'), flush=True)
print(virsh('vol-delete', '--pool', 'default', owner['volume']), flush=True)
assert owner['domain'] not in virsh('list', '--all', '--name').splitlines()
assert owner['volume'] not in [line.split()[0] for line in virsh('vol-list', 'default').splitlines()[2:] if line.strip()]
states = {name: virsh('domstate', name) for name in ['ottrin-qa-20261011', 'ottrin-index-qa', 'win11ltsc']}
assert all(state == 'shut off' for state in states.values())
(root / 'FINAL_CLEANUP.json').write_text(json.dumps({'status': 'pass', 'owned_domain_volume_nvram_tpm': 'removed after normal shutdown', 'backing_image': 'preserved', 'original_domains': states}, indent=2) + '\n')
