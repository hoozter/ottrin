"""Read only the owned fixture through QGA, hash exact bytes, discard downloads."""
from pathlib import Path
import json,hashlib,tempfile,sys
p=Path(__file__).resolve().parent;g=p.parent/'windows-copy-undo-gui-current'
sys.path.insert(0,str(p));import native_compiler as n
state=json.loads((g/'SESSION.json').read_text());assert state['status']=='waiting for owned input'
assert json.loads((g/'desktop.json').read_text(encoding='utf-8-sig'))['op']=='verify'
close=[json.loads(f.read_text(encoding='utf-8-sig')) for f in g.glob('*-close.json')]
assert len(close)==1 and close[0]['verification']['normal_app_close'][0]['exit']==0
n.GROOT=r'C:\OttrinWindowQA'
files={'source\\note.txt':'source original\n','source\\other.txt':'other original\n','source\\bundle\\nested.txt':'nested directory bytes\n','source\\contact.vcf':'BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ottrin QA\r\nEND:VCARD\r\n','renamed-destination\\note.txt':'destination original\n','destination\\note.txt':'unrelated old-path occupant\n','batch-destination\\note.txt':'unrelated batch old-path occupant\n'}
results={}
with tempfile.TemporaryDirectory(prefix='.read-only-bytes-',dir=g) as directory:
 for i,(name,expected) in enumerate(files.items()):
  local=Path(directory)/str(i);n.download('cases/ordinary/fixture/'+name,local);data=local.read_bytes();assert data==expected.encode('utf-8'),name
  results[name]={'sha256':hashlib.sha256(data).hexdigest(),'bytes':len(data)}
 config=Path(directory)/'config';n.download('cases/ordinary/roaming/ottrin/config.json',config);settings=json.loads(config.read_text(encoding='utf-8-sig'))
 assert settings['theme']=='Dark' and settings['default_view_mode']=='List'
(g/'FINAL_BYTES.json').write_text(json.dumps({'status':'pass','method':'QGA read-only download and exact raw byte equality against independently declared UTF-8/CRLF fixtures; temporary host downloads removed','files':results,'saved_preferences':{'theme':settings['theme'],'default_view_mode':settings['default_view_mode']},'normal_close':close[0]['verification'],'scope':'After single and batch Undo following parent renames,old-path replacements preserved; native hash evidence does not broaden UI or installer scope'},indent=2)+'\n')
print('Exact raw bytes and saved appearance verified; host downloads removed')
