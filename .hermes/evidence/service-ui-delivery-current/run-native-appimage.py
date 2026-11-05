"""Registered owned display/bus/profile/process teardown for production source GUI QA."""
from pathlib import Path
import importlib.util,json,subprocess
R=Path.cwd();E=Path(__file__).resolve().parent
s=importlib.util.spec_from_file_location('desktop',R/'scripts/linux-desktop-qa/session.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
with m.OwnedDesktop(E/'native-appimage') as desktop:
 desktop.env.update(NO_AT_BRIDGE='0',GTK_MODULES='gail:atk-bridge');desktop.env.pop('AT_SPI_BUS_ADDRESS',None)
 subprocess.run(['gsettings','set','org.gnome.desktop.interface','toolkit-accessibility','true'],env=desktop.env,check=True,capture_output=True,timeout=15)
 worker=desktop.spawn(['/usr/bin/python3',str(E/'native-appimage-worker.py')],'worker.log')
 code=worker.wait(timeout=240);assert code==0,code
 result=json.loads((E/'native-appimage/RESULT.json').read_text());assert result['status'].startswith('pass')
print('Staged AppImage GUI worker passed; private desktop and state removed; original review required')
