from pathlib import Path
import argparse,gzip,hashlib,json,re,secrets,shutil,struct,subprocess,sys,tempfile
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p));import transport as n

parser=argparse.ArgumentParser();selection=parser.add_mutually_exclusive_group();selection.add_argument('--destination-admission',action='store_true');selection.add_argument('--cleanup-non-group',action='store_true');selection.add_argument('--cleanup-group',action='store_true');selection.add_argument('--configured-actions',action='store_true');selection.add_argument('--group-exit',action='store_true');selection.add_argument('--group-mixed',action='store_true');selection.add_argument('--full',action='store_true');selection.add_argument('--group-observation',action='store_true');selection.add_argument('--group-start',action='store_true');selection.add_argument('--group-shared-prepare',action='store_true');selection.add_argument('--group-shared-unlink',action='store_true');selection.add_argument('--group-payload',action='store_true');selection.add_argument('--group-metadata',action='store_true');selection.add_argument('--peer-unlink-codec',action='store_true');selection.add_argument('--peer-binding',action='store_true');selection.add_argument('--peer-ack',action='store_true');selection.add_argument('--journal-core',action='store_true');selection.add_argument('--copy-journal',action='store_true');selection.add_argument('--copy-dispatch',action='store_true');selection.add_argument('--copy-registry',action='store_true');selection.add_argument('--copy-claim-binding',action='store_true');selection.add_argument('--claim-ledger',action='store_true');selection.add_argument('--journal-list',action='store_true');selection.add_argument('--journal-remaining',action='store_true');selection.add_argument('--symlink-policy',action='store_true');selection.add_argument('--exact',choices=['copy_transaction::windows::journal::tests::claim_processes::windows_native_staged_copy_claims_exclude_processes_and_survive_hard_exit','copy_transaction::windows::journal::tests::claim_publication::windows_native_copy_prepared_claim_publishers_race_without_overwrite','copy_transaction::windows::journal::tests::claim_releases::windows_native_copy_claim_release_keeps_terminal_journals_leased_during_undo_retry','copy_transaction::windows::journal::tests::windows_native_copy_claim_survives_stage_drop_and_reuses_a_released_generation','copy_transaction::windows::dispatch::tests::windows_native_copy_dispatch_preserves_an_outstanding_undo_destination_claim','copy_transaction::windows::journal::tests::windows_native_destination_admission_respects_existing_target_case_aliases','copy_transaction::windows::journal::tests::windows_native_copy_discovery_resolves_parent_on_an_owned_other_volume','copy_transaction::windows::journal::tests::windows_native_copy_directory_case_policy_preserves_mixed_trees_and_rejects_drift','tests::windows_copy_recovery_activity_integration','tests::windows_retained_copy_activity_keeps_evidence_and_actions_separate','tests::operation_admission_preserves_files_during_undo_and_releases_after_completion','tests::queued_operation_footprints_cover_delete_permissions_batches_and_rename_targets','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_start_barrier_releases_only_relinquished_publication_claims','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_unrecognized_registry_names_stay_visible_and_block_mutation','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_configured_registry_writer_supports_registry_destinations','copy_transaction::windows::security::tests::new_copy_candidate_inherits_the_real_parent_mandatory_label','copy_transaction::windows::links::tests::windows_native_copy_link_capture_and_install_preserves_relative_absolute_and_dangling_links']);parser.add_argument('--watchdog-ms',type=int);parser.add_argument('--crate',choices=['ottrin_platform','ottrin_ui'],default='ottrin_platform');parser.add_argument('--evidence-dir',type=Path);parser.add_argument('--reuse-binary',action='store_true');options=parser.parse_args();assert (options.crate=='ottrin_ui')==(options.exact in ['tests::windows_retained_copy_activity_keeps_evidence_and_actions_separate','tests::operation_admission_preserves_files_during_undo_and_releases_after_completion','tests::queued_operation_footprints_cover_delete_permissions_batches_and_rename_targets','tests::windows_copy_recovery_activity_integration'])
if options.evidence_dir:
 p=options.evidence_dir.resolve();p.mkdir(parents=True,exist_ok=True)

