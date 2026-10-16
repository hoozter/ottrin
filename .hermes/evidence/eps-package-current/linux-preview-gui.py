"""Actual packaged PLY/Affinity/EPS Inspector rendering on a private Xvfb desktop."""
from pathlib import Path
import hashlib,json,os,shutil,signal,subprocess,sys,tempfile,time
import numpy as np
from PIL import Image
artifact=Path(sys.argv[1]).resolve();out=Path(__file__).resolve().parent/'linux-preview-gui';out.mkdir(exist_ok=True)
root=Path(tempfile.mkdtemp(prefix='op-preview-package-',dir=Path('.hermes/qa').resolve()));app=None;display=None;env=None
result={'status':'running','artifact':artifact.name,'sha256':hashlib.sha256(artifact.read_bytes()).hexdigest(),'scope':'Private Xvfb/X11 and profile, software OpenGL, Dark 100%; shared user manager control bus for transient EPS sandbox units; no physical GPU claim','cases':{}}
def run(args,**kwargs):return subprocess.run(args,timeout=25,**kwargs)
def own_pids():
 found=[]
 for proc in Path('/proc').iterdir():
  try:
   if not proc.name.isdigit() or proc.stat().st_uid!=os.getuid():continue
   values=dict(x.split(b'=',1) for x in (proc/'environ').read_bytes().split(b'\0') if b'=' in x)
   if values.get(b'XDG_STATE_HOME')==os.fsencode(root/'state'):found.append(int(proc.name))
  except OSError:pass
 return found
