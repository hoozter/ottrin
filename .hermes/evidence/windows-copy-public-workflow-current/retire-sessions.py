from pathlib import Path
import fcntl,os,json,subprocess,shutil,datetime
base=Path.cwd();crates={'ottrin_platform','ottrin_ui','ottrin_search'}
# Only configurations finalized during this task; retain their newest session.
cutoff=datetime.datetime(2026,10,26,0,tzinfo=datetime.timezone.utc).timestamp()
consumers=subprocess.check_output(['ps','-eo','comm=']).decode().splitlines();assert not any(x.strip() in ['rustc','cargo'] for x in consumers)
rows=[]
for tree in ['target/debug/incremental','target/x86_64-pc-windows-gnu/debug/incremental']:
 for config in (base/tree).iterdir():
  if not config.is_dir() or config.name.rsplit('-',1)[0] not in crates:continue
  sessions=sorted([p for p in config.iterdir() if p.is_dir() and not p.name.endswith('-working')],key=lambda p:p.stat().st_mtime)
  if len(sessions)<2 or sessions[-1].stat().st_mtime<cutoff:continue
  replacement=sessions[-1]
  for obsolete in sessions[:-1]:
   lock=obsolete.with_name(obsolete.name.rsplit('-',1)[0]+'.lock')
   assert lock.is_file(),lock
   with lock.open('r+b') as handle:
    fcntl.flock(handle,fcntl.LOCK_EX|fcntl.LOCK_NB);fcntl.lockf(handle,fcntl.LOCK_EX|fcntl.LOCK_NB)
    assert replacement.is_dir()
    size=sum(p.stat().st_size for p in obsolete.rglob('*') if p.is_file())
    rows.append({'removed':str(obsolete.relative_to(base)),'current_replacement':str(replacement.relative_to(base)),'logical_bytes':size})
    shutil.rmtree(obsolete)
   lock.unlink()
receipt={'scope':'Same-configuration superseded finalized sessions only; no Cargo/rustc consumers, both compiler lock styles acquired; newest session and all other configurations retained','removed':rows,'logical_bytes':sum(r['logical_bytes'] for r in rows)}
(base/'.hermes/evidence/windows-copy-public-workflow-current/CACHE_RETIREMENT.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(len(rows),'superseded sessions removed;',receipt['logical_bytes'],'logical bytes')
