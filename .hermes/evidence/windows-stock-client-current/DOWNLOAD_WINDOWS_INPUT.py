"""Download one official clean-client ISO; register ownership and remove partial output."""
from pathlib import Path
import datetime,hashlib,json,shutil,signal,urllib.request,urllib.parse
r=Path.cwd();e=Path(__file__).resolve().parent;cache=r/'.hermes/tools/windows-stock-client';cache.mkdir(exist_ok=True);output=cache/'Windows11EnterpriseEvaluation.iso';partial=output.with_suffix('.iso.part');assert not output.exists() and not partial.exists()
source='https://go.microsoft.com/fwlink/?LinkId=2382600&clcid=0x409&country=us&culture=en-us';expected=8225329152
owner={'status':'preparing official input','source_page':'https://www.microsoft.com/en-us/evalcenter/download-windows-11-enterprise','download':source,'path':str(output),'partial_path':str(partial),'expected_bytes':expected,'purpose':'Independent Microsoft Windows client comparison for the failed native MSI rollback gate','pending_consumers':['Verify Microsoft image/version and signed bootstrap inputs','Install one independent disposable Windows client; transfer ISO once and remove media after install consumer completes'],'cleanup_owner':str(Path(__file__).resolve()),'created_utc':datetime.datetime.now(datetime.timezone.utc).isoformat()}
def save():
 temporary=e/'DOWNLOAD_OWNER.json.tmp';temporary.write_text(json.dumps(owner,indent=2)+'\n');temporary.replace(e/'DOWNLOAD_OWNER.json')
def cancelled(*_):raise KeyboardInterrupt('Owned clean-client input cancelled')
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,cancelled)
save()
try:
 assert shutil.disk_usage(r).free>expected+18*1024**3,'Need ISO plus18GiB installation margin before allocating input'
 with urllib.request.urlopen(source,timeout=45) as response:
  host=urllib.parse.urlparse(response.url).hostname;assert host=='software-static.download.prss.microsoft.com';assert int(response.headers['Content-Length'])==expected
  owner.update(status='downloading official ISO',resolved_url=response.url,received_bytes=0);save();digest=hashlib.sha256();checkpoint=0
  with partial.open('xb') as stream:
   while chunk:=response.read(8*1024*1024):
    stream.write(chunk);digest.update(chunk);owner['received_bytes']+=len(chunk);assert owner['received_bytes']<=expected
    if owner['received_bytes']-checkpoint>=256*1024*1024:checkpoint=owner['received_bytes'];save();print(f'Downloaded {checkpoint}/{expected} bytes',flush=True)
 assert partial.stat().st_size==expected;partial.replace(output);owner.update(status='downloaded; image/signer verification and clean install still pending',sha256=digest.hexdigest(),received_bytes=expected);save();print(json.dumps(owner,indent=2))
except BaseException as error:
 if partial.exists():partial.unlink()
 owner.update(status='download failed or cancelled; no partial retained',error=repr(error));save();raise
