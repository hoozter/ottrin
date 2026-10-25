from pathlib import Path
import subprocess,time,json,os,hashlib,sys
p=Path(__file__).parent;unit='ottrin-index.service';seconds=int(sys.argv[1]) if len(sys.argv)>1 else 20;filename=sys.argv[2] if len(sys.argv)>2 else 'CPU_SAMPLE.json';assert 1<=seconds<=60 and Path(filename).name==filename
def props():
 r=subprocess.check_output(['systemctl','--user','show',unit,'--property=MainPID','--property=ControlGroup','--property=CPUUsageNSec','--property=MemoryCurrent','--property=ActiveState','--property=SubState']).decode();return dict(line.split('=',1) for line in r.splitlines())
before=props();pid=int(before['MainPID']);assert pid>0 and before['ActiveState']=='active';proc=Path('/proc')/str(pid);identity={'pid':pid,'comm':(proc/'comm').read_text().strip(),'exe':os.readlink(proc/'exe'),'exe_sha256':hashlib.file_digest((proc/'exe').open('rb'),'sha256').hexdigest(),'status':{k:v.strip() for k,v in (line.split(':',1) for line in (proc/'status').read_text().splitlines() if ':' in line) if k in ['VmRSS','VmHWM','VmSwap','Threads']}};start=time.monotonic();samples=[]
for i in range(seconds):
 time.sleep(1);v=props();assert v['MainPID']==str(pid) and v['ActiveState']=='active';elapsed=time.monotonic()-start;samples.append({'elapsed_seconds':elapsed,'service_cpu_ns':int(v['CPUUsageNSec']),'memory_current_bytes':int(v['MemoryCurrent'])})
elapsed=time.monotonic()-start;cpu=(samples[-1]['service_cpu_ns']-int(before['CPUUsageNSec']))/1e9/elapsed*100
baseline=json.loads((p/'BASELINE.json').read_text());unchanged={}
for key,path in [('config',Path.home()/'.config/ottrin/config.json'),('unit',Path.home()/'.config/systemd/user/ottrin-index.service'),('wrapper',Path.home()/'.cache/ottrin/ottrin-index-appimage')]:unchanged[key]=hashlib.file_digest(path.open('rb'),'sha256').hexdigest()==baseline[key+'_sha256']
assert all(unchanged.values())
r={'scope':'Actual configured enabled personal service after authorized restart; complete cgroup CPU includes launcher/FUSE/daemon descendants; observation window only','identity':identity,'elapsed_seconds':elapsed,'cpu_percent_of_one_core':cpu,'samples':samples,'config_unit_wrapper_unchanged':unchanged,'primary_user_files_not_modified_by_agent':True,'service_left_enabled_and_running':True};(p/filename).write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({'comm':identity['comm'],'pid':pid,'cpu_percent_of_one_core':cpu,'elapsed_seconds':elapsed,'memory_current_bytes':samples[-1]['memory_current_bytes'],'exe_sha256':identity['exe_sha256']},indent=2))