watchdog_ms=options.watchdog_ms if options.watchdog_ms is not None else (900000 if options.full or options.copy_journal or options.journal_remaining or options.journal_core else 300000 if options.group_exit or options.group_mixed or options.group_shared_unlink or options.group_shared_prepare or options.peer_ack or options.peer_binding or options.peer_unlink_codec or options.group_metadata or options.group_payload or options.group_start or options.group_observation or options.exact or options.symlink_policy else 600000)
assert 0<watchdog_ms<=900000
arguments=('windows_native_copy_cleanup --skip windows_native_copy_cleanup_group --nocapture --test-threads=1') if options.cleanup_non_group else ('windows_native_copy_cleanup_configured --nocapture --test-threads=1') if options.configured_actions else ('windows_native_copy_cleanup_group_process_exit --nocapture --test-threads=1') if options.group_exit else ('windows_native_copy_cleanup_group_mixed --nocapture --test-threads=1') if options.group_mixed else ('windows_native_copy_cleanup_group_shared_unlink --nocapture --test-threads=1') if options.group_shared_unlink else ('windows_native_copy_cleanup_group_shared_prepare --nocapture --test-threads=1') if options.group_shared_prepare else ('windows_native_copy_cleanup_peer_acknowledgement --nocapture --test-threads=1') if options.peer_ack else ('copy_transaction::windows::journal:: --skip copy_transaction::windows::journal::tests::cleanup:: --nocapture --test-threads=1') if options.journal_core else ('windows_native_copy_cleanup_peer_binding --nocapture --test-threads=1') if options.peer_binding else ('copy_transaction::windows::journal:: --nocapture --test-threads=1') if options.copy_journal else ('windows_native_copy_cleanup_peer_unlink_codec --nocapture --test-threads=1') if options.peer_unlink_codec else ('windows_native_copy_cleanup_group_metadata --nocapture --test-threads=1') if options.group_metadata else ('windows_native_copy_cleanup_group_payload --nocapture --test-threads=1') if options.group_payload else ('windows_native_copy_cleanup_group_start --nocapture --test-threads=1') if options.group_start else ('windows_native_copy_cleanup_group_observation --nocapture --test-threads=1') if options.group_observation else ('--exact '+options.exact+' --nocapture --test-threads=1') if options.exact else '--exact copy_transaction::windows::links::tests::windows_native_copy_symlink_policy_denial_retains_private_candidate --ignored --nocapture --test-threads=1' if options.symlink_policy else ('--nocapture --test-threads=1' if options.full else 'windows_native_copy_cleanup --nocapture --test-threads=1')

if options.copy_claim_binding:
 arguments='copy_transaction::windows::dispatch:: copy_transaction::windows::journal::tests::windows_native_copy_claim_ --nocapture --test-threads=1'

if options.claim_ledger:
 arguments='copy_transaction::windows::dispatch:: copy_transaction::windows::journal::tests::windows_native_copy_claim_ copy_transaction::windows::journal::claims:: windows_native_copy_cleanup_start_ windows_native_copy_cleanup_unpaired_start_ windows_native_copy_cleanup_configured_group_action_ windows_native_copy_cleanup_group_start_uncertain_member_never_refreshes_by_id copy_transaction::windows::journal::tests::claim_processes::windows_native_staged_copy_claims_exclude_processes_and_survive_hard_exit copy_transaction::windows::journal::tests::claim_releases:: copy_transaction::windows::journal::tests::name_release_action:: copy_transaction::windows::journal::tests::claim_retirement:: copy_transaction::windows::journal::tests::claim_orphans:: copy_transaction::windows::journal::tests::claim_repair:: copy_transaction::windows::journal::tests::claim_publication:: --nocapture --test-threads=1'

if options.copy_registry:
 arguments='copy_transaction::windows::registry:: --nocapture --test-threads=1'

if options.copy_dispatch:
 arguments='copy_transaction::windows::dispatch:: --nocapture --test-threads=1'

if options.destination_admission:
 arguments='windows_native_destination_admission_ --nocapture --test-threads=1'
