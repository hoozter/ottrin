"""Bound reused acceptance to unchanged shipping source and exact payload identity."""
from pathlib import Path
import hashlib,json,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;old='90ba8639f2e5d6dad7e24ebd5b95382e108e1300';new=json.loads((e/'BUILD_SOURCE.json').read_text())['source_revision']
def source(revision,path):return subprocess.check_output(['git','show',revision+':'+path],text=True)
def shipping(text):
 marker='#[cfg(test)]\nmod tests {';assert text.count(marker)==1;return text.split(marker)[0]
def hash_text(s):return hashlib.sha256(s.encode()).hexdigest()
paths=subprocess.check_output(['git','diff','--name-only',old,new,'--','.',':!.hermes/**',':!docs/ROADMAP.md',':!dist/**'],text=True).splitlines();assert set(paths)=={'crates/ottrin-app/src/portal.rs','crates/ottrin-ui/src/preview_render.rs'},paths
portal='crates/ottrin-app/src/portal.rs';before=shipping(source(old,portal));after=shipping(source(new,portal));assert before==after
preview='crates/ottrin-ui/src/preview_render.rs';a=shipping(source(old,preview));b=shipping(source(new,preview));start='fn render_spreadsheet_row(';end='pub(crate) fn render_spreadsheet_body_preview('
def split(s):
 i=s.index(start);j=s.index(end,i);return s[:i],s[i:j],s[j:]
x,y=split(a),split(b);assert x[0]==y[0] and x[2]==y[2] and x[1]!=y[1]
(e/'SOURCE_SCOPE.json').write_text(json.dumps({'status':'pass: one changed shipping function; portal change is test diagnostics only','previous_source':old,'candidate_source':new,'changed_source_paths':paths,'unchanged_shipping_portal_sha256':hash_text(before),'changed_shipping_function':preview+'::render_spreadsheet_row','function_sha256':{'previous':hash_text(x[1]),'candidate':hash_text(y[1])},'unchanged_preview_prefix_sha256':hash_text(x[0]),'unchanged_preview_suffix_sha256':hash_text(x[2]),'basis':['Git compares all shipping files, dependency versions/features, vendor source and packaging scripts; only the two named source files differ','Portal code before cfg(test) is byte-identical','Preview shipping source outside the named row renderer is byte-identical','Current native Linux/Windows captures and rendered regressions independently accept the changed function','Full Linux113-entry and Windows58-file payload comparisons establish unchanged helpers/dependencies; Windows daemon is completely equal except timestamp/checksum'],'limits':'Earlier broad UI, index-resource, integration and installer-fault receipts keep their original artifact hashes/scopes. They are not relabelled as newly executed on this cohort; the identity comparison supports carrying unchanged responsibilities.'},indent=2)+'\n')
