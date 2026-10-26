from pathlib import Path
import os,json,subprocess,tempfile,hashlib,signal
from PIL import Image
root=Path.cwd();e=root/'.hermes/evidence/linux-filename-index-package-current';binary=root/'.hermes/packaging-tmp/.linux-filename-index-extract/squashfs-root/usr/libexec/ottrin/ottrin-html-render';unit='ottrin-render-packaged-html-'+str(os.getpid());report={'scope':'Exact packaged release HTML helper, direct fixed protocol on the existing unrestricted-userns host; not protected installation or main app HTML GUI acceptance','binary_sha256':hashlib.file_digest(binary.open('rb'),'sha256').hexdigest()}
def cancelled(*_):raise KeyboardInterrupt('owned packaged HTML check cancelled')
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,cancelled)
restriction=Path('/proc/sys/kernel/apparmor_restrict_unprivileged_userns');before=restriction.read_text().strip();report['restriction_before']=before
assert before=='0','Use the hosted installed-helper lane for restricted kernels'
try:
 with tempfile.TemporaryDirectory(prefix='.ottrin-packaged-html-',dir='/var/tmp') as t:
  p=Path(t);p.chmod(0o700)
  for name in ['profile','output']:(p/name).mkdir(mode=0o700)
  source=p/'input.html';source.write_text('''<!doctype html><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'"><style>html,body{margin:0;background:#123456}.card{margin:24px;width:120px;height:80px;background:#abcdef}</style><div class="card"></div><script>document.body.style.background='#ff0000'</script>''');source.chmod(0o600)
  run=subprocess.run(['systemd-run','--user','--wait','--collect','--pipe','--quiet','--unit='+unit,'--property=Type=exec','--property=MemoryMax=512M','--property=TasksMax=256','--property=RuntimeMaxSec=12s','--property=LimitCORE=0','--property=LimitFSIZE=16777216',str(binary),str(p),'256'],capture_output=True,text=True,timeout=15)
  (e/'packaged-html.stderr').write_text(run.stderr);report['exit']=run.returncode;assert run.returncode==0,run.stderr
  image=Image.open(p/'output/output.png').convert('RGB');assert image.size==(256,256) and image.getpixel((1,1))==(18,52,86) and image.getpixel((30,30))==(171,205,239)
  image.save(e/'packaged-html.png');report['pixels_and_script_refusal']='pass'
 report['private_profile_removed']=not p.exists();report['status']='pass'
finally:
 active=subprocess.run(['systemctl','--user','is-active',unit+'.service'],capture_output=True,text=True)
 if active.returncode==0:subprocess.run(['systemctl','--user','stop',unit+'.service'],check=True)
 report['restriction_after']=restriction.read_text().strip();assert report['restriction_after']==before
 (e/'PACKAGED_HTML.json').write_text(json.dumps(report,indent=2)+'\n')
