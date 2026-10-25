"""Verify this staged Linux cohort with production verifier blocks; retain one owned AppImage extraction for GUI QA."""
from pathlib import Path
import hashlib,json,os,subprocess,sys,tempfile
root=Path.cwd();e=root/'.hermes/evidence/linux-sandbox-package-current'
build=json.loads((e/'BUILD_SOURCE.json').read_text());expected=build['source_revision'];stage=Path(build['stage'])
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==expected
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],text=True).strip()
extract=root/'.hermes/packaging-tmp/.linux-sandbox-extract';assert not extract.exists();extract.mkdir()
(e/'EXTRACTION_OWNER.json').write_text(json.dumps({'path':str(extract),'source':expected,'pending_consumers':['production package content/closure verification','Dark100/Light150 native GUI and file operations'],'cleanup':'remove native/portable copies after content gate; remove remaining AppImage after GUI and resource checks'},indent=2)+'\n')
source=(root/'packaging/verify-release-artifacts.sh').read_text()
def chunk(start,end):return source[source.index(start):source.index(end,source.index(start))]
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
artifacts={p.name:sha(p) for p in stage.iterdir() if p.is_file() and p.name.endswith(('.tar.gz','.AppImage','.deb','.rpm'))};assert len(artifacts)==4
for name in artifacts:
 subprocess.run(['sha256sum','-c',name+'.sha256'],cwd=stage,check=True)
appimage=stage/'Ottrin-0.1.0-x86_64.AppImage';subprocess.run([str(appimage),'--appimage-extract'],cwd=extract,check=True,stdout=subprocess.DEVNULL)
env=os.environ.copy();env.update(ROOT=str(root),APPIMAGE_EXTRACT_DIR=str(extract),LINUX_APPIMAGE=str(appimage),LINUX_ARCHIVE=str(stage/'Ottrin-0.1.0-linux-x86_64-portable.tar.gz'),LINUX_DEB=str(stage/'ottrin_0.1.0_amd64.deb'),LINUX_RPM=str(stage/'ottrin-0.1.0-1.x86_64.rpm'),VERSION='0.1.0',EXPECTED_SOURCE_REVISION=expected,PATH=str(e/'rpm-bin')+':'+env['PATH'])
script='set -euo pipefail\nsource "$ROOT/packaging/package-lifecycle.sh"\nsource "$ROOT/packaging/linux/bundle-runtime-libs.sh"\n'
script+=chunk('verify_bundled_font_notice() {','\nLINUX_CONTENTS=')
script+=chunk('verify_source_revision() {','\nverify_source_revision \\\n')
script+=chunk('verify_linux_privileged_helper_policy_bytes() {','\nPORTABLE_POLICY=')
script+=chunk('PORTABLE_EXTRACT_DIR=','# The privileged helper policy controls')
script+='\nverify_bundled_font_notice portable "$PORTABLE_STAGE/third-party-notices/BUNDLED_FONTS.md"\n'
script+=chunk('# Native packages are release deliverables too.','# The AppImage is a separately assembled release artifact.')
subprocess.run(['bash','-c',script],env=env,check=True)
portable=extract/'linux-portable/Ottrin-0.1.0-linux-x86_64';app=extract/'squashfs-root'
for a,b in [('libexec/ottrin/ottrin','usr/bin/ottrin'),('libexec/ottrin/ottrin-indexd','usr/bin/ottrin-indexd'),('bin/ottrin-priv-helper','usr/libexec/ottrin/ottrin-priv-helper'),('libexec/ottrin/ottrin-media-playback','usr/bin/ottrin-media-playback'),('libexec/ottrin/ottrin-legacy-doc-render','usr/bin/ottrin-legacy-doc-render'),('libexec/ottrin/pdfium/lib/libpdfium.so','usr/bin/pdfium/lib/libpdfium.so')]:assert sha(portable/a)==sha(app/b),(a,b)
for name in ['USER_GUIDE.md','DEVELOPMENT.md','ROADMAP.md','RELEASE_NOTES.md','RELEASE_CHECKLIST.md','SEARCH_PERFORMANCE.md']:
 data=(root/'docs'/name).read_bytes();assert (portable/'docs'/name).read_bytes()==data
 for folder in ['squashfs-root','deb','rpm']:assert (extract/folder/'usr/share/doc/ottrin'/name).read_bytes()==data
for p in [portable/'SOURCE_REVISION.txt',app/'usr/share/doc/ottrin/SOURCE_REVISION.txt']:
 text=p.read_text();assert 'Git revision: '+expected in text and 'Working tree: clean' in text
for name in ['deb','rpm']:assert 'Tracked tree: clean' in (extract/name/'usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text()
for kind,binary in [('appimage',app/'usr/bin/ottrin-indexd'),('rpm',extract/'rpm/usr/bin/ottrin-indexd')]:
 subprocess.run(['python3','packaging/tests/verify-linux-native-daemon-ownership.py','--binary',str(binary),'--output',str(e/('PACKAGED_NAMESPACE_'+kind.upper()+'.json'))],check=True)
result={'status':'pass','source_revision':expected,'artifacts':artifacts,'verifier_sha256':sha(root/'packaging/verify-release-artifacts.sh'),'checks':['production portable/AppImage runtime closure and playback checks','production native payload, dependencies, policy, hooks and code identity blocks','exact bundled-font notice in all four formats','six matching portable/AppImage runtime payloads','exact current documentation and clean source notes','actual AppImage and RPM index daemon namespace/ownership refusal checks'],'scope':'package contents and scoped native namespace checks; GUI, resource and actual distribution installation remain separate','extraction':str(extract),'pending_consumer':'native GUI; remove remaining extraction afterwards'}
(e/'PACKAGE_CONTENT_ACCEPTANCE.json').write_text(json.dumps(result,indent=2)+'\n')
# These copies have finished their content-only consumer. Keep just AppRun for GUI.
import shutil
for name in ['linux-portable','deb','rpm']:shutil.rmtree(extract/name)
for p in extract.iterdir():
 if p.is_file():p.unlink()
print(json.dumps(result,indent=2))
