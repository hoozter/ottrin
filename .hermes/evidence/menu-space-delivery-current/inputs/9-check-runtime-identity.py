"""Bind unchanged runtime evidence to both exact AppImages, without a second extraction."""
from pathlib import Path
import argparse,hashlib,json,subprocess
r=Path.cwd();parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True);e=parser.parse_args().output.resolve();assert e.is_relative_to(r/'.hermes/evidence')
read=lambda path:json.loads(path.read_text())
def sha(path):
 with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
old=r/'dist/packages/Ottrin-0.1.0-x86_64.AppImage';prior=read(r/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json');assert sha(old)==prior['artifacts'][old.name]
content=read(e/'PACKAGE_CONTENT_ACCEPTANCE.json');build=read(e/'BUILD_SOURCE.json');candidate=Path(build['stage'])/old.name;assert sha(candidate)==content['artifacts'][old.name];extract=Path(content['extraction'])/'squashfs-root'
offset=subprocess.check_output([str(old),'--appimage-offset'],text=True).strip();assert offset.isdigit()
results={}
for path in ['usr/bin/ottrin-indexd','usr/bin/ottrin-media-playback','usr/bin/ottrin-legacy-doc-render','usr/libexec/ottrin/ottrin-priv-helper','usr/libexec/ottrin/ottrin-html-render','usr/bin/pdfium/lib/libpdfium.so']:
 with subprocess.Popen(['unsquashfs','-offset',offset,'-cat',str(old),path],stdout=subprocess.PIPE,stderr=subprocess.PIPE) as process:
  digest=hashlib.file_digest(process.stdout,'sha256').hexdigest();error=process.stderr.read().decode();assert process.wait()==0,error
 assert digest==sha(extract/path),path;results[path]=digest
assert results['usr/bin/ottrin-indexd']==prior['indexd_binary_sha256'];assert results['usr/libexec/ottrin/ottrin-html-render']==prior['html_binary_sha256']
(e/'UNCHANGED_RUNTIME_IDENTITY.json').write_text(json.dumps({'status':'pass','previous_source':prior['source_revision'],'previous_appimage_sha256':prior['artifacts'][old.name],'candidate_source':build['source_revision'],'candidate_appimage_sha256':content['artifacts'][old.name],'identical_complete_payloads':results,'scope':'Exact previously accepted daemon/helper/renderer binaries. Resource receipts retain original artifact/source scopes; no new personal or sustained workload claim. New app GUI/native installation are independently verified.'},indent=2)+'\n')
