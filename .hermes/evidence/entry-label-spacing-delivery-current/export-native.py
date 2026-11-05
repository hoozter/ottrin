"""Export terminal owned native outputs without copying private bootstrap."""
from pathlib import Path
import argparse,hashlib,json,zipfile
p=argparse.ArgumentParser();p.add_argument('case',choices=['assembly','native-windows']);a=p.parse_args()
f=Path(__file__).resolve().parent/a.case
owner=json.loads((f/'OWNER.json').read_text());assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed']
archive=f/'RESULTS.zip';out=f/'results';out.mkdir(exist_ok=False);members=[]
if (f/'ARCHIVE_TRANSFER.json').exists():
 transfer=json.loads((f/'ARCHIVE_TRANSFER.json').read_text());assert archive.stat().st_size==transfer['bytes']
 assert hashlib.sha256(archive.read_bytes()).hexdigest()==transfer['sha256']
with zipfile.ZipFile(archive) as z:
 for entry in z.infolist():
  assert len(Path(entry.filename).parts)==1 and not entry.is_dir() and entry.file_size<15_000_000
  data=z.read(entry);private=entry.filename in {'OWNER.json','DESKTOP_READY.json','DESKTOP_WAITING.json'}
  assert private or entry.filename.endswith(('.json','.png','.stdout','.stderr'))
  if not private:(out/entry.filename).write_bytes(data)
  members.append({'name':entry.filename,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'private_bootstrap_not_extracted':private})
(f/'TRANSPORT_CONTENT.json').write_text(json.dumps({'members':members,'private_bootstrap_excluded':True},indent=2)+'\n')
print('Exported safe terminal results; inspect actual outcome and original captures before acceptance')
