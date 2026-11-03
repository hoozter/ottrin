from pathlib import Path
import json,hashlib,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;old='4a1682bdee98b16b63a7723785091399e99b505f';new=json.loads((e/'BUILD_SOURCE.json').read_text())['source_revision']
paths=subprocess.check_output(['git','diff','--name-only',old,new,'--','.',':!.hermes/**',':!dist/**',':!docs/ROADMAP.md',':!docs/RELEASE_NOTES.md',':!scripts/linux-desktop-qa.py',':!scripts/tests/test_linux_desktop_qa.py'],text=True).splitlines();assert paths==['crates/ottrin-ui/src/lib.rs'],paths
path=paths[0]
def read(rev):return subprocess.check_output(['git','show',rev+':'+path],text=True)
def sha(s):return hashlib.sha256(s.encode()).hexdigest()
a=read(old);b=read(new);marker='\n#[cfg(test)]\nmod tests {';assert marker in a and marker in b
before=a.split(marker,1)[0];after=b.split(marker,1)[0];line='        app.request_miller_reveal(app.state.active_tab().id);\n';assert after.count(line)==1;assert after.replace(line,'',1)==before
report={'status':'pass: exactly one shipping constructor call differs from4a','previous_runtime_source':old,'candidate_source':new,'changed_shipping_path':path,'added_call':line.strip(),'unchanged_shipping_source_sha256':sha(before),'basis':['All other crates, dependencies, vendor source, packaging and build inputs unchanged','Only new one-shot Miller reveal call in shipping UI constructor; added rendered regression is test-only','Source rendered RED/GREEN, full464-pass suite and native Dark100/Light150 captures recorded in parent','Payload bytes and native changed-function acceptance remain separate gates'],'limits':'No older native receipt is relabelled as executed on this build. Current staged binaries still require content/native acceptance.'}
(e/'SOURCE_SCOPE.json').write_text(json.dumps(report,indent=2)+'\n');print(report['status'])
