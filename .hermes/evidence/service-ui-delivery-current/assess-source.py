"""Bind this Linux UI delivery to explicit platform boundaries and runtime bytes."""
from pathlib import Path
import copy,hashlib,json,re,subprocess,tomllib
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
source=read(e/'BUILD_SOURCE.json')['source_revision'];previous=read(e/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json');before=previous['source_revision']
def blob(revision,path):return subprocess.check_output(['git','show',revision+':'+path])
def toml(revision,path):return tomllib.loads(blob(revision,path).decode())
changed=subprocess.check_output(['git','diff','--name-only',before,source],text=True).splitlines()
metadata=[p for p in changed if p.startswith(('.hermes/evidence/','docs/','dist/')) or p=='README.md']
code=sorted(set(changed)-set(metadata))
expected=sorted(['Cargo.toml','Cargo.lock','crates/ottrin-platform/Cargo.toml','crates/ottrin-platform/src/windows_folder_integration.rs','crates/ottrin-shell/Cargo.toml','crates/ottrin-shell/src/lib.rs','crates/ottrin-shell/src/server.rs','crates/ottrin-shell/src/settings.rs','crates/ottrin-ui/src/lib.rs','packaging/tests/windows-msix-template.ps1','packaging/verify-release-artifacts.sh','packaging/windows/msix-manifest.xml','packaging/windows/package-msix.ps1','packaging/windows/package-portable.ps1','packaging/windows/package-portable.sh'])
assert code==expected,(code,expected)
a=toml(before,'Cargo.toml');b=toml(source,'Cargo.toml');a['workspace']['members'].append('crates/ottrin-shell');assert a==b
key='cfg(target_os = "windows")';a=toml(before,'crates/ottrin-platform/Cargo.toml');b=toml(source,'crates/ottrin-platform/Cargo.toml');a['target'][key]['dependencies']['ottrin-shell']={'path':'../ottrin-shell'};assert a==b
shell=toml(source,'crates/ottrin-shell/Cargo.toml');assert set(shell)=={'package','lib','target'} and set(shell['target'])=={key}
text=blob(source,'crates/ottrin-shell/src/lib.rs').decode();text=re.sub(r'^\s*//!.*$','',text,flags=re.M);text=re.sub(r'\s+','',text)
assert text=='#[cfg(target_os="windows")]modserver;#[cfg(target_os="windows")]modsettings;#[cfg(target_os="windows")]pubusesettings::{enabled,package_directory,set_enabled};'
platform=re.sub(r'\s+','',blob(source,'crates/ottrin-platform/src/lib.rs').decode());assert '#[cfg(target_os="windows")]#[path="windows_folder_integration.rs"]pubmodfolder_integration;' in platform
assert blob(before,'crates/ottrin-platform/src/lib.rs')==blob(source,'crates/ottrin-platform/src/lib.rs')
def packages(revision):
 return {(p['name'],p['version'],p.get('source','')):p for p in toml(revision,'Cargo.lock')['package']}
a=packages(before);b=packages(source);new=('ottrin-shell','0.1.0','');assert set(b)==set(a)|{new}
b.pop(new);platform=('ottrin-platform','0.1.0','');b[platform]['dependencies'].remove('ottrin-shell');assert a==b
old=blob(before,'packaging/verify-release-artifacts.sh').decode();current=blob(source,'packaging/verify-release-artifacts.sh').decode();added=[line for line in current.splitlines(keepends=True) if 'ottrin_shell' in line]
assert set(added)=={"  '/bin/ottrin_shell\\.dll$' \\\n","  'bin/ottrin_shell.dll' \\\n"} and len(added)==2
for line in added:current=current.replace(line,'',1)
assert current==old,'Production Linux verifier changed outside the explicitly reviewed Windows additions'
content=read(e/'PACKAGE_CONTENT_ACCEPTANCE.json');assert content['source_revision']==source
for role in ['index_daemon','privileged_helper','html_renderer']:assert content['runtime_sha256'][role]==previous['runtime_sha256'][role],role
result={'status':'pass explicit Linux source/platform-boundary and unchanged non-GUI runtime scope','source_revision':source,'previous_source_revision':before,'changed_code_paths':code,'metadata_paths':metadata,'checks':['Only shared UI source changes affect Linux runtime implementation','Platform dependency addition and new shell implementation are Windows-gated; common dependency versions and Linux graph entries unchanged','Existing platform lib Windows module gate unchanged','Production Linux verifier bytes unchanged after removing exactly two Windows DLL requirements','Actual four-format daemon/helper/HTML hashes exactly match previous delivery'],'runtime_sha256':content['runtime_sha256'],'limits':['Shared UI changed and requires fresh mounted-image acceptance, recorded separately','No Windows artifact/native acceptance inherited from this Linux assessment','Full theme/appearance audit and protected Copy remain separate']}
(e/'SOURCE_SCOPE.json').write_text(json.dumps(result,indent=2)+'\n');print(result['status'])
