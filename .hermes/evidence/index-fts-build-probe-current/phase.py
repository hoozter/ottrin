"""Replay storage phases only in a disposable consistent generated-cache copy."""
from pathlib import Path
from contextlib import closing
from urllib.parse import quote
import importlib.util,json,sqlite3,subprocess,os,uuid,hashlib,signal,resource
r=Path.cwd();out=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('workspace',r/'scripts/test-workspace.py');w=importlib.util.module_from_spec(spec);spec.loader.exec_module(w)
for sig in (signal.SIGINT,signal.SIGTERM):signal.signal(sig,w.cancelled)
resource.setrlimit(resource.RLIMIT_CORE,(0,0))
records=[json.loads(s) for s in (out/'search-build.jsonl').read_text().splitlines()]
assert records[-1]=={'reason':'build-finished','success':True}
exe=next(Path(d['executable']) for d in records if d.get('reason')=='compiler-artifact' and d.get('executable') and d['target']['name']=='ottrin_search')
source=Path.home()/'.cache/ottrin/search.db';config=json.loads((Path.home()/'.config/ottrin/config.json').read_text())['search'];assert not config['content_indexing_enabled']
pid=subprocess.check_output(['systemctl','--user','show','ottrin-index.service','--property=MainPID','--value'],text=True).strip();assert int(pid)>0
report={'scope':'Real Rust storage-phase replay only; private generated-cache copy; no filesystem freshness or daemon performance acceptance','binary_sha256':hashlib.file_digest(exe.open('rb'),'sha256').hexdigest(),'personal_daemon_pid_before':pid}
def readonly(path):return sqlite3.connect('file:'+quote(str(path))+'?mode=ro',uri=True,isolation_level=None)
try:
 with w.isolated_environment(directory='/var/tmp',prefix='.ottrin-index-phase-') as (root,env):
  nonce=uuid.uuid4().hex;(root/'.phase-owner').write_text(nonce)
  report['fixture']=str(root)
  (out/'PHASE_OWNER.json').write_text(json.dumps({'path':str(root),'purpose':report['scope'],'pending_consumer':'this bounded native Rust test; copies removed after its process exits'})+'\n')
  dest=Path(env['XDG_CACHE_HOME'])/'ottrin';dest.mkdir()
  with closing(readonly(source)) as src:
   src.execute('BEGIN');gen=src.execute('SELECT active_generation_id FROM search_store_state WHERE singleton=1').fetchone()[0]
   if not config.get('include_roots'):config['include_roots']=[os.fsdecode(row[0]) for row in src.execute('SELECT DISTINCT root_path FROM search_generation_items WHERE generation_id=?',[gen])]
   with closing(readonly(source.with_name(source.name+f'.content-{gen}.db'))) as content:
    content.execute('BEGIN')
    for inp,path in [(src,dest/'search.db'),(content,dest/f'search.db.content-{gen}.db')]:
     with closing(sqlite3.connect(path)) as target:inp.backup(target,pages=2048,sleep=.01)
    content.execute('COMMIT')
   src.execute('COMMIT')
  (root/'phase-config.json').write_text(json.dumps(config));env['OTTRIN_PHASE_PROFILE_ROOT']=str(root);env['OTTRIN_PHASE_PROFILE_NONCE']=nonce
  with (out/'phase.stdout').open('w') as stdout,(out/'phase.stderr').open('w') as stderr:
   result=subprocess.run([str(exe),'--exact','db::generation_tests::profile_staged_storage_phases','--ignored','--nocapture','--test-threads=1'],env=env,stdout=stdout,stderr=stderr,timeout=180)
  assert result.returncode==0,'Inspect owned phase logs'
  assert '1 passed' in (out/'phase.stdout').read_text(), 'Exact profiling test must execute'
  report.update(json.loads((root/'PHASES.json').read_text()));report['status']='pass for named phase replay'
 report['fixture_removed']=not root.exists()
 report['personal_daemon_pid_after']=subprocess.check_output(['systemctl','--user','show','ottrin-index.service','--property=MainPID','--value'],text=True).strip();assert report['personal_daemon_pid_after']==pid
finally:(out/'PHASES.json').write_text(json.dumps(report,indent=2)+'\n')
