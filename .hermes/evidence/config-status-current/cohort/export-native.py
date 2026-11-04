"""Export only reviewed result members; keep private bootstrap out of public evidence."""
from pathlib import Path
import argparse,hashlib,json,zipfile
parser=argparse.ArgumentParser();parser.add_argument('case',choices=['native','services','registration-trace']);args=parser.parse_args();e=Path(__file__).resolve().parent/args.case
assert json.loads((e/'OWNER.json').read_text())['cleanup']['domain_removed']
out=e/'results';out.mkdir(exist_ok=False);members=[]
with zipfile.ZipFile(e/'RESULTS.zip') as z:
 for i in z.infolist():
  assert len(Path(i.filename).parts)==1 and not i.is_dir() and i.file_size<15_000_000
  data=z.read(i);private=i.filename in {'OWNER.json','DESKTOP_READY.json'}
  assert private or i.filename.endswith(('.json','.png'))
  if not private:(out/i.filename).write_bytes(data)
  members.append({'name':i.filename,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'private_bootstrap_not_extracted':private})
(e/'TRANSPORT_CONTENT.json').write_text(json.dumps({'members':members,'private_bootstrap_excluded':True},indent=2)+'\n')
