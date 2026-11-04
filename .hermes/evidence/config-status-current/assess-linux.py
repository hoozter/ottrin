"""Assess exact-package native preservation/retry after reviewed original captures."""
from pathlib import Path
import json,hashlib
r=Path.cwd();e=Path(__file__).resolve().parent;f=e/'linux-candidate'
read=lambda p:json.loads(p.read_text())
control=read(f/'CONTROL.json');result=read(f/'FUNCTIONAL_RESULT.json');content=read(e/'PACKAGE_CONTENT_ACCEPTANCE.json');teardown=read(f/'TEARDOWN.json')
assert result['status'].startswith('pass native settings error/preservation/save-retry')
assert control['source_revision']==content['source_revision']==read(e/'BUILD_SOURCE.json')['source_revision']
assert control['artifact_sha256']==content['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
assert len(result['cases'])==7
for row in result['cases'][:4]:assert row['exit']==1 and row['original_bytes_preserved'] and 'preserved' in row['stderr']
assert result['cases'][4]['editor_stays_open'] and result['cases'][4]['original_config_bytes_preserved'] and result['cases'][4]['draft_radius']==24
assert result['cases'][5]['dark_radius']==24 and result['cases'][5]['light_radius']==0
assert result['cases'][6]['backend']=='active in the index daemon' and result['cases'][6]['login']=='Start index at login'
assert result['source_preserved'] and result['normal_exit']==0
assert teardown['status']=='closed' and teardown['profile_removed'] and not Path(teardown['profile']).exists()
assert hashlib.sha256((f/'WORKER_SOURCE.py').read_bytes()).hexdigest()==control['worker_sha256']==hashlib.sha256((e/'linux-settings-worker.py').read_bytes()).hexdigest()
captures=[]
for name,note in [('theme-save-refused.png','Reviewed original: editor and24px draft remain visible; complete read-only save error is visible below the action footer.'),('theme-save-retried.png','Reviewed original: editor closed after retry; Settings shows selected saved theme and shared24px window corners.'),('search-health.png','Reviewed original: watcher active in index daemon; login state separately says no login service installed, without falsely implying indexing is stopped.')]:
 p=f/name;captures.append({'path':str(p.relative_to(e)),'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'review':note})
v={'status':'pass scoped Linux native settings rejection/save-retry and Search status','source_revision':control['source_revision'],'artifact_sha256':control['artifact_sha256'],'cases':7,'captures':captures,'teardown':teardown,'namespace_gate_pending_at_native_launch':control['namespace_content_gate_pending_at_launch'],'content_gate_now_verified':True,'scope':'Exact AppImage/private X11/software Dark150. Main/Properties/Inspector/index CLI startup errors are stderr diagnostics, not Linux native error-dialog acceptance. Real editor read-only save refusal retains draft and original bytes; repaired retry persists. Search labels, normal close and source preservation pass. No whole-appearance, sustained CPU, physical graphics or whole-roadmap claim.'}
(e/'LINUX_NATIVE_ACCEPTANCE.json').write_text(json.dumps(v,indent=2)+'\n');print(v['status'])
