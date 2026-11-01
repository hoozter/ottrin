"""Retry the unchanged guard only when every refused process has actually exited."""
from pathlib import Path
import subprocess,json,re,time
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'cleanup-attempts';out.mkdir(exist_ok=False)
command=['python3','/home/campbell/.codex/skills/computer-cleanup/scripts/rust_incremental.py','--process-reviews',str(r/'.hermes/evidence/preview-footer-layout-current/PROCESS_REVIEWS.json'),'--apply',str(r/'target/debug/incremental'),str(r/'target/x86_64-pc-windows-gnu/debug/incremental')]
assert not Path('/proc/3563758').exists(),'Previous refused process is still live'
for attempt in range(1,52):
 p=subprocess.run(command,text=True,capture_output=True);(out/f'{attempt:02d}.log').write_text(p.stdout+p.stderr)
 print('Guard attempt',attempt,'exit',p.returncode,p.stdout[-180:],flush=True)
 if p.returncode==0:
  (out/'RESULT.json').write_text(json.dumps({'status':'pass','attempts':attempt,'scope':'Every attempt uses the unchanged live-consumer/newest-session/exclusive-lock guard. A retry is permitted only after all PIDs from its refusal no longer exist; no exception added.','previous_refused_pid_3563758_exited':True},indent=2)+'\n');break
 match=re.search(r'gaps=\[([\d, ]+)\]',p.stdout+p.stderr)
 assert match,'Unexpected refusal requires investigation'
 pids=[int(x.strip()) for x in match[1].split(',')]
 check={str(pid):Path('/proc',str(pid)).exists() for pid in pids};(out/f'{attempt:02d}-PID_RECHECK.json').write_text(json.dumps(check,indent=2)+'\n')
 assert not any(check.values()),'Live unreadable process requires investigation; no retry'
else:raise RuntimeError('Investigate repeated process churn before any further cleanup; all guard controls retained')
