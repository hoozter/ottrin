from pathlib import Path
import importlib.util,json,hashlib,time,re,subprocess,signal
p=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('native_actions',p/'native-actions.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);s=module.Session('dark100')
completed=False
def cancelled(*_): raise KeyboardInterrupt("owned GUI smoke cancelled")
for sig in [signal.SIGTERM,signal.SIGINT]: signal.signal(sig,cancelled)
try:
 raw=s.run(['xprop','-id',s.window,'-len','1048576','-f','_NET_WM_ICON','32c','_NET_WM_ICON']).stdout
 values=[int(v.strip()) for v in raw.split('=',1)[1].split(',')];width,height=values[:2];assert width>0 and height>0 and len(values)>=width*height+2 and any(values[2:])
 from PIL import Image
 icon=Image.open(s.root/'assets/icon.png').convert('RGBA');expected=[(a<<24)|(r<<16)|(g<<8)|b for r,g,b,a in icon.getdata()];assert icon.size==(width,height) and values[2:]==expected
 (s.out/'ICON_VALIDATED.json').write_text(json.dumps({'status':'pass','width':width,'height':height,'elements':len(values),'argb_property_sha256':hashlib.sha256(raw.encode()).hexdigest(),'scope':'All native ARGB pixels match assets/icon.png; Ottrin WM_CLASS; no physical taskbar acceptance','validation_correction':'Use explicit xprop length above default 250000 bytes to avoid truncating a 256x256 icon; prior failed size assertion was a reader limit' },indent=2)+'\n')
 contact=s.fixture/'contact/contact.vcf';before=hashlib.sha256(contact.read_bytes()).hexdigest();s.nav(contact.parent);s.click(150,115);s.keys('space')
 def previews():
  s.guard();r=subprocess.run(['xdotool','search','--onlyvisible','--name','Quick Inspector'],env=s.env,capture_output=True,text=True);return r.stdout.splitlines() if r.returncode==0 else []
 end=time.monotonic()+15
 while not previews() and time.monotonic()<end:time.sleep(.1)
 windows=previews();assert len(windows)==1,windows
 s.run(['xdotool','windowactivate','--sync',windows[0]]);time.sleep(.3);s.run(['import','-window',windows[0],str(s.out/'vcf-preview.png')]);s.run(['xdotool','key','--clearmodifiers','space'])
 end=time.monotonic()+10
 while previews() and time.monotonic()<end:time.sleep(.1)
 assert not previews(),'Space failed to close read-only contact preview';assert hashlib.sha256(contact.read_bytes()).hexdigest()==before
 s.capture('after-readonly-preview');(s.out/'GUI_SMOKE.json').write_text(json.dumps({'status':'pass','artifact_sha256':s.data['artifact_sha256'],'source_revision':s.data['source_revision'],'scope':'Actual AppImage Dark/List100 on owned Xvfb/KWin display; native icon data, startup, read-only contact preview Space close and unchanged VCF bytes','contact_sha256':before,'interaction_correction':'Select the observed file row before Space; Home did not select it after address-bar navigation','broader_limits':'No actual HTML/Office provider GUI, fresh distro lifecycle, physical desktop or personal profile acceptance'},indent=2)+'\n');s.click(1072,18);print('Native icon and read-only contact preview Space-close checks passed; product close requested.')
 completed=True
finally:
 if not completed: (s.out/"STOP").touch()
