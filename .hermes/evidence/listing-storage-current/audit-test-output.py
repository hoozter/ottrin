"""Discover exact-configuration superseded standalone test outputs; no deletion."""
from pathlib import Path
import json,os,subprocess,time
r=Path.cwd();out=Path(__file__).resolve().parent;groups={};preserved=[]
fields=['rustc','features','declared_features','target','profile','path','rustflags','config','compile_kind']
for configuration in ['debug','x86_64-pc-windows-gnu/debug']:
 root=r/'target'/configuration
 for folder in (root/'.fingerprint').glob('ottrin-*'):
  fingerprints=list(folder.glob('test-*.json'))
  if len(fingerprints)!=1:continue
  fp=fingerprints[0];meta=json.loads(fp.read_text());suffix=folder.name.rsplit('-',1)[1];name=fp.stem.split('-',2)[2].replace('-','_');binary=root/'deps'/(name+'-'+suffix+('.exe' if configuration.startswith('x86') else ''))
  if not binary.is_file():continue
  companions=[binary,binary.with_suffix('.d') if binary.suffix=='.exe' else binary.with_name(binary.name+'.d'),folder]
  # Windows Cargo dep-info omits the executable .exe extension.
  assert companions[1].exists(),companions[1]
  expected={fp.name,fp.stem,'dep-'+fp.stem,'invoked.timestamp'}
  if not set(p.name for p in folder.iterdir())<=expected:preserved.append({'path':str(binary),'reason':'Fingerprint has additional consumers/outputs'});continue
  key=json.dumps({'configuration':configuration,'crate':folder.name.rsplit('-',1)[0],'test_target':fp.stem,**{k:meta[k] for k in fields}},sort_keys=True)
  groups.setdefault(key,[]).append({'binary':str(binary),'modified_ns':binary.stat().st_mtime_ns,'bytes':binary.stat().st_size,'links':binary.stat().st_nlink,'paths':list(map(str,companions)),'fingerprint':str(fp)})
candidates=[];retained=[]
for key,items in groups.items():
 items.sort(key=lambda a:a['modified_ns'],reverse=True);newest=items[0];retained.append({'group':json.loads(key),'replacement':newest})
 for old in items[1:]:
  if old['links']!=1:preserved.append({'path':old['binary'],'reason':'Additional hard-link consumer retained'});continue
  if newest['modified_ns']<=old['modified_ns']:raise AssertionError('Replacement must be strictly newer')
  candidates.append({'group':json.loads(key),'obsolete':old,'replacement':newest})
report={'status':'read-only candidate discovery; references/processes/replacement verification still required','standalone_tests_only':True,'groups':retained,'candidates':candidates,'preserved':preserved,'candidate_binary_bytes':sum(x['obsolete']['bytes'] for x in candidates),'prohibited':'No library/dependency/current binary/incremental/session/source/package removal'}
(out/'TEST_OUTPUT_CANDIDATES.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'configuration_groups':len(groups),'candidate_executables':len(candidates),'candidate_binary_bytes':report['candidate_binary_bytes'],'hardlinks_or_additional_consumers_preserved':len(preserved)},indent=2))
