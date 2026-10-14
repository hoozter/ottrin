"""Stage one test artifact in the owned disposable guest directory."""
import base64, hashlib, pathlib, sys
from guest_exec import qga
source=pathlib.Path(sys.argv[1]); name=sys.argv[2]
assert name and all(c.isalnum() or c in '-_.' for c in name)
handle=qga('guest-file-open',{'path':'C:\\OttrinQA-20261011\\'+name,'mode':'wb'})
count=0
try:
    with source.open('rb') as stream:
        while chunk:=stream.read(48*1024):
            result=qga('guest-file-write',{'handle':handle,'buf-b64':base64.b64encode(chunk).decode()})
            assert result['count']==len(chunk),result
            count+=len(chunk)
    qga('guest-file-flush',{'handle':handle})
finally:
    qga('guest-file-close',{'handle':handle})
print(name,count,hashlib.sha256(source.read_bytes()).hexdigest(),flush=True)
