import sys,base64,json,pathlib
sys.path.insert(0,str(pathlib.Path('.hermes/evidence/windows-20261011').resolve()))
from guest_exec import qga
out=pathlib.Path('.hermes/evidence/windows-office-loop-current')
def pull(path,target):
 h=qga('guest-file-open',{'path':path,'mode':'rb'})
 try:
  with target.open('wb') as f:
   while True:
    r=qga('guest-file-read',{'handle':h,'count':65536});f.write(base64.b64decode(r.get('buf-b64','')))
    if r.get('eof'):break
 finally:qga('guest-file-close',{'handle':h})
for folder,names in [('office-loop-current',['control.err','control.out','pump.err','pump.out','results.json']),('office-loop-current2',['document.docx.err','document.docx.out','sheet.xls.err','sheet.xls.out','results.json']),('office-loop-current3',['sheet.xls.err','sheet.xls.out','slides.odp.err','slides.odp.out','results.json'])]:
 d=out/folder;d.mkdir(exist_ok=True)
 for name in names:pull('C:\\OttrinQA-20261011\\'+folder+'\\'+name,d/name)
pull('C:\\OttrinQA-20261011\\office-loop-current2\\document.docx\\output\\kit-result.pdf',pathlib.Path('/tmp/ottrin-office-writer-control.pdf'))
print('Collected all three completed native probe receipts and Writer PDF')
