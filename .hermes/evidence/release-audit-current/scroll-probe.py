from pathlib import Path
import importlib.util,json,subprocess,time,sys
r=Path.cwd();e=r/'.hermes/evidence/release-audit-current/scroll-probe';e.mkdir(exist_ok=True)
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');dmod=importlib.util.module_from_spec(spec);spec.loader.exec_module(dmod)
with dmod.OwnedDesktop(e) as d:
 d.env.pop('NO_AT_BRIDGE',None);files=d.profile/'files';files.mkdir()
 for i in range(40):(files/f'row-{i:02}.txt').write_text('Owned scroll target.\n')
 config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(default_view_mode='List',bookmarks=[]);config['search'].update(include_roots=[str(files)],content_indexing_enabled=False);p=d.profile/'config/ottrin/config.json';p.parent.mkdir(parents=True);p.write_text(json.dumps(config))
 subprocess.run(['gdbus','call','--session','--dest','org.a11y.Bus','--object-path','/org/a11y/bus','--method','org.freedesktop.DBus.Properties.Set','org.a11y.Status','IsEnabled','<true>'],env=d.env,check=True,capture_output=True)
 app=d.spawn([str(r/'target/debug/ottrin-app'),str(files),'--renderer','opengl'],'app.log')
 code="import importlib.util,pathlib,sys,time,json;spec=importlib.util.spec_from_file_location('qa',sys.argv[1]);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);m.target_process_id=int(sys.argv[2]);node=m.wait_for(lambda:m.visible_entry('row-39.txt'),'native last row');out=pathlib.Path(sys.argv[3]);chain=[];p=node\nfor i in range(8):\n try:\n  v=p.queryComponent().getExtents(m.pyatspi.DESKTOP_COORDS);chain.append({'role':p.getRoleName(),'name':p.name,'bounds':list(v)});p=p.parent\n except Exception as error:chain.append({'error':str(error)});break\nresult={'ancestors':chain}\ntry:result['scroll_to']=node.queryComponent().scrollTo(m.pyatspi.SCROLL_ANYWHERE)\nexcept Exception as error:result['error']=repr(error)\ntime.sleep(.5);v=node.queryComponent().getExtents(m.pyatspi.DESKTOP_COORDS);result['after']=list(v);out.write_text(json.dumps(result,indent=2)+'\\n')"
 probe=d.spawn(['/usr/bin/python3','-c',code,str(r/'scripts/linux-desktop-qa.py'),str(app.pid),str(e/'RESULT.json')],'probe.log');assert probe.wait(timeout=30)==0
