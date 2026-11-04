"""Guarded retirement of superseded Ottrin index-daemon build executables only."""
from pathlib import Path
import fcntl,hashlib,importlib.util,json,os,shutil,subprocess,sys,tempfile
sys.dont_write_bytecode=True
root=Path.cwd();rows=json.loads(Path('/tmp/ottrin-superseded-indexd-review.json').read_text());assert len(rows)==28
assert not Path('/tmp/ottrin-superseded-indexd-references.txt').read_text().strip(),'Current launcher/producer source references need review'
paths={Path(row['path']).resolve() for row in rows}
for row in rows:
 p=Path(row['path']);replacement=Path(row['replacement']);assert p.stat().st_nlink==1 and replacement.stat().st_mtime_ns>p.stat().st_mtime_ns
 assert Path(row['newer_complete_fingerprint']).is_file()
 final=(root/'target/debug/ottrin-indexd') if p.suffix!='.exe' else (root/'target/x86_64-pc-windows-gnu/debug/ottrin-indexd.exe')
 assert os.path.samefile(final,replacement),'Newest replacement must be the current stable target binary'
 for candidate in [p,replacement]:
  with candidate.open('rb') as stream:header=stream.read(4)
  assert header.startswith(b'\x7fELF') or header.startswith(b'MZ')
 assert subprocess.run(['fuser',str(p)],capture_output=True).returncode==1
# Incoming links are checked separately from process references and hard links.
for base in [root,Path('/home/campbell/.local/bin'),Path('/home/campbell/.local/share/applications'),Path('/home/campbell/.config/systemd/user'),Path('/home/campbell/.config/autostart')]:
 output=subprocess.check_output(['find',str(base),'-xdev','-type','l','-print0'])
 for raw in output.split(b'\0'):
  if not raw:continue
  p=Path(os.fsdecode(raw))
  try:target=p.resolve()
  except (OSError,RuntimeError):continue
  assert target not in paths,'Incoming symbolic link to candidate: '+str(p)
canonical=Path('/home/campbell/.local/state/computer-cleanup');lock=canonical/'run.lock';assert lock.is_file() and not lock.is_symlink()
with lock.open('r+') as held:
 fcntl.flock(held,fcntl.LOCK_EX|fcntl.LOCK_NB)
 spec=importlib.util.spec_from_file_location('cleanup','/home/campbell/.codex/skills/computer-cleanup/scripts/cleanup.py');c=importlib.util.module_from_spec(spec);spec.loader.exec_module(c)
 # Root has no allocatable space. Store the same guarded plan/ledger on tmpfs,
 # then persist compact receipts to the canonical state after space is freed.
 temp=Path(tempfile.mkdtemp(prefix='ottrin-space-recovery-',dir='/tmp'));c.STATE=temp
 reviews=json.loads((root/'.hermes/evidence/theme-breadcrumb-current/INCREMENTAL_REVIEW.json').read_text())['process_reviews']
 for review in reviews:assert all(review[k]==v for k,v in c.process_identity(review['pid']).items())
 for proc in Path('/proc').iterdir():
  if proc.name.isdigit():
   try:comm=(proc/'comm').read_text().strip()
   except (OSError,ProcessLookupError):continue
   assert comm not in {'cargo','rustc','rust-analyzer'},'Current compiler consumer must finish first'
 entries=[]
 for row in rows:
  reason='Superseded generated index-daemon executable with a newer complete same-compiler/target/profile/features/source-path/rustflags/config/compile-kind replacement hard-linked to the current stable target binary'
  evidence='Compiler fingerprint comparison, final Cargo completion marker, ELF/PE signature, newer current replacement, single-link old executable, no launcher/producer source references, no incoming symlinks, no fuser consumer; source, libraries, current targets and all packages preserved. Replacement: '+row['replacement']
  for p in [Path(row['path']),Path(row['fingerprint']),Path(row['dep_info'])]:
   if p.exists():entries.append({'path':str(p),'kind':'build-cache','reason':reason,'evidence':evidence})
 decisions=temp/'decisions.json';decisions.write_text(json.dumps({'entries':entries,'process_reviews':reviews},indent=2)+'\n')
 before=shutil.disk_usage(root);c.prepare(type('Args',(),{'decisions':str(decisions)})());c.apply(None);after=shutil.disk_usage(root)
 for row in rows:assert not Path(row['path']).exists() and Path(row['replacement']).exists()
 report={'status':'completed exact superseded Ottrin executable cleanup; not a whole-machine audit','removed_executables':len(rows),'paths':rows,'compiler_settings_match':True,'no_current_callers_or_incoming_links':True,'measured_filesystem_free_delta':after.free-before.free,'free_after_bytes':after.free,'preserved':['All source/user data','Current stable target binaries and packages','All dependency libraries','Newest cache sessions','All VM bases/protected environments and current paused upgrade fixture'],'temporary_state_reason':'Root full; guarded plan/ledger temporarily on tmpfs, copied back after recovery'}
 dest=root/'.hermes/evidence/storage-recovery-current';dest.mkdir(exist_ok=True)
 (dest/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n');shutil.copy2(decisions,dest/'REMOVALS.json');shutil.copy2(temp/'actions.jsonl',dest/'actions.jsonl');shutil.copy2(Path(__file__),dest/'recover.py')
 (canonical/'ottrin-space-recovery.json').write_text(json.dumps(report,indent=2)+'\n')
 shutil.rmtree(temp)
 print('Reclaimed bytes',report['measured_filesystem_free_delta'],'free bytes',after.free)
