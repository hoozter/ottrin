"""Accept Linux-only desktop/helper changes without promoting the Windows candidate."""
from pathlib import Path
import argparse,hashlib,json,os,shutil,subprocess,tempfile
r=Path.cwd();parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True);args=parser.parse_args();e=args.output.resolve();assert e.is_relative_to(r/'.hermes/evidence');public=r/'dist/packages'
def read(p):return json.loads(p.read_text())
def sha(p):
 with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
b=read(e/'BUILD_SOURCE.json');source=b['source_revision'];stage=Path(b['stage']);content=read(e/'PACKAGE_CONTENT_ACCEPTANCE.json');extract=Path(content['extraction'])
def same_product_inputs(before,after):
 subprocess.run(['git','diff','--exit-code',before,after,'--','.',':!.hermes/evidence/**',':!docs/ROADMAP.md',':!dist/**'],check=True,stdout=subprocess.PIPE)
head=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip();same_product_inputs(source,head)
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],text=True).strip()
assert content['status']=='pass' and content['source_revision']==source
binding=read(e/'HOSTED_SOURCE_SCOPE.json');assert binding['candidate_source']==source
hosted=read(e/'HOSTED_FINAL.json');assert hosted['headSha']==binding['hosted_tested_source'] and hosted['conclusion']=='success'
same_product_inputs(hosted['headSha'],source)
for name in ['verify (ubuntu-latest)','search-performance (ubuntu-latest)','verify (windows-latest)','search-performance (windows-latest)']:
 assert next(j for j in hosted['jobs'] if j['name']==name)['conclusion']=='success'
artifact=content['artifacts']['Ottrin-0.1.0-x86_64.AppImage'];sessions={}
for name in ['main-dark100','main-light150','properties-dark100','properties-light150','icon','upgrade-view']:
 a=read(e/name/'RESULT.json');assert a['artifact_sha256']==artifact
 assert a['teardown']['profile_removed'] and not Path(a['teardown']['profile']).exists()
 if name.startswith('main-'):assert a['normal_exit']==0 and a['initial_files']==a['final_files'] and a['index_survived_window_close']
 elif name.startswith('properties-'):assert a['normal_exit']==0 and a['source_and_config_preserved'] and a['no_index_started']
 else:assert a['status']=='pass'
 sessions[name]=str((e/name/'RESULT.json').relative_to(r))
assert read(e/'VISUAL_REVIEW.json')['status']=='reviewed'
appearance=read(e/'APPEARANCE_ACCEPTANCE.json');assert appearance['status'].startswith('pass') and appearance['source_revision']==source and appearance['artifact_sha256']==artifact
for name in ['appearance-dark100','appearance-light150']:
 a=read(e/name/'RESULT.json');assert a['source_revision']==source and a['artifact_sha256']==artifact and a['normal_exit']==0 and a['source_and_config_preserved']
 assert a['teardown']['profile_removed'] and not Path(a['teardown']['profile']).exists()
for distro,package in [('fedora','ottrin-0.1.0-1.x86_64.rpm'),('ubuntu','ottrin_0.1.0_amd64.deb')]:
 a=read(e/distro/'VM_RESULT.json');cleanup=read(e/distro/'VM_CLEANUP.json')
 assert a['status']=='pass within actor scope' and a['source']==source and not a['setup_control']
 assert a['package_sha256']==content['artifacts'][package]
 assert cleanup['domain_removed'] and len(cleanup['volumes_removed'])==3 and a['temporary_seed_and_ssh_keys_removed']
