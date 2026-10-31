"""Read the complete native icon through Xlib,including properties xprop truncates."""
from pathlib import Path
import ctypes as c,hashlib,importlib.util,json,sys
from PIL import Image
p=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('native_actions',p/'native-actions.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
s=m.Session(sys.argv[1]);x=c.CDLL('libX11.so.6')
x.XOpenDisplay.argtypes=[c.c_char_p];x.XOpenDisplay.restype=c.c_void_p
x.XInternAtom.argtypes=[c.c_void_p,c.c_char_p,c.c_int];x.XInternAtom.restype=c.c_ulong
x.XGetWindowProperty.argtypes=[c.c_void_p,c.c_ulong,c.c_ulong,c.c_long,c.c_long,c.c_int,c.c_ulong,c.POINTER(c.c_ulong),c.POINTER(c.c_int),c.POINTER(c.c_ulong),c.POINTER(c.c_ulong),c.POINTER(c.POINTER(c.c_ubyte))]
x.XFree.argtypes=[c.c_void_p];x.XCloseDisplay.argtypes=[c.c_void_p]
d=x.XOpenDisplay(s.env['DISPLAY'].encode());assert d
raw=c.POINTER(c.c_ubyte)()
try:
 atom=x.XInternAtom(d,b'_NET_WM_ICON',1);assert atom
 kind=c.c_ulong();fmt=c.c_int();count=c.c_ulong();remaining=c.c_ulong()
 assert x.XGetWindowProperty(d,int(s.window),atom,0,262144,0,0,c.byref(kind),c.byref(fmt),c.byref(count),c.byref(remaining),c.byref(raw))==0
 assert fmt.value==32 and count.value==65538 and remaining.value==0
 values=[v&0xffffffff for v in c.cast(raw,c.POINTER(c.c_ulong))[:count.value]];assert values[:2]==[256,256]
 image=Image.open(Path.cwd()/'assets/icon.png').convert('RGBA');assert image.size==(256,256)
 assert values[2:]==[a<<24|r<<16|g<<8|b for r,g,b,a in image.getdata()]
finally:
 if raw:x.XFree(raw)
 x.XCloseDisplay(d)
(s.out/'ICON_VALIDATED.json').write_text(json.dumps({'status':'pass','width':256,'height':256,'all_argb_pixels_match':True,'complete_cardinals':len(values),'property_u32le_sha256':hashlib.sha256(b''.join(v.to_bytes(4,'little') for v in values)).hexdigest(),'scope':'Actual packaged native window icon;no physical taskbar acceptance','xlib_storage_released':True},indent=2)+'\n')
print('Complete65538 native icon cardinals match the asset')
