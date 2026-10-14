from pathlib import Path
import hashlib,json,re,subprocess,tarfile,tempfile,shutil,sys
repo=Path.cwd();base=Path(sys.argv[2]).resolve().parent;packages=Path(sys.argv[1]).resolve();receipt_path=Path(sys.argv[2]).resolve();s=(repo/'packaging/verify-release-artifacts.sh').read_text()
def function(name):
 start=s.index(name+'() {');end=s.index('\n}\n',start)+3;return s[start:end]
start=s.index('# Native packages are release deliverables');end=s.index("verify_source_revision \\\n  'Linux AppImage'",start);block=s[start:end]
revision=json.loads((packages/'Ottrin-0.1.0-x86_64.AppImage.provenance.json').read_text())['revision']
with tempfile.TemporaryDirectory(prefix='ottrin-native-gate-') as temp:
 root=Path(temp)
 subprocess.run([str(packages/'Ottrin-0.1.0-x86_64.AppImage'),'--appimage-extract'],cwd=temp,check=True,stdout=subprocess.DEVNULL)
 script=root/'check.sh';script.write_text('set -euo pipefail\nROOT="$1"\nAPPIMAGE_EXTRACT_DIR="$2"\nEXPECTED_SOURCE_REVISION="$3"\nVERSION=0.1.0\nLINUX_DEB="${4:-$ROOT/dist/packages/ottrin_0.1.0_amd64.deb}"\nLINUX_RPM="$5"\nsource "$ROOT/packaging/package-lifecycle.sh"\nexport PATH="$ROOT/.hermes/tools/rpm/usr/bin:$PATH"\nexport LD_LIBRARY_PATH="$ROOT/.hermes/tools/rpm/usr/lib/x86_64-linux-gnu"\nrpm() { "$ROOT/.hermes/tools/rpm/usr/bin/rpm" --define "_rpmconfigdir $ROOT/.hermes/tools/rpm/usr/lib/rpm" "$@"; }\n'+function('verify_source_revision')+'\n'+function('verify_linux_privileged_helper_policy_bytes')+'\n'+block)
 p=subprocess.run(['bash',str(script),str(repo),temp,revision,str(packages/'ottrin_0.1.0_amd64.deb'),str(packages/'ottrin-0.1.0-1.x86_64.rpm')],capture_output=True,text=True);assert p.returncode==0,p.stderr
 notes={name:(root/name/'usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text() for name in ['deb','rpm']}
 with tarfile.open(packages/'Ottrin-0.1.0-linux-x86_64-portable.tar.gz') as tar:
  prefix='Ottrin-0.1.0-linux-x86_64/'
  pairs=[('libexec/ottrin/ottrin','usr/bin/ottrin'),('libexec/ottrin/ottrin-indexd','usr/bin/ottrin-indexd'),('libexec/ottrin/ottrin-media-playback','usr/bin/ottrin-media-playback'),('libexec/ottrin/ottrin-legacy-doc-render','usr/bin/ottrin-legacy-doc-render'),('bin/ottrin-priv-helper','usr/libexec/ottrin/ottrin-priv-helper'),('libexec/ottrin/pdfium/lib/libpdfium.so','usr/bin/pdfium/lib/libpdfium.so')]
  for portable_path,appimage_path in pairs:
   assert hashlib.sha256(tar.extractfile(prefix+portable_path).read()).digest()==hashlib.sha256((root/'squashfs-root'/appimage_path).read_bytes()).digest(),portable_path
  provenance=tar.extractfile(prefix+'SOURCE_REVISION.txt').read().decode()
  assert 'Git revision: '+revision in provenance and 'Working tree: clean' in provenance,provenance
portable=packages/'Ottrin-0.1.0-linux-x86_64-portable.tar.gz'
b=s[s.index('for entry in \\\n'):];b=b[:b.index('; do')];patterns=re.findall(r"'([^']+)'",b)
with tarfile.open(portable) as tar:
 names=tar.getnames()
 for pattern in patterns:assert any(re.search(pattern,n) for n in names),pattern
 assert not any('/bin/pdfium/' in n for n in names)
receipt={'date':'2026-10-15','verifier_source_sha256':hashlib.sha256(s.encode()).hexdigest(),'checks':['Current exact DEB/RPM accepted by production native verification block','Five executables and PDFium renderer: DEB bytes and RPM code/readonly sections match exact AppImage','Payload-derived GLIBC, polkit metadata and native embedded revision verified','Portable five executables/PDFium match exact AppImage; embedded clean revision matches','All 19 portable entry requirements match exact archive'],'artifacts':{name:{'sha256':hashlib.sha256((packages/name).read_bytes()).hexdigest(),'source_revision':revision} for name in ['Ottrin-0.1.0-x86_64.AppImage','Ottrin-0.1.0-linux-x86_64-portable.tar.gz','ottrin_0.1.0_amd64.deb','ottrin-0.1.0-1.x86_64.rpm']},'embedded_native_provenance':notes,'cleanup':'Private AppImage/native package extractions and modified fixture removed','limits':['Full multi-platform release verifier not run: exact current-release Windows/macOS artifacts remain separate gates','Native install/authentication behavior not covered by metadata verifier']}
receipt_path.write_text(json.dumps(receipt,indent=2)+'\n')
print('Current production native package comparison and portable paths PASS; extraction cleaned')
