import os,pathlib,subprocess,tempfile,time,signal,json,sys,shutil
from PIL import Image
import numpy as np
binary=pathlib.Path(sys.argv[1]).resolve();out=pathlib.Path(sys.argv[2]).resolve();out.mkdir(exist_ok=True)
root=pathlib.Path(tempfile.mkdtemp(prefix='ottrin-preview-safety-',dir=pathlib.Path.cwd()/'.hermes/qa'));app=None;display=None
theme=sys.argv[3] if len(sys.argv)>3 else 'Dark'
scale=float(sys.argv[4]) if len(sys.argv)>4 else 1.0
font_scale=float(sys.argv[5]) if len(sys.argv)>5 else 1.0
check_fixed='--assert-fixed' in sys.argv
result={'theme':theme,'desktop_scale':scale,'font_scale':font_scale};captured=[]
def run(args,**kw):return subprocess.run(args,timeout=20,**kw)
def shot(win,path):run(['import','-window',win,str(path)],env=env,check=True)
def key(win,k):run(['xdotool','windowfocus',win,'key','--clearmodifiers',k],env=env,check=True)
def title(win):return subprocess.check_output(['xdotool','getwindowname',win],env=env,text=True,timeout=20).strip()
def inspector():
 q=run(['xdotool','search','--onlyvisible','--name','Ottrin Quick Inspector'],env=env,capture_output=True,text=True)
 return q.stdout.splitlines()[0] if q.returncode==0 and q.stdout.strip() else None
def private_pids():
 found=[]
 for p in pathlib.Path('/proc').iterdir():
  if not p.name.isdigit() or int(p.name)==os.getpid():continue
  try:
   v=dict(e.split(b'=',1) for e in (p/'environ').read_bytes().split(b'\0') if b'=' in e)
   if p.stat().st_uid==os.getuid() and v.get(b'HOME')==os.fsencode(root) and v.get(b'XDG_STATE_HOME')==os.fsencode(root/'state'):found.append(int(p.name))
  except OSError:pass
 return found
