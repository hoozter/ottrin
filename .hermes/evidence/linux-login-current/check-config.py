from pathlib import Path
import importlib.util,json,os,subprocess,sys
sys.dont_write_bytecode=True
R=Path.cwd();E=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('workspace',R/'scripts/test-workspace.py');w=importlib.util.module_from_spec(spec);spec.loader.exec_module(w)
with w.isolated_environment() as (profile,env):
 runtime=profile/'runtime';runtime.mkdir();env['XDG_RUNTIME_DIR']=str(runtime);env['DBUS_SESSION_BUS_ADDRESS']='unix:path='+str(runtime/'no-bus')
 config=profile/'config/ottrin/config.json';config.parent.mkdir();data={'theme':'Dark','default_view_mode':'Miller','search_setup_completed':True,'window_size':[1100,800],'search':{'include_roots':[str(profile/'fixtures')],'content_indexing_enabled':False}};config.write_text(json.dumps(data))
 p=subprocess.run([str(R/'target/release/ottrin-app'),'index','status','--json'],env=env,capture_output=True,text=True,timeout=15)
 report={'scope':'Read-only CLI status with disposable config and runtime; host DBus explicitly unavailable','exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr}
 assert p.returncode!=0 and 'list_columns' in p.stderr,report
report['profile_removed']=not profile.exists();(E/'CONFIG_INPUT_FAILURE.json').write_text(json.dumps(report,indent=2)+'\n');print(report)
