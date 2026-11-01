"""Public portal client; the real desktop frontend activates the Ottrin backend."""
import json,os,time
from pathlib import Path
from gi.repository import Gio,GLib
p=Path(os.environ['OTTRIN_PORTAL_QA']);fixture=os.fsencode(os.environ['OTTRIN_PORTAL_FIXTURE']);parent=os.environ['OTTRIN_PORTAL_PARENT'];responses={};requests={};ctx=GLib.MainContext.default();deadline=time.monotonic()+150
c=Gio.DBusConnection.new_for_address_sync(os.environ['DBUS_SESSION_BUS_ADDRESS'],Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT|Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,None,None)
def write(name,data):
 f=p/name;t=f.with_name(f.name+'.next');t.write_text(json.dumps(data,indent=2)+'\n');os.replace(t,f)
def call(dest,path,iface,method,args=None,reply=None):return c.call_sync(dest,path,iface,method,args,GLib.VariantType.new(reply) if reply else None,Gio.DBusCallFlags.NONE,10000,None)
def pump():
 while ctx.pending():ctx.iteration(False)
 assert time.monotonic()<deadline;time.sleep(.01)
def gate(name):
 write('STAGE.json',{'waiting_for':name})
 while not (p/('GO-'+name)).exists():pump()
def replied(connection,sender,path,interface,signal,parameters,data):responses[path]=parameters.unpack();write('RESPONSES.json',responses)
c.signal_subscribe('org.freedesktop.portal.Desktop','org.freedesktop.portal.Request','Response',None,None,Gio.DBusSignalFlags.NONE,replied,None)
# Set only this private bus's activation environment, never the real session.
keys=['DISPLAY','DBUS_SESSION_BUS_ADDRESS','XDG_RUNTIME_DIR','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME','XDG_STATE_HOME','HOME','XDG_DESKTOP_PORTAL_DIR','WINIT_UNIX_BACKEND','LIBGL_ALWAYS_SOFTWARE','NO_AT_BRIDGE']
call('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','UpdateActivationEnvironment',GLib.Variant('(a{ss})',({k:os.environ[k] for k in keys if k in os.environ},)),'()')
gate('start')
def request(key,title,method='OpenFile',options=None):
 opts=options or {};opts['handle_token']=GLib.Variant('s',key)
 path=call('org.freedesktop.portal.Desktop','/org/freedesktop/portal/desktop','org.freedesktop.portal.FileChooser',method,GLib.Variant('(ssa{sv})',(parent,title,opts)),'(o)').unpack()[0];requests[key]=path;write('REQUESTS.json',requests);return path

def wait_response(key):
 while requests[key] not in responses:pump()
 return responses[requests[key]]
try:
 path=request('cancel','Real portal Cancel QA');gate('close-cancel');call('org.freedesktop.portal.Desktop',path,'org.freedesktop.portal.Request','Close',None,'()')
 # Public Request.Close may intentionally suppress the public Response signal.
 gate('save')
 request('save','Real portal native Save QA','SaveFile',{'current_file':GLib.Variant('ay',fixture+b'/original\x80.txt\0')});assert wait_response('save')[0]==0;gate('batch')
 request('batch','Real portal batch Save QA','SaveFiles',{'current_folder':GLib.Variant('ay',fixture+b'\0'),'files':GLib.Variant('aay',[b'original\x80.txt\0',b'other.txt\0'])});assert wait_response('batch')[0]==0
 write('CLIENT_RESULT.json',{'status':'pass','requests':requests,'responses':responses});gate('finish')
finally:c.close_sync(None)
