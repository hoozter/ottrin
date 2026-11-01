"""Exercise real private Documents FD export, FUSE reads, permissions and removal."""
import os,json
from pathlib import Path
from gi.repository import Gio,GLib
out=Path(os.environ['OTTRIN_DOCUMENT_QA']);fixture=Path(os.environ['OTTRIN_DOCUMENT_FIXTURE']);source=fixture/os.fsdecode(b'original\x80.txt');original=source.read_bytes()
c=Gio.DBusConnection.new_for_address_sync(os.environ['DBUS_SESSION_BUS_ADDRESS'],Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT|Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,None,None)
dest='org.freedesktop.portal.Documents';root='/org/freedesktop/portal/documents';iface=dest;document=None
report={'scope':'Direct production Documents API and actual private FUSE export; not a sandboxed application/chooser frontend identity check'}
def call(method,signature=None,args=()):return c.call_sync(dest,root,iface,method,GLib.Variant(signature,args) if signature else None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()
try:
 xml=c.call_sync(dest,root,'org.freedesktop.DBus.Introspectable','Introspect',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0];(out/'DOCUMENTS_INTROSPECTION.xml').write_text(xml)
 mount=bytes(call('GetMountPoint')[0]).rstrip(b'\0');assert mount==os.fsencode(os.environ['XDG_RUNTIME_DIR'])+b'/doc',mount
 fds=Gio.UnixFDList.new();fd=os.open(source,os.O_PATH|os.O_CLOEXEC)
 try:
  index=fds.append(fd);reply,returned=c.call_with_unix_fd_list_sync(dest,root,iface,'Add',GLib.Variant('(hbb)',(index,False,False)),GLib.VariantType.new('(s)'),Gio.DBusCallFlags.NONE,5000,fds,None);document=reply.unpack()[0]
 finally:os.close(fd)
 exported=Path(os.fsdecode(mount+b'/'+document.encode()+b'/'+os.fsencode(source.name)));assert exported.read_bytes()==original
 call('GrantPermissions','(ssas)',(document,'org.ottrin.QA',['read']))
 info=call('Info','(s)',(document,));assert info[1]['org.ottrin.QA']==['read'],info
 assert bytes(info[0]).rstrip(b'\0')==os.fsencode(source)
 call('Delete','(s)',(document,));document=None;assert not exported.exists();assert source.read_bytes()==original
 report.update(status='pass',native_source_hex=os.fsencode(source).hex(),native_source_preserved=True,exported_native_filename_read=True,permissions_verified=True,export_removed=True,mount_is_private=True)
finally:
 if document:call('Delete','(s)',(document,))
 c.close_sync(None);(out/'EXPORT_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
