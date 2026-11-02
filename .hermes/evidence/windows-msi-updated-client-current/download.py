"""Verified public servicing inputs; no guest or host OS mutation."""
from pathlib import Path
import hashlib,json,signal,urllib.request
def cancelled(signum,frame):raise SystemExit(128+signum)
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,cancelled)
e=Path(__file__).resolve().parent;cache=Path.cwd()/'.hermes/tools/windows-client-update';cache.mkdir(exist_ok=True)
manifest=json.loads((e/'UPDATE_INPUTS.json').read_text());report={'status':'downloading','cache':str(cache),'purpose':'Pending disposable Windows MSI current-client comparison','files':[]}
def save():(e/'DOWNLOAD.json').write_text(json.dumps(report,indent=2)+'\n')
save()
try:
 for item in manifest['files']:
  p=cache/item['filename'];temp=p.with_suffix('.download');assert not temp.exists()
  if not p.exists():
   try:
    with urllib.request.urlopen(item['url'],timeout=60) as response,temp.open('xb') as output:
     while data:=response.read(1024*1024):output.write(data)
    with temp.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==item['sha256']
    temp.rename(p)
   finally:temp.unlink(missing_ok=True)
  with p.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==item['sha256']
  report['files'].append({**item,'bytes':p.stat().st_size});save()
 report['status']='verified; pending disposable guest servicing';save()
except BaseException as error:
 report.update(status='failed; partial owned downloads removed',error=repr(error));save();raise
