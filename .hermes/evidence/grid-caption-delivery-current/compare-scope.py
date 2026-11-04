"""Bind retained native acceptance to unchanged source and exact runtime bytes."""
from pathlib import Path
import hashlib,json,subprocess
R=Path.cwd();E=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
content=read(E/'PACKAGE_CONTENT_ACCEPTANCE.json');previous=read(E/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json')
source=content['source_revision'];old=previous['source_revision']
assert old=='dacedd8dfe7bd04cbfc4208d319d17a9d797deae'
assert content['status']=='pass' and content['content_extraction_removed']
def obj(revision,name):return subprocess.check_output(['git','show',revision+':'+name])
changed=subprocess.check_output(['git','diff','--name-only',old,source,'--','crates'],text=True).splitlines()
assert set(changed)=={'crates/ottrin-ui/src/file_grid.rs','crates/ottrin-ui/src/lib.rs'},changed
removed=b'''/// How many label chars to show in grid cells at a given scale factor.
fn item_chars_for_scale(scale: f32) -> f32 {
    (10.0 * scale).round().max(6.0)
}

'''
prior=obj(old,'crates/ottrin-ui/src/lib.rs');assert prior.count(removed)==1
assert prior.replace(removed,b'')==obj(source,'crates/ottrin-ui/src/lib.rs')
# Grid dimensions and hit geometry do not change. Only obsolete character-count
# fields/setup are removed before the measured label painting block.
prior_grid=obj(old,'crates/ottrin-ui/src/file_grid.rs').decode();current_grid=obj(source,'crates/ottrin-ui/src/file_grid.rs').decode()
prior_prefix=prior_grid[:prior_grid.index('    let name = truncate_middle(')]
for line in ['    label_chars: usize,\n','        let label_chars = ((item_chars_for_scale(scale) / colors.font_scale) as usize).max(6);\n','            label_chars,\n']:
 assert line in prior_prefix;prior_prefix=prior_prefix.replace(line,'')
assert prior_prefix==current_grid[:current_grid.index('    // Measure the actual font')]
unchanged={name:digest for name,digest in content['runtime_sha256'].items() if name!='application'}
assert set(unchanged)=={'index_daemon','privileged_helper','html_renderer'}
for name,digest in unchanged.items():assert previous['runtime_sha256'][name]==digest,(name,digest,previous['runtime_sha256'][name])
assert previous['runtime_sha256']['application']!=content['runtime_sha256']['application']
old_e=R/previous['evidence']
for name in ['LINUX_PRIVILEGED_COPY_ACCEPTANCE.json','LINUX_DAEMON_RUNTIME.json','login/ACCEPTANCE.json','REPLACEMENT_ACCEPTANCE.json']:
 gate=read(old_e/name);assert gate['source_revision']==old and gate['status'].startswith('pass')
report={'status':'pass exact unchanged-source/runtime scope binding; original native acceptance is not relabelled as a fresh run','source_revision':source,'prior_source_revision':old,'changed_crate_files':changed,'changes':'Grid filename painting uses measured two-line separator wrapping; lib.rs only removes its superseded character-count helper. Grid allocation/hit geometry before label painting is unchanged.','unchanged_runtime_sha256':unchanged,'application_sha256':content['runtime_sha256']['application'],'original_native_evidence':previous['evidence'],'retained_scopes':['Original DAC helper authentication/Copy/restart/history/Undo/cleanup','Original DAC daemon private idle sample, watcher/query/restart/shutdown','Original DAC native DEB login/two-reboot and aa-to-DAC binary replacement'],'required_new_acceptance':['Actual new AppImage browser and cooperating-picker caption layout, theme/scaling and source preservation','Exact-source all-four-job CI before publication'],'limits':['Previous native scopes remain tied to DAC and its actual packages; unchanged source and binary identities justify retaining them without repeating unaffected matrices.','No Windows packaged acceptance, full U08/theme audit, physical graphics or daily-driver completion.']}
(E/'SOURCE_SCOPE.json').write_text(json.dumps(report,indent=2)+'\n');print(report['status'])
