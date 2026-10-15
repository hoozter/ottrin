import pathlib,subprocess,tempfile,os,hashlib,json,sys
root=pathlib.Path.cwd();stage=root/'.hermes/packaging-tmp/linux-current';expected=sys.argv[1]
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==expected
source=(root/'packaging/verify-release-artifacts.sh').read_text()
def chunk(start,end):return source[source.index(start):source.index(end,source.index(start))]
def sha(p):return hashlib.file_digest(p.open('rb'),'sha256').hexdigest()
with tempfile.TemporaryDirectory(prefix='ottrin-linux-final-',dir=root/'.hermes/qa/copy-payload-current') as directory:
 temp=pathlib.Path(directory);app=stage/'Ottrin-0.1.0-x86_64.AppImage'
 subprocess.run([str(app),'--appimage-extract'],cwd=temp,check=True,stdout=subprocess.DEVNULL)
 script='set -euo pipefail\nsource "$ROOT/packaging/package-lifecycle.sh"\n'
 script+=chunk('verify_source_revision() {','\nverify_source_revision \\\n')
 script+=chunk('verify_linux_privileged_helper_policy_bytes() {','\nPORTABLE_POLICY=')
 script+=chunk('# Native packages are release deliverables too.','# The AppImage is a separately assembled release artifact.')
 env=os.environ.copy();env.update(ROOT=str(root),APPIMAGE_EXTRACT_DIR=str(temp),LINUX_DEB=str(stage/'ottrin_0.1.0_amd64.deb'),LINUX_RPM=str(stage/'ottrin-0.1.0-1.x86_64.rpm'),VERSION='0.1.0',EXPECTED_SOURCE_REVISION=expected,PATH=str(root/'.hermes/tools/rpm/usr/bin')+':'+env['PATH'],RPM_CONFIGDIR=str(root/'.hermes/tools/rpm/usr/lib/rpm'),LD_LIBRARY_PATH=str(root/'.hermes/tools/rpm/usr/lib/x86_64-linux-gnu'))
 subprocess.run(['bash','-c',script],env=env,check=True)
 archive=stage/'Ottrin-0.1.0-linux-x86_64-portable.tar.gz'
 subprocess.run(['tar','-xzf',str(archive),'-C',str(temp)],check=True)
 portable=temp/'Ottrin-0.1.0-linux-x86_64';appdir=temp/'squashfs-root'
 pairs=[('libexec/ottrin/ottrin','usr/bin/ottrin'),('libexec/ottrin/ottrin-indexd','usr/bin/ottrin-indexd'),('bin/ottrin-priv-helper','usr/libexec/ottrin/ottrin-priv-helper'),('libexec/ottrin/ottrin-media-playback','usr/bin/ottrin-media-playback'),('libexec/ottrin/ottrin-legacy-doc-render','usr/bin/ottrin-legacy-doc-render'),('libexec/ottrin/pdfium/lib/libpdfium.so','usr/bin/pdfium/lib/libpdfium.so')]
 for a,b in pairs:assert sha(portable/a)==sha(appdir/b),(a,b)
 for name in ['USER_GUIDE.md','DEVELOPMENT.md','ROADMAP.md','RELEASE_NOTES.md','RELEASE_CHECKLIST.md']:
  current=(root/'docs'/name).read_bytes()
  assert (portable/'docs'/name).read_bytes()==current
  for d in ['squashfs-root','deb','rpm']:assert (temp/d/'usr/share/doc/ottrin'/name).read_bytes()==current
 for d in [portable/'SOURCE_REVISION.txt',appdir/'usr/share/doc/ottrin/SOURCE_REVISION.txt']:
  text=d.read_text();assert 'Git revision: '+expected in text and 'Working tree: clean' in text
 for d in ['deb','rpm']:assert 'Tracked tree: clean' in (temp/d/'usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text()
 for kind,executable in [('appimage',appdir/'usr/bin/ottrin-indexd'),('rpm',temp/'rpm/usr/bin/ottrin-indexd')]:
  subprocess.run(['python3',str(root/'packaging/tests/verify-linux-native-daemon-ownership.py'),'--binary',str(executable),'--output',str(root/'.hermes/evidence/copy-payload-current'/('PACKAGED_NAMESPACE_'+kind.upper()+'.json'))],check=True)
 artifacts={p.name:sha(p) for p in stage.iterdir() if p.suffix in ['.rpm','.deb','.AppImage'] or p.name.endswith('.tar.gz')}
 result={'status':'pass','source_revision':expected,'artifacts':artifacts,'checks':['production native verifier block; helper bytes/mode, hooks, dependencies, DEB exact and RPM code identities','portable six payload byte identities','all four formats contain exact current guide and release docs','clean embedded source revision','all package runtime identities checked against the actual rebuilt AppImage'],'scope':'Linux package content verification; native runtime acceptance retains separately recorded exact artifact scopes','verifier_sha256':sha(root/'packaging/verify-release-artifacts.sh'),'cleanup':'private extractions removed by TemporaryDirectory'}
(root/'.hermes/evidence/copy-payload-current/FINAL_PACKAGE_IDENTITIES.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
