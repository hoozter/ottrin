"""Compare private document-portal mount roots without changing the real session."""
from pathlib import Path
import importlib.util,subprocess,os,json,time
r=Path.cwd();e=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');d=importlib.util.module_from_spec(spec);spec.loader.exec_module(d)
results=[]
for case,parent in [('var-tmp','/var/tmp'),('user-runtime','/run/user/'+str(os.getuid()))]:
 children=[];report={'case':case,'parent':parent,'status':'starting'}
 def cleanup(profile):
  report['owned_children']=d.cleanup_profile_children(profile)
  report['owned_mounts_unmounted']=d.cleanup_owned_mounts(profile)
 try:
  with d.workspace.isolated_environment(directory=parent,prefix='ottrin-doc-qa-',before_remove=cleanup) as (profile,env):
   runtime=profile/'runtime';runtime.mkdir(mode=0o700);env['XDG_RUNTIME_DIR']=str(runtime);report['profile']=str(profile)
   with (e/(case+'-bus.log')).open('w') as buslog,(e/(case+'-document.log')).open('w') as documentlog:
    try:
     bus=subprocess.Popen(['dbus-daemon','--session','--nofork','--print-address=1'],env=env,stdout=subprocess.PIPE,stderr=buslog,text=True,start_new_session=True);children.append(bus);env['DBUS_SESSION_BUS_ADDRESS']=bus.stdout.readline().strip();assert env['DBUS_SESSION_BUS_ADDRESS'].startswith('unix:')
     doc=subprocess.Popen(['/usr/libexec/xdg-document-portal','--verbose'],env=env,stdout=documentlog,stderr=documentlog,start_new_session=True);children.append(doc)
     deadline=time.monotonic()+5
     while time.monotonic()<deadline:
      mounts=[line for line in Path('/proc/self/mountinfo').read_text().splitlines() if line.split()[4]==str(runtime/'doc')]
      if mounts or doc.poll() is not None:break
      time.sleep(.05)
     report.update(status='observed',document_exit=doc.poll(),mounts=mounts,mounted_document_portal=bool(mounts))
    finally:
     for child in reversed(children):d.workspace.stop_tests(child)
     children=[]
  report['profile_removed']=not profile.exists()
 finally:
  for child in reversed(children):d.workspace.stop_tests(child)
  results.append(report);(e/'PROBE.json').write_text(json.dumps(results,indent=2)+'\n')
print(json.dumps(results,indent=2))
