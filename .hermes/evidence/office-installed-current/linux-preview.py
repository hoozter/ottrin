"""Actual current AppImage temporal PDF, read-only navigation and explicit save."""
from pathlib import Path
import csv,dbus,hashlib,importlib.util,io,json,os,subprocess,time,sys
import numpy as np
from PIL import Image
r=Path.cwd();edit_only='--edit-only' in sys.argv;e=Path(__file__).resolve().parent/('linux-edit' if edit_only else 'linux-preview');assert not e.exists()
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
accepted=json.loads((r/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json').read_text());payload=json.loads((r/'.hermes/evidence/assistive-delivery-current/CURRENT_PAYLOAD.json').read_text());image=r/'dist/packages/Ottrin-0.1.0-x86_64.AppImage'
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
assert sha(image)==accepted['artifacts'][image.name];result={'status':'running','source_revision':accepted['source_revision'],'artifact_sha256':sha(image),'scope':'Current packaged Linux Dark100 native X11/software; temporal PDF transition, read-only navigation and explicit text Save; not physical or every-file acceptance'}
try:
 with desktop.OwnedDesktop(e/'desktop') as d:
  root=d.profile
  fixture=root/'files';fixture.mkdir();objects=[b'<< /Type /Catalog /Pages 2 0 R >>',b'<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>',b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>',b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>']
  stream=b'BT /F1 24 Tf 60 700 Td (PAGE ONE) Tj ET'
  objects.append(b'<< /Length '+str(len(stream)).encode()+b' >>\nstream\n'+stream+b'\nendstream')
  objects.append(b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 4 0 R >> >> /Contents 7 0 R >>')
  stream=b'BT /F1 24 Tf 60 700 Td (PAGE TWO) Tj ET'
  objects.append(b'<< /Length '+str(len(stream)).encode()+b' >>\nstream\n'+stream+b'\nendstream')
  pdf=b'%PDF-1.4\n';offsets=[0]
  for i,obj in enumerate(objects,1):offsets.append(len(pdf));pdf+=str(i).encode()+b' 0 obj\n'+obj+b'\nendobj\n'
  xref=len(pdf);pdf+=b'xref\n0 8\n0000000000 65535 f \n'+b''.join(f'{offset:010} 00000 n \n'.encode() for offset in offsets[1:])+f'trailer\n<< /Size 8 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode()
  (fixture/'b.pdf').write_bytes(pdf)

  (fixture/'a.vcf').write_text('BEGIN:VCARD\nVERSION:3.0\nFN:Read Only Contact\nEND:VCARD\n');original=(fixture/'a.vcf').read_bytes()
  (fixture/'c.txt').write_bytes(b'Explicit save proof.\n');text_original=(fixture/'c.txt').read_bytes();source_hashes={p.name:sha(p) for p in fixture.iterdir()}
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(theme='Dark',default_view_mode='Miller',window_size=[800,600],bookmarks=[],installed_office_previews_enabled=False);config['search'].update(include_roots=[str(fixture)],content_indexing_enabled=False,refresh_interval_hours=0)
  cp=root/'config/ottrin/config.json';cp.parent.mkdir(parents=True);cp.write_text(json.dumps(config)+'\n')
  d.env.pop('NO_AT_BRIDGE',None);connection=dbus.bus.BusConnection(d.env['DBUS_SESSION_BUS_ADDRESS']);status=connection.get_object('org.a11y.Bus','/org/a11y/bus');dbus.Interface(status,'org.freedesktop.DBus.Properties').Set('org.a11y.Status','IsEnabled',dbus.Boolean(True));bus=dbus.bus.BusConnection(str(dbus.Interface(status,'org.a11y.Bus').GetAddress()));control=dbus.Interface(bus.get_object('org.freedesktop.DBus','/org/freedesktop/DBus'),'org.freedesktop.DBus');registry=dbus.Interface(bus.get_object('org.a11y.atspi.Registry','/org/a11y/atspi/accessible/root'),'org.a11y.atspi.Accessible')
  app=d.spawn([str(image),'--select',str(fixture/'a.vcf'),'--renderer','opengl'],'app.log')
  def run(cmd):return desktop.run(cmd,d.env)
  def windows():
   q=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid)],d.env,False);return {w:run(['xdotool','getwindowname',w]).stdout.strip() for w in q.stdout.split()} if q.returncode==0 else {}
  def wait_window(suffix):
   deadline=time.monotonic()+25
   while True:
    assert app.poll() is None
    found=[w for w,t in windows().items() if t==suffix or t.endswith(suffix)]
    if len(found)==1:return found[0]
    assert time.monotonic()<deadline,('Native window missing',suffix,windows());time.sleep(.1)
  def key(w,k):run(['xdotool','windowactivate','--sync',w,'key','--clearmodifiers',k])
  def shot(w,name):run(['import','-window',w,str(e/name)])
  def invoke(label):
   deadline=time.monotonic()+10
   while True:
    queue=[(str(n),str(p)) for n,p in registry.GetChildren() if int(control.GetConnectionUnixProcessID(str(n)))==app.pid];seen=set();found=[]
    while queue:
     n,p=queue.pop(0)
     if (n,p) in seen:continue
     seen.add((n,p));assert len(seen)<1500
     try:
      obj=bus.get_object(n,p,introspect=False);a=dbus.Interface(obj,'org.a11y.atspi.Accessible');props=dbus.Interface(obj,'org.freedesktop.DBus.Properties')
      if str(props.Get('org.a11y.atspi.Accessible','Name'))==label:found.append((n,p))
      queue.extend((str(x),str(y)) for x,y in a.GetChildren() if str(x)==n)
     except dbus.exceptions.DBusException as error:
      if error.get_dbus_name()!='org.freedesktop.DBus.Error.UnknownObject':raise
    if len(found)==1:break
    assert time.monotonic()<deadline,('Native control missing/ambiguous',label,len(found));time.sleep(.1)
   action=dbus.Interface(bus.get_object(*found[0]),'org.a11y.atspi.Action');assert action.GetActions() and action.DoAction(0)
  main=wait_window('Ottrin');assert sha(Path('/proc')/str(app.pid)/'exe')==payload['payload']['usr/bin/ottrin']['sha256'];result['mapped_gui_sha256']=sha(Path('/proc')/str(app.pid)/'exe');time.sleep(2)
  if not edit_only:
   key(main,'Down');time.sleep(2);shot(main,'pdf-controls.png');ocr=run(['tesseract',str(e/'pdf-controls.png'),'stdout','--psm','11','tsv']).stdout;words=list(csv.DictReader(io.StringIO(ocr),delimiter='\t'));matches=[v for v in words if v['text'].lower()=='width' and int(v['top'])<180];assert matches,'Actual Fit width control not found';v=matches[0];run(['xdotool','mousemove','--window',main,str(int(v['left'])+int(v['width'])//2),str(int(v['top'])+int(v['height'])//2),'click','1']);key(main,'Up');time.sleep(.2);key(main,'Down')
   frames=[];start=time.monotonic()
   for i in range(100):
    time.sleep(.06);frame=root/'frame.png';run(['import','-window',main,str(frame)]);a=np.array(Image.open(frame).convert('RGB'));mask=(a.min(axis=2)>235)&((a.max(axis=2)-a.min(axis=2))<8);xs=np.where(mask.sum(axis=0)>80)[0];ys=np.where(mask.sum(axis=1)>50)[0];box=[int(xs.min()),int(ys.min()),int(xs.max()+1),int(ys.max()+1)] if len(xs) and len(ys) else None;frames.append({'frame':i,'elapsed_seconds':time.monotonic()-start,'box':box})
    if i in [0,10,30,99]:(e/('pdf-frame-'+str(i)+'.png')).write_bytes(frame.read_bytes())
   settled=[f['box'] for f in frames[30:] if f['box']];assert len(settled)>50 and len(set(map(str,settled)))==1,'PDF page bounds oscillate or fail to render';result['pdf_frames']=frames;result['settled_pdf_bounds']=settled[0]
   key(main,'Up');time.sleep(.3);key(main,'space');ins=wait_window('a.vcf');shot(ins,'readonly-vcf.png');key(ins,'Down');wait_window('b.pdf');key(ins,'Up');wait_window('a.vcf');key(ins,'space');time.sleep(.5);assert all('Quick Inspector' not in t for t in windows().values());assert sha(fixture/'a.vcf')==source_hashes['a.vcf'];result['readonly_navigation_and_space']='pass'
  key(main,'Down');time.sleep(.2);key(main,'Down');time.sleep(.3);key(main,'space');ins=wait_window('c.txt');invoke('Edit text');time.sleep(.4);key(ins,'ctrl+End');run(['xdotool','type','--clearmodifiers','EXPLICIT_EDIT_MARKER']);time.sleep(.2);assert (fixture/'c.txt').read_bytes()==text_original,'Draft changed source before Save';shot(ins,'draft-before-save.png');key(ins,'ctrl+s');time.sleep(.8);assert (fixture/'c.txt').read_bytes()==text_original+b'EXPLICIT_EDIT_MARKER','Explicit Save did not publish exact expected bytes';shot(ins,'explicit-saved.png');key(ins,'Escape');time.sleep(.3);assert all('Quick Inspector' not in t for t in windows().values());result['explicit_edit']={'draft_did_not_write':True,'saved_exact_expected_bytes':True,'clean_editor_closed':True}
  assert sha(fixture/'b.pdf')==source_hashes['b.pdf'] and sha(fixture/'a.vcf')==source_hashes['a.vcf'];invoke('Close window');assert app.wait(timeout=15)==0;result['normal_main_exit']=0;result['status']='pass within current explicit Edit/Save scope' if edit_only else 'pass within current temporal PDF/read-only/explicit Save scope'
finally:
 result['teardown']=json.loads((e/'desktop/TEARDOWN.json').read_text()) if (e/'desktop/TEARDOWN.json').exists() else None
 (e/'RESULT.json').write_text(json.dumps(result,indent=2)+'\n')
