"""Run the full search suite in an owned temporary namespace and clean on exit."""
import json,os,pathlib,re,signal,subprocess,tempfile,time
base=pathlib.Path(__file__).resolve().parent
process=None
receipt={'status':'running','scope':'Full Linux search-library suite, serial, with private TMPDIR'}
def cancel(signum,frame):raise KeyboardInterrupt
for sig in [signal.SIGINT,signal.SIGTERM]:signal.signal(sig,cancel)
def stop_owned():
 if process is None:return
 try:os.killpg(process.pid,signal.SIGTERM)
 except ProcessLookupError:pass
 if process.poll() is None:
  try:process.wait(timeout=10)
  except subprocess.TimeoutExpired:os.killpg(process.pid,signal.SIGKILL);process.wait()
try:
 with tempfile.TemporaryDirectory(prefix='os-') as name:
  fixture=pathlib.Path(name);env=os.environ.copy();env['TMPDIR']=name
  try:
   with (base/'LINUX_FINAL.stdout').open('w') as out:
    process=subprocess.Popen(['cargo','test','-p','ottrin-search','--lib','--','--test-threads=1'],env=env,stdout=out,stderr=subprocess.STDOUT,start_new_session=True)
    receipt['owned_process_group']=process.pid
    receipt['exit']=process.wait(timeout=180)
  finally:stop_owned()
  text=(base/'LINUX_FINAL.stdout').read_text()
  assert receipt['exit']==0,text[-1500:]
  match=re.search(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',text);assert match and int(match[1])>0
  receipt.update(status='pass',passed=int(match[1]),failed=int(match[2]),ignored=int(match[3]))
except BaseException as error:
 receipt.update(status='fail',error=repr(error));raise
finally:
 receipt['cleanup']='Owned process group stopped before private TMPDIR removal'
 (base/'LINUX_FINAL.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt,indent=2))
