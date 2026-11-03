"""Resume observation of the same still-running actor; never relaunch it."""
from pathlib import Path
import ast,base64,importlib.util,json,os,re,shutil,signal,subprocess,time,xml.etree.ElementTree as E,zipfile,hashlib
r=Path.cwd();e=r/'.hermes/evidence/windows-window-radius-current/native';state=json.loads((e/'OWNER.json').read_text());name=state['domain'];uid=state['uuid'];volume=name+'.qcow2';iso_volume=name+'-fixture.iso';fixture=e/'fixture';backing=state['base_disk']
class Args:use_stock_base=True
args=Args()
def virsh(*a):return subprocess.check_output(['virsh','-c','qemu:///system',*a],text=True).strip()
def save(n,v):(e/n).write_text(json.dumps(v,indent=2)+'\n')
assert virsh('domuuid',name)==uid
assert not Path('/proc/1157886').exists(),'Do not race original controller'
tree=ast.parse((r/'.hermes/evidence/assistive-delivery-current/windows/cohort-client.py').read_text());nodes=[n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name in ['backing_chain','cleanup']];assert len(nodes)==2
exec(compile(ast.Module(body=nodes,type_ignores=[]),'canonical-cleanup','exec'))
spec=importlib.util.spec_from_file_location('g',r/'.hermes/evidence/windows-20261011/guest_exec.py');g=importlib.util.module_from_spec(spec);spec.loader.exec_module(g);g.DOMAIN=name
pending=None
def ps(code,timeout=30):
 global pending
 assert pending is None
 encoded=base64.b64encode(("$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);"+code).encode('utf-16le')).decode();pending=g.qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-EncodedCommand',encoded],'capture-output':True})['pid'];deadline=time.monotonic()+timeout
 while time.monotonic()<deadline:
  result=g.qga('guest-exec-status',{'pid':pending})
  if result.get('exited'):
   pending=None;assert result.get('exitcode')==0,result;return base64.b64decode(result.get('out-data','')).decode('utf-8-sig').strip()
  time.sleep(.5)
 raise TimeoutError('Existing guest read handle '+str(pending)+' remains pending; do not relaunch')
try:
 prepared=json.loads((e/'GUEST_PREPARATION.json').read_text());root=prepared['root'];assert re.fullmatch(r'C:\\WINDOWS\\TEMP\\ottrin-msi-[a-f0-9]{32}',root,re.I)
 deadline=time.monotonic()+180
 while True:
  v=json.loads(ps("$p='"+root+"\\results\\RESULT.json';if(Test-Path $p){Get-Content $p -Raw}else{'{}'}"));save('RECOVERY_LIVE_RESULT.json',v)
  if v.get('status') not in [None,'running']:break
  assert time.monotonic()<deadline,'Same actor did not complete; preserve partial evidence'
  time.sleep(3)
 save('GUEST_RESULT.json',v)
 encoded=ps("Compress-Archive '"+root+"\\results\\*' 'C:\\OttrinRadiusResults.zip';[Convert]::ToBase64String([IO.File]::ReadAllBytes('C:\\OttrinRadiusResults.zip'))");zbytes=base64.b64decode(encoded);(e/'RESULTS.zip').write_bytes(zbytes)
 with zipfile.ZipFile(e/'RESULTS.zip') as z:
  extracted=e/'results';extracted.mkdir(exist_ok=False);members=[]
  for i in z.infolist():
   assert not i.is_dir() and Path(i.filename).name==i.filename
   data=z.read(i);(extracted/i.filename).write_bytes(data);members.append({'name':i.filename,'sha256':hashlib.sha256(data).hexdigest(),'bytes':len(data)})
  save('TRANSPORT_CONTENT.json',{'archive_sha256':hashlib.sha256(zbytes).hexdigest(),'members':members,'all_members_extracted_equal':True})
 save('RECOVERY.json',{'original_controller_exit':143,'original_controller_absent':True,'same_domain_uuid':uid,'same_actor_not_restarted':True,'terminal_actor_status':v['status']})
finally:
 cleanup()
 # Retire only this exact private display/profile after native VM/media teardown.
 profile=Path(state['private_console']['home']).parent;assert str(profile).startswith('/var/tmp/.ottrin-picker-qa-')
 owned=[]
 for p in Path('/proc').iterdir():
  if not p.name.isdecimal():continue
  try:
   env=dict(x.split(b'=',1) for x in (p/'environ').read_bytes().split(b'\0') if b'=' in x)
   if env.get(b'HOME')==str(profile/'home').encode() and env.get(b'DISPLAY')==state['private_console']['display'].encode():owned.append(int(p.name))
  except (FileNotFoundError,PermissionError,ProcessLookupError):pass
 # Xvfb is spawned before DISPLAY assignment; identify the saved viewer's private X display independently.
 for p in Path('/proc').iterdir():
  if not p.name.isdecimal():continue
  try:
   env=dict(x.split(b'=',1) for x in (p/'environ').read_bytes().split(b'\0') if b'=' in x)
   if env.get(b'HOME')==str(profile/'home').encode() and os.readlink(p/'exe')=='/usr/bin/Xvfb':owned.append(int(p.name))
  except (FileNotFoundError,PermissionError,ProcessLookupError):pass
 for pid in sorted(set(owned),reverse=True):
  try:os.kill(pid,signal.SIGTERM)
  except ProcessLookupError:pass
 deadline=time.monotonic()+10
 while any(Path('/proc',str(pid)).exists() for pid in owned) and time.monotonic()<deadline:time.sleep(.2)
 live=[]
 for pid in owned:
  p=Path('/proc',str(pid))
  if p.exists() and (p/'stat').read_text().split()[2]!='Z':live.append(pid)
 assert not live,live
 assert not any(line.split()[4].startswith(str(profile)+'/') for line in Path('/proc/self/mountinfo').read_text().splitlines())
 shutil.rmtree(profile);save('RECOVERY_CLEANUP.json',{'canonical_vm_cleanup':state.get('cleanup'),'private_profile_removed':str(profile),'owned_processes_stopped':sorted(set(owned)),'current_packages_preserved':True})
print('Same actor observed and owned resources retired')
