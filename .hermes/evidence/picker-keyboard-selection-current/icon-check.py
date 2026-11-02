"""Read the complete owned X11 icon property with an explicit length bound."""
from pathlib import Path
import importlib.util,json,hashlib,time,re
from PIL import Image
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'icon';assert not out.exists()
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
build=json.loads((e/'BUILD_SOURCE.json').read_text());content=json.loads((e/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text());binary=Path(build['stage'])/'Ottrin-0.1.0-x86_64.AppImage'
with binary.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==content['artifacts'][binary.name]
report={'artifact_sha256':content['artifacts'][binary.name]}
try:
 with desktop.OwnedDesktop(out) as d:
  fixture=d.profile/'fixture';fixture.mkdir();app=d.spawn([str(binary),'picker'],'app.stderr',stdin=True,output_name='response.json');app.stdin.write(json.dumps({'mode':'open','initial_directory':{'encoding':'unix-bytes','units_hex':str(fixture).encode().hex()}}));app.stdin.close();deadline=time.monotonic()+15
  while True:
   assert app.poll() is None
   windows=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','Ottrin'],d.env,False).stdout.strip().splitlines()
   if windows:break
   assert time.monotonic()<deadline;time.sleep(.05)
  w=windows[-1];raw=desktop.run(['xprop','-id',w,'-len','1048576','-f','_NET_WM_ICON','32c','_NET_WM_ICON'],d.env).stdout;values=[int(x) for x in re.findall(r'\d+',raw.split('=',1)[1])];image=Image.open(r/'assets/icon.png').convert('RGBA');expected=[a<<24|red<<16|green<<8|blue for red,green,blue,a in image.getdata()];assert values[:2]==list(image.size) and values[2:]==expected
  report.update(status='pass',actual_pixels_equal_source_icon=True,dimensions=list(image.size),pixels=len(expected),source_icon_sha256=hashlib.sha256((r/'assets/icon.png').read_bytes()).hexdigest(),scope='Complete actual chooser X11 icon property; main uses the same embedded icon loader. Physical taskbar acceptance remains separate.')
  desktop.run(['xdotool','windowactivate','--sync',w],d.env);desktop.run(['xdotool','key','--clearmodifiers','Escape'],d.env);app.wait(timeout=5);assert app.returncode==0
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
