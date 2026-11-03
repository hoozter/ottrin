from pathlib import Path
import importlib.util,json,os,re,signal,subprocess,sys,time
r=Path.cwd();binary=Path(sys.argv[1]);out=Path(sys.argv[2]);home=Path.home();docs=home/'Documents';docs.mkdir();original=docs/'preserve.txt';original.write_bytes(b'owned original\n')
s=importlib.util.spec_from_file_location('qa',r/'scripts/linux-desktop-qa.py');q=importlib.util.module_from_spec(s);s.loader.exec_module(q)
with (out/'app.log').open('w') as log:
 app=subprocess.Popen([str(binary)],stderr=log,start_new_session=True);q.target_process_id=app.pid
 try:
  q.wait_for(q.ottrin_app,'owned packaged application')
  window=q.wait_for(lambda:q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin$',capture=True,check=False).stdout.split() or None,'actual main window')[0]
  def location():
   q.xkey(window,'ctrl+l');q.run('xdotool','type','--clearmodifiers','--delay','1',str(docs));q.xkey(window,'Return');q.wait_for(lambda:q.visible_entry('preserve.txt'),'exact owned Documents fixture')
  def begin(action,name):
   location();q.click_named_control(window,'Menu');q.click_named_control(window,action)
   field=q.wait_for(lambda:next((n for n in q.all_nodes(q.ottrin_app()) if n.getRoleName() in {'entry','text'} and q.pyatspi.STATE_FOCUSED in n.getState().getStates()),None),'focused creation field')
   expected='Folder name' if action=='New Folder' else 'File name';assert field.name==expected,(field.name,expected)
   observed={'role':field.getRoleName(),'accessible_name':field.name,'initial_value':field.queryText().getText(0,-1)}
   q.xkey(window,'ctrl+a');q.run('xdotool','type','--clearmodifiers','--delay','1',name);time.sleep(.2);assert field.queryText().getText(0,-1)==name
   observed['typed_value']=field.queryText().getText(0,-1);return observed
  cases=[]
  f=begin('New File','created.txt');q.screenshot(out,'new-file');q.click_named_control(window,'Create');q.wait_for(lambda:(docs/'created.txt').is_file(),'created empty file');assert (docs/'created.txt').read_bytes()==b'';cases.append({'case':'new-file','field':f,'status':'pass'})
  f=begin('New Folder','CreatedFolder');q.screenshot(out,'new-folder');q.click_named_control(window,'Create');q.wait_for(lambda:(docs/'CreatedFolder').is_dir(),'created folder');assert not list((docs/'CreatedFolder').iterdir());cases.append({'case':'new-folder','field':f,'status':'pass'})
  f=begin('New File','cancelled.txt');q.click_named_control(window,'Cancel');assert not (docs/'cancelled.txt').exists();cases.append({'case':'cancel-new-file','field':f,'status':'pass'})
  f=begin('New File','preserve.txt');q.wait_for(lambda:any(n.name=='An entry with that name already exists.' for n in q.all_nodes(q.ottrin_app())),'existing-entry refusal');q.screenshot(out,'existing-entry-refusal');q.click_named_control(window,'Cancel');assert original.read_bytes()==b'owned original\n';cases.append({'case':'existing-file-refused','field':f,'status':'pass'})
  location();q.click_entry(window,'preserve.txt');q.xkey(window,'F2')
  field=q.wait_for(lambda:next((n for n in q.all_nodes(q.ottrin_app()) if n.getRoleName()=='entry' and n.name=='New name'),None),'named rename editor')
  assert field.queryText().getText(0,-1)=='preserve.txt'
  q.xkey(window,'ctrl+a');q.run('xdotool','type','--clearmodifiers','--delay','1','cancelled-rename.txt');assert field.queryText().getText(0,-1)=='cancelled-rename.txt'
  q.screenshot(out,'rename');q.click_named_control(window,'Cancel');assert original.read_bytes()==b'owned original\n' and not(docs/'cancelled-rename.txt').exists();cases.append({'case':'named-rename-edit-cancel','field':{'role':'entry','accessible_name':'New name','typed_value':'cancelled-rename.txt'},'status':'pass'})
  q.xkey(window,'ctrl+l');field=q.wait_for(lambda:next((n for n in q.all_nodes(q.ottrin_app()) if n.getRoleName()=='entry' and n.name=='Location'),None),'named location editor');assert field.queryText().getText(0,-1)==str(docs);cases.append({'case':'named-location','value':field.queryText().getText(0,-1),'status':'pass'});q.xkey(window,'Escape')
  q.xkey(window,'ctrl+f');field=q.wait_for(lambda:next((n for n in q.all_nodes(q.ottrin_app()) if n.getRoleName()=='entry' and n.name=='Search files and folders'),None),'named search editor');cases.append({'case':'named-search','value':field.queryText().getText(0,-1),'status':'pass'});q.xkey(window,'Escape')
  entries=list(docs.iterdir());snapshot=[{'name':p.name,'directory':p.is_dir(),'symlink':p.is_symlink(),'mode':p.stat().st_mode&0o777,'uid':p.stat().st_uid,'device':p.stat().st_dev,'inode':p.stat().st_ino,'children':[x.name for x in p.iterdir()] if p.is_dir() else None} for p in entries];(out/'FILESYSTEM.json').write_text(json.dumps(snapshot,indent=2)+'\n')
  assert not any(p.is_symlink() for p in entries)
  retained=[p for p in entries if re.fullmatch(r'\.ottrin-txn-[0-9a-f]{32}',p.name)]
  assert len(retained)==2 and all(p.is_dir() and not list(p.iterdir()) and p.stat().st_mode&0o777==0o700 and p.stat().st_uid==os.getuid() for p in retained)
  assert {p.name for p in entries if p not in retained}=={'preserve.txt','created.txt','CreatedFolder'}
  q.click_accessible_name('Close window','push button');assert app.wait(timeout=15)==0
  (out/'FUNCTIONAL_RESULT.json').write_text(json.dumps({'status':'pass packaged main-menu creation/cancellation/refusal scope','cases':cases,'source_bytes_preserved':True,'created_file_empty':True,'created_folder_empty':True,'normal_close_exit':app.returncode,'limit':'Native AT-SPI creation/rename/location/search field names and values; whole UI, assistive usability and other platforms separate'},indent=2)+'\n')
 finally:
  try:os.killpg(app.pid,signal.SIGTERM)
  except ProcessLookupError:pass
  try:app.wait(timeout=10)
  except subprocess.TimeoutExpired:os.killpg(app.pid,signal.SIGKILL);app.wait()
