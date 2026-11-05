"""Bind the compiled test to current source, then enter the teardown-owning driver."""
from pathlib import Path
import hashlib,json,os,re,sys
r=Path.cwd();e=Path(__file__).resolve().parent
name=sys.argv[1];assert re.fullmatch(r'native-unit-[a-z-]+',name)
state=json.loads((e/'VERIFY_STATE.json').read_text());assert state['status'].startswith('pass')
for path,digest in state['source_files'].items():
 assert hashlib.sha256((r/path).read_bytes()).hexdigest()==digest,path
artifacts=[]
for line in (e/'native-test-build.jsonl').read_text().splitlines():
 try:value=json.loads(line)
 except json.JSONDecodeError:continue
 if value.get('reason')=='compiler-artifact' and value.get('executable'):artifacts.append(value)
assert len(artifacts)==1
binary=Path(artifacts[0]['executable']);assert binary.parent==r/'target/x86_64-pc-windows-gnu/debug/deps' and binary.is_file()
(e/'NATIVE_TEST_INPUT.json').write_text(json.dumps({'executable':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'source_verification':'VERIFY_STATE.json','scope':'Direct native COM/Unicode path and manifest reference/ID regression; no actual package activation or Explorer surface acceptance'},indent=2)+'\n')
driver=r/'.hermes/evidence/theme-delivery-current/native-driver.py'
os.execv(sys.executable,[sys.executable,str(driver),'--native-test',str(binary),'--test-filter','server::tests::packaged_shell_com_contract','--use-stock-base','--output',str(e/name)])
