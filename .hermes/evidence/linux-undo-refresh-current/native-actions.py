from pathlib import Path
import hashlib,json,os,re,subprocess,time
class Session:
 def __init__(self,name):
  self.root=Path.cwd();self.out=self.root/'.hermes/evidence/linux-undo-refresh-current'/name;self.data=json.loads((self.out/'SESSION.json').read_text());self.fixture=Path(self.data['fixture']);self.env=os.environ.copy();self.env.update(self.data['environment']);self.window=self.data['window'];self.guard()
 def guard(self):
  pid=self.data['app_pid'];raw=Path(f'/proc/{pid}/stat').read_text().split(') ',1)[1].split();assert raw[19]==self.data['app_start_time'];values=Path(f'/proc/{pid}/environ').read_bytes().split(b'\0');assert os.fsencode('DISPLAY='+self.env['DISPLAY']) in values;assert os.fsencode('XDG_RUNTIME_DIR='+self.env['XDG_RUNTIME_DIR']) in values
 def run(self,args,window=None):
  self.guard();return subprocess.run(args,env=self.env,check=True,text=True,capture_output=True,timeout=15)
 def focus(self):
  self.run(['xdotool','windowactivate','--sync',self.window]);assert self.run(['xdotool','getactivewindow']).stdout.strip()==self.window
 def log(self,event):
  with (self.out/'actions.jsonl').open('a') as f:f.write(json.dumps({'time':time.time(),**event})+'\n')
 def keys(self,key):
  self.focus();self.run(['xdotool','key','--clearmodifiers',key]);self.log({'key':key});time.sleep(.25)
 def nav(self,path):
  path=Path(path).resolve();assert path.is_relative_to(self.fixture.resolve());assert path.is_dir();self.keys('ctrl+l');self.run(['xdotool','type','--clearmodifiers','--delay','1',str(path)]);self.keys('Return');time.sleep(.4);self.log({'navigate':str(path)})
 def capture(self,name):
  self.run(['import','-window',self.window,str(self.out/(name+'.png'))]);self.log({'capture':name})
 def click(self,x,y):
  self.focus();geometry=self.run(['xdotool','getwindowgeometry','--shell',self.window]).stdout;values=dict(line.split('=',1) for line in geometry.splitlines() if '=' in line);assert 0<=x<int(values['WIDTH']) and 0<=y<int(values['HEIGHT']);self.run(['xdotool','mousemove','--window',self.window,str(x),str(y),'click','1']);self.log({'click':[x,y]});time.sleep(.4)
 def wait(self,predicate):
  deadline=time.monotonic()+10
  while time.monotonic()<deadline:
   self.guard()
   if predicate():return
   time.sleep(.05)
  raise AssertionError('Packaged file-state condition did not complete')
 def checkpoint(self,name,expected,absent=()):
  files={}
  for relative,data in expected.items():
   p=self.fixture/relative;assert p.read_bytes()==data,(relative,p.read_bytes());files[relative]=hashlib.sha256(data).hexdigest()
  for relative in absent:assert not (self.fixture/relative).exists(),relative
  self.log({'file_checkpoint':name,'files':files,'absent':list(absent)});self.capture(name)
