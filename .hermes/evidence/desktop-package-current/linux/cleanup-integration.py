"""Real packaged uninstall restores private folder registration; refusals retain it."""
from pathlib import Path
import importlib.util,json,time,hashlib
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'linux-cleanup';assert not out.exists();b=Path(json.loads((e/'BUILD_SOURCE.json').read_text())['stage'])/'Ottrin-0.1.0-x86_64.AppImage'
s=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
report={'binary_sha256':hashlib.sha256(b.read_bytes()).hexdigest()}
try:
 with m.OwnedDesktop(out) as d:
  source=d.profile/'original.txt';source.write_bytes(b'Uninstall must preserve user source and preferences.\n');original=source.read_bytes()
  cfg=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];cfg.pop('restore_session',None);cfg.pop('last_session_dirs',None)
  cp=d.profile/'config/ottrin/config.json';cp.parent.mkdir();cp.write_text(json.dumps(cfg)+'\n');before=cp.read_bytes()
  mime=d.profile/'config/mimeapps.list';baseline='[Default Applications]\ninode/directory=previous.desktop;\napplication/pdf=reader.desktop;\n';mime.write_text(baseline)
  def cli(*args,check=True):return m.run([str(b),*args],d.env,check)
  cli('folders','enable');receipt=d.profile/'state/ottrin/folder-registration.json';registered=receipt.read_bytes();installed=mime.read_bytes()
  backend=d.spawn([str(b),'file-manager'],'backend.log');end=time.monotonic()+15
  while 'true' not in m.run(['gdbus','call','--session','--dest','org.freedesktop.DBus','--object-path','/org/freedesktop/DBus','--method','org.freedesktop.DBus.NameHasOwner','org.freedesktop.FileManager1'],d.env).stdout:
   assert backend.poll() is None;assert time.monotonic()<end;time.sleep(.05)
  cache=d.profile/'cache/ottrin/preview_cache';blocked=cache/'blocked';blocked.mkdir(parents=True);keep=blocked/'preserved.txt';keep.write_bytes(b'Unexpected cache directories must not be deleted.\n')
  refused=cli('--uninstall',check=False);assert refused.returncode!=0;assert receipt.read_bytes()==registered and mime.read_bytes()==installed and keep.exists() and backend.poll() is None
  keep.unlink();blocked.rmdir();report['blocked_cache_retained_registration']=True
  desktop=d.profile/'data/applications/org.ottrin.Ottrin.Folders.desktop';own=desktop.read_bytes();desktop.write_bytes(own+b'X-Foreign-Edit=true\n')
  refused=cli('--uninstall',check=False);assert refused.returncode!=0;assert receipt.read_bytes()==registered and mime.read_bytes()==installed and desktop.read_bytes()!=own and backend.poll() is None
  desktop.write_bytes(own);report['modified_activation_retained']=True
  removed=cli('--uninstall');(out/'uninstall.stdout').write_text(removed.stdout);(out/'uninstall.stderr').write_text(removed.stderr);backend.wait(timeout=10);assert backend.returncode==0
  assert mime.read_text()==baseline;assert not receipt.exists();assert not desktop.exists();assert not (d.profile/'data/dbus-1/services/org.freedesktop.FileManager1.service').exists();assert cp.read_bytes()==before and source.read_bytes()==original
  report.update(status='pass',defaults_restored=True,backend_stopped=True,preferences_and_source_preserved=True)
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None;(e/'LINUX_CLEANUP_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
