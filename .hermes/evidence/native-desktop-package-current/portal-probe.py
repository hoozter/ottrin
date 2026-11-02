"""Real ordinary-user frontend request; verify the mapped chooser belongs to Ottrin."""
import json,os,subprocess,sys,time
from pathlib import Path
from gi.repository import Gio,GLib
expected=Path(sys.argv[1]);title='Ottrin Native RPM Picker QA'
connection=Gio.DBusConnection.new_for_address_sync(os.environ['DBUS_SESSION_BUS_ADDRESS'],Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT|Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,None,None)
handle=None;window=None;started=time.monotonic();report={'expected_executable':str(expected)}
def call(path,interface,method,parameters=None):
 return connection.call_sync('org.freedesktop.portal.Desktop',path,interface,method,parameters,None,Gio.DBusCallFlags.NONE,10000,None)
try:
 handle=call('/org/freedesktop/portal/desktop','org.freedesktop.portal.FileChooser','OpenFile',GLib.Variant('(ssa{sv})',('',title,{'handle_token':GLib.Variant('s','ottrin_native_rpm_qa'),'modal':GLib.Variant('b',False)}))).unpack()[0]
 deadline=time.monotonic()+15
 while time.monotonic()<deadline:
  result=subprocess.run(['xdotool','search','--onlyvisible','--name',title],capture_output=True,text=True,timeout=2)
  if result.returncode==0:
   ids=result.stdout.split();assert len(ids)==1,ids;window=ids[0]
   pid=int(subprocess.check_output(['xdotool','getwindowpid',window],text=True,timeout=2).strip())
   actual=Path(os.readlink(f'/proc/{pid}/exe'));assert actual==expected,(actual,expected)
   report.update(chooser_pid=pid,mapped_ms=round((time.monotonic()-started)*1000),window_title=subprocess.check_output(['xdotool','getwindowname',window],text=True,timeout=2).strip())
   break
  time.sleep(.05)
 else:raise RuntimeError('No mapped Ottrin chooser from the real desktop frontend')
 call(handle,'org.freedesktop.portal.Request','Close');handle=None
 deadline=time.monotonic()+5
 while time.monotonic()<deadline:
  if subprocess.run(['xdotool','getwindowname',window],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=2).returncode!=0:break
  time.sleep(.05)
 else:raise RuntimeError('Owned chooser remained visible after Request.Close')
 report['status']='pass'
finally:
 if handle:
  try:call(handle,'org.freedesktop.portal.Request','Close')
  finally:connection.close_sync(None)
 else:connection.close_sync(None)
print(json.dumps(report))
