"""Admit native evidence reuse only for byte/mode/link-identical runtime inputs."""
from pathlib import Path
import hashlib,json,subprocess,tarfile
R=Path.cwd();E=Path(__file__).resolve().parent;B=R/'.hermes/evidence/helper-readiness-delivery-current'
baseline=json.loads((B/'NATIVE_ACCEPTED_PAYLOAD.json').read_text());build=json.loads((E/'BUILD_RESULT.json').read_text())
archive=Path(build['stage'])/'Ottrin-0.1.0-linux-x86_64-portable.tar.gz'
assert hashlib.sha256(archive.read_bytes()).hexdigest()==build['artifacts'][archive.name]
entries={}
with tarfile.open(archive) as source:
 for member in source:
  path='/'.join(Path(member.name).parts[1:])
  if not path:continue
  assert path not in entries
  entries[path]={'type':member.type.decode(),'mode':member.mode,'linkname':member.linkname}
  if member.isfile():entries[path]['sha256']=hashlib.file_digest(source.extractfile(member),'sha256').hexdigest()
assert entries.keys()==baseline['entries'].keys()
changes={p:{'baseline':baseline['entries'][p],'current':entry} for p,entry in entries.items() if entry!=baseline['entries'][p]}
metadata={'SOURCE_REVISION.txt','docs/ROADMAP.md'}
runtime_changes=set(changes)-metadata
# Package construction, policies and launchers have not changed between these
# checkpoints. Current contents verification separately binds all four formats.
subprocess.run(['git','diff','--exit-code',baseline['source_revision'],build['source_revision'],'--','packaging','scripts','vendor','Cargo.toml','Cargo.lock','build-all.sh'],check=True,stdout=subprocess.PIPE)
report={'status':'pass unchanged native runtime admission' if not runtime_changes else 'changed runtime inputs require fresh native acceptance','source_revision':build['source_revision'],'baseline_source_revision':baseline['source_revision'],'baseline_archive_sha256':baseline['archive_sha256'],'current_archive_sha256':build['artifacts'][archive.name],'all_runtime_bytes_modes_links_identical':not runtime_changes,'changed_runtime_paths':sorted(runtime_changes),'changes':changes,'baseline_native_execution':str((B/'LINUX_PRIVILEGED_COPY_ACCEPTANCE.json').relative_to(R)),'baseline_login_execution':'.hermes/evidence/linux-login-current/ACCEPTANCE.json','scope':'Every portable payload file, directory, launcher, policy, dependency, file mode and link matches native-tested41c except the listed source note/roadmap. Current four-format content/closure verification and an actual current-AppImage FUSE smoke remain separate gates.'}
(E/'NATIVE_RUNTIME_EQUIVALENCE.json').write_text(json.dumps(report,indent=2)+'\n');print(report['status']);print('Runtime changes:',sorted(runtime_changes))
