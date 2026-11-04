"""Check the Windows-only diagnostic text correction without rerunning unchanged UI tests."""
from pathlib import Path
import json,os,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;source='a3db57fea33f81a4441fa3751c7b690c3279b68b'
original=subprocess.check_output(['git','show',source+':crates/ottrin-app/src/main.rs'],text=True)
def without_reporter(text):
 a=text.index('fn report_windows_startup_failure(');b=text.index('\n#[cfg(target_os = "windows")]\nfn install_windows_startup_panic_hook()',a)
 return text[:a]+text[b:]
assert without_reporter((r/'crates/ottrin-app/src/main.rs').read_text())==without_reporter(original)
subprocess.run(['git','diff','--exit-code',source,'--','crates/ottrin-ui','crates/ottrin-search','crates/ottrin-core','crates/ottrin-app/src/bin','Cargo.toml','Cargo.lock'],check=True,stdout=subprocess.PIPE)
env={**os.environ,'CARGO_BUILD_JOBS':'1','PKG_CONFIG_PATH':str(r/'.hermes/tools/gstreamer-dev/usr/lib/x86_64-linux-gnu/pkgconfig')}
steps=[('dialog-native-clippy',['cargo','clippy','-p','ottrin-app','--all-targets','--','-D','warnings']),('dialog-windows-clippy',['cargo','clippy','--target','x86_64-pc-windows-gnu','-p','ottrin-app','--all-targets','--','-D','warnings']),('dialog-format',['cargo','fmt','--all','--','--check'])]
v={'status':'running','unchanged_ui_test_source':source,'scope':'Only existing Windows startup reporter now shows actual summary/details and log path or logging failure; Ui/Search/Core/indexd source unchanged from487-pass checkpoint. Actual native error dialog still requires packaged acceptance.','checks':[]}
try:
 for name,cmd in steps:
  with (e/(name+'.log')).open('w') as log:code=subprocess.run(cmd,env=env,stdout=log,stderr=subprocess.STDOUT).returncode
  v['checks'].append({'name':name,'command':cmd,'exit':code});assert code==0,name
 v['status']='pass exact Windows-only source correction; native package acceptance pending'
finally:(e/'DIALOG_CHECKS.json').write_text(json.dumps(v,indent=2)+'\n')
