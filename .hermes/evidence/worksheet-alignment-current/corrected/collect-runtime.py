"""Extract exact raw native receipts once, retaining member hashes for cleanup."""
from pathlib import Path,PurePosixPath
import argparse,hashlib,json,zipfile
parser=argparse.ArgumentParser();parser.add_argument('--case',type=Path,required=True);args=parser.parse_args();e=args.case.resolve();assert e.parent==Path(__file__).resolve().parent
archive=e/'RESULTS.zip';out=e/'results';assert archive.is_file() and not out.exists()
with zipfile.ZipFile(archive) as z:
 assert all(not PurePosixPath(n).is_absolute() and '..' not in PurePosixPath(n).parts for n in z.namelist());members={n:hashlib.sha256(z.read(n)).hexdigest() for n in z.namelist() if not n.endswith('/')};z.extractall(out)
assert json.loads((out/'RESULT.json').read_bytes().decode('utf-8-sig'))==json.loads((e/'RUNTIME_RESULT.json').read_text())
(e/'RESULT_MEMBERS.json').write_text(json.dumps({'archive_sha256':hashlib.sha256(archive.read_bytes()).hexdigest(),'members':members,'raw_result_equals_driver_result':True},indent=2)+'\n')
