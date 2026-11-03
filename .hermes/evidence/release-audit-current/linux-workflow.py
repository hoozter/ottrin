from pathlib import Path
from contextlib import ExitStack
import hashlib,importlib.util,json,os,signal,subprocess,sys,time
import argparse,re
import tarfile,tempfile
parser=argparse.ArgumentParser();parser.add_argument('--case',required=True);inputs=parser.add_mutually_exclusive_group();inputs.add_argument('--binary',type=Path);inputs.add_argument('--portable-archive',type=Path);args=parser.parse_args();assert re.fullmatch('[a-z0-9-]+',args.case)
root=Path.cwd();e=root/'.hermes/evidence/release-audit-current'/args.case;e.mkdir(exist_ok=False)
binary=(args.binary or args.portable_archive).resolve() if (args.binary or args.portable_archive) else root/'dist/packages/Ottrin-0.1.0-x86_64.AppImage';accepted=json.loads((root/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json').read_text());digest=hashlib.file_digest(binary.open('rb'),'sha256').hexdigest();assert args.binary or digest==accepted['artifacts'][binary.name]
spec=importlib.util.spec_from_file_location('desktop',root/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
report={'status':'running','artifact_sha256':digest,'runtime_source':accepted['source_revision'],'scope':'Existing complete packaged daily-driver smoke in private X11/AT-SPI/software desktop; actual filesystem/UI results required','driver_sha256':hashlib.sha256((root/'scripts/linux-desktop-qa.py').read_bytes()).hexdigest()}
def save():(e/'CONTROL.json').write_text(json.dumps(report,indent=2)+'\n')
if args.binary:report.update(scope='Source GUI daily-driver smoke; no package acceptance',source_patch_sha256=hashlib.sha256(subprocess.check_output(['git','diff','--','crates'])).hexdigest())
save()
try:
 with ExitStack() as package_stack:
  if args.portable_archive:
   extraction=Path(package_stack.enter_context(tempfile.TemporaryDirectory(prefix='ottrin-portable-workflow-',dir='/var/tmp')))
   report['owned_extraction']=str(extraction);save()
   with tarfile.open(binary,'r:gz') as archive:
    members=archive.getmembers();tops={Path(m.name).parts[0] for m in members};assert len(tops)==1
    archive.extractall(extraction,filter='data')
   package_root=extraction/next(iter(tops));binary=package_root/'bin/ottrin';assert binary.is_file() and os.access(binary,os.X_OK)
   report.update(scope='Exact delivered portable archive daily-driver workflow through its real bin/ottrin launcher in private X11/AT-SPI/software desktop',launcher_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),portable_source=(package_root/'SOURCE_REVISION.txt').read_text().strip());assert ('Git revision: '+accepted['source_revision']) in report['portable_source'].splitlines();save()
  d=package_stack.enter_context(desktop.OwnedDesktop(e))
  qa=d.profile/'workflow';qa.mkdir();report['qa_root']=str(qa);save()
  # The nested fixture has distinct HOME/XDG roots. Register their teardown
  # before a process can allocate them, and reap the controller before cleanup.
  with ExitStack() as stack:
   stack.callback(desktop.cleanup_owned_mounts,qa)
   stack.callback(desktop.cleanup_profile_children,qa)
   d.env.pop('NO_AT_BRIDGE',None)
   code="import importlib.util,pathlib,signal,sys;signal.signal(signal.SIGTERM,lambda s,f:sys.exit(128+s));signal.signal(signal.SIGINT,lambda s,f:sys.exit(128+s));spec=importlib.util.spec_from_file_location('workflow',sys.argv[1]);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);qa=pathlib.Path(sys.argv[2]);sys.argv=[sys.argv[1],'--binary',sys.argv[3],'--evidence',sys.argv[4]];sys.exit(m.run_qa(qa))"
   p=d.spawn(['/usr/bin/python3','-c',code,str(root/'scripts/linux-desktop-qa.py'),str(qa),str(binary),str(e)],'workflow.log')
   stack.callback(desktop.workspace.stop_tests,p)
   deadline=time.monotonic()+420
   while p.poll() is None:
    assert time.monotonic()<deadline,'Owned workflow finite lease expired'
    time.sleep(.3)
   report['exit']=p.returncode;save();assert p.returncode==0,'Actual workflow failed; inspect preserved report/log'
   result=json.loads((e/'results.json').read_text());assert result['results'] and all(x['status']=='passed' for x in result['results']);report['checks']=result['results'];report['status']='pass within actual workflow scope';save()
except BaseException as error:
 report['status']='failed current packaged workflow';report['error']=repr(error);save();raise
finally:
 if (e/'TEARDOWN.json').exists():report['teardown']=json.loads((e/'TEARDOWN.json').read_text());save()
 if report.get('owned_extraction'):
  report['portable_extraction_removed']=not Path(report['owned_extraction']).exists();save()
  assert report['portable_extraction_removed'],'Owned portable extraction remains after registered teardown'
