from pathlib import Path
import os,json,subprocess,tempfile,time,signal
from PIL import Image
out=Path('html-probe-evidence');out.mkdir();owned=Path('/usr/libexec/ottrin/ottrin-html-profile-probe');policy=Path('/etc/apparmor.d/ottrin-html-profile-probe');runtime=Path('/usr/libexec/ottrin/html-probe-runtime');unit='ottrin-html-probe-'+str(os.getpid());report={'status':'running','uid':os.getuid(),'controller_userns':os.readlink('/proc/self/ns/user'),'browser_metadata':str(os.stat('/opt/google/chrome/chrome'))};p=None;installed=False;loaded=False
assert os.getuid()!=0
assert not owned.exists() and not policy.exists() and not runtime.exists()
def run(args,**kw):return subprocess.run(args,check=True,**kw)
def cancelled(*_):raise KeyboardInterrupt('probe cancelled')
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,cancelled)
try:
 restriction=Path('/proc/sys/kernel/apparmor_restrict_unprivileged_userns').read_text().strip();assert restriction=='1';report['restriction_before']=restriction
 with tempfile.TemporaryDirectory(prefix='ottrin-html-probe-') as t:
  root=Path(t);os.chmod(root,0o700)
  for n in ['profile','output']:(root/n).mkdir(mode=0o700)
  (root/'input.html').write_text('''<!doctype html><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'"><style>html,body{margin:0;background:#123456}.card{margin:24px;width:120px;height:80px;background:#abcdef}</style><div class="card"></div><script>document.body.style.background='#ff0000'</script>''');os.chmod(root/'input.html',0o600)
  run(['cc','-O2','-Wall','-Wextra','-Werror','scripts/ci/html-profile-probe.c','-o',str(root/'probe')])
  (root/'injection.c').write_text('#include <stdlib.h>\n#include <stdio.h>\n__attribute__((constructor)) static void injected(void){const char *p=getenv("OTTRIN_PROBE_MARKER");if(p){FILE *f=fopen(p,"w");if(f){fputs("injected",f);fclose(f);}}}\n')
  run(['cc','-shared','-fPIC','-O2',str(root/'injection.c'),'-o',str(root/'injection.so')])

  run(['sudo','install','-D','-m','0755',str(root/'probe'),str(owned)]);installed=True
  run(['sudo','cp','-a','/opt/google/chrome',str(runtime)])
  run(['sudo','chmod','-R','go-w',str(runtime)])
  report['trusted_runtime_browser_sha256']=__import__('hashlib').sha256((runtime/'chrome').read_bytes()).hexdigest()
  assert report['trusted_runtime_browser_sha256']==__import__('hashlib').sha256(Path('/opt/google/chrome/chrome').read_bytes()).hexdigest()
  run(['sudo','install','-m','0644','scripts/ci/html-profile-probe.profile',str(policy)])
  run(['sudo','apparmor_parser','-r',str(policy)]);loaded=True
  with (out/'stdout.txt').open('w') as stdout,(out/'stderr.txt').open('w') as stderr:
   p=subprocess.Popen(['systemd-run','--user','--wait','--collect','--pipe','--quiet','--unit='+unit,'--property=Type=exec','--property=MemoryMax=512M','--property=TasksMax=256','--property=RuntimeMaxSec=20s','--property=LimitCORE=0','--setenv=LD_PRELOAD='+str(root/'injection.so'),'--setenv=OTTRIN_PROBE_MARKER='+str(root/'injected'),str(owned),str(root)],stdout=stdout,stderr=stderr)
   deadline=time.monotonic()+25;witnesses={}
   while p.poll() is None and time.monotonic()<deadline:
    cg=subprocess.run(['systemctl','--user','show',unit+'.service','--property=ControlGroup','--value'],capture_output=True,text=True).stdout.strip()
    if cg.startswith('/user.slice/'):
     try:
      ids=(Path('/sys/fs/cgroup')/cg.lstrip('/')/'cgroup.procs').read_text().split()
      for pid in ids:
       proc=Path('/proc')/pid
       try:
        args=(proc/'cmdline').read_bytes();s=(proc/'status').read_text();label=(proc/'attr/current').read_text().strip()
        if b'--type=renderer' in args:
         fields={k:v.strip() for k,v in (line.split(':',1) for line in s.splitlines() if ':' in line)}
         witnesses[pid]={'userns':os.readlink(proc/'ns/user'),'seccomp':fields.get('Seccomp'),'caps':fields.get('CapEff'),'no_new_privs':fields.get('NoNewPrivs'),'profile':label}
       except (FileNotFoundError,ProcessLookupError):pass
     except FileNotFoundError:pass
    time.sleep(.01)
   report['exit']=p.wait(timeout=5);report['renderer_witnesses']=witnesses
  assert report['exit']==0,(out/'stderr.txt').read_text()
  assert not (root/'injected').exists(),'Caller-controlled preload executed under the trusted profile'
  report['caller_preload_denied']=True
  image=Image.open(root/'output/output.png').convert('RGB');assert image.size==(256,256);assert image.getpixel((1,1))==(18,52,86);assert image.getpixel((30,30))==(171,205,239)
  image.save(out/'render.png')
  assert witnesses,'renderer was not observed'
  assert any(w['seccomp']=='2' and w['caps']=='0000000000000000' and w['no_new_privs']=='1' and 'ottrin-html-profile-probe' in w['profile'] for w in witnesses.values()),witnesses
  report['pixel_checks']='pass; inert static layout and blocked script'
 report['private_profile_removed']=not root.exists();report['status']='pass'
except BaseException as e:
 report['status']='fail';report['error']=repr(e);raise
finally:
 if p is not None and p.poll() is None:
  subprocess.run(['systemctl','--user','stop',unit+'.service'],capture_output=True);p.wait(timeout=10)
 if loaded:run(['sudo','apparmor_parser','-R',str(policy)])
 if policy.exists():run(['sudo','rm','--',str(policy)])
 if installed:run(['sudo','rm','--',str(owned)])
 if runtime.exists():run(['sudo','rm','-rf','--',str(runtime)])
 report['restriction_after']=Path('/proc/sys/kernel/apparmor_restrict_unprivileged_userns').read_text().strip()
 report['owned_policy_binary_removed']=not policy.exists() and not owned.exists() and not runtime.exists()
 report['host_restriction_unchanged']=report.get('restriction_before')==report['restriction_after']
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