def cancelled(signum,frame):raise SystemExit('Owned GUI QA cancelled')
for sig in [signal.SIGINT,signal.SIGTERM]:signal.signal(sig,cancelled)
try:
 run([str(artifact),'--appimage-extract'],cwd=root,check=True,stdout=subprocess.DEVNULL)
 for name in ['config/ottrin','cache','data','state','runtime','files']:(root/name).mkdir(parents=True)
 (root/'runtime').chmod(0o700)
 from fixtures import create
 ply,affinity,eps=create(root/'files')
 template=json.loads(Path('.hermes/evidence/address-selection-current/VALID_CONFIG_TEMPLATE.json').read_text());template.update(theme='Dark',window_size=[1000,760],restore_session=False,default_view_mode='Miller',search_setup_completed=True)
 template['search'].update(include_roots=[str(root/'files')],use_system_locate=False,content_indexing_enabled=False)
 (root/'config/ottrin/config.json').write_text(json.dumps(template))
 read,write=os.pipe();display=subprocess.Popen(['Xvfb','-displayfd',str(write),'-screen','0','1500x1050x24','-nolisten','tcp'],pass_fds=(write,),stdout=subprocess.DEVNULL,stderr=(out/'display.log').open('w'));os.close(write)
 number=b''
 while not number.endswith(b'\n'):
  block=os.read(read,32);assert block,display.poll();number+=block
 os.close(read);assert number.decode().strip().isdigit()
 env=os.environ.copy()
 for name in ['WAYLAND_DISPLAY','LD_LIBRARY_PATH','OTTRIN_INDEX_SOCKET','OTTRIN_PDFIUM_DIR']:env.pop(name,None)
 manager_runtime=os.environ.get('XDG_RUNTIME_DIR','/run/user/'+str(os.getuid()))
 env.setdefault('DBUS_SESSION_BUS_ADDRESS','unix:path='+manager_runtime+'/bus')
 env.update(DISPLAY=':'+number.decode().strip(),HOME=str(root),XDG_CONFIG_HOME=str(root/'config'),XDG_CACHE_HOME=str(root/'cache'),XDG_DATA_HOME=str(root/'data'),XDG_STATE_HOME=str(root/'state'),XDG_RUNTIME_DIR=manager_runtime,WINIT_UNIX_BACKEND='x11',WINIT_X11_SCALE_FACTOR='1.0',LIBGL_ALWAYS_SOFTWARE='1',OTTRIN_RENDERER='opengl',OTTRIN_DEV_HELPER_AUTOBUILD='0')
 for label,source in [('ply',ply),('affinity',affinity),('eps',eps)]:
  before=hashlib.sha256(source.read_bytes()).hexdigest();log=(out/(label+'.log')).open('w')
  app=subprocess.Popen([str(root/'squashfs-root/AppRun'),'preview',str(source)],env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
  deadline=time.monotonic()+35;win=None;pixels=0
  while time.monotonic()<deadline:
   assert app.poll() is None,(label,'early exit',app.returncode)
   query=run(['xdotool','search','--onlyvisible','--name','Ottrin Quick Inspector'],env=env,capture_output=True,text=True)
   if query.returncode==0 and query.stdout.strip():
    win=query.stdout.splitlines()[0];run(['import','-window',win,str(out/(label+'.png'))],env=env,check=True)
    image=np.array(Image.open(out/(label+'.png')).convert('RGB')).astype(np.int16);r,g,b=image[:,:,0],image[:,:,1],image[:,:,2]
    if label in ['affinity','eps']:
     mask=(g>140)&(r<60)&(b<140);pixels=int(mask.sum());ys,xs=np.where(mask);span=[int(xs.max()-xs.min()),int(ys.max()-ys.min())] if pixels else [0,0];ready=pixels>1000
     if label=='eps':ready=ready and int(((r>140)&(g<80)&(b<90)).sum())>1000
    else:
     light=(image.min(axis=2)>235)&((image.max(axis=2)-image.min(axis=2))<12);cx=np.where(light.sum(axis=0)>150)[0];cy=np.where(light.sum(axis=1)>150)[0];pixels=0;span=[0,0];ready=False
     if len(cx)>200 and len(cy)>200:
      canvas=image[cy.min():cy.max()+1,cx.min():cx.max()+1];cr,cg,cb=canvas[:,:,0],canvas[:,:,1],canvas[:,:,2];mask=(cr+20<cg)&(cr+20<cb)&(cr<220)&(cg>70)&(cb>100);pixels=int(mask.sum());ys,xs=np.where(mask);span=[int(xs.max()-xs.min()),int(ys.max()-ys.min())] if pixels else [0,0]
      ready=pixels>80 and span[0]>.65*len(cx) and span[1]>.55*len(cy)
    if ready:break
   time.sleep(.2)
  assert win and ready,(label,'rendered fixture surface missing',pixels,span)
  title=run(['xdotool','getwindowname',win],env=env,capture_output=True,text=True,check=True).stdout.strip();assert source.name in title,title
  geometry=dict(x.split('=',1) for x in run(['xdotool','getwindowgeometry','--shell',win],env=env,capture_output=True,text=True,check=True).stdout.splitlines() if '=' in x)
  run(['xdotool','mousemove','--window',win,str(int(geometry['WIDTH'])-22),'18','click','1'],env=env,check=True)
  assert app.wait(timeout=12)==0,(label,'unclean close',app.returncode)
  log.close();assert hashlib.sha256(source.read_bytes()).hexdigest()==before
  result['cases'][label]={'title':title,'rendered_fixture_pixels':pixels,'colored_pixel_span':span,'oracle':'Green/red PostScript drawing' if label=='eps' else ('Green embedded composite' if label=='affinity' else 'Blue wireframe edges spanning both projected axes'),'source_unchanged':True,'clean_exit':0}
 result['status']='pass'
except BaseException as error:
 result.update(status='fail',error=repr(error));raise
finally:
 if app and app.poll() is None:
  os.killpg(app.pid,signal.SIGTERM)
  try:app.wait(timeout=5)
  except subprocess.TimeoutExpired:os.killpg(app.pid,signal.SIGKILL);app.wait()
 for pid in own_pids():
  try:os.kill(pid,signal.SIGTERM)
  except ProcessLookupError:pass
 deadline=time.monotonic()+7
 while own_pids() and time.monotonic()<deadline:time.sleep(.1)
 left=own_pids()
 for pid in left:
  try:os.kill(pid,signal.SIGKILL)
  except ProcessLookupError:pass
 if display:
  display.terminate()
  try:display.wait(timeout=5)
  except subprocess.TimeoutExpired:display.kill();display.wait()
 left=own_pids();shutil.rmtree(root);result['cleanup']={'profile_extraction_removed':not root.exists(),'private_processes':left}
 if left:result['status']='fail'
 (out/'RESULTS.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
