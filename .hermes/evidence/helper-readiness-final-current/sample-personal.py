"""Read-only live managed-index sample; never stop or reconfigure it."""
from pathlib import Path
import hashlib,json,os,socket,struct,subprocess,time
E=Path(__file__).resolve().parent

def unit():return dict(line.split('=',1) for line in subprocess.check_output(['systemctl','--user','show','ottrin-index.service','-p','MainPID','-p','ActiveState'],text=True).splitlines())
def status():
 endpoint=Path(os.environ.get('XDG_RUNTIME_DIR',f'/run/user/{os.getuid()}'))/'ottrin/index-v1.sock'
 with socket.socket(socket.AF_UNIX) as client:
  client.settimeout(5);client.connect(str(endpoint));data=json.dumps({'version':2,'request':{'type':'status'}}).encode();client.sendall(struct.pack('!I',len(data))+data)
  def exact(n):
   value=b''
   while len(value)<n:
    part=client.recv(n-len(value));assert part;value+=part
   return value
  count=struct.unpack('!I',exact(4))[0];assert count<1024*1024
  frame=json.loads(exact(count));assert frame['version']==2 and frame['response']['type']=='status';d=frame['response']['diagnostics']
  return {k:d[k] for k in ['status','indexed_items','watcher_ready']}|{'has_error':bool(d.get('last_error'))}
before=unit();assert before['ActiveState']=='active';pid=before['MainPID'];proc=Path('/proc')/pid
sha=hashlib.sha256((proc/'exe').read_bytes()).hexdigest();assert sha==json.loads((E/'LINUX_DAEMON_RUNTIME.json').read_text())['binary_sha256']
def counters():
 f=(proc/'stat').read_text().rsplit(')',1)[1].split();return f[19],int(f[11])+int(f[12])
initial=status();identity,start=counters();clock=time.monotonic();time.sleep(30);seconds=time.monotonic()-clock
end_identity,end=counters();assert identity==end_identity and unit()==before
cpu=100*(end-start)/os.sysconf('SC_CLK_TCK')/seconds
report={'status':'pass read-only live personal-daemon sample','source_revision':json.loads((E/'BUILD_SOURCE.json').read_text())['source_revision'],'pid':int(pid),'binary_sha256':sha,'before':initial,'after':status(),'seconds':seconds,'cpu_one_core_percent':cpu,'scope':'Actual personal managed index, no agent queries during measurement and no reconfiguration. External filesystem activity/other clients are uncontrolled; this is one30-second sample, not sustained acceptance.'}
(E/'PERSONAL_DAEMON_SAMPLE.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
