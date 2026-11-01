"""Private fake frontend exercises the standard backend methods, not product bypasses."""
import os,json,time
from pathlib import Path
from gi.repository import Gio,GLib
p=Path(os.environ['OTTRIN_PORTAL_QA']);fixture=os.fsencode(os.environ['OTTRIN_PORTAL_FIXTURE']);parent=os.environ['OTTRIN_PORTAL_PARENT'];bus='org.freedesktop.impl.portal.desktop.ottrin';obj='/org/freedesktop/portal/desktop';iface='org.freedesktop.impl.portal.FileChooser';results={}
def write(name,data):
 f=p/name;t=f.with_name(f.name+'.next');t.write_text(json.dumps(data,indent=2)+'\n');os.replace(t,f)
def connect():return Gio.DBusConnection.new_for_address_sync(os.environ['DBUS_SESSION_BUS_ADDRESS'],Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT|Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,None,None)
def call(c,dest,path,interface,method,args,reply=None):return c.call_sync(dest,path,interface,method,args,GLib.VariantType.new(reply) if reply else None,Gio.DBusCallFlags.NONE,10000,None)
def name(c):
 r=call(c,'org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','RequestName',GLib.Variant('(su)',('org.freedesktop.portal.Desktop',4)),'(u)').unpack();assert r==(1,),r
frontend=connect();name(frontend);rogue=connect();write('CLIENT.json',{'status':'ready','owner':frontend.get_unique_name()})
ctx=GLib.MainContext.default();deadline=time.monotonic()+180

def pump():
 while ctx.pending():ctx.iteration(False)
 assert time.monotonic()<deadline,'Owned frontend lease expired';time.sleep(.01)
def gate(name):
 write('STAGE.json',{'waiting_for':name})
 while not (p/('GO-'+name)).exists():pump()
def wait_result(name):
 while name not in results:pump()
 return results[name]
def request(c,key,title,method='OpenFile',options=None,handle=None):
 path=handle or '/org/freedesktop/portal/desktop/request/qa/'+key
 def completed(source,result,key):
  try:r=source.call_finish(result).unpack();results[key]={'code':r[0],'values':r[1]}
  except GLib.Error as error:results[key]={'error':error.message}
  write('RESPONSES.json',results)
 c.call(bus,obj,iface,method,GLib.Variant('(osssa{sv})',(path,'org.ottrin.QA',parent,title,options or {})),GLib.VariantType.new('(ua{sv})'),Gio.DBusCallFlags.NONE,90000,None,completed,key)
 return path

def close(c,path):call(c,bus,path,'org.freedesktop.impl.portal.Request','Close',None,'()')
gate('start')
try:
 request(rogue,'unauthorized','Must never appear');assert 'error' in wait_result('unauthorized')
 path=request(frontend,'cancel','Portal Close QA');gate('close-cancel');close(frontend,path);assert wait_result('cancel')['code']==1
 gate('save')
 options={'current_file':GLib.Variant('ay',fixture+b'/original\x80.txt\0'),'current_folder':GLib.Variant('ay',b'/wrong\0'),'current_name':GLib.Variant('s','wrong.txt')}
 request(frontend,'save','Portal native Save QA','SaveFile',options);assert wait_result('save')['code']==0;gate('batch')
 options={'current_folder':GLib.Variant('ay',fixture+b'\0'),'files':GLib.Variant('aay',[b'original\x80.txt\0',b'original\x80.txt\0',b'second.txt\0'])}
 request(frontend,'batch','Portal batch Save QA','SaveFiles',options);assert wait_result('batch')['code']==0;gate('concurrent')
 a=request(frontend,'concurrent_a','Portal concurrent A');b=request(frontend,'concurrent_b','Portal concurrent B')
 request(frontend,'duplicate','Must never duplicate',handle=a);assert 'error' in wait_result('duplicate')
 try:close(rogue,a)
 except GLib.Error as error:results['unauthorized_close']={'error':error.message};write('RESPONSES.json',results)
 else:raise AssertionError('Rogue close accepted')
 gate('close-a');close(frontend,a);assert wait_result('concurrent_a')['code']==1;assert 'concurrent_b' not in results;gate('close-b');close(frontend,b);assert wait_result('concurrent_b')['code']==1;gate('owner-loss')
 request(frontend,'lost_owner','Portal lost owner QA');gate('disconnect-owner');frontend.close_sync(None);gate('reconnect')
 frontend=connect();name(frontend);request(frontend,'reconnected','Portal reconnected QA');gate('close-reconnected');close(frontend,'/org/freedesktop/portal/desktop/request/qa/reconnected');assert wait_result('reconnected')['code']==1
 write('CLIENT_RESULT.json',{'status':'pass','results':results});gate('finish')
finally:
 frontend.close_sync(None);rogue.close_sync(None)
