from pathlib import Path
import argparse,hashlib,importlib.util,json,re
parser=argparse.ArgumentParser();parser.add_argument('--case',required=True);args=parser.parse_args();assert re.fullmatch('[a-z0-9-]+',args.case)
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/args.case;out.mkdir(exist_ok=False)
s=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
build=json.loads((e/'BUILD_SOURCE.json').read_text());image=Path(build['stage'])/'Ottrin-0.1.0-x86_64.AppImage';accepted=json.loads((e/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text());sha=hashlib.sha256(image.read_bytes()).hexdigest();assert sha==accepted['artifacts'][image.name]
report={'status':'running','source_revision':build['source_revision'],'artifact_sha256':sha,'scope':'Private X11/software Dark100 exact staged AppImage main-menu New File/Folder, cancellation and occupied-name refusal; no whole-roadmap or complete accessibility claim'}
try:
 with m.OwnedDesktop(out) as d:
  d.env.pop('NO_AT_BRIDGE',None);m.run(['gsettings','set','org.gnome.desktop.interface','toolkit-accessibility','true'],d.env)
  p=d.spawn(['/usr/bin/python3',str(e/'creation-worker.py'),str(image),str(out)],'worker.log');assert p.wait(timeout=120)==0,'Native creation workflow failed; inspect preserved log';report['status']='functional checks pass; native captures require review'
except BaseException as error:
 report.update(status='failed native creation workflow',error=repr(error));raise
finally:
 if (out/'TEARDOWN.json').exists():report['teardown']=json.loads((out/'TEARDOWN.json').read_text())
 (out/'CONTROL.json').write_text(json.dumps(report,indent=2)+'\n')
