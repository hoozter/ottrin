from pathlib import Path
import os,json,subprocess,tempfile,time,signal
from PIL import Image
assert os.environ.get('GITHUB_ACTIONS')=='true' and os.environ.get('RUNNER_ENVIRONMENT')=='github-hosted'
out=Path('html-sandbox-evidence');out.mkdir(exist_ok=True)
owned=Path('/usr/libexec/ottrin/ottrin-html-render');unit='ottrin-render-html-check-'+str(os.getpid())
report={'status':'running','uid':os.getuid(),'controller_userns':os.readlink('/proc/self/ns/user')};p=None
assert os.getuid()!=0
def run(args,**kw):return subprocess.run(args,check=True,**kw)
def cancelled(*_):raise KeyboardInterrupt('production HTML check cancelled')
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,cancelled)
try:
 run([str(owned),'--check'])
 restriction=Path('/proc/sys/kernel/apparmor_restrict_unprivileged_userns').read_text().strip();assert restriction=='1';report['restriction_before']=restriction
 with tempfile.TemporaryDirectory(prefix='ottrin-html-probe-') as t:
  root=Path(t);os.chmod(root,0o700)
  for n in ['profile','output']:(root/n).mkdir(mode=0o700)
  (root/'input.html').write_text('''<!doctype html><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'"><style>html,body{margin:0;background:#123456}.card{margin:24px;width:120px;height:80px;background:#abcdef}</style><div class="card"></div><script>document.body.style.background='#ff0000'</script>''');os.chmod(root/'input.html',0o600)
  (root/'injection.c').write_text('#include <stdlib.h>\n#include <stdio.h>\n__attribute__((constructor)) static void injected(void){const char *p=getenv("OTTRIN_PROBE_MARKER");if(p){FILE *f=fopen(p,"w");if(f){fputs("injected",f);fclose(f);}}}\n')
  run(['cc','-shared','-fPIC','-O2',str(root/'injection.c'),'-o',str(root/'injection.so')])
  control_env=dict(os.environ,LD_PRELOAD=str(root/'injection.so'),OTTRIN_PROBE_MARKER=str(root/'control'))
  run(['/usr/bin/true'],env=control_env)
  assert (root/'control').read_text()=='injected','Preload control did not execute'
  report['preload_control_executed']=True
  # Record the host's separate policy for unconfined profile changes. This
  # setting is never changed by Ottrin or this check. On hardened hosts an
  # already-running caller must not enter the rendering profile directly.
  transition_setting=Path('/proc/sys/kernel/apparmor_restrict_unprivileged_unconfined')
  report['unconfined_transition_restriction']=transition_setting.read_text().strip() if transition_setting.exists() else 'unavailable'
  (root/'transition.c').write_text(r'''#include <dlfcn.h>
#include <stdio.h>
#include <errno.h>
int main(void) {
 void *lib=dlopen("libapparmor.so.1",RTLD_NOW);
 if (!lib) return 2;
 int (*change)(const char*)=dlsym(lib,"aa_change_profile");
 if (!change) return 3;
 errno=0; int result=change("ottrin-html-render"); int error=errno;
 printf("%d %d\n",result,error); return 0;
}
''')
  run(['cc',str(root/'transition.c'),'-ldl','-o',str(root/'transition')])
  transition=run([str(root/'transition')],capture_output=True,text=True)
  report['direct_profile_change_result']=transition.stdout.strip()
  if report['unconfined_transition_restriction']=='1':
   assert transition.stdout.startswith('-1 '),'Host policy permitted an unprivileged direct profile change'
   report['direct_profile_change']='denied by host policy'
  else:
   report['direct_profile_change']='host policy dependent; no application-level prevention claimed'
  for bad in [[str(root),'--no-sandbox'],[str(root),'256','/usr/bin/true']]:
   refused=subprocess.run([str(owned),*bad],capture_output=True,text=True,timeout=3)
   assert refused.returncode!=0,('Unsafe HTML helper protocol admitted',bad)
  report['arbitrary_command_flags_refused']=True

  with (out/'stdout.txt').open('w') as stdout,(out/'stderr.txt').open('w') as stderr:
   p=subprocess.Popen(['systemd-run','--user','--wait','--collect','--pipe','--quiet','--unit='+unit,'--property=Type=exec','--property=MemoryMax=512M','--property=TasksMax=256','--property=RuntimeMaxSec=20s','--property=LimitCORE=0','--setenv=LD_PRELOAD='+str(root/'injection.so'),'--setenv=OTTRIN_PROBE_MARKER='+str(root/'injected'),str(owned),str(root),'256'],stdout=stdout,stderr=stderr)
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
  assert all(w['seccomp']=='2' and w['caps']=='0000000000000000' and w['no_new_privs']=='1' and w['profile']=='ottrin-html-render (enforce)' for w in witnesses.values()),witnesses
  report['pixel_checks']='pass; inert static layout and blocked script'
 report['private_profile_removed']=not root.exists();report['status']='pass'
except BaseException as e:
 report['status']='fail';report['error']=repr(e);raise
finally:
 if p is not None and p.poll() is None:
  subprocess.run(['systemctl','--user','stop',unit+'.service'],check=True);p.wait(timeout=10)
 report['restriction_after']=Path('/proc/sys/kernel/apparmor_restrict_unprivileged_userns').read_text().strip()
 report['host_restriction_unchanged']=report.get('restriction_before')==report['restriction_after']
 assert report['host_restriction_unchanged']
 (out/'RENDER_AND_PRELOAD.json').write_text(json.dumps(report,indent=2)+'\n')