if options.cleanup_group:
 arguments='windows_native_copy_cleanup_group --nocapture --test-threads=1'

if options.exact=='tests::operation_admission_preserves_files_during_undo_and_releases_after_completion':
 arguments += ' tests::queued_operation_footprints_cover_delete_permissions_batches_and_rename_targets tests::independent_copy_reads_can_run_together_but_exclude_writers tests::dirty_quick_inspector_native_close_request_is_cancelled tests::dirty_quick_inspector_text_edits_require_an_explicit_close_or_cancel tests::quick_inspector_text_save_conflict_keeps_the_editor_buffer tests::unchanged_quick_inspector_text_save_does_not_create_an_undo_operation tests::quick_inspector_text_save_is_session_undoable operation_paths::tests::indexed_claims_match_component_prefix_reference operation_paths::tests::large_independent_batches_preserve_read_and_write_exclusion'

if options.exact=='tests::windows_copy_recovery_activity_integration':
 arguments += ' --ignored'

if options.exact=='copy_transaction::windows::journal::tests::windows_native_copy_discovery_resolves_parent_on_an_owned_other_volume':arguments += ' --ignored'

if options.journal_list:arguments='copy_transaction::windows::journal:: --list'
if options.journal_remaining:
 progress=json.loads((p/'PEER_UNLINK_CODEC_COMPLETED_BEFORE_TIMEOUT.json').read_text())
 skip=progress['completed']+progress['ignored']
 expected=json.loads((p/'PEER_UNLINK_CODEC_JOURNAL_LIST.json').read_text())
 remaining=set(expected)-set(skip)
 assert remaining and progress['interrupted'] in remaining
 # Full names are substring filters in libtest; never skip a remaining name
 # merely because it extends a completed name.
 skip=[name for name in skip if not any(name in candidate for candidate in remaining)]
 assert all(re.fullmatch(r'copy_transaction::windows::journal::[A-Za-z0-9_:]+',name) for name in skip)
 arguments='copy_transaction::windows::journal:: --nocapture --test-threads=1 '+''.join(' --skip '+name for name in skip)
 assert len(arguments)<28000
 (p/'PEER_UNLINK_CODEC_REMAINING_SELECTION.json').write_text(json.dumps({'skip':skip,'remaining':sorted(remaining)},indent=2)+'\n')

def sections(path):
 data=path.read_bytes();header=struct.unpack_from('<I',data,0x3c)[0];assert data[header:header+4]==b'PE\0\0'
 count=struct.unpack_from('<H',data,header+6)[0];optional=struct.unpack_from('<H',data,header+20)[0]
 symbols,number=struct.unpack_from('<II',data,header+12);strings=symbols+number*18;result={}
 for index in range(count):
  at=header+24+optional+index*40;name=data[at:at+8].split(b'\0')[0]
  if name.startswith(b'/'):name=data[strings+int(name[1:]):].split(b'\0')[0]
  name=name.decode();size,offset=struct.unpack_from('<II',data,at+16)
  if not name.startswith('.debug'):result[name]=hashlib.sha256(data[offset:offset+size]).hexdigest()
 return result