def cancel(signum,frame):raise SystemExit('cancelled')
for sig in [signal.SIGINT,signal.SIGTERM]:signal.signal(sig,cancel)
try:
 r,w=os.pipe();display=subprocess.Popen(['Xvfb','-displayfd',str(w),'-screen','0','1400x1000x24','-nolisten','tcp'],pass_fds=(w,),stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);os.close(w)
 data=b''
 while not data.endswith(b'\n'):data+=os.read(r,32)
 number=data.decode().strip();os.close(r);(root/'runtime').mkdir(mode=0o700)
 env=os.environ.copy();env.pop('WAYLAND_DISPLAY',None);env.pop('OTTRIN_PDFIUM_DIR',None);env.pop('LD_LIBRARY_PATH',None)
 env.update(DISPLAY=':'+number,HOME=str(root),XDG_RUNTIME_DIR=str(root/'runtime'),XDG_CONFIG_HOME=str(root/'config'),XDG_DATA_HOME=str(root/'data'),XDG_CACHE_HOME=str(root/'cache'),XDG_STATE_HOME=str(root/'state'),OTTRIN_RENDERER='opengl',WINIT_UNIX_BACKEND='x11',LIBGL_ALWAYS_SOFTWARE='1',OTTRIN_DEV_HELPER_AUTOBUILD='0')
 fixture=root/'files';fixture.mkdir();objects=[b'<< /Type /Catalog /Pages 2 0 R >>',b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 381] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>',b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>']
 stream=b'BT /F1 24 Tf 60 700 Td (PAGE ONE) Tj ET'
 objects.append(b'<< /Length '+str(len(stream)).encode()+b' >>\nstream\n'+stream+b'\nendstream')
 objects.append(b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 381] /Resources << /Font << /F1 4 0 R >> >> /Contents 7 0 R >>')
 stream=b'BT /F1 24 Tf 60 700 Td (PAGE TWO) Tj ET'
 objects.append(b'<< /Length '+str(len(stream)).encode()+b' >>\nstream\n'+stream+b'\nendstream')
 pdf=b'%PDF-1.4\n';offsets=[0]
 for i,obj in enumerate(objects,1):offsets.append(len(pdf));pdf+=str(i).encode()+b' 0 obj\n'+obj+b'\nendobj\n'
 xref=len(pdf);pdf+=b'xref\n0 8\n0000000000 65535 f \n'+b''.join(f'{offset:010} 00000 n \n'.encode() for offset in offsets[1:])+f'trailer\n<< /Size 8 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode()
 (fixture/'b.pdf').write_bytes(pdf)

 (fixture/'a.vcf').write_text('BEGIN:VCARD\nVERSION:3.0\nFN:Read Only Contact\nEND:VCARD\n');original=(fixture/'a.vcf').read_bytes()
 config=root/'config/ottrin/config.json';config.parent.mkdir(parents=True)
 template=json.loads(pathlib.Path('.hermes/evidence/address-selection-current/VALID_CONFIG_TEMPLATE.json').read_text());template['theme']=theme;template['window_size']=[800,600];template['default_view_mode']='Miller';template['search']['include_roots']=[str(fixture)];template['search']['content_indexing_enabled']=False
 if font_scale!=1.0:
  template['custom_themes']=[{'name':'Preview QA','base_preset':'Ottrin','dark':{'font_scale':font_scale},'light':{'font_scale':font_scale}}];template['active_custom_theme']='Preview QA'
 env['WINIT_X11_SCALE_FACTOR']=str(scale)
 config.write_text(json.dumps(template))
 with (out/'app.log').open('w') as log:
  app=subprocess.Popen(['dbus-run-session','--',str(binary),'--select',str(fixture/'a.vcf')],env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
 deadline=time.monotonic()+25;win=None
 while time.monotonic()<deadline:
  assert app.poll() is None,'app exited'
  q=run(['xdotool','search','--onlyvisible','--name','^Ottrin$'],env=env,capture_output=True,text=True)
  if q.returncode==0 and q.stdout.strip():win=q.stdout.splitlines()[0];break
  time.sleep(.2)
 assert win,'missing main window';time.sleep(2)
 key(win,'Down');time.sleep(2)
 control=out/'controls.png';shot(win,control)
 ocr=subprocess.check_output(['tesseract',str(control),'stdout','--psm','11','tsv'],stderr=subprocess.DEVNULL,text=True,timeout=20)
 import csv,io
 words=list(csv.DictReader(io.StringIO(ocr),delimiter='\t'))
 matches=[v for v in words if v['text'].lower()=='width' and int(v['top'])<180]
 assert matches, 'Fit width control not found in real UI'
 v=matches[0];run(['xdotool','mousemove','--window',win,str(int(v['left'])+int(v['width'])//2),str(int(v['top'])+int(v['height'])//2),'click','1'],env=env,check=True)
 key(win,'Up');time.sleep(.2);key(win,'Down')
 for i in range(100):
  time.sleep(.06);frame=root/'frame.png';shot(win,frame)
  a=np.array(Image.open(frame).convert('RGB'));mask=(a.min(axis=2)>235)&((a.max(axis=2)-a.min(axis=2))<8)
  xs=np.where(mask.sum(axis=0)>80)[0];ys=np.where(mask.sum(axis=1)>50)[0]
  box=[int(xs.min()),int(ys.min()),int(xs.max()+1),int(ys.max()+1)] if len(xs) and len(ys) else None
  captured.append({'frame':i,'box':box})
  if i in [0,10,30,99]:shutil.copyfile(frame,out/('pdf-frame-'+str(i)+'.png'))
 key(win,'Up');time.sleep(.3);key(win,'space');time.sleep(.8);ins=inspector();assert ins,'missing VCF Inspector'
 shot(ins,out/'vcf-inspector.png');result['opened']=title(ins)
 a=np.array(Image.open(out/'vcf-inspector.png').convert('RGB'));y=int(24*scale)
 result['header_edge_color']=a[y,int(2*scale)].tolist();result['header_center_color']=a[y,a.shape[1]//2].tolist()
 key(ins,'Down');time.sleep(.6);result['after_down']=title(ins)
 key(ins,'Up');time.sleep(.4);result['after_up']=title(ins)
 key(ins,'space');time.sleep(.5);result['space_closed']=inspector() is None
 result['vcf_bytes_unchanged']=(fixture/'a.vcf').read_bytes()==original
 result['frames']=captured
 (out/'RESULTS.json').write_text(json.dumps(result,indent=2)+'\n')
 if check_fixed:
  assert result['after_down'].endswith('b.pdf') and result['after_up'].endswith('a.vcf'), 'Read-only arrows did not navigate files'
  assert result['space_closed'] and result['vcf_bytes_unchanged'], 'Read-only close/data check failed'
  assert result['header_edge_color']==result['header_center_color'], 'Title bar is inset or mismatched'
  if theme=='Dark':
   settled=[v['box'] for v in captured[30:] if v['box']]
   assert len(settled)>30, 'PDF did not render for enough frames'
   assert len(set(map(str,settled)))==1, 'PDF size still oscillates after settling'
 # Close through the real window control, before stopping the owned display.
 geometry=dict(line.split('=',1) for line in run(['xdotool','getwindowgeometry','--shell',win],env=env,capture_output=True,text=True,check=True).stdout.splitlines() if '=' in line)
 run(['xdotool','mousemove','--window',win,str(int(geometry['WIDTH'])-22),'18','click','1'],env=env,check=True)
 assert app.wait(timeout=12)==0, 'Main window did not close cleanly'
 result['gui_exit']=app.returncode
 (out/'RESULTS.json').write_text(json.dumps(result,indent=2)+'\n')

finally:
 if app and app.poll() is None:
  os.killpg(app.pid,signal.SIGTERM)
  try:app.wait(timeout=5)
  except subprocess.TimeoutExpired:os.killpg(app.pid,signal.SIGKILL);app.wait()
 for pid in private_pids():
  try:os.kill(pid,signal.SIGTERM)
  except ProcessLookupError:pass
 deadline=time.monotonic()+5
 while private_pids() and time.monotonic()<deadline:time.sleep(.1)
 for pid in private_pids():
  try:os.kill(pid,signal.SIGKILL)
  except ProcessLookupError:pass
 if display:
  display.terminate()
  try:display.wait(timeout=5)
  except subprocess.TimeoutExpired:display.kill();display.wait()
 shutil.rmtree(root);(out/'CLEANUP.json').write_text(json.dumps({'profile_removed':not root.exists(),'private_processes':private_pids()})+'\n')
