"""Check that actual helper references match supported destination spellings."""
from pathlib import Path
import hashlib,importlib.util,json,os,subprocess,sys
sys.dont_write_bytecode=True
r=Path.cwd();e=Path(__file__).resolve().parent;spec=importlib.util.spec_from_file_location('w',r/'scripts/test-workspace.py');w=importlib.util.module_from_spec(spec);spec.loader.exec_module(w)
assert os.geteuid()!=0
binary=r/'target/debug/ottrin-priv-helper';report={'status':'running','helper_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'scope':'Actual debug helper ordinary-user stdio destination symlink/dot-component spellings; not additional native elevation acceptance','cases':[]}
with w.isolated_environment() as (profile,env):
 source=profile/'source.txt';source.write_text('alias fixture');destination=profile/'destination';destination.mkdir();alias=profile/'alias';alias.symlink_to(destination,target_is_directory=True)
 def request(command):
  p=subprocess.run([str(binary),'--stdio-json'],input=json.dumps({'command':command,'context':{'cwd':str(profile)}}),capture_output=True,text=True,env=env,timeout=15);assert p.returncode==0 and not p.stderr;return json.loads(p.stdout)
 for spelling in [alias,destination/'..'/'destination']:
  result=request({'File':{'Copy':{'sources':[str(source)],'destination':str(spelling),'conflict':'Overwrite'}}});assert result['status']=='Success',result
  reference=result['payload']['Copy'];parent=Path(reference['paths'][0][1]).parent;assert parent==spelling,(str(spelling),str(parent))
  assert (destination/source.name).read_text()=='alias fixture'
  assert request({'UndoCopy':{'transactions':reference['transactions']}})['status']=='Success'
  assert request({'CleanupCopy':{'transactions':reference['transactions']}})['status']=='Success'
  assert not (destination/source.name).exists();report['cases'].append({'destination_spelling':str(spelling.relative_to(profile)),'reference_parent_matches_request':True,'undo_and_cleanup':'Success'})
report.update(status='pass actual helper reference spellings for symlink and dot-component destinations',private_profile_removed=not profile.exists())
(e/'ALIAS_ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
