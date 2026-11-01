"""Native backend property contract on an owned bus, without a GUI or index."""
from pathlib import Path
import argparse,importlib.util,subprocess,os,json,hashlib
r=Path.cwd();e=Path(__file__).resolve().parent;parser=argparse.ArgumentParser();parser.add_argument('--phase',choices=['before','after'],required=True);phase=parser.parse_args().phase
spec=importlib.util.spec_from_file_location('workspace',r/'scripts/test-workspace.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
binary=r/'target/debug/ottrin-app';report={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'phase':phase};children=[]
try:
 with m.isolated_environment(prefix='ottrin-portal-property-') as (profile,env):
  with (e/(phase+'-bus.log')).open('w') as buslog,(e/(phase+'-backend.log')).open('w') as backendlog:
   try:
    bus=subprocess.Popen(['dbus-daemon','--session','--nofork','--print-address=1'],env=env,stdout=subprocess.PIPE,stderr=buslog,text=True,start_new_session=True);children.append(bus);env['DBUS_SESSION_BUS_ADDRESS']=bus.stdout.readline().strip()
    backend=subprocess.Popen([str(binary),'portal'],env=env,stdout=backendlog,stderr=backendlog,start_new_session=True);children.append(backend)
    subprocess.run(['gdbus','wait','--session','org.freedesktop.impl.portal.desktop.ottrin'],env=env,check=True,capture_output=True,timeout=5)
    values={}
    for name in ['version','Version']:
     p=subprocess.run(['gdbus','call','--session','--dest','org.freedesktop.impl.portal.desktop.ottrin','--object-path','/org/freedesktop/portal/desktop','--method','org.freedesktop.DBus.Properties.Get','org.freedesktop.impl.portal.FileChooser',name],env=env,capture_output=True,text=True,timeout=5);values[name]={'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr}
    report.update(properties=values,no_index_started=not (profile/'runtime/ottrin/index-v1.sock').exists())
    expected='Version' if phase=='before' else 'version';assert values[expected]['exit']==0 and 'uint32 4' in values[expected]['stdout'];assert values['version' if phase=='before' else 'Version']['exit']!=0
    report['status']='pass: control reproduces wrong property' if phase=='before' else 'pass: standard lowercase property is reachable'
   finally:
    for child in reversed(children):m.stop_tests(child)
    children=[]
  report['profile_removed']=False
 report['profile_removed']=not profile.exists()
finally:
 for child in reversed(children):m.stop_tests(child)
 (e/(phase.upper()+'.json')).write_text(json.dumps(report,indent=2)+'\n')
