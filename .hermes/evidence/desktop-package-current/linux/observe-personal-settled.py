"""Read-only sixty-second observation of the delivered personal index daemon."""
from pathlib import Path
import os,time,json,hashlib,subprocess,signal
p=Path(__file__).resolve().parent;root=Path.cwd();out=p/'PERSONAL_SETTLED_INTERVAL.json'
def status():
 r=subprocess.run([str(root/'target/release/ottrin-app'),'index','status','--json'],capture_output=True,text=True,check=True,timeout=10)
 d=json.loads(r.stdout)
 return {k:v for k,v in d.items() if k not in ['active_root']}
def save():out.write_text(json.dumps(report,indent=2)+'\n')
def cancelled(*_):raise KeyboardInterrupt('Read-only CPU observation cancelled')
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,cancelled)
pid=int(subprocess.check_output(['systemctl','--user','show','ottrin-index.service','--property=MainPID','--value'],text=True));assert pid>0
proc=Path('/proc')/str(pid)
def snapshot():
 values=(proc/'stat').read_text().rsplit(')',1)[1].split();memory=dict(line.split(':',1) for line in (proc/'status').read_text().splitlines() if ':' in line)
 assert values[19]==start_ticks
 return {'cpu_ticks':int(values[11])+int(values[12]),'rss_kib':int(memory['VmRSS'].split()[0]),'time':time.monotonic()}
start_ticks=(proc/'stat').read_text().rsplit(')',1)[1].split()[19];expected=json.loads((root/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json').read_text())['indexd_binary_sha256'];assert hashlib.file_digest((proc/'exe').open('rb'),'sha256').hexdigest()==expected
report={'status':'observing','pid':pid,'start_ticks':start_ticks,'daemon_sha256':expected,'scope':'Personal configured Home index,read-only watched interval. Process RSS excludes shared/kernel file cache;CPU percent is one logical core. No algorithm change or physical-device claim.','before_status':status()};save()
try:
 before=snapshot();samples=[before]
 for _ in range(12):time.sleep(5);samples.append(snapshot())
 after=samples[-1];report.update(after_status=status(),elapsed_seconds=after['time']-before['time'],cpu_seconds=(after['cpu_ticks']-before['cpu_ticks'])/os.sysconf('SC_CLK_TCK'),rss_kib_range=[min(v['rss_kib'] for v in samples),max(v['rss_kib'] for v in samples)])
 report['one_core_cpu_percent']=100*report['cpu_seconds']/report['elapsed_seconds']
 a,b=report['before_status'],report['after_status'];report['scan_timestamps_unchanged']=a['last_scan_started_unix_secs']==b['last_scan_started_unix_secs'] and a['last_scan_completed_unix_secs']==b['last_scan_completed_unix_secs'];report['watched_ready_at_both_ends']=all(s['status']=='ready' and s['watcher_ready'] and s['detail'].startswith('Ready') and not s['error'] and not s['watcher_error'] for s in [a,b]);report['status']='measured watched Ready interval' if report['scan_timestamps_unchanged'] and report['watched_ready_at_both_ends'] else 'measured mixed interval;not idle acceptance'
except BaseException as error:report['status']='incomplete';report['error']=repr(error);raise
finally:save()
print(json.dumps({k:report[k] for k in ['status','pid','elapsed_seconds','cpu_seconds','one_core_cpu_percent','rss_kib_range']},indent=2))
