"""Bounded minimal MSI controls in one owned throwaway VM overlay."""
from pathlib import Path
import base64, gzip, hashlib, importlib.util, json, os, secrets, signal, subprocess, time, uuid
import xml.etree.ElementTree as ET
ROOT = Path.cwd()
OUT = Path(__file__).resolve().parent
DOMAIN = 'ottrin-msi-base-control-qa'
VOLUME = DOMAIN + '.qcow2'
GROOT = r'C:\OttrinMsiControlQA'
created_volume = False
created_domain = False
report = {'status': 'running', 'purpose': 'Unrelated minimal per-user MSI engine controls; plain and ARP-icon variants; no Ottrin runtime/controller'}
def virsh(*args):
    return subprocess.check_output(['virsh', '-c', 'qemu:///system', *args], text=True, timeout=60).strip()
def volume_names():
    rows = virsh('vol-list', 'default').splitlines()[2:]
    names = []
    for row in rows:
        if not row.strip(): continue
        parts = row.split()
        assert len(parts) == 2, 'Ambiguous pool volume name; inspect before mutation'
        names.append(parts[0])
    return names
def save():
    (OUT / 'CONTROL.json').write_text(json.dumps(report, indent=2) + '\n')
def cancelled(*_):
    raise KeyboardInterrupt('Owned MSI control cancelled')
for sig in [signal.SIGINT, signal.SIGTERM]: signal.signal(sig, cancelled)
spec = importlib.util.spec_from_file_location('transport', ROOT / '.hermes/evidence/windows-office-contract-current/transport.py')
n = importlib.util.module_from_spec(spec); spec.loader.exec_module(n)
n.guest.DOMAIN = DOMAIN
n.GROOT = GROOT

def run_ps(script, timeout=180):
    pid = n.start("$ErrorActionPreference='Stop'\n" + script)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = n.poll(pid)
        if result is not None:
            assert result['exit'] == 0, result
            return result['out-data']
        time.sleep(.5)
    raise TimeoutError('Owned guest process remains live: ' + str(pid))
def ready():
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        try:
            n.qga('guest-ping')
            return
        except RuntimeError: time.sleep(1)
    raise TimeoutError('Owned VM QGA readiness timeout')
def collect():
    raw = run_ps(r"Get-ChildItem 'C:\OttrinMsiControlQA\results' -File | Select-Object -ExpandProperty Name | ConvertTo-Json")
    files = json.loads(raw)
    if isinstance(files, str): files = [files]
    for name in files:
        assert Path(name).name == name and name not in ['', '.', '..']
        path = OUT / ('native-' + name)
        n.download('results/' + name, path)
        if path.suffix == '.log':
            path.with_suffix('.log.gz').write_bytes(gzip.compress(path.read_bytes(), mtime=0)); path.unlink()
    report['native'] = json.loads((OUT / 'native-RESULT.json').read_text(encoding='utf-8-sig'))

def retire():
    cleanup = {}
    if created_domain:
        if virsh('domstate', DOMAIN) != 'shut off': cleanup['stop'] = virsh('destroy', DOMAIN)
        assert not virsh('snapshot-list', DOMAIN, '--name'), 'Owned domain acquired snapshots; inspect before retirement'
        cleanup['undefine'] = virsh('undefine', DOMAIN, '--nvram', '--tpm')
    if created_volume:
        for domain in virsh('list', '--all', '--name').splitlines():
            for line in virsh('domblklist', domain, '--details').splitlines()[2:]:
                assert '/var/lib/libvirt/images/' + VOLUME not in line, 'Another domain uses owned overlay'
        for name in volume_names():
            if name == VOLUME: continue
            xml = ET.fromstring(virsh('vol-dumpxml', '--pool', 'default', name))
            for path in xml.findall('.//backingStore/path'):
                assert Path(path.text).name != VOLUME, 'A dependent backing volume exists'
        cleanup['delete_volume'] = virsh('vol-delete', '--pool', 'default', VOLUME)
    parent_xml = virsh('dumpxml', '--inactive', 'win11ltsc') + '\n'
    cleanup['parent_xml_unchanged'] = parent_xml.strip() == (OUT / 'PARENT.xml').read_text().strip()
    cleanup['retained_guests_off'] = all(virsh('domstate', d) == 'shut off' for d in ['ottrin-qa-20261011', 'ottrin-copy-cleanup-qa', 'ottrin-index-qa', 'win11ltsc'])
    assert cleanup['parent_xml_unchanged'] and cleanup['retained_guests_off']
    report['cleanup'] = cleanup
    owner = json.loads((OUT / 'OWNERSHIP.json').read_text()); owner['pending_consumers'] = []; owner['cleanup_complete'] = True
    (OUT / 'OWNERSHIP.json').write_text(json.dumps(owner, indent=2) + '\n')
