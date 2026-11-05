"""Bounded clean-client control with teardown registered before VM allocation."""
from pathlib import Path
from contextlib import ExitStack
import argparse, base64, re, hashlib, importlib.util, json, os, shutil, signal, subprocess, sys, time, urllib.request, uuid, xml.etree.ElementTree as E
def cancelled(signum, frame):raise SystemExit(128+signum)
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,cancelled)
parser=argparse.ArgumentParser();parser.add_argument('--update-manifest',type=Path);parser.add_argument('--output',type=Path);parser.add_argument('--msix-parse-only',action='store_true');parser.add_argument('--msix-stage',type=Path);parser.add_argument('--msix-runtime',type=Path);parser.add_argument('--msix-upgrade-from',type=Path);parser.add_argument('--msix-startup-disabled',action='store_true');parser.add_argument('--prepare-updated-base',action='store_true');parser.add_argument('--use-updated-base',action='store_true');parser.add_argument('--use-stock-base',action='store_true');parser.add_argument('--native-test',type=Path);parser.add_argument('--test-filter');parser.add_argument('--native-helper',type=Path);parser.add_argument('--native-msi-builder',type=Path);parser.add_argument('--native-elevated',action='store_true');parser.add_argument('--application-msi',type=Path);parser.add_argument('--application-nsis-stage',type=Path);parser.add_argument('--application-uia-stage',type=Path);parser.add_argument('--native-transport-probe',action='store_true');parser.add_argument('--office-installed',action='store_true');parser.add_argument('--office-worksheet',action='store_true');parser.add_argument('--application-actor',type=Path);parser.add_argument('--application-native-test',type=Path);parser.add_argument('--boundary-fixture',type=Path);parser.add_argument('--platform-fixture-test',type=Path);parser.add_argument('--application-system-companion',type=Path);parser.add_argument('--display-resolution',choices=['2560x1600']);parser.add_argument('--display-driver',type=Path);args=parser.parse_args()
assert not args.office_worksheet or args.office_installed,'Worksheet surface requires the installed Office fixture'
assert not args.office_installed or (args.application_nsis_stage and args.output and not args.use_stock_base and not args.use_updated_base),'Installed Office uses its exact declared retained-provider clone'
assert not args.application_actor or ((args.application_nsis_stage or args.application_native_test) and args.use_stock_base and args.output and not args.office_installed),'Explicit actor requires a receipt-bound package and stock-client case'
assert not args.display_resolution or args.application_actor,'Explicit virtual resolution requires an owned stock-client application actor'
assert not args.display_driver or (args.display_resolution and args.application_actor),'Display driver setup is only for an explicitly configured owned application clone'
assert not args.application_actor or (args.application_actor.is_file() and not args.application_actor.is_symlink()),'Actor must be a regular reviewable input'
application_case=bool(args.application_msi or args.application_nsis_stage or args.application_uia_stage or args.application_native_test)
assert not args.native_transport_probe or (args.use_stock_base and args.output and not any([application_case,args.native_msi_builder,args.native_elevated,args.update_manifest,args.msix_parse_only,args.msix_stage,args.msix_runtime,args.native_test])), 'Transport probe requires an isolated stock case'
assert sum(map(bool,[args.application_msi,args.application_nsis_stage,args.application_uia_stage,args.application_native_test]))<=1
assert not args.application_uia_stage or (args.use_stock_base and args.output and not any([args.native_msi_builder,args.native_elevated,args.update_manifest,args.msix_parse_only,args.msix_stage,args.msix_runtime,args.native_test])), "UIAutomation requires its own stock ordinary-user case"
assert not args.application_nsis_stage or ((args.use_stock_base or args.office_installed) and args.output and not any([args.native_msi_builder,args.native_elevated,args.update_manifest,args.msix_parse_only,args.msix_stage,args.msix_runtime,args.native_test])), 'NSIS requires its own stock ordinary-user case'
assert not args.application_msi or ((args.use_stock_base or args.use_updated_base) and args.output and not any([args.native_msi_builder,args.native_elevated,args.update_manifest,args.msix_parse_only,args.msix_stage,args.msix_runtime,args.native_test])),'Application MSI requires a fresh declared stock or prepared updated ordinary-user case'
assert not args.boundary_fixture or args.application_native_test
assert not args.platform_fixture_test or (args.application_native_test and args.application_actor and args.use_stock_base and args.output and not args.boundary_fixture), 'UI platform fixture requires an explicit native UI actor and its isolated stock case'
assert not args.application_system_companion or (args.application_native_test and args.application_native_test.name.startswith('ottrin_platform-') and args.application_actor and args.use_stock_base and args.output and not any([args.boundary_fixture,args.platform_fixture_test])), 'SYSTEM companion requires a declared platform test and explicit ordinary actor on its owned stock clone'
assert not args.application_native_test or (args.use_stock_base and args.output and args.application_actor and not any([args.native_msi_builder,args.native_elevated,args.update_manifest,args.msix_parse_only,args.msix_stage,args.msix_runtime,args.native_test,args.native_helper])), 'Native boundary test requires an explicit isolated ordinary-user actor'
assert not args.native_elevated or args.native_msi_builder,'Elevated diagnostic requires the native file-only builder; never an application deployment mode'
assert not args.native_helper or (args.native_test and args.test_filter=='windows_privileged_transport::tests::windows_helper_pipe_'), 'Helper input only supports the explicit production-helper test'
assert not args.test_filter or args.native_test
assert not args.native_test or (args.test_filter and re.fullmatch(r'[A-Za-z0-9_:]+',args.test_filter))
assert not args.native_msi_builder or ((args.use_updated_base or args.use_stock_base) and args.output and not any([args.update_manifest,args.msix_parse_only,args.msix_stage,args.msix_runtime,args.native_test]))
assert not args.msix_startup_disabled or args.msix_upgrade_from
assert not args.msix_upgrade_from or args.msix_runtime
assert not args.prepare_updated_base or args.update_manifest
assert not(args.use_updated_base and args.use_stock_base)
assert not((args.use_updated_base or args.use_stock_base) and args.update_manifest)
assert sum(map(bool,[args.update_manifest,args.msix_parse_only,args.msix_stage,args.msix_runtime,args.native_test]))<=1
interactive_case=bool(args.msix_runtime or not(args.update_manifest or args.msix_parse_only or args.msix_stage or args.native_test or args.native_transport_probe))
assert bool(args.output)==bool(args.update_manifest or args.msix_parse_only or args.msix_stage or args.msix_runtime or args.use_updated_base or args.use_stock_base or args.office_installed or args.native_test),'New comparison needs an explicit new evidence root'
r=Path.cwd();e=args.output.resolve() if args.output else Path(__file__).resolve().parent
assert not args.output or not (e/'OWNER.json').exists(),'Do not replace an existing owned run'
# Fail before allocating a VM when the interactive console controller is absent.
if interactive_case:
 helper=e.parent/'privacy-control.py'
 assert helper.is_file() and not helper.is_symlink(), f'Missing owned interactive controller: {helper}'
if args.application_system_companion:
 assert args.application_system_companion.is_file() and not args.application_system_companion.is_symlink() and args.application_system_companion.resolve().parent==args.application_actor.resolve().parent==e.parent
