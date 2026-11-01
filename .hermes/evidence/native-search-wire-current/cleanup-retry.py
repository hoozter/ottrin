"""Revalidate existing inspection exceptions, then inspect/apply unchanged cache guard."""
from pathlib import Path
import json,hashlib,stat,subprocess,re
r=Path.cwd();e=Path(__file__).resolve().parent;review=r/'.hermes/evidence/preview-native-title-current/PROCESS_REVIEWS.json'
for x in json.loads(review.read_text())['process_reviews']:
 p=Path('/proc')/str(x['pid']);assert (p/'comm').read_text().strip()==x['comm'];assert (p/'stat').read_text().split(') ')[1].split()[19]==x['start_ticks'];assert hashlib.sha256((p/'cmdline').read_bytes()).hexdigest()==x['command_sha256']
 f=Path(x['declared_system_executable']);assert hashlib.sha256(f.read_bytes()).hexdigest()==x['system_executable_sha256']
 for a in [f,*f.parents]:
  s=a.stat();assert s.st_uid==0 and not s.st_mode&(stat.S_IWGRP|stat.S_IWOTH),a
command=['python3','/home/campbell/.codex/skills/computer-cleanup/scripts/rust_incremental.py','--process-reviews',str(review),str(r/'target/debug/incremental'),str(r/'target/x86_64-pc-windows-gnu/debug/incremental')]
for phase in ['inspect','apply']:
 p=subprocess.run(command+(['--apply'] if phase=='apply' else []),text=True,capture_output=True);(e/f'INCREMENTAL_RETRY_{phase.upper()}.log').write_text(p.stdout+p.stderr);print(phase,p.returncode,p.stdout[-220:],flush=True)
 assert p.returncode==0,'Inspect guard refusal before any retry; retain all safeguards'
# Only this task's Python bytecode; source producers and compact evidence remain.
for folder in [e/'__pycache__',e/'windows-native/__pycache__']:
 if folder.exists():
  for p in folder.iterdir():assert p.is_file() and p.suffix=='.pyc';p.unlink()
  folder.rmdir()
p=r/'scripts/__pycache__/index-populated-qa.cpython-312.pyc'
if p.exists():p.unlink()
