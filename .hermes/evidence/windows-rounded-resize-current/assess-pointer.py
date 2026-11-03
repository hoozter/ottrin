"""Assess exact native gestures; retain historical region and performance scope."""
from pathlib import Path
import hashlib,json,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def save(n,v):(e/n).write_text(json.dumps(v,indent=2)+'\n')
source=read(e/'BUILD_SOURCE.json')['source_revision'];assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==source
linux=[];originals={}
for name in ['linux-pointer-dark','linux-pointer-light']:
 folder=e/name;control=read(folder/'CONTROL.json');result=read(folder/'FUNCTIONAL_RESULT.json');teardown=read(folder/'TEARDOWN.json')
 assert control['source_revision']==source and result['status'].startswith('pass') and len(result['cases'])==4 and result['source_bytes_preserved']
 assert teardown['status']=='closed' and teardown['profile_removed'] and not Path(teardown['profile']).exists()
 captures=list(folder.glob('*.png'));assert len(captures)==4
 for p in captures:originals[str(p.relative_to(e))]=sha(p)
 linux.append({'control':control,'result':result,'originals_reviewed':4})
save('LINUX_NATIVE_ACCEPTANCE.json',{'status':'pass exact AppImage native pointer resize and exclusive titlebar ownership','source_revision':source,'artifact_sha256':linux[0]['control']['artifact_sha256'],'cases':8,'originals_reviewed':8,'cohorts':linux,'limits':['Dark100/Light150 private X11 software desktop','Main SE and Settings NW actual gestures at0/24 radius; no full appearance or hardware acceptance']})
folder=e/'native-pointer';result=read(folder/'results/RESULT.json');owner=read(folder/'OWNER.json');assert result['status'].startswith('pass') and len(result['cases'])==8
assert result['package']['source_revision']==source and result['source_preserved'] and result['preferences_preserved'] and result['remaining_owned_installed_processes']==0
assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed'] and len(owner['cleanup']['owned_volumes_removed'])==2
for case in result['cases']:
 p=folder/'results'/case['capture'];originals[str(p.relative_to(e))]=sha(p)
save('WINDOWS_NATIVE_ACCEPTANCE.json',{'status':'pass exact installed NSIS pointer resize and exclusive titlebar ownership','source_revision':source,'installer_sha256':result['package']['files'][0]['sha256'],'cases':8,'originals_reviewed':8,'result':str((folder/'results/RESULT.json').relative_to(r)),'source_preferences_preserved':True,'normal_close_supported_uninstall':True,'owned_cleanup':owner['cleanup'],'limits':['Dark/default96DPI software VM; forcedNative and explicitOpenGL','Main SE and Settings NW actual gestures at0/24 radius','Live editor radius changes and nondefault Windows DPI remain unverified','No new GUI CPU pass; prior Native33.54/OpenGL3.70 percent strict2percent failures remain open']})
save('ORIGINAL_REVIEW.json',{'status':'all16originalcaptures directly reviewed','source_revision':source,'originals':originals,'findings':'Square0 and rounded24 frames match intended shapes; mainSE grows both axes and SettingsNW shifts origin while shrinking both axes. No visual permission to waive GUI performance failure or accept entire interface.'})
previous='cbc7cd7ca8c1994b7b80d6a26dd6549369c7096a'
changed=subprocess.check_output(['git','diff','--name-only',previous,source,'--','crates','vendor','Cargo.toml','Cargo.lock','packaging','scripts','.github'],text=True).splitlines()
expected=['crates/ottrin-search/src/lib.rs','crates/ottrin-ui/src/lib.rs','crates/ottrin-ui/src/picker.rs','crates/ottrin-ui/src/properties.rs','crates/ottrin-ui/src/window_chrome.rs','vendor/eframe/Cargo.toml','vendor/eframe/OTTRIN_PATCH.md','vendor/eframe/src/lib.rs','vendor/eframe/src/native/glow_integration.rs','vendor/eframe/src/native/wgpu_integration.rs','vendor/eframe/src/window_shape.rs'];assert changed==expected,changed
save('SOURCE_SCOPE.json',{'status':'pass inspected exact source scope','previous_source':previous,'candidate_source':source,'changed_files':changed,'shipping_behavior':'SharedWindowsnativeviewportGDIregion, maximized rectangular frame, theme-radius-aware visible-edge resize and exclusive titlebar input. Search change confined to existing cfg(test) function. No daemon runtime, installer, service, picker protocol or media implementation changes.','native_region_reference':'NATIVE_SCOPE_COMPARISON.json','daemon_identity':['LINUX_INDEX_ELF_COMPARISON.json','WINDOWS_INDEX_PE_COMPARISON.json'],'limits':'Identity and source inspection carry only unchanged responsibilities, not a newly executed full roadmap matrix.'})
for name,path in [('PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json',r/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json'),('PREVIOUS_WINDOWS_CANDIDATE_ACCEPTANCE.json',r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json')]:
 assert not (e/name).exists();(e/name).write_bytes(path.read_bytes())
print('Accepted16nativepointergestures/16reviewedoriginals; full appearance/performance remains open')
