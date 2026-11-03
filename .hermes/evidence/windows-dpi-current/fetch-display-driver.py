"""Extract only the pinned signed display driver, then retire the consumed ISO."""
from pathlib import Path
import hashlib,json,os,signal,subprocess,urllib.request,shutil
r=Path.cwd();e=Path(__file__).resolve().parent;cache=r/'.hermes/tools/windows-display-driver';iso=e/'virtio-win.iso';partial=iso.with_suffix('.iso.part');pin=json.loads((r/'.hermes/evidence/windows-stock-client-current/VIRTIO_DOWNLOAD_OWNER.json').read_text());report={'url':pin['download'],'expected_bytes':pin['expected_bytes'],'expected_sha256':pin['sha256'],'status':'starting','cleanup_owner':str(Path(__file__).resolve()),'purpose':'Exact Windows11 amd64 signed virtual display driver for owned DPI QA; no host/base installation'}
def save():(e/'DISPLAY_DRIVER_SOURCE.json').write_text(json.dumps(report,indent=2)+'\n')
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def stop(sig,_):raise SystemExit(128+sig)
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,stop)
assert not cache.exists() and not iso.exists() and not partial.exists();save()
try:
 with urllib.request.urlopen(pin['download'],timeout=30) as response,partial.open('xb') as out:
  while data:=response.read(1024*1024):out.write(data)
 assert partial.stat().st_size==pin['expected_bytes'] and sha(partial)==pin['sha256'];partial.rename(iso);report.update(status='exact pinned ISO verified; extracting selected display driver',actual_sha256=sha(iso));save()
 listing=subprocess.check_output(['7z','l','-ba',str(iso)],text=True);members=[]
 for line in listing.splitlines():
  item=line.split(maxsplit=5)
  if len(item)==6 and item[5].startswith('viogpudo/w11/amd64/') and item[5].endswith(('.inf','.cat','.sys')):members.append(item[5])
 assert {Path(v).suffix for v in members}=={'.inf','.cat','.sys'} and len(members)==3,members
 cache.mkdir();subprocess.run(['7z','e','-y','-o'+str(cache),str(iso),*members],stdout=subprocess.DEVNULL,check=True)
 files={p.name:sha(p) for p in cache.iterdir()};assert len(files)==3
 manifest={'files':files,'iso_sha256':pin['sha256'],'version':'0.1.302','source':pin['download'],'members':members,'native_trust_required':'Windows Authenticode catalog Valid and native pnputil install; no imported certificates/test-signing/security-policy changes','cleanup_owner':'windows-dpi-current; retain only selected driver while owned DPI fixture consumers need it'};(cache/'DRIVER_INPUT.json').write_text(json.dumps(manifest,indent=2)+'\n');report.update(status='selected display payload extracted; native signature/install acceptance pending',cache=str(cache),files=files,selected_bytes=sum(p.stat().st_size for p in cache.iterdir()));save()
except BaseException as error:
 report.update(status='failed; partial inputs retired',error=repr(error));
 if cache.exists():shutil.rmtree(cache)
 raise
finally:
 for p in [partial,iso]:
  if p.exists():
   assert p.is_file() and p.stat().st_nlink==1 and subprocess.run(['fuser',str(p)],capture_output=True).returncode==1;p.unlink()
 report['consumed_iso_removed']=not iso.exists() and not partial.exists();save()
