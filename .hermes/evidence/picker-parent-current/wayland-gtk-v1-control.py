"""Owned KWin virtual compositor plus real GTK foreign surface export."""
from pathlib import Path
import importlib.util,json,subprocess,hashlib,time,os
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'wayland';assert not out.exists();binary=r/'target/debug/ottrin-app'
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
report={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'scope':'Actual native Wayland chooser attachment to GTK exported foreign parent and parent-loss cancellation in owned virtual KWin; not physical desktop or Windows acceptance'}
try:
 with desktop.OwnedDesktop(out) as d:
  compiler=['cc',str(e/'wayland-caller.c'),'-o',str(d.profile/'caller'),*subprocess.check_output(['pkg-config','--cflags','--libs','gtk+-3.0'],text=True).split()];subprocess.run(compiler,check=True)
  wm=d.spawn(['kwin_wayland','--virtual','--width','1280','--height','900','--socket','ottrin-parent-qa','--no-lockscreen','--no-global-shortcuts'],'wayland-wm.log');deadline=time.monotonic()+20
  while not (d.profile/'runtime/ottrin-parent-qa').exists():
   assert wm.poll() is None and time.monotonic()<deadline;time.sleep(.1)
  d.env.update(WAYLAND_DISPLAY='ottrin-parent-qa',WINIT_UNIX_BACKEND='wayland',GDK_BACKEND='wayland',WAYLAND_DEBUG='1')
  fixture=d.profile/'fixture';fixture.mkdir();f=fixture/'original.txt';f.write_bytes(b'unchanged\n')
  parent=d.spawn([str(d.profile/'caller')],'caller.stderr',pipe=True);handle=d.ready_line(parent);assert handle and '\x00' not in handle
  report['exported_handle']=handle
  request={'title':'Owned native Wayland picker QA','parent_window':'wayland:'+handle,'initial_directory':{'encoding':'unix-bytes','units_hex':os.fsencode(fixture).hex()}}
  app=d.spawn([str(binary),'picker'],'picker.stderr',stdin=True,output_name='response.json');app.stdin.write(json.dumps(request));app.stdin.close();deadline=time.monotonic()+20
  while True:
   assert app.poll() is None,('Picker exited early',app.returncode)
   log=(out/'picker.stderr').read_text()
   if 'set_parent_of(' in log:break
   assert time.monotonic()<deadline,'No native Wayland parent request';time.sleep(.1)
  time.sleep(.4);assert app.poll() is None and wm.poll() is None
  parent.terminate();parent.wait(timeout=5);app.wait(timeout=5);assert app.returncode==0
  response=json.loads((out/'response.json').read_text());assert not response['accepted'] and response['paths']==[]
  preferences=json.loads((d.profile/'config/ottrin/picker.json').read_text());assert preferences['last_directory']['units_hex']==os.fsencode(fixture).hex()
  assert f.read_bytes()==b'unchanged\n';assert not (d.profile/'runtime/ottrin/index-v1.sock').exists();assert wm.poll() is None
  report.update(status='pass',native_import_and_parent_request=True,parent_loss_cancelled=True,normal_exit=0,preferences_saved=True,original_bytes_preserved=True,no_index_started=True,compositor_survived=True)
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