previous=read(public/'LINUX_PACKAGES_ACCEPTANCE.json');indexd=sha(extract/'squashfs-root/usr/bin/ottrin-indexd')
identity=read(e/'UNCHANGED_RUNTIME_IDENTITY.json')
assert identity['status']=='pass' and identity['candidate_source']==source
assert identity['candidate_appimage_sha256']==artifact and identity['previous_source']==previous['source_revision']
assert identity['previous_appimage_sha256']==previous['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
assert identity['identical_complete_payloads']['usr/bin/ottrin-indexd']==indexd==previous['indexd_binary_sha256']
resource_receipts={}
prior_evidence=r/previous['evidence']
for name in ['POPULATED_INDEX.json','CONTENT_INDEX.json']:
 resource=read(prior_evidence/name)
 assert resource['status']=='pass' and resource['provenance']['revision']==previous['source_revision']
 assert resource['provenance']['sha256']==identity['previous_appimage_sha256']
 assert resource['cleanup']['daemon_exited'] and resource['cleanup']['private_profile_removed']
 for label in ['populated_idle','after_burst_idle','after_restart_idle']:
  assert resource[label]['seconds']>=30 and resource[label]['cpu_percent_one_core']<0.5
 assert resource['offline_changes'] and resource['burst']['generation_unchanged'] and resource['burst']['scan_started_unchanged']
 if name=='CONTENT_INDEX.json':
  assert resource['live_same_size_edit']['mtime_ns_preserved'] and resource['offline_same_size_edit']['mtime_ns_preserved']
 resource_receipts[name]=str((prior_evidence/name).relative_to(r))
for browser,app_id in [('firefox','firefox'),('flatpak','org.kde.falkon')]:
 a=read(e/('browser-'+browser)/'RESULT.json')
 assert a['status']=='pass' and a['appimage_sha256']==artifact and a['source_revision']==source
 assert a['actual_portal_app_id']==app_id and a['source_sha256']==a['saved_sha256']=='d33cc86d0fcf836c80ee17a691eecb202f4f85240b50754202aba67e8a8af6c6'
 assert a['no_index_started'] and a['teardown']['profile_removed'] and not Path(a['teardown']['profile']).exists()
for name in ['menu-arrows','menu-space']:
 a=read(e/name/'RESULT.json')
 assert a['status'].startswith('pass') and a['artifact_sha256']==artifact and a['source_revision']==source
 assert a['normal_exit']==0 and a['source_files_unchanged'] and a['teardown']['profile_removed'] and not Path(a['teardown']['profile']).exists()
 if name=='menu-arrows':assert a['main_selection_retained_behind_menu'] and a['inspector_remained_open_after_popup_escape'] and a['second_escape_closes_inspector']
 else:assert not a['main_menu_space_opened_inspector'] and not a['inspector_menu_space_closed_inspector'] and a['normal_space_opens_and_closes_inspector']
namespace=read(e/'PACKAGED_NAMESPACE_APPIMAGE.json');assert namespace['status']=='pass' and namespace['binary_sha256']==indexd
# RPM strips symbols. Bind its native test to its own exact ELF bytes, then
# compare code sections just as packaging/verify-release-artifacts.sh does.
with tempfile.TemporaryDirectory(prefix='.rpm-identity-',dir=e) as temporary:
 temporary=Path(temporary);rpm_binary=temporary/'ottrin-indexd';tools=r/'.hermes/tools/rpm'
 with rpm_binary.open('wb') as output:
  subprocess.run(['bash','-c','set -euo pipefail; "$1" "$2" | cpio --extract --to-stdout ./usr/bin/ottrin-indexd','rpm-identity',str(tools/'usr/bin/rpm2cpio'),str(stage/'ottrin-0.1.0-1.x86_64.rpm')],env={**os.environ,'LD_LIBRARY_PATH':str(tools/'usr/lib/x86_64-linux-gnu')},stdout=output,stderr=subprocess.PIPE,check=True,timeout=60)
 namespace=read(e/'PACKAGED_NAMESPACE_RPM.json');assert namespace['status']=='pass' and namespace['binary_sha256']==sha(rpm_binary)
 code_identity={}
 for section in ['.text','.rodata']:
  for label,binary in [('rpm',rpm_binary),('appimage',extract/'squashfs-root/usr/bin/ottrin-indexd')]:
   subprocess.run(['objcopy','--dump-section',section+'='+str(temporary/(label+'-section')),str(binary),str(temporary/(label+'-dump'))],check=True)
  assert (temporary/'rpm-section').read_bytes()==(temporary/'appimage-section').read_bytes()
  code_identity[section]=sha(temporary/'rpm-section')
 (e/'RPM_DAEMON_IDENTITY.json').write_text(json.dumps({'status':'pass','rpm_binary_sha256':sha(rpm_binary),'appimage_binary_sha256':indexd,'matched_code_sections':code_identity,'basis':'Same code-section comparison as production verify-release-artifacts.sh; RPM symbol stripping changes complete ELF bytes','temporary_extraction_removed_on_context_exit':True},indent=2)+'\n')
for name,digest in read(e/'DELIVERABLE_BASELINE.json').items():assert sha(r/name)==digest
for name,digest in content['artifacts'].items():assert sha(stage/name)==digest
files=[p for root in [stage,extract] for p in root.rglob('*') if p.is_file() and not p.is_symlink()]
assert all(p.stat().st_nlink==1 for p in files)
assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
(e/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(previous,indent=2)+'\n')
receipt={'status':'pass within exact Linux package, private GUI and fresh native distribution scopes','source_revision':source,'artifacts':content['artifacts'],'evidence':str(e.relative_to(r)),'indexd_binary_sha256':indexd,'html_binary_sha256':sha(extract/'squashfs-root/usr/libexec/ottrin/ottrin-html-render'),'checks':content['checks'],'gui_sessions':sessions,'appearance_receipt':str((e/'APPEARANCE_ACCEPTANCE.json').relative_to(r)),'browser_sessions':['browser-firefox/RESULT.json','browser-flatpak/RESULT.json'],'menu_keyboard_sessions':['menu-arrows/RESULT.json','menu-space/RESULT.json'],'native_distribution_lifecycle':['Fedora44 DNF/RPM','Ubuntu24.04 APT/DEB'],'native_scope':'Normal setup without controls, actual portal chooser, idle status, same-version reinstall, modified-asset refusal, online/offline native removal and preference restoration, portable picker surviving native removal. Source/config unchanged.','resource_identity':{'daemon_binary_identical_to_previous':indexd==previous['indexd_binary_sha256'],'receipts':resource_receipts,'note':'Prior exact source928b7937 10000-file metadata/content controls retain original source/artifact scopes; complete daemon/helper binary equality binds unchanged runtime evidence. Current GUI/browser/native lifecycle checked separately. No new personal or sustained workload claim.'},'hosted_source':{'run':binding['run'],'tested_source_revision':hosted['headSha'],'product_inputs_match_package_source':True,'linux':'success','performance':'success','windows_state_at_linux_promotion':hosted['status']},'scope':'Local four-format Linux delivery. Physical graphics/devices, external production picker use and final owner acceptance remain separate. Windows internal candidate and failed MSI rollback gate unchanged.','cleanup':'Owned GUI profiles/processes/displays/buses/mounts and both native VM domains/volumes/credentials removed; staging/extraction retired after promotion. Current compiler caches and compact receipts retained.'}
for name in content['artifacts']:
 subprocess.run(['bash','-c','set -euo pipefail;source "$1";ottrin_publish_artifact "$2" "$3"','publish',str(r/'packaging/package-lifecycle.sh'),str(stage/name),str(public/name)],check=True)
 for suffix in ['.sha256','.provenance.json']:
  side=stage/(name+suffix)
  if side.exists():shutil.move(side,public/side.name)
 assert sha(public/name)==content['artifacts'][name]
for path in [public/'LINUX_PACKAGES_ACCEPTANCE.json',e/'ACCEPTANCE.json']:path.write_text(json.dumps(receipt,indent=2)+'\n')
assert not any(stage.iterdir());stage.rmdir();shutil.rmtree(extract)
b.update(pending_consumers=[],cleanup_complete=True,delivery_status='Accepted local Linux cohort promoted; staging/extraction removed');(e/'BUILD_SOURCE.json').write_text(json.dumps(b,indent=2)+'\n')
print('Promoted four Linux packages; staging and extraction removed.')
