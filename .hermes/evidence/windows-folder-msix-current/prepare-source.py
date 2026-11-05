"""Real shallow Git source fixture; do not impersonate Git or rewrite stage provenance."""
from pathlib import Path
import hashlib,json,shutil,subprocess,zipfile
import signal
def cancelled(signum, frame): raise SystemExit(128 + signum)
for signum in (signal.SIGTERM, signal.SIGINT): signal.signal(signum, cancelled)
r=Path.cwd();e=Path(__file__).resolve().parent;source=r/'.hermes/packaging-tmp/.windows-msix-source';archive=e/'source.zip'
assert not source.exists() and not archive.exists()
revision=json.loads((e/'windows/BUILD_SOURCE.json').read_text())['source_revision']
subprocess.run(['git','merge-base','--is-ancestor',revision,'HEAD'],check=True)
subprocess.run(['git','diff','--exit-code',revision,'--','crates','Cargo.toml','Cargo.lock','packaging','scripts','build-all.sh'],check=True)
depth=int(subprocess.check_output(['git','rev-list','--count',revision+'..HEAD'],text=True).strip())+1
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],text=True).strip()
result={'source_revision':revision,'purpose':'Pending native MSIX assembly using real Git and exact current stage','archive':str(archive),'status':'preparing'}
try:
 subprocess.run(['git','clone','--no-hardlinks','--no-checkout','--filter=blob:none','--upload-pack=git -c uploadpack.allowFilter=true upload-pack','--depth',str(depth),r.as_uri(),str(source)],check=True)
 def git(*args):return subprocess.check_output(['git','-C',str(source),*args],text=True).strip()
 git('config','core.filemode','false');git('config','core.autocrlf','false')
 git('sparse-checkout','set','--cone','packaging/windows','docs','assets')
 git('checkout','--detach',revision)
 assert git('rev-parse','HEAD')==revision and not git('status','--porcelain','--untracked-files=no')
 with zipfile.ZipFile(archive,'x',compression=zipfile.ZIP_DEFLATED) as z:
  for p in source.rglob('*'):
   assert not p.is_symlink()
   if p.is_file():z.write(p,p.relative_to(source))
 with archive.open('rb') as f:result['sha256']=hashlib.file_digest(f,'sha256').hexdigest()
 result.update(status='prepared',archive_bytes=archive.stat().st_size,real_shallow_clone=True,filter_requested=True,source_scope='Exact Git commit and sparse packager inputs; omitted blobs are not assembly inputs. No new Git revision or forged provenance.')
except BaseException:
 archive.unlink(missing_ok=True);raise
finally:
 if source.exists():
  files=[p for p in source.rglob('*') if p.is_file()];assert all(p.stat().st_nlink==1 for p in files)
  assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
  shutil.rmtree(source)
 result['temporary_clone_removed']=not source.exists()
 (e/'SOURCE_INPUT.json').write_text(json.dumps(result,indent=2)+'\n')
