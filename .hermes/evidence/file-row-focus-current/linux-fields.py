from pathlib import Path
import argparse,hashlib,importlib.util,json,re
parser=argparse.ArgumentParser();parser.add_argument('--case',required=True);parser.add_argument('--theme',choices=['Dark','Light'],required=True);args=parser.parse_args();assert re.fullmatch('[a-z0-9-]+',args.case)
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/args.case;out.mkdir(exist_ok=False)
s=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
build=json.loads((e/'BUILD_SOURCE.json').read_text());image=Path(build['stage'])/'Ottrin-0.1.0-x86_64.AppImage';accepted=json.loads((e/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text());sha=hashlib.sha256(image.read_bytes()).hexdigest();assert sha==accepted['artifacts'][image.name]
report={'status':'running','source_revision':build['source_revision'],'artifact_sha256':sha,'theme':args.theme,'scope':'Private X11/software Dark100 or Light150 exact staged AppImage native focused-file-row F2 and Cancel-to-F2 focus lifecycle, Settings navigation roles/actions, matching filename geometry and source preservation; no whole-roadmap or complete accessibility claim'}
try:
 with m.OwnedDesktop(out,scale='1' if args.theme=='Dark' else '1.5') as d:
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(theme=args.theme,bookmarks=[],default_view_mode='List');config['search']['include_roots']=[str(d.profile/'home/Documents')];config['search']['content_indexing_enabled']=False;pref=d.profile/'config/ottrin/config.json';pref.parent.mkdir(parents=True);pref.write_text(json.dumps(config)+'\n')
  d.env.pop('NO_AT_BRIDGE',None);m.run(['gsettings','set','org.gnome.desktop.interface','toolkit-accessibility','true'],d.env)
  (out/'WORKER_SOURCE.py').write_bytes((e/'creation-worker.py').read_bytes());report['worker_sha256']=hashlib.sha256((out/'WORKER_SOURCE.py').read_bytes()).hexdigest()
  p=d.spawn(['/usr/bin/python3',str(out/'WORKER_SOURCE.py'),str(image),str(out)],'worker.log');assert p.wait(timeout=120)==0,'Native creation workflow failed; inspect preserved log';report['status']='functional checks pass; native captures require review'
except BaseException as error:
 report.update(status='failed native creation workflow',error=repr(error));raise
finally:
 if (out/'TEARDOWN.json').exists():report['teardown']=json.loads((out/'TEARDOWN.json').read_text())
 (out/'CONTROL.json').write_text(json.dumps(report,indent=2)+'\n')
