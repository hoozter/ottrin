"""Retire only named test executables superseded in the same compiler configuration."""
from pathlib import Path
import json, subprocess, hashlib, shutil
r=Path.cwd();e=Path(__file__).resolve().parent
pairs=[('ottrin-app','test-bin-ottrin-app','ottrin_app','cde13dff8326dae7',['5a7d15a1d5cb82cd','784d090767fe7159','66202a2670c4eb88','582da267dc7093f9']),('ottrin-ui','test-lib-ottrin_ui','ottrin_ui','3ec86bde0f8d579d',['bf8f43a2b00d6c52']),('ottrin-platform','test-lib-ottrin_platform','ottrin_platform','78311865f0064c30',['f1839937b05b6fe2','2a29cbaf6c6f8a33'])]
keys=['rustc','features','declared_features','target','profile','path','rustflags','config','compile_kind'];plan=[]
for package,kind,stem,current,old in pairs:
 replacement=r/'target/debug/deps'/f'{stem}-{current}';fp=r/'target/debug/.fingerprint'/f'{package}-{current}';new=json.loads((fp/(kind+'.json')).read_text());listed=subprocess.run([str(replacement),'--list'],capture_output=True,text=True,check=True);assert 'test' in listed.stdout
 for suffix in old:
  artifact=r/'target/debug/deps'/f'{stem}-{suffix}';fingerprint=r/'target/debug/.fingerprint'/f'{package}-{suffix}';prior=json.loads((fingerprint/(kind+'.json')).read_text());assert all(prior.get(key)==new.get(key) for key in keys),(artifact,'compiler configuration differs');assert artifact.stat().st_mtime_ns<replacement.stat().st_mtime_ns;assert artifact.stat().st_nlink==1
  live=subprocess.run(['fuser',str(artifact)],capture_output=True,text=True);assert live.returncode==1 and not live.stdout,(artifact,live.stdout,live.stderr)
  refs=subprocess.run(['rg','-l','--fixed-strings',suffix,'.hermes/evidence','scripts','-g','*.py','-g','*.ps1','-g','*.sh'],capture_output=True,text=True);assert refs.returncode in (0,1)
  hits=[line for line in refs.stdout.splitlines() if line!=str(Path(__file__).relative_to(r))];assert not hits,(artifact,hits)
  depinfo=artifact.with_suffix('.d');paths=[artifact];paths.extend([depinfo] if depinfo.exists() else [])
  plan.append({'artifact':str(artifact),'replacement':str(replacement),'bytes':sum(p.stat().st_size for p in paths),'sha256':hashlib.sha256(artifact.read_bytes()).hexdigest(),'compiler_keys_matched':keys,'current_replacement_lists_tests':True,'live_consumers':False,'producer_references':hits,'hard_links':1,'paths':paths,'fingerprint':fingerprint})
(e/'REPLACED_TEST_CLEANUP_PLAN.json').write_text(json.dumps([{k:v for k,v in item.items() if k not in ('paths','fingerprint')} for item in plan],indent=2)+'\n')
for item in plan:
 for path in item['paths']:path.unlink()
 shutil.rmtree(item['fingerprint'])
(e/'REPLACED_TEST_CLEANUP.json').write_text(json.dumps({'status':'pass','retired':len(plan),'logical_bytes_removed':sum(item['bytes'] for item in plan),'retained':'Current binaries and test replacements,dependency libraries,other compiler configurations and useful caches'},indent=2)+'\n')
