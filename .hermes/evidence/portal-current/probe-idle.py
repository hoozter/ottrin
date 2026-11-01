import importlib.util, pathlib, subprocess, os, time, json
root=pathlib.Path.cwd(); spec=importlib.util.spec_from_file_location('workspace',root/'scripts/test-workspace.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
report={};children=[]
try:
 with module.isolated_environment(prefix='ottrin-portal-idle-') as (profile,env):
  env['XDG_CURRENT_DESKTOP']='OTTRINQA';definitions=profile/'definitions';definitions.mkdir();(definitions/'portals.conf').write_text('[preferred]\ndefault=none\n');env['XDG_DESKTOP_PORTAL_DIR']=str(definitions)
  bus=subprocess.Popen(['dbus-daemon','--session','--nofork','--print-address=1'],env=env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,start_new_session=True);children.append(bus);env['DBUS_SESSION_BUS_ADDRESS']=bus.stdout.readline().strip()
  with open(root/'.hermes/evidence/portal-current/IDLE_FRONTEND.log','w') as log:
   frontend=subprocess.Popen(['/usr/libexec/xdg-desktop-portal','--replace'],env=env,stdout=log,stderr=log,start_new_session=True);children.append(frontend)
   for _ in range(100):
    call=subprocess.run(['gdbus','call','--session','--dest','org.freedesktop.DBus','--object-path','/org/freedesktop/DBus','--method','org.freedesktop.DBus.NameHasOwner','org.freedesktop.portal.Desktop'],env=env,capture_output=True,text=True,timeout=3)
    if 'true' in call.stdout:break
    time.sleep(.05)
   for suffix in ['request','session']:
    call=subprocess.run(['gdbus','call','--session','--dest','org.freedesktop.portal.Desktop','--object-path','/org/freedesktop/portal/desktop/'+suffix,'--method','org.freedesktop.DBus.Introspectable.Introspect'],env=env,capture_output=True,text=True,timeout=3);report[suffix]={'code':call.returncode,'stdout':call.stdout,'stderr':call.stderr}
   for child in reversed(children):module.stop_tests(child)
   children=[]
finally:
 for child in reversed(children):module.stop_tests(child)
(root/'.hermes/evidence/portal-current/IDLE_INTROSPECTION.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
