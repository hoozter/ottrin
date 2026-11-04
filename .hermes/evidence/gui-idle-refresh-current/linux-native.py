"""Own a baseline or candidate desktop; teardown is registered before allocation."""
from pathlib import Path
import argparse, hashlib, importlib.util, json

parser=argparse.ArgumentParser();parser.add_argument('case',choices=['baseline','candidate']);args=parser.parse_args()
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/('linux-'+args.case);out.mkdir(exist_ok=False)
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
if args.case=='baseline':
    accepted=json.loads((r/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json').read_text());source=accepted['source_revision'];image=r/'dist/packages/Ottrin-0.1.0-x86_64.AppImage'
else:
    accepted=json.loads((e/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text());source=accepted['source_revision'];image=Path(json.loads((e/'BUILD_SOURCE.json').read_text())['stage'])/'Ottrin-0.1.0-x86_64.AppImage'
digest=hashlib.sha256(image.read_bytes()).hexdigest();assert accepted['artifacts'][image.name]==digest
report={'status':'running','case':args.case,'source_revision':source,'artifact_sha256':digest,'scope':'Private software X11 Dark150; actual native workflow and idle sample. Full platform/physical acceptance separate.'}
try:
    with m.OwnedDesktop(out,scale='1.5') as d:
        config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config']
        pref=d.profile/'config/ottrin/config.json';pref.parent.mkdir(parents=True);pref.write_text(json.dumps(config)+'\n')
        icd=Path('/usr/share/vulkan/icd.d/lvp_icd.json');assert icd.is_file();d.env['VK_DRIVER_FILES']=str(icd)
        report['vulkan_cpu_icd']={'path':str(icd),'sha256':hashlib.sha256(icd.read_bytes()).hexdigest()}
        d.env.pop('NO_AT_BRIDGE',None);m.run(['gsettings','set','org.gnome.desktop.interface','toolkit-accessibility','true'],d.env)
        (out/'WORKER_SOURCE.py').write_bytes((e/'linux-idle-worker.py').read_bytes())
        report['worker_sha256']=hashlib.sha256((out/'WORKER_SOURCE.py').read_bytes()).hexdigest()
        worker=d.spawn(['/usr/bin/python3',str(out/'WORKER_SOURCE.py'),str(image),str(out)],'worker.log')
        assert worker.wait(timeout=240)==0, 'Native idle workflow failed; inspect preserved result/log'
        report['status']='functional pass; CPU and original captures require review'
except BaseException as error:
    report.update(status='failed',error=repr(error));raise
finally:
    if (out/'TEARDOWN.json').exists():report['teardown']=json.loads((out/'TEARDOWN.json').read_text())
    (out/'CONTROL.json').write_text(json.dumps(report,indent=2)+'\n')
