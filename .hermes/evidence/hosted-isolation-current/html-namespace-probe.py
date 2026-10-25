from pathlib import Path
import os,tempfile,subprocess,json
r=Path(__file__).resolve().parent;unit='ottrin-html-diagnostic-'+str(os.getpid());report={'unit':unit,'scope':'nested namespaces disabled only inside owned Bubblewrap; host policy unchanged'}
with tempfile.TemporaryDirectory(prefix='ottrin-html-boundary-') as t:
 p=Path(t);(p/'profile').mkdir();(p/'out').mkdir();(p/'input.html').write_text('<!doctype html><style>body{background:#123456}</style><p>owned diagnostic</p>')
 args=['systemd-run','--user','--wait','--collect','--pipe','--quiet','--unit='+unit,'--property=Type=exec','--property=TasksMax=256','--property=MemoryMax=512M','--property=RuntimeMaxSec=10s','--property=LimitCORE=0','/usr/bin/bwrap','--unshare-all','--unshare-user','--disable-userns','--die-with-parent','--clearenv','--setenv','HOME','/profile','--setenv','LANG','C.UTF-8']
 for path in ['/usr','/etc','/bin','/lib','/lib64','/opt']:
  if Path(path).exists():args+=['--ro-bind',path,path]
 args+=['--proc','/proc','--dev','/dev','--tmpfs','/tmp','--dir','/input','--bind',str(p/'profile'),'/profile','--bind',str(p/'out'),'/output','--ro-bind',str(p/'input.html'),'/input/document.html','/opt/google/chrome/chrome','--headless=new','--disable-gpu','--disable-javascript','--disable-background-networking','--disable-component-update','--disable-default-apps','--disable-extensions','--disable-sync','--metrics-recording-only','--no-first-run','--no-default-browser-check','--user-data-dir=/profile','--screenshot=/output/output.png','--window-size=256,256','file:///input/document.html']
 try:
  c=subprocess.run(args,capture_output=True,text=True,timeout=15);report.update(exit=c.returncode,stdout=c.stdout[-65536:],stderr=c.stderr[-65536:],output_exists=(p/'out/output.png').exists())
 finally:
  stop=subprocess.run(['systemctl','--user','stop',unit+'.service'],capture_output=True,text=True);report['unit_stop_exit']=stop.returncode
report['private_profile_removed']=not p.exists();(r/'HTML_NESTED_NAMESPACE_REPRO.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