pid=n.start(r"$t=Get-ScheduledTask 'Ottrin Copy Cleanup QA';if($t.State.ToString()-in@('Running','Queued')){throw 'Owned actor still live'};if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned native test process remains'}")
(p/'preflight.pid').write_text(str(pid));n.wait(pid,45)
rows=[json.loads(line) for line in (p/'build.jsonl').read_text().splitlines()]
source=Path(next(x['executable'] for x in rows if x.get('reason')=='compiler-artifact' and x.get('executable') and x.get('profile',{}).get('test') and x.get('target',{}).get('name')==options.crate))
assert rows[-1]['reason']=='build-finished' and rows[-1]['success']
with tempfile.TemporaryDirectory(prefix='windows-copy-cleanup-',dir='.hermes/packaging-tmp') as temporary:
 exe=Path(temporary)/'platform.exe';shutil.copy2(source,exe)
 subprocess.run(['x86_64-w64-mingw32-strip','--preserve-dates','--strip-debug',str(exe)],check=True)
 assert sections(exe)==sections(source)
 digest=hashlib.sha256(exe.read_bytes()).hexdigest()
 (p/'TEST_BINARY.json').write_text(json.dumps({'test_crate':options.crate,'test_arguments':arguments,'watchdog_ms':watchdog_ms,'compiler_profile':next(x['profile'] for x in rows if x.get('executable')==str(source)),'compiler_path':str(source),'compiler_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'transferred_sha256':digest,'retained_sections':sections(exe)},indent=2)+'\n')
 if options.reuse_binary:
  pid=n.start(r"if((Get-FileHash C:\OttrinCopyCleanupQA\platform.exe).Hash.ToLowerInvariant()-ne'HASH'){throw 'Native reused test hash mismatch'}".replace('HASH',digest));n.wait(pid,45)
 else:
  archive=Path(temporary)/'platform.gz'
  with exe.open('rb') as src,gzip.open(archive,'wb',compresslevel=1) as dest:shutil.copyfileobj(src,dest)
  n.upload(archive,'platform.gz')
  pid=n.start(r'''$s=[IO.File]::OpenRead('C:\OttrinCopyCleanupQA\platform.gz');$z=[IO.Compression.GZipStream]::new($s,[IO.Compression.CompressionMode]::Decompress);$d=[IO.File]::Create('C:\OttrinCopyCleanupQA\platform.exe');try{$z.CopyTo($d)}finally{$d.Dispose();$z.Dispose();$s.Dispose()};Remove-Item C:\OttrinCopyCleanupQA\platform.gz;if((Get-FileHash C:\OttrinCopyCleanupQA\platform.exe).Hash.ToLowerInvariant()-ne'HASH'){throw 'Native test hash mismatch'}'''.replace('HASH',digest))
  (p/'transfer.pid').write_text(str(pid));n.wait(pid,45)
dispatch={'id':secrets.token_hex(16),'status':'preparing','test_arguments':arguments,'transferred_sha256':digest,'watchdog_ms':watchdog_ms}
(p/'DISPATCH.json').write_text(json.dumps(dispatch,indent=2)+'\n')
for name in ['RESULTS.json','platform.stdout','platform.stderr']:(p/name).unlink(missing_ok=True)
actor=p/'actor.ps1'
actor.write_text(r'''$ErrorActionPreference='Stop';$result=@{status='running';dispatch_id='DISPATCH_TOKEN';test_arguments='TEST_ARGUMENTS'};$process=$null;$temp='C:\OttrinCopyCleanupQA\temp'
try {
 if($env:USERNAME-ne'OttrinCopyQA'){throw 'Wrong test account'}
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent();if(([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Elevated test account'}
 $result.ordinary_user=$true;$result.test_binary_hash=(Get-FileHash C:\OttrinCopyCleanupQA\platform.exe).Hash.ToLowerInvariant();if($result.test_binary_hash-ne'EXPECTED_HASH'){throw 'Actor binary hash mismatch'}
 if(Test-Path $temp){throw 'Owned temporary namespace exists'};New-Item -ItemType Directory $temp|Out-Null;$acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetSecurityDescriptorSddlForm('D:P(A;OICI;FA;;;OW)',[Security.AccessControl.AccessControlSections]::Access);Set-Acl -LiteralPath $temp -AclObject $acl;$env:TEMP=$temp;$env:TMP=$temp
 $process=Start-Process C:\OttrinCopyCleanupQA\platform.exe -ArgumentList 'TEST_ARGUMENTS' -WorkingDirectory C:\Windows\System32 -PassThru -RedirectStandardOutput C:\OttrinCopyCleanupQA\results\platform.stdout -RedirectStandardError C:\OttrinCopyCleanupQA\results\platform.stderr
 $null=$process.Handle;if(-not$process.WaitForExit(TEST_DEADLINE)){throw "Owned test remains live: $($process.Id)"}
 $result.exit=$process.ExitCode;if($result.exit-ne0){throw 'Native cleanup observation failed'}
 $result.temporary_entries=@(Get-ChildItem -LiteralPath $temp -Force).Count;if($result.temporary_entries){throw 'Tests left temporary entries'}
 $result.status='pass'
} catch {$result.status='fail';$result.error=$_.Exception.Message}
finally {
 try {
  if($process-and-not$process.HasExited){
   & C:\Windows\System32\taskkill.exe /PID $process.Id /T /F|Out-Null
   if(-not$process.WaitForExit(10000)){throw 'Owned native test did not stop'}
  }
  if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned native processes remain during teardown'}
  if(Test-Path $temp){
   for($attempt=0;$attempt-lt5;$attempt++){
    try {Remove-Item -LiteralPath $temp -Recurse -Force;break}
    catch {if($attempt-eq4){throw};Start-Sleep -Milliseconds 200}
   }
  }
 } catch {$result.status='fail';$result.teardown_error=$_.ToString()}
 finally {
  try {if($process){foreach($name in @('stdout','stderr')){$path='C:\OttrinCopyCleanupQA\results\platform.'+$name;if(Test-Path $path){$result[$name+'_sha256']=(Get-FileHash $path).Hash.ToLowerInvariant()}}}} catch {$result.status='fail';$result.artifact_error=$_.Exception.Message}
  $result|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\results\RESULTS.json.tmp
  Move-Item -LiteralPath C:\OttrinCopyCleanupQA\results\RESULTS.json.tmp -Destination C:\OttrinCopyCleanupQA\results\RESULTS.json -Force
 }
}
if($result.status-ne'pass'){exit 1}
'''.replace('TEST_ARGUMENTS',arguments).replace('TEST_DEADLINE',str(watchdog_ms)).replace('DISPATCH_TOKEN',dispatch['id']).replace('EXPECTED_HASH',digest))
if options.exact=='tests::windows_copy_recovery_activity_integration':
 actor.write_text(actor.read_text().replace('$process=Start-Process',"$env:OTTRIN_TEST_WINDOWS_PLATFORM_FIXTURE_EXE='C:\\OttrinCopyCleanupQA\\platform-fixture.exe';$process=Start-Process"))
if options.exact=='copy_transaction::windows::journal::tests::windows_native_copy_discovery_resolves_parent_on_an_owned_other_volume':
 actor.write_text(actor.read_text().replace('$process=Start-Process',r"$v=Get-Content C:\OttrinCopyCleanupQA\SECONDARY_VOLUME.json -Raw|ConvertFrom-Json;$volume=Get-Volume -DriveLetter $v.drive_letter;if($volume.FileSystem-ne'NTFS'-or$volume.UniqueId-ne$v.volume_unique_id-or$volume.FileSystemLabel-ne'OttrinCopyQA'-or([Security.Principal.WindowsIdentity]::GetCurrent().User.Value-ne$v.actor_sid)-or((Get-Content -LiteralPath (Join-Path $v.root '.ottrin-owned-secondary-volume') -Raw).Trim()-ne$v.token)){throw 'Owned secondary volume identity guard failed'};$env:OTTRIN_TEST_WINDOWS_COPY_DISCOVERY_VOLUME=$v.root;$process=Start-Process"))
n.upload(actor,'actor.ps1')
pid=n.start(r"$t=Get-ScheduledTask 'Ottrin Copy Cleanup QA';if($t.State.ToString()-in@('Running','Queued')){throw 'Owned actor still live'};if([int]$t.Principal.RunLevel-ne0){throw 'Actor must remain limited'};foreach($path in @('C:\OttrinCopyCleanupQA\results\RESULTS.json','C:\OttrinCopyCleanupQA\results\RESULTS.json.tmp')){if(Test-Path $path){Remove-Item -LiteralPath $path -Force}};Start-ScheduledTask $t.TaskName")
(p/'dispatch.pid').write_text(str(pid));n.wait(pid,45)
dispatch['status']='dispatched';(p/'DISPATCH.json').write_text(json.dumps(dispatch,indent=2)+'\n')
print('Native cleanup observation dispatched',flush=True)
