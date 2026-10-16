"""Dismiss first-login shell UI only in this task's owned, isolated COW guest."""
from pathlib import Path
import json,socket,struct,subprocess,time,urllib.parse,xml.etree.ElementTree as ET,sys
p=Path(__file__).resolve().parent;owner=json.loads((p/'ottrin-init-windows-eps-qa-OWNERSHIP.json').read_text());domain=owner['domain'];assert domain=='ottrin-init-windows-eps-qa'
def virsh(*args):return subprocess.check_output(['virsh','-c','qemu:///system',*args],text=True).strip()
xml=ET.fromstring(virsh('dumpxml',domain));assert xml.find('./devices/disk/source').get('file')=='/var/lib/libvirt/images/'+owner['volume'];assert not xml.findall('./devices/interface')
url=urllib.parse.urlparse(virsh('domdisplay',domain));port=int(xml.find('./devices/graphics').get('port'));assert url.scheme=='vnc' and url.hostname=='127.0.0.1' and 5900<=port<6000 and url.port in [port,port-5900]
with socket.create_connection((url.hostname,port),timeout=10) as stream:
 def read(n):
  data=b''
  while len(data)<n:
   block=stream.recv(n-len(data));assert block;data+=block
  return data
 assert read(12).startswith(b'RFB ');stream.sendall(b'RFB 003.008\n');count=read(1)[0];types=read(count);assert 1 in types;stream.sendall(b'\x01');assert read(4)==b'\0'*4;stream.sendall(b'\x01');header=read(24);width,height=struct.unpack('!HH',header[:4]);length=struct.unpack('!I',header[20:])[0];assert length<4096;name=read(length);assert domain.encode() in name,name
 for down in ([] if '--app-live' in sys.argv else [1,0]):stream.sendall(struct.pack('!BBHI',4,down,0,0xff1b));time.sleep(.1)
 for mask in [1,0]:stream.sendall(struct.pack('!BBHH',5,mask,100 if '--app-live' in sys.argv else 5,25 if '--app-live' in sys.argv else 5));time.sleep(.1)
 time.sleep(.3)
(p/'GUEST_MENU_DISMISSAL.json').write_text(json.dumps({'domain':domain,'loopback_vnc':port,'framebuffer':[width,height],'input':'Title-bar click outside menu during owned app run' if '--app-live' in sys.argv else 'Escape and blank-desktop click before app launch','scope':'Owned no-NIC COW guest only; original three domains remain off'},indent=2)+'\n')