try:
    assert DOMAIN not in virsh('list', '--all', '--name').splitlines()
    assert VOLUME not in volume_names()
    assert all(virsh('domstate', d) == 'shut off' for d in ['ottrin-qa-20261011', 'win11ltsc', 'ottrin-copy-cleanup-qa', 'ottrin-index-qa'])
    report['volume_create'] = virsh('vol-create-as', 'default', VOLUME, '64G', '--format', 'qcow2', '--backing-vol', 'win11ltsc.qcow2', '--backing-vol-format', 'qcow2'); created_volume = True; save()
    tree = ET.parse(OUT / 'PARENT.xml'); root = tree.getroot(); root.find('name').text = DOMAIN; root.find('uuid').text = str(uuid.uuid4())
    if root.find('genid') is not None: root.find('genid').text = str(uuid.uuid4())
    root.find('./os/nvram').text = '/var/lib/libvirt/qemu/nvram/' + DOMAIN + '_VARS.fd'
    root.find('memory').text = root.find('currentMemory').text = '4194304'
    for disk in root.findall('./devices/disk'):
        if disk.get('device') != 'disk': root.find('devices').remove(disk)
    disks = root.findall('./devices/disk'); assert len(disks) == 1
    disks[0].find('source').set('file', '/var/lib/libvirt/images/' + VOLUME)
    for node in root.findall('./devices/interface'): root.find('devices').remove(node)
    xml_path = OUT / 'DOMAIN.xml'; tree.write(xml_path, encoding='unicode')
    report['define'] = virsh('define', str(xml_path)); created_domain = True; save()
    report['start'] = virsh('start', DOMAIN); save(); ready()
    run_ps(r"if(Test-Path 'C:\OttrinMsiControlQA'){throw 'Owned guest root exists'};New-Item -ItemType Directory 'C:\OttrinMsiControlQA'|Out-Null")
    pins = json.loads((ROOT / '.hermes/evidence/windows-20261011/windows-msi-compiler-downloads.json').read_text()); cache = ROOT / '.hermes/tools/windows-msi-native'
    assert hashlib.sha512((cache / 'runtime.zip').read_bytes()).hexdigest() == pins['runtime_sha512']
    assert hashlib.sha256((cache / 'wix.nupkg').read_bytes()).hexdigest() == pins['wix_sha256']
    for source, name in [(cache/'runtime.zip','runtime.zip'),(cache/'wix.nupkg','wix.zip'),(ROOT/'assets/icon.ico','icon.ico'),(OUT/'control.txt','control.txt'),(OUT/'plain.wxs','plain.wxs'),(OUT/'icon.wxs','icon.wxs'),(OUT/'actor.ps1','actor.ps1')]: n.upload(source, name)
    report['compiler_output'] = run_ps(r'''
$b='C:\OttrinMsiControlQA';Set-Location $b
Expand-Archive "$b\runtime.zip" "$b\dotnet";Expand-Archive "$b\wix.zip" "$b\wix-package"
& "$b\dotnet\dotnet.exe" "$b\wix-package\tools\net6.0\any\wix.dll" --version
if($LASTEXITCODE-ne0){exit $LASTEXITCODE}
foreach($case in @('plain','icon')){
 & "$b\dotnet\dotnet.exe" "$b\wix-package\tools\net6.0\any\wix.dll" build -arch x64 -o "$b\$case.msi" "$b\$case.wxs"
 if($LASTEXITCODE-ne0){exit $LASTEXITCODE}
}
$errors=$null;$tokens=$null;$null=[Management.Automation.Language.Parser]::ParseFile("$b\actor.ps1",[ref]$tokens,[ref]$errors)
if($errors.Count){throw ($errors|Out-String)}
'''); save()
    password = secrets.token_urlsafe(28) + 'Aa1!'
    run_ps(r'''
$b='C:\OttrinMsiControlQA';$name='OttrinMsiControlQA'
if(Get-LocalUser $name -ErrorAction SilentlyContinue){throw 'Owned account exists'}
$k=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon',$true)
if('DefaultPassword'-in$k.GetValueNames()){throw 'Existing autologon credential; refuse overwrite'}
try{
 $u=New-LocalUser -Name $name -Password (ConvertTo-SecureString '__PASSWORD__' -AsPlainText -Force) -Description 'Owned disposable minimal MSI control' -PasswordNeverExpires
 Add-LocalGroupMember -SID S-1-5-32-545 -Member $u
 $acl=New-Object Security.AccessControl.DirectorySecurity;$acl.SetAccessRuleProtection($true,$false)
 foreach($sid in @('S-1-5-18','S-1-5-32-544')){$acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule((New-Object Security.Principal.SecurityIdentifier($sid)),'FullControl','ContainerInherit,ObjectInherit','None','Allow')))}
 $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($u.SID,'ReadAndExecute','ContainerInherit,ObjectInherit','None','Allow')));Set-Acl $b $acl
 New-Item -ItemType Directory "$b\results"|Out-Null;$acl=Get-Acl "$b\results";$acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($u.SID,'Modify','ContainerInherit,ObjectInherit','None','Allow')));Set-Acl "$b\results" $acl
 $k.SetValue('DefaultUserName',$name);$k.SetValue('DefaultDomainName',$env:COMPUTERNAME);$k.SetValue('DefaultPassword','__PASSWORD__');$k.SetValue('AutoAdminLogon','1')
}finally{$k.Dispose()}
'''.replace('__PASSWORD__',password)); del password
    n.qga('guest-exec', {'path': r'C:\Windows\System32\shutdown.exe', 'arg': ['/r','/t','2'], 'capture-output': True})
    time.sleep(5); ready()
    deadline=time.monotonic()+180
    while time.monotonic()<deadline:
        value=run_ps("@(Get-CimInstance Win32_Process -Filter \"Name='explorer.exe'\"|Where-Object {(Invoke-CimMethod $_ -MethodName GetOwner).User-eq'OttrinMsiControlQA'}).Count")
        if value.strip()=='1': break
        time.sleep(2)
    else: raise TimeoutError('Owned ordinary interactive account did not become ready')
    run_ps(r'''
$a=New-ScheduledTaskAction -Execute powershell.exe -Argument '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\OttrinMsiControlQA\actor.ps1'
$p=New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\OttrinMsiControlQA" -LogonType Interactive -RunLevel Limited
$s=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 20)
Register-ScheduledTask 'Ottrin MSI Minimal Control' -Action $a -Principal $p -Settings $s|Out-Null
Start-ScheduledTask 'Ottrin MSI Minimal Control'
''')
    deadline=time.monotonic()+1200
    while time.monotonic()<deadline:
        value=run_ps(r"Test-Path 'C:\OttrinMsiControlQA\results\COMPLETE'")
        if value.strip()=='True': break
        time.sleep(2)
    else: raise TimeoutError('Owned native MSI controls did not finish')
    collect(); report['status']='observed'; save()
    assert report['native']['status']=='observed', report['native']
except BaseException as error:
    report['status']='failed'; report['error']=repr(error); save(); raise
finally:
    try: retire()
    finally: save()
print(json.dumps(report,indent=2))
