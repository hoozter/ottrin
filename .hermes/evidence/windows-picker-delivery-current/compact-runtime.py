"""Retire completed fixture exports/console captures, retaining exact native evidence."""
from pathlib import Path
import argparse,hashlib,json,zipfile
parser=argparse.ArgumentParser();parser.add_argument('--case',type=Path,required=True);e=parser.parse_args().case.resolve();root=Path(__file__).resolve().parent
assert e.parent==root and e.name in {'upgrade-enabled','upgrade-disabled'}
assert not(e/'CLEANUP.json').exists(),'Completed compaction already recorded'
owner=json.loads((e/'OWNER.json').read_text());viewer=json.loads((e/'viewer/TEARDOWN.json').read_text());accepted=json.loads((e/'ACCEPTANCE.json').read_text())
assert accepted['status'].startswith('pass within actual native')
assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed'] and viewer['profile_removed'] and not Path(viewer['profile']).exists()
zp=e/'RESULTS.zip';members=[];out=e/'results';out.mkdir()
with zipfile.ZipFile(zp) as z:
 for info in z.infolist():
  name=Path(info.filename);assert not name.is_absolute() and len(name.parts)==1 and not info.is_dir()
  data=z.read(info);(out/name).write_bytes(data);members.append({'path':info.filename,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
removed=[]
for p in [zp,e/'RESULT_EXPORT_PROCESS.json',*e.glob('*.png'),*e.joinpath('viewer').glob('*.log')]:
 if p.exists():
  removed.append({'path':str(p.relative_to(e)),'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()});p.unlink()
report={'status':'pass: native update/login/removal complete; owned guest/two volumes/private console removed; redundant export and consumed whole-console frames compacted','guest':owner['cleanup'],'viewer':viewer,'archive_members':members,'removed_completed_outputs':removed,'retained':'Exact native member bytes and relevant application screenshots under results/; shared packages/base await remaining current native package consumers'}
(e/'CLEANUP.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'members':len(members),'removed_bytes':sum(x['bytes'] for x in removed)}))