e.mkdir(parents=True,exist_ok=True)
(e/'DRIVER_SOURCE.py').write_bytes(Path(__file__).read_bytes())
name='ottrin-boundary-client' if args.application_native_test else 'ottrin-uia-client' if args.application_uia_stage else 'ottrin-transport-probe' if args.native_transport_probe else 'ottrin-nsis-client' if args.application_nsis_stage else 'ottrin-native-test' if args.native_test else 'ottrin-msix-runtime' if args.msix_runtime else 'ottrin-msix-assembly' if args.msix_stage else 'ottrin-msix-parser' if args.msix_parse_only else 'ottrin-msi-updated-client' if args.update_manifest else 'ottrin-msi-clean-client';uid=str(uuid.uuid4());volume=name+'.qcow2';iso_volume=name+'-fixture.iso'
def virsh(*args):return subprocess.check_output(['virsh','-c','qemu:///system',*args],text=True).strip()
def save(n,data):(e/n).write_text(json.dumps(data,indent=2)+'\n')
def nsis_input(stage):
 assert not stage.is_symlink()
 stage=stage.resolve()
 if stage==r/'dist/windows-candidate':
  accepted=json.loads((r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json').read_text());revision=accepted['source_commit'];artifacts=accepted['artifacts'];receipt_root=(r/accepted['evidence_directory']).resolve();assert receipt_root.is_relative_to(r/'.hermes/evidence')
  payload=json.loads((receipt_root/'PAYLOAD.json').read_text());assert len(payload)==accepted['payload_files']==58
 else:
  build=json.loads((e.parent/'windows/BUILD_RESULT.json').read_text());accepted=json.loads((e.parent/'HOST_PAYLOAD.json').read_text())
  assert stage==r/'.hermes/packaging-tmp/.windows-picker-stage'==Path(build['stage'])
  assert build['status'].startswith('portable and NSIS build pass') and accepted['status']=='pass within host archive/content scope'
  assert accepted['source_revision']==build['source_revision'] and accepted['artifacts']==build['artifacts']
  revision=build['source_revision'];artifacts=build['artifacts'];payload=json.loads((e.parent/'PAYLOAD.json').read_text());assert len(payload)==accepted['payload_files']==59
 return revision,artifacts,payload
assert name not in virsh('list','--all','--name').splitlines()
if not args.use_stock_base and not args.office_installed:
 assert virsh('domstate','win11ltsc')=='shut off'
 assert virsh('domuuid','win11ltsc')=='4e160378-2610-4072-b528-048e4c9f509e'
 assert not virsh('snapshot-list','win11ltsc','--name')
for v in [volume,iso_volume]:assert subprocess.run(['virsh','-c','qemu:///system','vol-info','--pool','default',v],capture_output=True).returncode!=0
pins=json.loads((r/'.hermes/evidence/windows-20261011/windows-msi-compiler-downloads.json').read_text())
cache=r/'.hermes/tools/msi-compiler';cache.mkdir(exist_ok=True)
for filename,url,algorithm,digest in ([] if args.native_test or args.msix_runtime or args.native_msi_builder or application_case or args.native_transport_probe else [('runtime.zip',pins['runtime_url'],'sha512',pins['runtime_sha512']),('wix.zip',pins['wix_url'],'sha256',pins['wix_sha256'])]):
 p=cache/filename
 if not p.exists():
  temp=p.with_suffix('.download');assert not temp.exists()
  try:urllib.request.urlretrieve(url,temp);assert hashlib.new(algorithm,temp.read_bytes()).hexdigest()==digest;p.parent.mkdir(exist_ok=True);temp.rename(p)
  finally:temp.unlink(missing_ok=True)
 assert hashlib.new(algorithm,p.read_bytes()).hexdigest()==digest
fixture=e/'fixture';assert not fixture.exists()
scope='Actual same-identity development MSIX upgrade, ordinary runtime, retained configuration/index and real login/removal for the declared startup choice; Store acceptance remains separate' if args.msix_upgrade_from else 'Exact named Rust unit test under the owned Windows guest agent token; no ordinary desktop, installer or user-file workflow acceptance' if args.native_test else 'Actual development MSIX deployment, ordinary runtime, real login startup and removal ownership; no upgrade acceptance' if args.msix_runtime else 'Actual MSIX assembly and payload comparison; deployment remains separate' if args.msix_stage else 'Native MSIX parser/XML and SDK smoke; assembly/deployment remain separate' if args.msix_parse_only else 'Fresh disposable offline client overlay; ordinary-user file-only MSI control. No original VM or user profile mutation.'
if args.native_helper:scope='Five native Windows helper transport tests including the actual production executable in a Unicode/spaced path; guest-agent SYSTEM token, no interactive UAC or administrator Copy acceptance'
if args.native_transport_probe:scope='Native Windows PowerShell UTF8 command transport with exact non-ASCII roundtrip and legacy-codepage negative control; no Ottrin runtime, user profile or installer operation.'
if args.application_uia_stage:scope='Exact current Windows release GUI with explicit borrowed accepted runtime; ordinary-user native UIAutomation names/state/actions/main/picker/Settings/Inspector. Not a packaged delivery or physical adapter acceptance.'
if args.application_nsis_stage:scope='Exact current portable chooser and ordinary-user NSIS fresh install, accepted-baseline running-index upgrade, intent preservation, real cache-lock refusal and supported uninstall retry; no production elevation.'
if args.application_msi:scope='Exact held diagnostic Ottrin MSI ordinary-user install, real cleanup refusal, data/task preservation and supported retry on the declared prepared Windows client; no synthetic application fault or production elevation.'
if args.native_elevated:scope='Diagnostic elevated owned QA account, same native file-only MSI authoring. No ordinary-user acceptance, Ottrin executable, security-policy change or original profile mutation.'
if args.office_installed:scope='Current NSIS installed Office main/Inspector render and fallback on ordinary native WGPU/OpenGL; private clone of retained verified-provider QA base, not a clean stock installation'
if args.application_actor:scope='Exact staged/accepted NSIS and declared custom native application actor on an owned ordinary-user desktop; the frozen actor and result define exercised behavior. No implicit preview-safety, installer-recovery, index-upgrade or whole-roadmap acceptance.'
if args.application_native_test:scope='Exact declared native test executable under a fresh ordinary interactive account; frozen actor and observed named assertions define acceptance. No implicit preview, package, elevation or full-roadmap acceptance.'
if args.application_system_companion:scope='Explicit ordinary-client/SYSTEM-companion native caller and ownership mechanism observation on an owned offline clone; not alternate-administrator UAC, enabled Copy, package or whole-roadmap acceptance.'
backing='/var/lib/libvirt/images/win11ltsc.qcow2'
if args.office_installed:
 assert virsh('domstate','ottrin-copy-cleanup-qa')=='shut off' and virsh('domuuid','ottrin-copy-cleanup-qa')=='9b2e795f-fb04-4adb-95a7-8832f8894c67'
 backing='/var/lib/libvirt/images/ottrin-copy-cleanup-qa.qcow2'
 base_template=E.fromstring(virsh('dumpxml','ottrin-copy-cleanup-qa'));assert {x.get('file') for x in base_template.findall('./devices/disk/source')}=={backing}
 save('PROVIDER_BASE.json',{'domain':'ottrin-copy-cleanup-qa','uuid':virsh('domuuid','ottrin-copy-cleanup-qa'),'disk':backing,'xml_sha256':hashlib.sha256(E.tostring(base_template)).hexdigest(),'scope':'Retained provider base is shut off and only read through an owned COW overlay; source domain and snapshots never changed'})
if args.use_updated_base:
 prepared_base=json.loads((r/'.hermes/evidence/windows-msi-updated-client-current/UPDATED_BASE.json').read_text());assert prepared_base['status']=='ready for named pending native consumers';backing=prepared_base['path'];assert backing=='/var/lib/libvirt/images/ottrin-msi-updated-client.qcow2'
 base_xml=E.fromstring(virsh('vol-dumpxml','--pool','default',prepared_base['volume']));assert base_xml.findtext('./backingStore/path')==prepared_base['backing_original'];assert virsh('vol-path','--pool','default',prepared_base['volume'])==backing
 for domain in virsh('list','--all','--name').splitlines():assert backing not in {x.get('file') for x in E.fromstring(virsh('dumpxml',domain)).findall('./devices/disk/source')}
if args.use_stock_base:
 stock=r/'.hermes/evidence/windows-stock-client-current';prepared_base=json.loads((stock/'ACCEPTANCE.json').read_text());stock_owner=json.loads((stock/'OWNER.json').read_text())
 assert prepared_base['status'].startswith('pass within clean Microsoft install/bootstrap scope') and prepared_base['clean_agent_shutdown']
 assert prepared_base['encryption_observation']['protection_status']=='Off','Never clone a TPM-protected stock fixture'
 assert stock_owner['cleanup']['domain_removed'] and stock_owner['cleanup']['retained_pending_volume']=='ottrin-stock-client.qcow2'
 backing=prepared_base['disk'];assert backing=='/var/lib/libvirt/images/ottrin-stock-client.qcow2' and prepared_base['backing_image'] is None
 base_xml=E.fromstring(virsh('vol-dumpxml','--pool','default','ottrin-stock-client.qcow2'));assert not base_xml.findtext('./backingStore/path')
 for domain in virsh('list','--all','--name').splitlines():assert backing not in {x.get('file') for x in E.fromstring(virsh('dumpxml',domain)).findall('./devices/disk/source')}
available_vm_storage=shutil.disk_usage('/var/lib/libvirt/images').free
assert available_vm_storage>=24*1024**3, f'Owned Windows QA requires24GiB free headroom before allocation; available {available_vm_storage} bytes. Retire verified disposable output before retrying.'
state={'domain':name,'uuid':uid,'base_domain':'ottrin-copy-cleanup-qa' if args.office_installed else None if args.use_stock_base else 'win11ltsc','base_disk':backing,'network_interfaces':0,'volumes':[],'status':'preparing','public_compiler_cache':None if args.native_test or args.msix_runtime or args.native_msi_builder or application_case or args.native_transport_probe else str(cache),'scope':scope,'producer_sha256':hashlib.sha256((e/'DRIVER_SOURCE.py').read_bytes()).hexdigest()}
state['storage_at_start']={'available_bytes':available_vm_storage,'minimum_headroom_bytes':24*1024**3,'path':'/var/lib/libvirt/images'}
save('OWNER.json',state)
def backing_chain(path):
 chain=[]
 while path:
  path=str(Path(path).resolve());assert path not in chain,'Cyclic backing metadata';chain.append(path)
  meta=E.fromstring(virsh('vol-dumpxml',path,'--pool','default'))
  assert str(Path(meta.findtext('target/path')).resolve())==path
  path=meta.findtext('backingStore/path')
 return chain
def cleanup():
 try:
  if name in virsh('list','--all','--name').splitlines():
   assert virsh('domuuid',name)==uid
   assert not virsh('snapshot-list',name,'--name')
   root=E.fromstring(virsh('dumpxml',name));assert not root.findall('./devices/interface')
   assert all(not root.findall('./devices/'+tag) for tag in ['filesystem','hostdev','redirdev'])
   disk_sources={x.get('file') for x in root.findall('./devices/disk/source')};assert disk_sources=={'/var/lib/libvirt/images/'+volume,'/var/lib/libvirt/images/'+iso_volume}
   if virsh('domstate',name)!='shut off':virsh('destroy',name)
   virsh('undefine',name,'--nvram','--tpm')
  virsh('pool-refresh','default');owned_paths={'/var/lib/libvirt/images/'+v for v in state['volumes']};chains=[]
  for other in virsh('list','--all','--name').splitlines():
   for disk in E.fromstring(virsh('dumpxml',other)).findall('./devices/disk/source'):
    if not disk.get('file'):continue
    files=set(backing_chain(disk.get('file')));assert not files.intersection(owned_paths)
    chains.append({'retained_domain':other,'files':sorted(files)})
  for v in state['volumes']:
   files=set(backing_chain('/var/lib/libvirt/images/'+v));assert files.issubset(owned_paths|set(backing_chain(state['base_disk'])))
   chains.append({'owned_volume':v,'files':sorted(files)})
  state['checked_backing_chains']=chains
  state['owned_volume_allocations_before_removal']=[{'volume':v,'allocated_bytes':int(E.fromstring(virsh('vol-dumpxml','--pool','default',v)).findtext('allocation')),'virtual_capacity_bytes':int(E.fromstring(virsh('vol-dumpxml','--pool','default',v)).findtext('capacity'))} for v in state['volumes']]
  removed=[]
  for v in reversed(state['volumes']):
   if v==state.get('retained_pending_volume'):continue
   for domain in virsh('list','--all','--name').splitlines():
    assert '/var/lib/libvirt/images/'+v not in virsh('dumpxml',domain)
   virsh('vol-delete','--pool','default',v)
   removed.append(v)
  if fixture.exists():shutil.rmtree(fixture)
  state['cleanup']={'domain_removed':True,'owned_volumes_removed':removed,'fixture_removed':True,'base_still_off':all(backing not in virsh('dumpxml',d) for d in virsh('list','--all','--name').splitlines()) if args.use_stock_base else virsh('domstate',state['base_domain'])=='shut off','retained_pending_volume':state.get('retained_pending_volume')}
 except BaseException as error:state['cleanup_error']=repr(error);raise
 finally:save('OWNER.json',state)
def transfer_owned_guest_file(guest_path,destination,metadata,receipt_name):
 """Download an owned producer output with retry-safe offsets and exact identity."""
 size=metadata['bytes'];digest=metadata['sha256'].lower()
 assert isinstance(size,int) and 0<size<=512*1024*1024 and re.fullmatch('[0-9a-f]{64}',digest)
 handle=g.qga('guest-file-open',{'path':guest_path,'mode':'rb'});complete=False
 try:
  with destination.open('xb') as output:
   while output.tell()<size:
    offset=output.tell()
    while True:
     try:
      g.qga('guest-file-seek',{'handle':handle,'offset':offset,'whence':0})
      block=g.qga('guest-file-read',{'handle':handle,'count':min(65536,size-offset)})
      break
     except RuntimeError as error:
      if not any(part in str(error).lower() for part in ['guest agent is not responding','guest agent not available','guest agent is not connected','timed out','timeout']):raise
      assert virsh('domuuid',name)==uid and virsh('domstate',name)=='running';time.sleep(1)
    data=base64.b64decode(block.get('buf-b64',''));assert data and len(data)==block['count'];output.write(data)
  assert destination.stat().st_size==size
  with destination.open('rb') as stream:assert hashlib.file_digest(stream,'sha256').hexdigest()==digest
  complete=True
 finally:
  try:g.qga('guest-file-close',{'handle':handle})
  finally:
   if not complete:destination.unlink(missing_ok=True)
 save(receipt_name,{'status':'pass chunked verified owned output','guest_path':guest_path,'bytes':size,'sha256':digest,'chunk_bytes':65536,'uncertain_read_recovery':'Seek to exact original offset before retry; no process restart'})

def read_owned_guest_json(guest_path):
 handle=g.qga('guest-file-open',{'path':guest_path,'mode':'rb'});data=bytearray()
 try:
  while True:
   block=g.qga('guest-file-read',{'handle':handle,'count':min(4096,16385-len(data))})
   chunk=base64.b64decode(block.get('buf-b64',''));assert len(chunk)==block['count']
   data.extend(chunk);assert len(data)<=16384,'Owned metadata exceeds16KiB'
   if block.get('eof'):break
   assert chunk,'Owned metadata read made no progress without EOF'
  return json.loads(data.decode('utf-8-sig'))
 finally:g.qga('guest-file-close',{'handle':handle})

def preserve_existing_msix_results():
 # The export command writes this only after closing and hashing its archive.
 # Observe that durable output; never replay an uncertain Compress-Archive call.
 metadata=read_owned_guest_json(r'C:\OttrinOwnedMsixRuntimeResults.zip.json')
 transfer_owned_guest_file(r'C:\OttrinOwnedMsixRuntimeResults.zip',e/'RESULTS.zip',metadata,'ARCHIVE_TRANSFER.json')
 import zipfile
 with zipfile.ZipFile(e/'RESULTS.zip') as archive:assert archive.testzip() is None
 return metadata

def finish(kind,error,trace):
 if error is not None and not (isinstance(error,SystemExit) and error.code==0):
  state.update(status='failed or interrupted; see retained scope and error',error=repr(error))
  # Preserve owned actor output before destroying a cancelled application case.
  # A partial/running result is diagnostic evidence, never an acceptance result.
  if args.msix_runtime and not (e/'RESULTS.zip').exists() and 'g' in globals():
   try:
    metadata=preserve_existing_msix_results();state['interrupted_output']='RESULTS.zip recovered from durable export metadata; inspect native result, not controller status';save('INTERRUPTED_MSIX_EXPORT.json',metadata)
   except BaseException as export_error:state['interrupted_export_error']=repr(export_error)
  preparation=e/'GUEST_PREPARATION.json'
  if application_case and preparation.exists() and not (e/'RESULTS.zip').exists() and callable(globals().get('ps')):
   try:
    prepared=json.loads(preparation.read_text());guest_root=prepared['root']
    assert re.fullmatch(r'C:\\WINDOWS\\TEMP\\ottrin-msi-[a-f0-9]{32}',guest_root,re.I)
    assert name in virsh('list','--all','--name').splitlines() and virsh('domuuid',name)==uid
    code="$ErrorActionPreference='Stop';$p='"+guest_root+"\\results';if(-not(Test-Path $p)){throw 'Owned actor result directory missing'};$zip='"+guest_root+"\\interrupted-results.zip';if(Test-Path $zip){throw 'Unexpected existing interrupted transport'};Compress-Archive -LiteralPath $p -DestinationPath $zip;$f=Get-Item $zip;@{bytes=$f.Length;sha256=(Get-FileHash $zip).Hash.ToLowerInvariant()}|ConvertTo-Json -Compress"
    exported=ps(code,25)
    save('INTERRUPTED_EXPORT_PROCESS.json',{'exitcode':exported.get('exitcode'),'stderr':base64.b64decode(exported.get('err-data','')).decode('utf-8',errors='replace'),'scope':'Owned output captured on interruption; no terminal or passing native result inferred'})
    assert exported.get('exitcode')==0
    metadata=json.loads(base64.b64decode(exported['out-data']).decode('utf-8-sig'))
    transfer_owned_guest_file(guest_root+'\\interrupted-results.zip',e/'INTERRUPTED_RESULTS.zip',metadata,'INTERRUPTED_ARCHIVE_TRANSFER.json')
    state['interrupted_output']='INTERRUPTED_RESULTS.zip; inspect exact actor status before accepting any scope'
   except BaseException as export_error:state['interrupted_export_error']=repr(export_error)
  if args.prepare_updated_base and state.get('servicing_completed') and volume in state['volumes']:
   save('RECOVERY_BASE.json',{'status':'not accepted; pending owned updated-baseline recovery','path':'/var/lib/libvirt/images/'+volume,'volume':volume,'backing_original':state['base_disk'],'scope':'Successful servicing must not be repeated because its observer failed. Owned domain is stopped and removed; this sole owned disk remains for independent recovery and exact version/clean-shutdown verification.','cleanup_owner':'windows-msi-updated-client-current; current autonomous goal must recover or explicitly retire after consumers close'})
   state['retained_pending_volume']=volume
 cleanup()
 return False
with ExitStack() as stack:
 stack.push(finish)
 fixture.mkdir()
 for filename in ([] if args.native_test or args.msix_runtime or args.native_msi_builder or application_case or args.native_transport_probe else ['runtime.zip','wix.zip']):os.link(cache/filename,fixture/filename)
 if args.update_manifest:
  inputs=json.loads(args.update_manifest.read_text());update_cache=r/'.hermes/tools/windows-client-update'
  assert shutil.disk_usage('/var/lib/libvirt/images').free>24*1024**3,'Need room for owned ISO, servicing overlay and teardown'
  for item in inputs['files']:
   path=update_cache/item['filename']
   with path.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==item['sha256']
   os.link(path,fixture/path.name)
  (fixture/'update-inputs.json').write_text(json.dumps(inputs))
 if args.update_manifest or args.msix_parse_only or args.msix_stage:
  for filename in ['package-msix.ps1','msix-manifest.xml']:
   shutil.copy2(r/'packaging/windows'/filename,fixture/filename)
  os.link(cache/'windows-sdk-buildtools.nupkg',fixture/'windows-sdk-buildtools.zip')
  shutil.copy2(r/'packaging/tests/windows-msix-template.ps1',fixture/'windows-msix-template.ps1')
  shutil.copy2(fixture/'windows-msix-template.ps1',e/'SDK_TEMPLATE_SOURCE.ps1')
  state['sdk_template_sha256']=hashlib.sha256((e/'SDK_TEMPLATE_SOURCE.ps1').read_bytes()).hexdigest();save('OWNER.json',state)
  shutil.copy2(r/'assets/icon.png',fixture/'icon.png')
 if args.msix_stage:
  parent=e.parent;build=json.loads((parent/'windows/BUILD_RESULT.json').read_text());source_input=json.loads((parent/'SOURCE_INPUT.json').read_text());git_input=json.loads((r/'.hermes/evidence/windows-msix-current/GIT_INPUT.json').read_text());assert build['source_revision']==source_input['source_revision']
  assert not args.msix_stage.is_symlink() and args.msix_stage.resolve()==Path(build['stage']) and args.msix_stage.resolve().parent==r/'.hermes/packaging-tmp', 'Native assembly requires the exact declared owned Windows build stage'
  files=[('portable.zip',args.msix_stage/'Ottrin-0.1.0-windows-x64-portable.zip',build['artifacts']['Ottrin-0.1.0-windows-x64-portable.zip']),('source.zip',parent/'source.zip',source_input['sha256']),('mingit.zip',cache/'mingit.zip',git_input['sha256'])]
  assembly_inputs={'source_revision':build['source_revision'],'files':[]}
  for name_in_iso,path,digest in files:
   with path.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==digest
   os.link(path,fixture/name_in_iso);assembly_inputs['files'].append({'name':name_in_iso,'sha256':digest})
  (fixture/'assembly-inputs.json').write_text(json.dumps(assembly_inputs));shutil.copy2(parent/'assembly.ps1',fixture/'assembly.ps1')
  # The assembly fixture may also verify its rebuilt portable daemon in private
  # state. Bind the ordinary app's existing serialized config as an explicit input.
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config']
  (fixture/'daemon-config-template.json').write_text(json.dumps(config))
  assembly_inputs['files'].append({'name':'daemon-config-template.json','sha256':hashlib.sha256((fixture/'daemon-config-template.json').read_bytes()).hexdigest()})
  (fixture/'assembly-inputs.json').write_text(json.dumps(assembly_inputs))
 if args.msix_runtime:
  parent=e.parent;accepted=json.loads((parent/'ASSEMBLY_ACCEPTANCE.json').read_text());assert args.msix_runtime.resolve()==Path(accepted['artifact'])
  with args.msix_runtime.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==accepted['artifact_sha256']
  os.link(args.msix_runtime,fixture/'Ottrin.msix');shutil.copy2(parent/'runtime.ps1',fixture/'runtime.ps1');shutil.copy2(r/'.hermes/evidence/desktop-package-current/windows/gui/NATIVE_CONTROLS_INPUT.ps1',fixture/'native-controls.ps1')
  shutil.copy2(fixture/'runtime.ps1',e/'RUNTIME_SOURCE.ps1');shutil.copy2(fixture/'native-controls.ps1',e/'NATIVE_CONTROLS_SOURCE.ps1')
  runtime_input={'runtime_script_sha256':hashlib.sha256((fixture/'runtime.ps1').read_bytes()).hexdigest(),'native_controls_sha256':hashlib.sha256((fixture/'native-controls.ps1').read_bytes()).hexdigest(),'sha256':accepted['artifact_sha256'],'source_revision':accepted['source_revision'],'version':accepted['version'],'startup_enabled':not args.msix_startup_disabled}
  if args.msix_upgrade_from:
   previous=json.loads((parent/'PREVIOUS_PACKAGE.json').read_text());assert args.msix_upgrade_from.resolve()==Path(previous['retained_input'])
   with args.msix_upgrade_from.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==previous['sha256']
   versions=[tuple(int(part) for part in value.split('.')) for value in [previous['version'],runtime_input['version']]]
   assert all(len(version)==4 and all(0<=part<=65535 for part in version) for version in versions),versions
   assert versions[0]<versions[1], 'Actual MSIX update requires a strictly newer four-part version'
   os.link(args.msix_upgrade_from,fixture/'Previous.msix');runtime_input['previous']={'sha256':previous['sha256'],'version':previous['version'],'source_revision':previous['source_revision']}
  (fixture/'runtime-input.json').write_text(json.dumps(runtime_input));save('RUNTIME_INPUT.json',runtime_input)
 (fixture/'pins.json').write_text(json.dumps(pins))
 if args.native_transport_probe:
  transport_expected={'title':'Ottrin \u2014 Selection proof','text':'\u00c5\u00e4\u00f6 \u6587 \U0001f680','path':'C:\\Native\u00c5\\\u6587\U0001f680.txt'}
  (fixture/'transport.json').write_text(json.dumps(transport_expected,ensure_ascii=False),encoding='utf-8');iso_inputs=[fixture/'transport.json'];save('TRANSPORT_INPUT.json',transport_expected)
 elif args.native_test:
  executable=args.native_test.resolve();assert executable.parent in [r/'target/x86_64-pc-windows-gnu/release/deps',r/'target/x86_64-pc-windows-gnu/debug/deps'] and re.fullmatch(r'ottrin_(ui|search|core|platform|preview|shell)-[0-9a-f]{16}\.exe',executable.name)
  with executable.open('rb') as file:digest=hashlib.file_digest(file,'sha256').hexdigest()
  os.link(executable,fixture/'native-test.exe');iso_inputs=[fixture/'native-test.exe'];save('INPUT.json',{'executable':str(executable),'sha256':digest,'test_filter':args.test_filter,'exact':not bool(args.native_helper),'expected_pass_count':5 if args.native_helper else 1,'cargo_output_directory':executable.parent.parent.name})
  if args.native_helper:
   helper=args.native_helper.resolve();assert helper==r/'target/x86_64-pc-windows-gnu/debug/ottrin-priv-helper.exe' and helper.is_file() and not args.native_helper.is_symlink()
   with helper.open('rb') as file:helper_digest=hashlib.file_digest(file,'sha256').hexdigest()
   os.link(helper,fixture/'native-helper.exe');iso_inputs.append(fixture/'native-helper.exe');save('HELPER_INPUT.json',{'executable':str(helper),'sha256':helper_digest,'scope':'Production helper protocol negotiation and kernel caller binding; guest-agent token, no interactive UAC'})
 elif args.msix_runtime:
  iso_inputs=[fixture/n for n in ['Ottrin.msix','runtime.ps1','runtime-input.json','native-controls.ps1']]
  if args.msix_upgrade_from:iso_inputs.append(fixture/'Previous.msix')
 else:
  source=r/'.hermes/evidence/msi-hosted-control-current/source'
  actor=args.application_actor.read_text() if args.application_actor else (e.parent/'actor.ps1').read_text() if args.office_installed else (r/'.hermes/evidence/windows-assistive-current/actor.ps1').read_text() if args.application_uia_stage else (r/'.hermes/evidence/assistive-delivery-current/nsis-actor.ps1').read_text() if args.application_nsis_stage else (r/'.hermes/evidence/windows-msi-client-current/application-actor.ps1').read_text() if args.application_msi else (args.native_msi_builder.parent/'actor-input.ps1').read_text() if args.native_msi_builder and args.native_msi_builder.parent.name=='windows-msi-deferred-control-current' else (source/'msi-rollback-control-actor.ps1').read_text()
  if not application_case:
   actor=actor.replace(" foreach($kind in @('install','uninstall')) {", " $result.phase='waiting for owned ordinary desktop';Save;$deadline=[DateTime]::UtcNow.AddMinutes(8);$readyPath=Join-Path $out 'DESKTOP_READY.json';while(-not(Test-Path $readyPath)){if([DateTime]::UtcNow-ge$deadline){throw 'Owned first-login desktop was not made ready'};Start-Sleep -Milliseconds 250};$ready=Get-Content $readyPath -Raw|ConvertFrom-Json;Require ($ready.sid-eq$ExpectedSid) 'Desktop readiness belongs to another fixture';$result.desktop_ready=$true;$result.phase='executing native MSI rollback control';Save\n foreach($kind in @('install','uninstall')) {")
   actor=actor.replace(" $profilePath=[Environment]", " $session=(Get-Process -Id $PID).SessionId;Require ($session-gt0) 'Real ordinary interactive session required';$result.session_id=$session\n $profilePath=[Environment]")
  if args.native_elevated:
   ordinary="Require (-not([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary token required'"
   assert ordinary in actor and 'administrator=$false' in actor
   actor=actor.replace(ordinary,"Require (([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Diagnostic elevated token required'").replace('administrator=$false','administrator=$true').replace('waiting for owned ordinary desktop','waiting for owned diagnostic desktop').replace('Fresh native ordinary-user file-only MSI rollback after executed copy/removal; no Ottrin executable or application custom action','Diagnostic elevated-account native file-only MSI rollback; no ordinary-user acceptance or Ottrin executable')
  (fixture/'actor.ps1').write_text(actor)
  script=(source/'msi-rollback-control.ps1').read_text()
  start=script.index("if($env:GITHUB_ACTIONS")
  end=script.index("$id=[Security.Principal.WindowsIdentity]",start)
  script=script[:start]+script[end:]
  script=script.replace("$env:RUNNER_TEMP","$env:TEMP")
  start=script.index(" $repo=[IO.Path]::GetFullPath")
  end=script.index(" Expand-Archive",start)
  script=script[:start]+" $pins=Get-Content (Join-Path $PSScriptRoot 'pins.json') -Raw|ConvertFrom-Json\n foreach($file in @('runtime.zip','wix.zip')){Copy-Item (Join-Path $PSScriptRoot $file) (Join-Path $root $file)}\n if((Get-FileHash (Join-Path $root 'runtime.zip') -Algorithm SHA512).Hash-ne$pins.runtime_sha512-or(Get-FileHash (Join-Path $root 'wix.zip')).Hash-ne$pins.wix_sha256){throw 'Offline compiler input mismatch'}\n"+script[end:]
  script=script.replace("(Join-Path $PSScriptRoot 'msi-rollback-control-actor.ps1')","(Join-Path $PSScriptRoot 'actor.ps1')")
  start=script.index(" $arguments='-NoProfile -NonInteractive -ExecutionPolicy")
  end=script.index(" $process=Start-Process",start)
  script=script[:start]+" $invoke=\"& ([ScriptBlock]::Create([IO.File]::ReadAllText('$(Join-Path $root 'actor.ps1')'))) -Root '$root' -ExpectedSid '$($user.SID.Value)'\"\n $arguments='-NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand '+[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($invoke))\n"+script[end:]
  # A disposable overlay is the cleanup boundary for any engine registration defect.
  script=script.replace('hosted_runner_disposal_handles_any_residual_engine_registration','owned_overlay_disposal_handles_any_residual_engine_registration')
  # LocalSystem cannot use CreateProcessWithLogonW. Log on the fresh ordinary
  # account on this offline disposable clone instead; never alter the base.
  start=script.index(" $process=Start-Process")
  script=script[:start]+''' $action=New-ScheduledTaskAction -Execute "$env:SystemRoot\\System32\\WindowsPowerShell\\v1.0\\powershell.exe" -Argument $arguments
  $trigger=New-ScheduledTaskTrigger -AtLogOn -User ($env:COMPUTERNAME+'\\'+$name)
  $principal=New-ScheduledTaskPrincipal -UserId ($env:COMPUTERNAME+'\\'+$name) -LogonType Interactive -RunLevel Limited
  $taskName='OttrinOwnedMsiControl';Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Principal $principal -Force|Out-Null
  $plain=$credential.GetNetworkCredential().Password
  $key='HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon'
  foreach($pair in @(@('AutoAdminLogon','1'),@('DefaultUserName',$name),@('DefaultDomainName',$env:COMPUTERNAME),@('DefaultPassword',$plain))){Set-ItemProperty -Path $key -Name $pair[0] -Value $pair[1]}
  $plain=$null;$state.phase='ready for owned interactive reboot';$state.status='prepared';$state.task=$taskName;$state.actor_sid=$user.SID.Value;$exit=0;Save
 }catch{$state.status='failed interactive preparation';$state.error=$_.Exception.Message;$state.stack=$_.ScriptStackTrace;$exit=1}
 finally{if($password){$password.Dispose()};Save}
 exit $exit
 '''
  if args.native_elevated:
   membership="Add-LocalGroupMember -Group (Get-LocalGroup -SID 'S-1-5-32-545') -Member $user"
   assert membership in script and '-LogonType Interactive -RunLevel Limited' in script
   script=script.replace(membership,membership+";Add-LocalGroupMember -Group (Get-LocalGroup -SID 'S-1-5-32-544') -Member $user").replace('-LogonType Interactive -RunLevel Limited','-LogonType Interactive -RunLevel Highest')
   save('SECURITY_CONTEXT.json',{'purpose':'Single diagnostic variable: elevated QA actor token, compared with native ordinary-user control','scope':'Only newly created account in the offline disposable overlay; no policy/ACL/rollback bypass; not release acceptance','ordinary_comparison':'.hermes/evidence/windows-stock-control-current/GUEST_RESULT.json'})
  if args.native_msi_builder:
   assert args.native_msi_builder.resolve() in [r/'.hermes/evidence/windows-msi-native-control-current/build.ps1',r/'.hermes/evidence/windows-msi-deferred-control-current/build.ps1']
   if args.native_msi_builder.parent.name=='windows-msi-deferred-control-current':
    library=args.native_msi_builder.parent/'failure.dll';identity=json.loads((args.native_msi_builder.parent/'INPUT.json').read_text());assert hashlib.sha256(library.read_bytes()).hexdigest()==identity['failure_dll_sha256'];shutil.copy2(library,fixture/'failure.dll');save('FAILURE_LIBRARY.json',{'sha256':identity['failure_dll_sha256'],'source_sha256':identity['failure_source_sha256'],'scope':'Only returns1603; no state mutation or Ottrin executable'})
   start=script.index(" $pins=Get-Content")
   end=script.index(" Copy-Item (Join-Path $PSScriptRoot 'actor.ps1')",start)
   script=script[:start]+" & ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $PSScriptRoot 'native-build.ps1')))) -Root $root -Nonce $nonce\n"+script[end:]
   shutil.copy2(args.native_msi_builder,fixture/'native-build.ps1')
  if args.application_msi:
   diagnostic=r/'.hermes/evidence/windows-msi-package-control-current/input';receipt=json.loads((diagnostic/'DIAGNOSTIC_INPUT.json').read_text());artifact=args.application_msi.resolve();assert not args.application_msi.is_symlink() and artifact==Path(receipt['artifact'])==diagnostic/'Ottrin-0.1.0-windows-x64.msi'
   digest=hashlib.sha256(artifact.read_bytes()).hexdigest();assert digest==receipt['sha256']
   assert Path(receipt['payload_manifest'])==diagnostic/'payload.json';payload=json.loads((diagnostic/'payload.json').read_text());assert len(payload)==receipt['payload_files']==58
   os.link(artifact,fixture/'application.msi');(fixture/'payload.json').write_text(json.dumps(payload))
   config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];(fixture/'config-template.json').write_text(json.dumps(config))
   identity={'sha256':digest,'source_revision':receipt['source_revision'],'payload_files':len(payload),'scope':'Exact retained failing MSI diagnostic input, not rebuilt, patched or artificially faulted'};(fixture/'application-input.json').write_text(json.dumps(identity));save('APPLICATION_INPUT.json',identity)
   start=script.index(" $pins=Get-Content");end=script.index(" Copy-Item (Join-Path $PSScriptRoot 'actor.ps1')",start)
   script=script[:start]+" foreach($file in @('application.msi','application-input.json','payload.json','config-template.json')){Copy-Item (Join-Path $PSScriptRoot $file) (Join-Path $root $file)}\n $packageInput=Get-Content (Join-Path $root 'application-input.json') -Raw|ConvertFrom-Json;if((Get-FileHash (Join-Path $root 'application.msi')).Hash.ToLowerInvariant()-ne$packageInput.sha256){throw 'Actual MSI input mismatch'}\n"+script[end:]
  if args.office_installed:
   stage=args.application_nsis_stage.resolve();revision,artifacts,payload=nsis_input(args.application_nsis_stage)
   payload_root=stage/'windows-payload/Ottrin-0.1.0-windows-x64'
   assert {str(p.relative_to(payload_root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in payload_root.rglob('*') if p.is_file()}==payload
   assert 'Git revision: '+revision+'\nWorking tree: clean\n' in (payload_root/'SOURCE_REVISION.txt').read_text()
   setup=stage/'Ottrin-0.1.0-windows-x64-setup.exe';assert hashlib.sha256(setup.read_bytes()).hexdigest()==artifacts[setup.name];os.link(setup,fixture/'current-setup.exe')
   assert len(payload)==58
   (fixture/'payload.json').write_text(json.dumps(payload));identity={'source_revision':revision,'files':[{'name':'current-setup.exe','sha256':artifacts[setup.name]}],'fixtures':{},'ui_case':'worksheet' if args.office_worksheet else 'document'}
   application_files=['current-setup.exe','payload.json','application-input.json','native-controls.ps1','config-template.json']
   for fixture_name,fixture_source in [('01-office.docx','office-fidelity.docx'),('02-office.xlsx','office-fidelity.xlsx'),('03-office.pptx','office-fidelity.pptx')]:
    path=r/'crates/ottrin-preview/tests/fixtures'/fixture_source;shutil.copy2(path,fixture/fixture_name);application_files.append(fixture_name);identity['fixtures'][fixture_name]=hashlib.sha256(path.read_bytes()).hexdigest()
   (fixture/'application-input.json').write_text(json.dumps(identity));save('APPLICATION_INPUT.json',identity)
   config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];(fixture/'config-template.json').write_text(json.dumps(config))
   shutil.copy2(r/'.hermes/evidence/desktop-package-current/windows/gui/NATIVE_CONTROLS_INPUT.ps1',fixture/'native-controls.ps1')
   start=script.index(" $pins=Get-Content");end=script.index(" Copy-Item (Join-Path $PSScriptRoot 'actor.ps1')",start)
   script=script[:start]+" foreach($file in @('"+"','".join(application_files)+"')){Copy-Item (Join-Path $PSScriptRoot $file) (Join-Path $root $file)}\n"+script[end:]
  if args.application_native_test:
   executable=args.application_native_test.resolve();assert executable.parent==r/'target/x86_64-pc-windows-gnu/debug/deps' and re.fullmatch(r'(windows_job_native|ottrin_platform|ottrin_ui)-[0-9a-f]{16}\.exe',executable.name) and not args.application_native_test.is_symlink()
   with executable.open('rb') as stream:digest=hashlib.file_digest(stream,'sha256').hexdigest()
   # GNU embeds debug sections in the executable; copying hundreds of MB per
   # case changes the test workload. Retain the canonical compiler output and
   # stage only a verified debug-stripped executable, as MSVC stores PDBs apart.
   import mmap,struct
   def executable_text(path):
    with path.open('rb') as stream,mmap.mmap(stream.fileno(),0,access=mmap.ACCESS_READ) as data:
     assert data[:2]==b'MZ';pe=struct.unpack_from('<I',data,60)[0];assert data[pe:pe+4]==b'PE\0\0'
     count=struct.unpack_from('<H',data,pe+6)[0];optional=pe+24;assert struct.unpack_from('<H',data,optional)[0]==0x20b
     entry=struct.unpack_from('<I',data,optional+16)[0];sections=optional+struct.unpack_from('<H',data,pe+20)[0]
     rows=[]
     for index in range(count):
      at=sections+index*40
      if data[at:at+8].rstrip(b'\0')==b'.text':
       size,offset=struct.unpack_from('<II',data,at+16);rows.append({'entry_rva':entry,'bytes':size,'sha256':hashlib.sha256(data[offset:offset+size]).hexdigest()})
     assert len(rows)==1;return rows[0]
   staged=fixture/'boundary-test.exe';subprocess.run(['x86_64-w64-mingw32-strip','--strip-debug','-o',str(staged),str(executable)],check=True)
   code_identity=executable_text(executable);assert executable_text(staged)==code_identity,'Debug stripping changed executable code or its entry point'
   original_digest=digest
   with staged.open('rb') as stream:digest=hashlib.file_digest(stream,'sha256').hexdigest()
   identity={'source_revision':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'source_test_sha256':original_digest,'test_sha256':digest,'source_bytes':executable.stat().st_size,'staged_bytes':staged.stat().st_size,'strip_command':['x86_64-w64-mingw32-strip','--strip-debug'],'unchanged_executable_text':code_identity,'actor_sha256':hashlib.sha256(args.application_actor.read_bytes()).hexdigest(),'scope':scope}
   if args.boundary_fixture:
    companion=args.boundary_fixture.resolve();assert companion==r/'target/x86_64-pc-windows-gnu/debug/ottrin-native-job-fixture.exe' and not args.boundary_fixture.is_symlink()
    staged_companion=fixture/'boundary-fixture.exe';subprocess.run(['x86_64-w64-mingw32-strip','--strip-debug','-o',str(staged_companion),str(companion)],check=True)
    companion_code=executable_text(companion);assert executable_text(staged_companion)==companion_code
    with companion.open('rb') as stream:companion_source_sha=hashlib.file_digest(stream,'sha256').hexdigest()
    with staged_companion.open('rb') as stream:companion_sha=hashlib.file_digest(stream,'sha256').hexdigest()
    identity['fixture']={'source_sha256':companion_source_sha,'sha256':companion_sha,'source_bytes':companion.stat().st_size,'staged_bytes':staged_companion.stat().st_size,'unchanged_executable_text':companion_code}
   if args.platform_fixture_test:
    assert executable.name.startswith('ottrin_ui-'), 'Only the UI integration tests use a platform fixture compiler executable'
    companion=args.platform_fixture_test.resolve();assert companion.parent==r/'target/x86_64-pc-windows-gnu/debug/deps' and re.fullmatch(r'ottrin_platform-[0-9a-f]{16}\.exe',companion.name) and not args.platform_fixture_test.is_symlink()
    staged_companion=fixture/'platform-fixture-test.exe';subprocess.run(['x86_64-w64-mingw32-strip','--strip-debug','-o',str(staged_companion),str(companion)],check=True)
    companion_code=executable_text(companion);assert executable_text(staged_companion)==companion_code
    with companion.open('rb') as stream:companion_source_sha=hashlib.file_digest(stream,'sha256').hexdigest()
    with staged_companion.open('rb') as stream:companion_sha=hashlib.file_digest(stream,'sha256').hexdigest()
    identity['platform_fixture']={'source_sha256':companion_source_sha,'sha256':companion_sha,'source_bytes':companion.stat().st_size,'staged_bytes':staged_companion.stat().st_size,'unchanged_executable_text':companion_code}
   if args.application_system_companion:
    shutil.copyfile(args.application_system_companion,fixture/'system-companion.ps1');shutil.copyfile(args.application_system_companion,e/'SYSTEM_COMPANION_SOURCE.ps1');identity['system_companion_sha256']=hashlib.sha256(args.application_system_companion.read_bytes()).hexdigest()
   changed=set(subprocess.check_output(['git','diff','--name-only','HEAD','--','Cargo.toml','Cargo.lock','crates'],text=True).splitlines())
   changed.update(subprocess.check_output(['git','ls-files','--others','--exclude-standard','--','Cargo.toml','Cargo.lock','crates'],text=True).splitlines())
   identity['changed_product_sources']={path:hashlib.sha256((r/path).read_bytes()).hexdigest() if (r/path).is_file() else None for path in sorted(changed)}
   identity['source_scope']='Git revision is the reference base; changed_product_sources binds any uncommitted product snapshot. Executable hash/text bind the actual staged native test.'
   (fixture/'application-input.json').write_text(json.dumps(identity));save('APPLICATION_INPUT.json',identity);application_files=['boundary-test.exe','application-input.json']+(['boundary-fixture.exe'] if args.boundary_fixture else [])+(['platform-fixture-test.exe'] if args.platform_fixture_test else [])+(['system-companion.ps1'] if args.application_system_companion else [])
   start=script.index(" $pins=Get-Content");end=script.index(" Copy-Item (Join-Path $PSScriptRoot 'actor.ps1')",start)
   script=script[:start]+" foreach($file in @('"+"','".join(application_files)+"')){Copy-Item (Join-Path $PSScriptRoot $file) (Join-Path $root $file)}\n"+script[end:]
  if args.application_system_companion:
   # The sole additional elevated role lives entirely inside this owned clone.
   # The ordinary actor and real pipe still define request/caller admission.
   companion_setup=r''' $companion=Join-Path $root 'system-companion.ps1'
 $tokens=$null;$parseErrors=$null;$null=[Management.Automation.Language.Parser]::ParseFile($companion,[ref]$tokens,[ref]$parseErrors);if($parseErrors.Count){throw ($parseErrors|Out-String)}
 $probeData=Join-Path $root 'probe-data';New-Item -ItemType Directory $probeData|Out-Null;Grant $probeData $user.SID 'FullControl'
 $protected=Join-Path $root 'system-only';New-Item -ItemType Directory $protected|Out-Null
 $protectedAcl=[Security.AccessControl.DirectorySecurity]::new();$protectedAcl.SetAccessRuleProtection($true,$false)
 foreach($sid in @('S-1-5-18','S-1-5-32-544')){$protectedAcl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid),'FullControl','ContainerInherit,ObjectInherit','None','Allow'))};Set-Acl $protected $protectedAcl
 $invokeCompanion="& ([ScriptBlock]::Create([IO.File]::ReadAllText('$companion'))) -Root '$root' -ExpectedSid '$($user.SID.Value)'"
 $companionArguments='-NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand '+[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($invokeCompanion))
 $companionAction=New-ScheduledTaskAction -Execute "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -Argument $companionArguments
 $companionPrincipal=New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
 $companionTask='OttrinOwnedCallerCompanion';Register-ScheduledTask -TaskName $companionTask -Action $companionAction -Trigger (New-ScheduledTaskTrigger -AtStartup) -Principal $companionPrincipal -Force|Out-Null
 $state.system_companion_task=$companionTask
'''
   insertion=" $credential=[Management.Automation.PSCredential]::new"
   assert script.count(insertion)==1
   script=script.replace(insertion,companion_setup+insertion)
  if args.application_nsis_stage and args.application_actor:
   stage=args.application_nsis_stage.resolve();revision,artifacts,payload=nsis_input(args.application_nsis_stage)
   setup=stage/'Ottrin-0.1.0-windows-x64-setup.exe';digest=hashlib.sha256(setup.read_bytes()).hexdigest();assert digest==artifacts[setup.name];os.link(setup,fixture/'current-setup.exe')
   identity={'source_revision':revision,'files':[{'name':'current-setup.exe','sha256':digest}],'actor_sha256':hashlib.sha256(args.application_actor.read_bytes()).hexdigest(),'scope':scope}
   (fixture/'payload.json').write_text(json.dumps(payload));(fixture/'application-input.json').write_text(json.dumps(identity));save('APPLICATION_INPUT.json',identity)
   config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];(fixture/'config-template.json').write_text(json.dumps(config))
   shutil.copy2(r/'.hermes/evidence/desktop-package-current/windows/gui/NATIVE_CONTROLS_INPUT.ps1',fixture/'native-controls.ps1')
   application_files=['current-setup.exe','payload.json','application-input.json','config-template.json','native-controls.ps1']
   start=script.index(" $pins=Get-Content");end=script.index(" Copy-Item (Join-Path $PSScriptRoot 'actor.ps1')",start)
   script=script[:start]+" foreach($file in @('"+"','".join(application_files)+"')){Copy-Item (Join-Path $PSScriptRoot $file) (Join-Path $root $file)}\n"+script[end:]
  if args.application_nsis_stage and not args.office_installed and not args.application_actor:
   package_evidence=e.parent;receipt=json.loads((package_evidence/'HOST_PAYLOAD.json').read_text());stage=args.application_nsis_stage.resolve();assert stage==r/'.hermes/packaging-tmp/.windows-picker-stage' and not args.application_nsis_stage.is_symlink()
   old=json.loads((r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json').read_text());previous=r/'dist/windows-candidate/Ottrin-0.1.0-windows-x64-setup.exe'
   files=[('portable.zip',stage/'Ottrin-0.1.0-windows-x64-portable.zip',receipt['artifacts']['Ottrin-0.1.0-windows-x64-portable.zip']),('current-setup.exe',stage/'Ottrin-0.1.0-windows-x64-setup.exe',receipt['artifacts']['Ottrin-0.1.0-windows-x64-setup.exe']),('previous-setup.exe',previous,old['artifacts'][previous.name])]
   identity={'source_revision':receipt['source_revision'],'files':[],'previous_source':old['source_commit']}
   for name_in_fixture,path,expected in files:
    with path.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==expected
    os.link(path,fixture/name_in_fixture);identity['files'].append({'name':name_in_fixture,'sha256':expected})
   payload_root=r/'dist/windows-candidate/windows-payload/Ottrin-0.1.0-windows-x64';old_payload={str(path.relative_to(payload_root)).replace('\\','/'):hashlib.sha256(path.read_bytes()).hexdigest() for path in payload_root.rglob('*') if path.is_file()};assert len(old_payload)==old['payload_files']==58
   (fixture/'previous-payload.json').write_text(json.dumps(old_payload));shutil.copy2(package_evidence/'PAYLOAD.json',fixture/'payload.json')
   config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];(fixture/'config-template.json').write_text(json.dumps(config))
   shutil.copy2(r/'.hermes/evidence/desktop-package-current/windows/gui/NATIVE_CONTROLS_INPUT.ps1',fixture/'native-controls.ps1')
   (fixture/'application-input.json').write_text(json.dumps(identity));save('APPLICATION_INPUT.json',identity)
   start=script.index(" $pins=Get-Content");end=script.index(" Copy-Item (Join-Path $PSScriptRoot 'actor.ps1')",start)
   application_files=['portable.zip','current-setup.exe','previous-setup.exe','payload.json','previous-payload.json','config-template.json','native-controls.ps1','application-input.json']
   script=script[:start]+" foreach($file in @('"+"','".join(application_files)+"')){Copy-Item (Join-Path $PSScriptRoot $file) (Join-Path $root $file)}\n"+script[end:]
  if args.application_uia_stage:
   stage=args.application_uia_stage.resolve();assert stage==r/'.hermes/packaging-tmp/.windows-assistive-stage' and not args.application_uia_stage.is_symlink()
   identity=json.loads((stage/'uia-input.json').read_text());archive=stage/'uia.zip';assert hashlib.sha256(archive.read_bytes()).hexdigest()==identity['archive_sha256']
   os.link(archive,fixture/'uia.zip');shutil.copy2(stage/'uia-input.json',fixture/'uia-input.json')
   config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];(fixture/'config-template.json').write_text(json.dumps(config))
   shutil.copy2(r/'.hermes/evidence/desktop-package-current/windows/gui/NATIVE_CONTROLS_INPUT.ps1',fixture/'native-controls.ps1')
   application_files=['uia.zip','uia-input.json','config-template.json','native-controls.ps1'];save('APPLICATION_INPUT.json',identity)
   start=script.index(" $pins=Get-Content");end=script.index(" Copy-Item (Join-Path $PSScriptRoot 'actor.ps1')",start)
   script=script[:start]+" foreach($file in @('"+"','".join(application_files)+"')){Copy-Item (Join-Path $PSScriptRoot $file) (Join-Path $root $file)}\n"+script[end:]
  if args.application_nsis_stage or args.application_uia_stage or args.application_native_test:
   marker=" Copy-Item (Join-Path $PSScriptRoot 'actor.ps1') (Join-Path $root 'actor.ps1')"
   assert marker in script
   script=script.replace(marker,marker+"\n $tokens=$null;$parseErrors=$null;$null=[Management.Automation.Language.Parser]::ParseFile((Join-Path $root 'actor.ps1'),[ref]$tokens,[ref]$parseErrors);if($parseErrors.Count){throw ($parseErrors|Out-String)}")
  (fixture/'controller.ps1').write_text(script);shutil.copy2(fixture/'controller.ps1',e/'controller.ps1');shutil.copy2(fixture/'actor.ps1',e/'actor.ps1')
  iso_inputs=[fixture/n for n in (['native-build.ps1','actor.ps1','controller.ps1'] if args.native_msi_builder else ['runtime.zip','wix.zip','pins.json','actor.ps1','controller.ps1'])]
  if args.application_nsis_stage or args.application_uia_stage or args.application_native_test:iso_inputs=[fixture/n for n in ['actor.ps1','controller.ps1',*application_files]]
  if args.application_msi:iso_inputs=[fixture/n for n in ['actor.ps1','controller.ps1','application.msi','application-input.json','payload.json','config-template.json']]
  if (fixture/'failure.dll').exists():iso_inputs.append(fixture/'failure.dll')
  if args.update_manifest:
   iso_inputs.extend(fixture/item['filename'] for item in inputs['files'])
   iso_inputs.append(fixture/'update-inputs.json')
  if args.update_manifest or args.msix_parse_only or args.msix_stage:
   iso_inputs.extend(fixture/name for name in ['package-msix.ps1','msix-manifest.xml','windows-sdk-buildtools.zip','windows-msix-template.ps1','icon.png'])
  if args.msix_stage:iso_inputs.extend(fixture/name for name in ['portable.zip','source.zip','mingit.zip','assembly-inputs.json','assembly.ps1','daemon-config-template.json'])
 # UDF carries the real >4GiB file size; the ISO9660 fallback is limited.
 # The Windows reader must recheck every update hash before WUSA executes it.
 if args.display_driver:
  driver=args.display_driver.resolve();assert driver==r/'.hermes/tools/windows-display-driver' and not args.display_driver.is_symlink()
  manifest=json.loads((driver/'DRIVER_INPUT.json').read_text());assert set(manifest['files'])=={'viogpudo.inf','viogpudo.cat','viogpudo.sys'}
  for filename,digest in manifest['files'].items():
   p=driver/filename;assert p.is_file() and not p.is_symlink() and hashlib.sha256(p.read_bytes()).hexdigest()==digest
   os.link(p,fixture/filename);iso_inputs.append(fixture/filename)
  shutil.copy2(driver/'DRIVER_INPUT.json',fixture/'display-driver-input.json');iso_inputs.append(fixture/'display-driver-input.json')
  prepare_source=r/'.hermes/evidence/windows-dpi-current/prepare-display-driver.ps1';shutil.copy2(prepare_source,fixture/'prepare-display-driver.ps1');iso_inputs.append(fixture/'prepare-display-driver.ps1')
  (e/'DISPLAY_DRIVER_PREPARE_SOURCE.ps1').write_bytes((fixture/'prepare-display-driver.ps1').read_bytes());state['display_driver_prepare_sha256']=hashlib.sha256((fixture/'prepare-display-driver.ps1').read_bytes()).hexdigest()
  state['display_driver_input']=manifest;save('OWNER.json',state)
 large_file_flags=['-allow-limited-size'] if args.update_manifest else []
 assert name==state['domain'],'VM allocation name changed during fixture preparation'
 controls_input=fixture/'native-controls.ps1'
 if controls_input.is_file():
  frozen=e/'NATIVE_CONTROLS_SOURCE.ps1'
  if frozen.exists():assert frozen.read_bytes()==controls_input.read_bytes()
  else:shutil.copy2(controls_input,frozen)
  save('NATIVE_CONTROLS_INPUT.json',{'sha256':hashlib.sha256(frozen.read_bytes()).hexdigest(),'scope':'Exact owned ISO input frozen before VM allocation; not runtime acceptance'})
 subprocess.run(['genisoimage','-quiet','-udf',*large_file_flags,'-iso-level','3','-J','-joliet-long','-r','-V','OTMSICONTROL','-o',str(fixture/'inputs.iso'),*map(str,iso_inputs)],check=True)
 virsh('vol-create-as','default',iso_volume,str((fixture/'inputs.iso').stat().st_size),'--format','raw');state['volumes'].append(iso_volume);save('OWNER.json',state)
 virsh('vol-upload','--pool','default',iso_volume,str(fixture/'inputs.iso'))
 host_iso=fixture/'inputs.iso';assert host_iso.stat().st_nlink==1
 assert subprocess.run(['fuser',str(host_iso)],capture_output=True).returncode==1
 state['uploaded_host_iso_removed_bytes']=host_iso.stat().st_size;host_iso.unlink();save('OWNER.json',state)
 virsh('vol-create-as','default',volume,'64G','--format','qcow2','--backing-vol',state['base_disk'],'--backing-vol-format','qcow2');state['volumes'].append(volume);save('OWNER.json',state)
 root=E.fromstring(E.tostring(base_template)) if args.office_installed else E.parse(stock/'DOMAIN_TEMPLATE.xml').getroot() if args.use_stock_base else E.fromstring(virsh('dumpxml','win11ltsc'));root.find('name').text=name;root.find('uuid').text=uid
 for tag in ['memory','currentMemory']:
  node=root.find(tag)
  if node is not None:node.text='4194304';node.set('unit','KiB')
 if root.find('genid') is not None:root.find('genid').text=str(uuid.uuid4())
 for backend in root.findall('./devices/tpm/backend'):
  assert backend.get('type')=='emulator'
  for source in list(backend.findall('source')):backend.remove(source)  # libvirt provisions independent state for the new UUID.
 nv=root.find('./os/nvram');nv.text='/var/lib/libvirt/qemu/nvram/'+name+'_VARS.fd'
 disks=root.findall('./devices/disk');assert len(disks)==1;disks[0].find('source').set('file','/var/lib/libvirt/images/'+volume)
 for interface in root.findall('./devices/interface'):root.find('devices').remove(interface)
 for tag in ['filesystem','hostdev','redirdev']:
  for device in root.findall('./devices/'+tag):root.find('devices').remove(device)
 state.update(host_filesystems=0,host_devices=0,usb_redirection=0);save('OWNER.json',state)
 if interactive_case and not root.findall("./devices/input[@type='tablet']"):
  E.SubElement(root.find('devices'),'input',{'type':'tablet','bus':'usb'})
 if interactive_case:
  state['absolute_pointer']='Clone-only virtual tablet present at boot; no host device passthrough'
  save('OWNER.json',state)
 if args.display_resolution:
  video=root.find('./devices/video/model');assert video is not None and video.get('type')=='virtio'
  for existing in list(video.findall('resolution')):video.remove(existing)
  width,height=args.display_resolution.split('x');E.SubElement(video,'resolution',{'x':width,'y':height})
  state['virtual_display_resolution']={'width':int(width),'height':int(height),'scope':'Minimum virtual video resolution for this owned clone only; no physical GPU or base-domain mutation'};save('OWNER.json',state)
 if interactive_case:
  graphics=root.find('./devices/graphics')
  if args.office_installed and graphics.get('type')=='vnc':
   graphics.clear();graphics.set('type','spice');graphics.set('autoport','yes');E.SubElement(graphics,'listen',{'type':'address','address':'127.0.0.1'});E.SubElement(graphics,'image',{'compression':'off'})
  assert graphics.get('type')=='spice'
  for node in list(graphics.findall('gl')):graphics.remove(node)
  acceleration=root.find('./devices/video/model/acceleration')
  if acceleration is not None:acceleration.set('accel3d','no')
  state['graphics_scope']='Owned software-only SPICE desktop; Office exercises declared native WGPU and bundled OpenGL; no physical adapter acceptance' if args.office_installed else 'Owned software-only SPICE desktop; interactive applications use explicit bundled OpenGL; no physical adapter acceptance'
  save('OWNER.json',state)
 cd=E.SubElement(root.find('devices'),'disk',{'type':'file','device':'cdrom'});E.SubElement(cd,'driver',{'name':'qemu','type':'raw'});E.SubElement(cd,'source',{'file':'/var/lib/libvirt/images/'+iso_volume});used={x.get('dev') for x in root.findall('./devices/disk/target')};cd_dev=next(value for value in ['sda','sdb','sdc','sdd'] if value not in used);E.SubElement(cd,'target',{'dev':cd_dev,'bus':'sata'});E.SubElement(cd,'readonly')
 xml=fixture/'domain.xml';E.ElementTree(root).write(xml,encoding='unicode');virsh('define',str(xml));virsh('start',name);state['status']='booting';save('OWNER.json',state)
 spec=importlib.util.spec_from_file_location('guest',r/'.hermes/evidence/windows-20261011/guest_exec.py');g=importlib.util.module_from_spec(spec);spec.loader.exec_module(g);g.DOMAIN=name
 if interactive_case:
  privacy_control=e/'PRIVACY_CONTROL_SOURCE.py';shutil.copy2(e.parent/'privacy-control.py',privacy_control)
  state['privacy_control_sha256']=hashlib.sha256(privacy_control.read_bytes()).hexdigest();save('OWNER.json',state)
  desktop_spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(desktop_spec);desktop_spec.loader.exec_module(desktop)
  private_console=stack.enter_context(desktop.OwnedDesktop(e/'viewer'))
  viewer=private_console.spawn(['virt-viewer','--connect','qemu:///system','--attach','--domain-name','--reconnect','--auto-resize=never','--spice-disable-audio','--spice-disable-usbredir',name],'viewer.log')
  time.sleep(3);state['private_console_viewer_start_exit']=viewer.poll();state['private_console']={'pid':viewer.pid,'display':private_console.env['DISPLAY'],'home':str(private_console.profile/'home')};save('OWNER.json',state)
 deadline=time.monotonic()+180
 while True:
  try:g.qga('guest-ping');break
  except RuntimeError:
   if time.monotonic()>deadline:raise
   time.sleep(2)
 def ps(code,timeout=500):
  code="[Console]::OutputEncoding=$OutputEncoding=[Text.UTF8Encoding]::new($false);"+code
  encoded=base64.b64encode(code.encode('utf-16le')).decode();pid=g.qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-EncodedCommand',encoded],'capture-output':True})['pid'];end=time.monotonic()+timeout
  state['guest_pid']=pid;save('OWNER.json',state)
  while True:
   try:result=g.qga('guest-exec-status',{'pid':pid})
   except RuntimeError as error:
    message=str(error)
    if not any(part in message.lower() for part in ['guest agent is not responding','guest agent not available','guest agent is not connected','timed out','timeout']):raise
    assert virsh('domuuid',name)==uid,'Owned guest identity changed during observation'
    assert virsh('domstate',name)=='running','Owned guest stopped during observation'
    state['guest_observation_interruption']={'pid':pid,'error':message,'process_restarted':False};save('OWNER.json',state);time.sleep(1);continue
   if result.get('exited'):return result
   if time.monotonic()>=end:
    state['guest_observation_window_elapsed']={'pid':pid,'seconds':timeout,'process_restarted':False};save('OWNER.json',state);end=time.monotonic()+timeout
   time.sleep(5 if timeout>180 else 1)
 def reboot_owned_guest():
  # Servicing can restart the agent. Wait for observation before issuing a
  # delayed ordinary Windows reboot, then prove a changed boot identity.
  # Once queued, uncertainty permits observation only, never a duplicate reboot.
  boot_command="(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().Ticks | ConvertTo-Json -Compress"
  def observed_boot():
   result=ps(boot_command,20)
   assert result.get('exitcode')==0,'Cannot inspect owned guest boot identity'
   return json.loads(base64.b64decode(result['out-data']).decode('utf-8-sig'))
  deadline=time.monotonic()+180
  while True:
   try:before=observed_boot();break
   except (RuntimeError,TimeoutError,AssertionError,json.JSONDecodeError):
    assert time.monotonic()<deadline,'Guest agent did not recover before reboot intent';time.sleep(3)
  receipt={'before_boot_ticks':before,'command':'shutdown.exe /r /t 15','scope':'Owned disposable guest only; no forced power reset','status':'intent recorded before single reboot request'}
  number=1+len(list(e.glob('REBOOT_*.json')));filename=f'REBOOT_{number}.json';save(filename,receipt)
  try:
   result=ps(r'& "$env:SystemRoot\System32\shutdown.exe" /r /t 15; if($LASTEXITCODE-ne0){throw "Owned Windows reboot request failed"}',20)
   receipt['request_exit']=result.get('exitcode');receipt['request_stderr']=base64.b64decode(result.get('err-data','')).decode('utf-8',errors='replace');assert result.get('exitcode')==0
  except (RuntimeError,TimeoutError,AssertionError) as error:receipt['request_observation_uncertain']=repr(error)
  save(filename,receipt);deadline=time.monotonic()+900
  while True:
   try:
    after=observed_boot()
    if after!=before:break
   except (RuntimeError,TimeoutError,AssertionError,json.JSONDecodeError):pass
   assert time.monotonic()<deadline,'Owned guest did not prove a changed boot identity after the single reboot request';time.sleep(3)
  receipt.update(status='normal reboot verified',after_boot_ticks=after);save(filename,receipt)
 result=ps("$ErrorActionPreference='Stop';$o=Get-CimInstance Win32_OperatingSystem;@{caption=$o.Caption;build=$o.BuildNumber;msi=(Get-Item \"$env:SystemRoot\\System32\\msi.dll\").VersionInfo.FileVersion;disable_msi=(Get-ItemProperty 'HKLM:\\Software\\Policies\\Microsoft\\Windows\\Installer' -ErrorAction SilentlyContinue).DisableMsi}|ConvertTo-Json -Compress")
 assert result['exitcode']==0;save('CLIENT_VERSION.json',json.loads(base64.b64decode(result['out-data']).decode('utf-8-sig')))
 if args.display_driver:
  code="$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});if($disc.Count-ne1){throw 'Owned driver media missing'};$path=Join-Path ($disc[0].DeviceID+'\\') 'prepare-display-driver.ps1';$source=[IO.File]::ReadAllText($path); & ([ScriptBlock]::Create($source)) -Output 'C:\\OttrinOwnedDisplayDriver' -SourceDirectory (Split-Path $path)"
  installed=ps(code,150);save('DISPLAY_DRIVER_PROCESS.json',installed);assert installed.get('exitcode')==0,'Native signed driver setup failed; inspect exact process result'
  driver_result=json.loads(base64.b64decode(installed.get('out-data','')).decode('utf-8-sig'));save('DISPLAY_DRIVER_PREPARATION.json',driver_result)
 if args.native_transport_probe:
  code="$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});if($disc.Count-ne1){throw 'Owned transport media missing'};$value=Get-Content (Join-Path ($disc[0].DeviceID+'\\') 'transport.json') -Raw -Encoding UTF8|ConvertFrom-Json;$value|ConvertTo-Json -Compress"
  result=ps(code,30);assert result.get('exitcode')==0
  raw=base64.b64decode(result['out-data']);actual=json.loads(raw.decode('utf-8-sig'));assert actual==transport_expected, 'Native UTF8 transport changed Unicode data'
  legacy=ps("[Console]::OutputEncoding=[Text.Encoding]::GetEncoding(437);"+code,30);assert legacy.get('exitcode')==0
  old_raw=base64.b64decode(legacy['out-data']);old_actual=json.loads(old_raw.decode('cp437'));assert old_actual!=transport_expected,'Negative control did not exercise lossy legacy output'
  save('TRANSPORT_ACCEPTANCE.json',{'status':'pass','expected':transport_expected,'actual':actual,'utf8_stdout_sha256':hashlib.sha256(raw).hexdigest(),'legacy_codepage_rejected':True,'legacy_observation':old_actual,'scope':scope});state['status']='native UTF8 roundtrip and legacy negative control pass';save('OWNER.json',state);raise SystemExit(0)
 if args.native_test:
  code="$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});if($disc.Count-ne1){throw 'Owned native test media missing'};$exe=Join-Path ($disc[0].DeviceID+'\\') 'native-test.exe';if((Get-FileHash $exe).Hash.ToLowerInvariant()-ne'"+digest+"'){throw 'Native test input mismatch'};$root=Join-Path $env:TEMP 'ottrin-native-test-"+uid+"';if(Test-Path $root){throw 'Existing native test fixture retained'};New-Item -ItemType Directory $root|Out-Null;$p=Start-Process $exe -ArgumentList @('--exact','"+args.test_filter+"','--nocapture','--test-threads=1') -PassThru -RedirectStandardOutput ($root+'\\stdout.txt') -RedirectStandardError ($root+'\\stderr.txt');$null=$p.Handle;try{if(-not$p.WaitForExit(180000)){$p.Kill();throw 'Owned native test timed out'};$p.WaitForExit();$code=$p.ExitCode;@{identity=@{sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;session=(Get-Process -Id $PID).SessionId};exitcode=$code;stdout=([string](Get-Content ($root+'\\stdout.txt') -Raw -Encoding UTF8));stderr=([string](Get-Content ($root+'\\stderr.txt') -Raw -Encoding UTF8));sha256=(Get-FileHash $exe).Hash}|ConvertTo-Json -Compress;if($code-ne0){exit 1}}finally{if(-not$p.HasExited){$p.Kill();$null=$p.WaitForExit(5000)};$p.Dispose()}"
  if args.native_helper:
   preparation="$helper=Join-Path ($disc[0].DeviceID+'\\') 'native-helper.exe';if((Get-FileHash $helper).Hash.ToLowerInvariant()-ne'"+helper_digest+"'){throw 'Native helper input mismatch'};$helperDir=Join-Path $root 'helper Å path';New-Item -ItemType Directory $helperDir|Out-Null;$env:OTTRIN_QA_PRIV_HELPER=Join-Path $helperDir 'ottrin-priv-helper.exe';Copy-Item $helper $env:OTTRIN_QA_PRIV_HELPER;"
   code=code.replace("$p=Start-Process $exe",preparation+"$p=Start-Process $exe").replace("'--exact',","").replace("'--nocapture','--test-threads=1'","'--nocapture','--test-threads=1','--include-ignored','--skip','windows_helper_pipe_native_uac_round_trip'")
  result=ps(code,210);save('RAW_NATIVE_TEST_PROCESS.json',result);output=base64.b64decode(result.get('out-data','')).decode('utf-8-sig');save('NATIVE_TEST_PROCESS.json',{'process_exit':result.get('exitcode'),'stdout':output,'stderr':base64.b64decode(result.get('err-data','')).decode('utf-8',errors='replace')});assert result.get('exitcode')==0,'Native unit test failed; retain exact output'
  benchmark=json.loads(output);save('NATIVE_TEST.json',benchmark);assert benchmark['exitcode']==0 and ('test result: ok. 5 passed; 0 failed' if args.native_helper else 'test result: ok. 1 passed; 0 failed') in benchmark['stdout'];state['status']='five native helper transport tests pass' if args.native_helper else 'exact selected native unit test passes';save('OWNER.json',state);raise SystemExit(0)
 if args.msix_runtime:
  code="$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});$d=$disc[0].DeviceID+'\\';$script=Join-Path $d 'runtime.ps1';$errors=$null;$tokens=$null;$ast=[Management.Automation.Language.Parser]::ParseFile($script,[ref]$tokens,[ref]$errors);if($errors.Count){throw ($errors|Out-String)};$helpers=@($ast.FindAll({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Exercise-StartupCheckbox'},$true));if($helpers.Count -ne 1){throw 'Expected one startup checkbox helper'};$parent=$helpers[0].Parent;while($parent){if($parent -is [Management.Automation.Language.IfStatementAst] -or $parent -is [Management.Automation.Language.LoopStatementAst] -or $parent -is [Management.Automation.Language.FunctionDefinitionAst]){throw 'Startup checkbox helper must be defined at unconditional script scope'};$parent=$parent.Parent}; & ([ScriptBlock]::Create([IO.File]::ReadAllText($script))) -Mode Prepare -Disc $d"
  result=ps(code,90);save('PREPARATION.json',{'process_exit':result.get('exitcode'),'stdout':base64.b64decode(result.get('out-data','')).decode('utf-8-sig'),'stderr':base64.b64decode(result.get('err-data','')).decode('utf-8',errors='replace')});assert result['exitcode']==0
  reboot_owned_guest();time.sleep(8);deadline=time.monotonic()+1200;state['status']='waiting for owned interactive MSIX deployment/runtime actor';save('OWNER.json',state)
  observer_log=stack.enter_context((e/'readiness.log').open('w'));observer=subprocess.Popen([sys.executable,str(privacy_control),str(e),'finish-msix'],stdout=observer_log,stderr=subprocess.STDOUT,start_new_session=True);stack.callback(desktop.workspace.stop_tests,observer);state['desktop_observer_pid']=observer.pid;save('OWNER.json',state)
  while True:
   try:
    result=ps("$p='C:\\OttrinOwnedMsixRuntime\\results\\RESULT.json';if(Test-Path $p){$s=[IO.File]::Open($p,'Open','Read',([IO.FileShare]::ReadWrite-bor[IO.FileShare]::Delete));$r=[IO.StreamReader]::new($s);try{$r.ReadToEnd()}finally{$r.Dispose()}}else{'{}'}",20)
    if result.get('exitcode')==0:
     runtime=json.loads(base64.b64decode(result.get('out-data','e30=')).decode('utf-8-sig'))
     if runtime.get('status')=='awaiting actual login' and not state.get('login_reboot_requested'):
      state['login_reboot_requested']=True;save('OWNER.json',state);save('BEFORE_LOGIN.json',runtime)
      reboot_owned_guest();state['status']='observing actual Windows login startup before any Ottrin CLI';save('OWNER.json',state)
     elif runtime.get('status') not in [None,'running','awaiting actual login']:break
   except (RuntimeError,TimeoutError,json.JSONDecodeError):pass
   assert time.monotonic()<deadline,'Native MSIX interactive test did not finish'
   time.sleep(3)
  save('RUNTIME_RESULT.json',runtime)
  export_code="$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$zip='C:\\OttrinOwnedMsixRuntimeResults.zip';if(Test-Path $zip){throw 'Unexpected existing owned export'};Compress-Archive 'C:\\OttrinOwnedMsixRuntime\\results\\*' $zip;$p=Get-Item $zip;$m=@{bytes=$p.Length;sha256=(Get-FileHash $zip).Hash.ToLowerInvariant()};[IO.File]::WriteAllText(($zip+'.json'),($m|ConvertTo-Json -Compress),[Text.UTF8Encoding]::new($false));$m|ConvertTo-Json -Compress"
  try:
   result=ps(export_code,30);save('RESULT_EXPORT_PROCESS.json',result);assert result['exitcode']==0
  except RuntimeError as export_error:
   # If guest-exec observation is lost, a completed sidecar is authoritative
   # evidence of this one export. Do not rerun its creation.
   save('EXPORT_OBSERVATION_INTERRUPTION.json',{'pid':state['guest_pid'],'error':repr(export_error),'process_restarted':False})
   preserve_existing_msix_results()
  else:preserve_existing_msix_results()
  state['status']='native MSIX runtime test finished; inspect result, not assumed accepted';save('OWNER.json',state)
  raise SystemExit(0)
 if args.update_manifest or args.msix_parse_only or args.msix_stage:
  result=ps("$ErrorActionPreference='Stop';$disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});if($disc.Count-ne1){throw 'Owned disc missing'};$script=Join-Path ($disc[0].DeviceID+'\\') 'package-msix.ps1';$errors=$null;$tokens=$null;$null=[Management.Automation.Language.Parser]::ParseFile($script,[ref]$tokens,[ref]$errors);if($errors.Count){throw ($errors|Out-String)};[xml]$manifest=Get-Content (Join-Path ($disc[0].DeviceID+'\\') 'msix-manifest.xml') -Raw;@{parser='Windows PowerShell 5.1';syntax_errors=0;source_sha256=(Get-FileHash $script).Hash;manifest_sha256=(Get-FileHash (Join-Path ($disc[0].DeviceID+'\\') 'msix-manifest.xml')).Hash;scope='Native parser and XML load; not package assembly/deployment'}|ConvertTo-Json -Compress",30)
  save('MSIX_NATIVE_PARSE.json',{'process_exit':result.get('exitcode'),'stdout':base64.b64decode(result.get('out-data','')).decode('utf-8-sig'),'stderr':base64.b64decode(result.get('err-data','')).decode('utf-8',errors='replace')});assert result['exitcode']==0
  code="$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});$d=$disc[0].DeviceID+'\\';$root='C:\\OttrinOwnedMsixSdk';if(Test-Path $root){throw 'Existing SDK fixture retained'};New-Item -ItemType Directory $root|Out-Null;Expand-Archive (Join-Path $d 'windows-sdk-buildtools.zip') $root;$tool=Join-Path $root 'bin\\10.0.26100.0\\x64\\makeappx.exe'; & ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $d 'windows-msix-template.ps1')))) -MakeAppx $tool -Template (Join-Path $d 'msix-manifest.xml') -Icon (Join-Path $d 'icon.png')"
  result=ps(code,90);save('SDK_NATIVE_SMOKE.json',{'process_exit':result.get('exitcode'),'stdout':base64.b64decode(result.get('out-data','')).decode('utf-8-sig'),'stderr':base64.b64decode(result.get('err-data','')).decode('utf-8',errors='replace')});assert result['exitcode']==0
  if args.msix_parse_only:
   state['status']='native parser/XML and SDK invocation pass; assembly/deployment remain required';save('OWNER.json',state)
   raise SystemExit(0)
  if args.msix_stage:
   result=ps("$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});$d=$disc[0].DeviceID+'\\'; & ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $d 'assembly.ps1')))) -Disc $d",240)
   save('ASSEMBLY_PROCESS.json',{'process_exit':result.get('exitcode'),'stdout':base64.b64decode(result.get('out-data','')).decode('utf-8-sig'),'stderr':base64.b64decode(result.get('err-data','')).decode('utf-8',errors='replace')});assert result['exitcode']==0,'Actual MSIX assembly failed; retain evidence'
   assembly=json.loads(base64.b64decode(result['out-data']).decode('utf-8-sig'));assert assembly['status']=='pass';save('ASSEMBLY.json',assembly)
   # Transfer only the exact declared generated package, never guest profiles.
   assert assembly['artifact'].startswith('C:\\OttrinOwnedMsixAssembly\\output\\Ottrin-') and assembly['artifact'].endswith('.msix')
   destination=args.msix_stage/Path(assembly['artifact'].replace('\\','/')).name;assert not destination.exists()
   handle=g.qga('guest-file-open',{'path':assembly['artifact'],'mode':'rb'});transferred=False
   try:
    with destination.open('xb') as output:
     while True:
      chunk=g.qga('guest-file-read',{'handle':handle,'count':32768});data=base64.b64decode(chunk.get('buf-b64',''));assert len(data)==chunk['count'];assert data or chunk.get('eof');output.write(data);assert output.tell()<=assembly['artifact_bytes']
      if chunk.get('eof'):break
    assert destination.stat().st_size==assembly['artifact_bytes']
    with destination.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest().upper()==assembly['artifact_sha256']
    transferred=True
   finally:
    try:g.qga('guest-file-close',{'handle':handle})
    finally:
     if not transferred:destination.unlink(missing_ok=True)
   state['status']='actual MSIX assembly and payload pass; candidate returned for separately scoped consumers';save('OWNER.json',state)
   raise SystemExit(0)
 if args.update_manifest:
  state['status']='servicing owned offline clone with supported Microsoft update';save('OWNER.json',state)
  verified=ps("$ErrorActionPreference='Stop';$disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});$d=$disc[0].DeviceID+'\\';$m=Get-Content (Join-Path $d 'update-inputs.json') -Raw|ConvertFrom-Json;$observed=@();foreach($item in $m.files){$p=Join-Path $d $item.filename;$h=(Get-FileHash $p).Hash.ToLowerInvariant();if($h-ne$item.sha256){throw 'Guest UDF update input hash mismatch'};$observed+=@{file=$item.filename;bytes=(Get-Item $p).Length;sha256=$h}};$observed|ConvertTo-Json -Compress",180)
  save('GUEST_UPDATE_INPUTS.json',{'process_exit':verified.get('exitcode'),'stdout':base64.b64decode(verified.get('out-data','')).decode('utf-8-sig'),'stderr':base64.b64decode(verified.get('err-data','')).decode('utf-8',errors='replace')});assert verified['exitcode']==0
  target=next(item for item in inputs['files'] if 'kb5127648' in item['filename'])
  code="$ErrorActionPreference='Stop';$disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});if($disc.Count-ne1){throw 'Owned update disc missing'};$msu=Join-Path ($disc[0].DeviceID+'\\') '"+target['filename']+"';$p=Start-Process wusa.exe -ArgumentList ('\"'+$msu+'\" /quiet /norestart') -PassThru;$null=$p.Handle;if(-not$p.WaitForExit(3600000)){$p.Kill();throw 'Owned update timed out after60minutes'};$p.WaitForExit();$code=$p.ExitCode;@{exitcode=$code;update='KB5127648';scope='Only disposable network-isolated clone; WUSA validates update applicability and trust'}|ConvertTo-Json -Compress;if($code-notin@(0,3010,2359302)){exit 1}"
  result=ps(code,3700);text=base64.b64decode(result.get('out-data','')).decode('utf-8-sig');save('SERVICING.json',{'process_exit':result.get('exitcode'),'output':text,'stderr':base64.b64decode(result.get('err-data','')).decode('utf-8',errors='replace')});assert result['exitcode']==0,'Supported update failed; no policy or signature bypass'
  state['servicing_completed']=True;save('OWNER.json',state)
  reboot_owned_guest();time.sleep(8);deadline=time.monotonic()+900
  while True:
   try:
    result=ps("$v=Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion';@{build=$v.CurrentBuildNumber;ubr=$v.UBR;msi=(Get-Item \"$env:SystemRoot\\System32\\msi.dll\").VersionInfo.FileVersion}|ConvertTo-Json -Compress",20)
    if result.get('exitcode')==0:
     version=json.loads(base64.b64decode(result['out-data']).decode('utf-8-sig'))
     if version['build']=='26100' and version['ubr']==9711:break
   except (RuntimeError,TimeoutError,json.JSONDecodeError):pass
   assert time.monotonic()<deadline,'Updated clone did not reach exact target build'
   time.sleep(3)
  save('UPDATED_CLIENT_VERSION.json',version)
  if args.prepare_updated_base:
   virsh('shutdown',name,'--mode','agent');deadline=time.monotonic()+600
   while virsh('domstate',name)!='shut off':
    assert time.monotonic()<deadline,'Updated fixture did not shut down cleanly';time.sleep(2)
   state['retained_pending_volume']=volume;state['status']='updated baseline stopped cleanly; retained for named native consumers'
   save('UPDATED_BASE.json',{'status':'ready for named pending native consumers','path':'/var/lib/libvirt/images/'+volume,'volume':volume,'backing_original':'/var/lib/libvirt/images/win11ltsc.qcow2','verified_version':version,'clean_agent_shutdown':True,'pending_consumers':['Current-client minimal and actual Ottrin MSI rollback/lifecycle','MSIX startup/update/removal acceptance'],'cleanup_owner':'windows-msi-updated-client-current; delete only after consumers close and domain/backing-chain checks pass'})
   save('OWNER.json',state);raise SystemExit(0)
 state['status']='running diagnostic elevated native control' if args.native_elevated else 'running ordinary-user native control';save('OWNER.json',state)
 code="$ErrorActionPreference='Stop';$disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});if($disc.Count-ne1){throw 'Owned fixture disc missing'};$script=Join-Path ($disc[0].DeviceID+'\\') 'controller.ps1';$out='C:\\OttrinMsiCleanClientResults';$source=[IO.File]::ReadAllText($script);$source=$source.Replace('$PSScriptRoot',\"'\"+(Split-Path $script)+\"'\"); & ([ScriptBlock]::Create($source)) -Output $out"
 result=ps(code);save('CONTROLLER_PROCESS.json',{'exitcode':result.get('exitcode'),'stdout':base64.b64decode(result.get('out-data','')).decode('utf-8',errors='replace'),'stderr':base64.b64decode(result.get('err-data','')).decode('utf-8',errors='replace')})
 assert result['exitcode']==0,'Interactive preparation failed; preserve controller evidence'
 prepared=ps("Get-Content 'C:\\OttrinMsiCleanClientResults\\OWNER.json' -Raw",30)
 owner=json.loads(base64.b64decode(prepared['out-data']).decode('utf-8-sig'));assert owner['status']=='prepared';save('GUEST_PREPARATION.json',owner)
 reboot_owned_guest();state['status']='waiting for owned interactive actor after reboot';save('OWNER.json',state)
 if args.application_uia_stage or args.application_nsis_stage or args.application_native_test:
  # Observe the fresh owned desktop promptly; actor ownership/nonce checks stay
  # unchanged. Register cancellation before the observer can allocate work.
  observer_log=stack.enter_context((e/'readiness.log').open('w'))
  observer=subprocess.Popen([sys.executable,str(privacy_control),str(e),'finish-application'],stdout=observer_log,stderr=subprocess.STDOUT,start_new_session=True)
  stack.callback(desktop.workspace.stop_tests,observer)
  state['desktop_observer_pid']=observer.pid;save('OWNER.json',state)
 if args.office_installed:
  state['desktop_readiness']='Private software console with clone-only absolute USB tablet; known-page observer selects No and sends ownership-checked readiness only after normal desktop. Native visual recovery remains required on an unrecognized page.';save('OWNER.json',state)
 time.sleep(8);deadline=time.monotonic()+1200
 while True:
  try:
   result=ps("$p='"+owner['root']+"\\results\\RESULT.json';if(Test-Path $p){$s=[IO.File]::Open($p,'Open','Read',([IO.FileShare]::ReadWrite-bor[IO.FileShare]::Delete));$r=[IO.StreamReader]::new($s);try{$r.ReadToEnd()}finally{$r.Dispose()}}else{'{}'}",20)
   if result.get('exitcode')==0:
    actor_result=json.loads(base64.b64decode(result.get('out-data','e30=')).decode('utf-8-sig'))
    observation={k:actor_result[k] for k in ['status','phase','error'] if k in actor_result}
    if isinstance(actor_result.get('runs'),list):observation['runs']=[{k:run[k] for k in ['case','threads','run','exit','elapsed_ms','timed_out','all_ten_pass'] if k in run} for run in actor_result['runs']]
    if observation and observation!=state.get('actor_observation'):
     state['actor_observation']=observation;save('OWNER.json',state)
    if actor_result.get('status') not in [None,'running']:break
  except (RuntimeError,TimeoutError,json.JSONDecodeError):pass
  assert time.monotonic()<deadline,'Interactive actor did not finish'
  time.sleep(3)
 save('GUEST_RESULT.json',actor_result)
 result=ps("$ErrorActionPreference='Stop';$owner=Get-Content 'C:\\OttrinMsiCleanClientResults\\OWNER.json' -Raw|ConvertFrom-Json;Copy-Item ($owner.root+'\\results\\*') 'C:\\OttrinMsiCleanClientResults' -Recurse;Compress-Archive 'C:\\OttrinMsiCleanClientResults\\*' 'C:\\OttrinMsiCleanClientResults.zip';$p=Get-Item 'C:\\OttrinMsiCleanClientResults.zip';@{bytes=$p.Length;sha256=(Get-FileHash $p.FullName).Hash.ToLowerInvariant()}|ConvertTo-Json -Compress",30)
 save('EXPORT_PREPARATION.json',result)
 assert result['exitcode']==0,'Owned receipt export failed; inspect EXPORT_PREPARATION.json'
 archive=json.loads(base64.b64decode(result['out-data']).decode('utf-8-sig'));assert archive['bytes']>0
 transfer_owned_guest_file(r'C:\OttrinMsiCleanClientResults.zip',e/'RESULTS.zip',archive,'ARCHIVE_TRANSFER.json')
 state['status']='control finished; inspect exact result, not assumed pass';save('OWNER.json',state)
print('Clean client control and owned overlay teardown finished.',flush=True)
