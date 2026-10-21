from pathlib import Path
import json,hashlib,os,shutil,subprocess
tool_dir=Path(__file__).resolve().parent
p=Path(os.environ.get("OTTRIN_WINDOWS_PACKAGE_EVIDENCE_DIR",str(tool_dir))).resolve()
assert p.parent==tool_dir.parent,"Evidence must have a workspace-owned family"
o=json.loads((p/'OWNERSHIP.json').read_text());stage=Path(o['stage']);candidate=Path(o['candidate']);backup=candidate.with_name('.windows-activity-previous');sha=lambda f:hashlib.sha256(f.read_bytes()).hexdigest()
assert json.loads((p/'TEARDOWN.json').read_text())['status']=='pass'
checks=json.loads((p/'PACKAGE_IDENTITIES.json').read_text());assert checks['status']=='pass' and checks['source_commit']==o['source_commit']
assert json.loads((p/'CLI_SMOKE.json').read_text(encoding='utf-8-sig'))['status']=='pass'
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==o['source_commit']
baseline=json.loads((p/'HOST_BASELINE.json').read_text());assert all(sha(Path(f))==h for f,h in baseline.items())
assert not stage.is_symlink() and not candidate.is_symlink() and not backup.exists()
consumers=[]
for proc in Path('/proc').iterdir():
 if not proc.name.isdigit():continue
 try:links=[proc/'cwd',*(proc/'fd').iterdir()]
 except (FileNotFoundError,PermissionError,ProcessLookupError):continue
 for link in links:
  try:target=os.readlink(link)
  except (FileNotFoundError,PermissionError,ProcessLookupError,OSError):continue
  if target==str(candidate) or target.startswith(str(candidate)+'/'):consumers.append({'pid':proc.name,'path':target})
assert not consumers,consumers
removed=[]
for name in {f.name for f in candidate.iterdir()}-{f.name for f in stage.iterdir()}:
 f=candidate/name
 assert name.startswith('.windows-portable.') and f.is_dir() and not f.is_symlink() and not list(f.iterdir())
 stat=f.stat();assert stat.st_uid==os.getuid() and stat.st_mtime<(p/'OWNERSHIP.json').stat().st_mtime
 removed.append({'path':str(f),'device':stat.st_dev,'inode':stat.st_ino,'reason':'Empty obsolete work directory from the portable builder; no accessible live fd/cwd consumer'})
(p/'REMOVED_EMPTY_WORK.json').write_text(json.dumps(removed,indent=2)+'\n')
for item in removed:Path(item['path']).rmdir()
assert {f.name for f in stage.iterdir()}=={f.name for f in candidate.iterdir()}
delta=json.loads((p/'PAYLOAD_DELTA.json').read_text());oldpayload=candidate/'windows-payload/Ottrin-0.1.0-windows-x64'
assert {f.relative_to(oldpayload).as_posix() for f in oldpayload.rglob('*') if f.is_file()}==set(delta)
assert all(sha(oldpayload/f)==data['old_sha256'] for f,data in delta.items())
assert not any(f.is_symlink() for f in candidate.rglob('*')) and not any(f.is_symlink() for f in stage.rglob('*'))
previous={'source_note':(oldpayload/'SOURCE_REVISION.txt').read_text(),'artifacts':{Path(f).name:h for f,h in baseline.items() if Path(f).parent.resolve()==candidate and not f.endswith('.sha256')},'scope':'Superseded generated internal candidate removed only after replacement verification; public Windows files preserved'}
(p/'PREVIOUS_CANDIDATE.json').write_text(json.dumps(previous,indent=2)+'\n')
(p/'PROMOTION_PLAN.json').write_text(json.dumps({'stage':str(stage),'candidate':str(candidate),'rollback':str(backup),'replacement_artifacts':checks['artifacts']},indent=2)+'\n')
candidate.rename(backup)
try:
 stage.rename(candidate)
 assert all(sha(candidate/name)==digest for name,digest in checks['artifacts'].items())
 payload=candidate/'windows-payload/Ottrin-0.1.0-windows-x64'
 assert all(sha(payload/f)==data['new_sha256'] for f,data in delta.items())
except BaseException:
 if candidate.exists():candidate.rename(stage)
 backup.rename(candidate)
 raise
shutil.rmtree(backup)
public={f:h for f,h in baseline.items() if Path(f).parent.resolve()!=candidate};assert all(sha(Path(f))==h for f,h in public.items())
assert not stage.exists() and not backup.exists()
(p/'boot.ppm').unlink(missing_ok=True)
receipt={'status':'pass','candidate':str(candidate),'source_commit':o['source_commit'],'artifacts':checks['artifacts'],'payload_files':checks['payload_files'],'previous_generated_candidate_removed':True,'removed_empty_previous_work_dirs':removed,'task_staging_directory_absent':True,'checked_accessible_host_fd_and_cwd_consumers':consumers,'public_windows_files_unchanged':True,'native_guest_teardown':'TEARDOWN.json'}
(p/'PROMOTION.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt,indent=2))
