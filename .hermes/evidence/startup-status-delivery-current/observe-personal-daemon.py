"""Read-only30-second observation of the current personal managed daemon; redact paths."""
from pathlib import Path
import hashlib,json,os,socket,struct,subprocess,time,sys
E=Path(__file__).resolve().parent
assert not sys.argv[1:] or sys.argv[1:]==["--follow-up"]
follow_up=bool(sys.argv[1:])

def unit():
 p=subprocess.run(['systemctl','--user','show','ottrin-index.service','-p','MainPID','-p','ActiveState'],capture_output=True,text=True,check=True,timeout=10)
 return dict(line.split('=',1) for line in p.stdout.splitlines())
before=unit();assert before['ActiveState']=='active';pid=int(before['MainPID']);assert pid>0
P=Path('/proc')/str(pid)
def state():
 f=(P/'stat').read_text().rsplit(')',1)[1].split()
 return {'start_ticks':f[19],'cpu_ticks':int(f[11])+int(f[12]),'rss_bytes':int(f[21])*os.sysconf('SC_PAGE_SIZE')}
def diagnostics():
 request=json.dumps({'version':2,'request':{'type':'status'}}).encode()
 with socket.socket(socket.AF_UNIX) as client:
  client.settimeout(4);client.connect(str(Path(os.environ['XDG_RUNTIME_DIR'])/'ottrin/index-v1.sock'))
  peer_pid,peer_uid,_=struct.unpack('3i',client.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12));assert peer_pid==pid and peer_uid==os.getuid()
  client.sendall(struct.pack('!I',len(request))+request)
  def exact(n):
   value=b''
   while len(value)<n:
    part=client.recv(n-len(value));assert part;value+=part
   return value
  n=struct.unpack('!I',exact(4))[0];assert n<=1024*1024
  result=json.loads(exact(n));assert result['version']==2 and result['response']['type']=='status'
  d=result['response']['diagnostics']
  return {k:d[k] for k in ['status','indexed_items','configured_root_count','watched_root_count','watcher_ready','loading_saved_snapshot']}
report={'scope':'Read-only personal managed daemon on currently deliveredc4/f37; not the stagedf245/93 daemon and not sustained-workload acceptance. No host GUI, preference or service mutation. Paths and file names omitted.','pid':pid,'binary_sha256':hashlib.sha256((P/'exe').read_bytes()).hexdigest(),'before_diagnostics':diagnostics()}
a=state();start=time.monotonic();time.sleep(30);elapsed=time.monotonic()-start;b=state()
assert unit()==before and a['start_ticks']==b['start_ticks']
report.update(follow_up_after_owned_build_and_vm_teardown=follow_up,after_diagnostics=diagnostics(),seconds=elapsed,cpu_one_core_percent=100*(b['cpu_ticks']-a['cpu_ticks'])/os.sysconf('SC_CLK_TCK')/elapsed,rss_before_bytes=a['rss_bytes'],rss_after_bytes=b['rss_bytes'],process_identity_preserved=True)
(E/('PERSONAL_DAEMON_FOLLOW_UP.json' if follow_up else 'PERSONAL_DAEMON_OBSERVATION.json')).write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
