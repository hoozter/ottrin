"""Scope source-GUI behavior to exact compiled bytes and reviewed original scenes."""
from pathlib import Path
import hashlib,json,subprocess
E=Path(__file__).resolve().parent;F=E/'native-source'
def read(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def save(p,v):p.write_text(json.dumps(v,indent=2)+'\n')
r=read(F/'RESULT.json');b=read(E/'GUI_BUILD_RESULT.json');t=read(F/'TEARDOWN.json')
assert r['status'].startswith('pass') and b['status']=='pass' and r['source_revision']==b['source_revision']
assert r['gui_sha256']==b['gui_sha256'] and r['index_daemon_sha256']==b['index_daemon_sha256'] and b['daemon_byte_identity_unchanged']
assert len(r['themes'])==2 and {x['theme'] for x in r['themes']}=={'Dark','Light'}
for x in r['themes']:assert x['transitions']==['Ready','Stopped','Needs attention','Ready'] and x['originals_preserved'] and x['same_gui_pid_across_transitions']>0 and x['recovered_daemon_pid_survives_gui_close']>0
assert len(r['settings_close_query_control'])==8 and all(x['settings_closed_exact_pid_and_name'] and x['superseded_any_match_query_still_finds_main'] for x in r['settings_close_query_control'])
assert t['status']=='closed' and t['profile_removed'] and not Path(t['profile']).exists()
assert all(x['exit'] is not None for x in t['owned_processes_reaped'])
# The sole derived crop was used only to inspect the tiny red footer glyph.
(F/'REVIEW_CROP.png').unlink(missing_ok=True)
frames={p.name:sha(p) for p in F.glob('*.png')};assert len(frames)==16 and frames==r['captures']
review={'status':'all16 original source GUI frames directly reviewed','original_captures':frames,'findings':['Both themes: Ready/recovered main icon and Services status are green.','Both themes: stopped main icon and Services status are neutral; no stale connection-error line.','Both themes: wrong-type endpoint shows red main icon and Needs attention with the real connect/probe error.','All8 Settings closes target the exact accessible viewport control and leave the main window alive. The corrected AND query confirms Settings is gone while the superseded any-match query still finds the main window.'],'scope':'Source release GUI on owned software X11 desktop/default DPI only; no distributed-package, Windows native, protected-Copy, personal profile, resource or full-interface acceptance.'}
save(F/'CAPTURE_REVIEW.json',review)
save(E/'NATIVE_SOURCE_ACCEPTANCE.json',{'status':'pass exact native source GUI stopped/error/recovery scope','source_revision':b['source_revision'],'gui_sha256':b['gui_sha256'],'daemon_sha256':b['index_daemon_sha256'],'same_gui_across_each_theme_transitions':True,'daemon_survives_normal_main_close':True,'originals_preserved':True,'settings_query_false_positive_reproduced':True,'original_captures_directly_reviewed':frames,'owned_teardown':t,'scope':review['scope'],'limits':['Not in current b020 stable packages; feature remains on branch/index-status-observation pending hosted/native Windows and package delivery gates.','Previous failed observer attempts remain failures, not accepted runs.']})
removed=[]
for name in ['native-source-focus-failed','native-source-shortcut-failed','native-source-query-failed']:
 old=E/name;cleanup=read(old/'TEARDOWN.json');assert cleanup['profile_removed'] and not Path(cleanup['profile']).exists()
 files=[p for p in old.iterdir() if p.name not in {'RESULT.json','TEARDOWN.json','WORKER_SOURCE.py','worker.log','RETIRED_OUTPUTS.json'}]
 for p in files:
  assert p.is_file() and not p.is_symlink() and p.stat().st_nlink==1
  assert subprocess.run(['fuser',str(p)],capture_output=True).returncode==1
  removed.append({'path':str(p.relative_to(E)),'bytes':p.stat().st_size,'sha256':sha(p)});p.unlink()
 save(old/'RETIRED_OUTPUTS.json',{'status':'consumed observer diagnostics retired; compact failed outcome, exact frozen worker and teardown retained','replacement':'native-source/RESULT.json','replacement_source':b['source_revision'],'retired':[x for x in removed if x['path'].startswith(name+'/')]})
save(E/'NATIVE_SOURCE_CLEANUP.json',{'status':'all4 private profiles/displays/buses/process trees removed; disposable failure captures/logs retired','retired_outputs':removed,'retained':[{'path':str(F),'purpose':'16 reviewed original source-GUI frames and compact acceptance/teardown evidence'},{'path':str(Path.cwd()/'target/release'),'purpose':'shared canonical release cache; compiled b961 source GUI for pending feature integration/package delivery'}],'no_owned_local_test_process_remaining':True})
print('Accepted16 directly reviewed original source-GUI scenes; retired consumed failed-observer output')
