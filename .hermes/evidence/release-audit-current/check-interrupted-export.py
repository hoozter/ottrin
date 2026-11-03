"""Exercise the actual owner's failure callback without allocating a guest."""
from pathlib import Path
from types import SimpleNamespace
import ast,base64,io,json,tempfile,unittest,zipfile
source=Path('.hermes/evidence/assistive-delivery-current/windows/cohort-client.py');tree=ast.parse(source.read_text());node=next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name=='finish');code=compile(ast.Module(body=[node],type_ignores=[]),str(source),'exec')
class ExportTests(unittest.TestCase):
 def exercise(self,error,root=r'C:\WINDOWS\TEMP\ottrin-msi-'+('a'*32)):
  with tempfile.TemporaryDirectory(prefix='ottrin-export-callback-') as tmp:
   e=Path(tmp);(e/'GUEST_PREPARATION.json').write_text(json.dumps({'root':root}));events=[];state={};data=io.BytesIO()
   with zipfile.ZipFile(data,'w') as z:z.writestr('results/RESULT.json',json.dumps({'status':'running'}))
   raw=data.getvalue()
   def ps(script,timeout):
    events.append(('export',timeout));return {'exitcode':0,'out-data':base64.b64encode(base64.b64encode(raw)).decode()}
   def virsh(*args):return 'owned-case' if args[0]=='list' else 'exact-uuid'
   values=dict(e=e,state=state,application_case=True,json=json,re=__import__('re'),base64=base64,ps=ps,virsh=virsh,name='owned-case',uid='exact-uuid',args=SimpleNamespace(prepare_updated_base=False),save=lambda name,value:(e/name).write_text(json.dumps(value)),cleanup=lambda:events.append(('cleanup',None)))
   exec(code,values);self.assertFalse(values['finish'](type(error),error,None));capture=e/'INTERRUPTED_RESULTS.zip'
   if error is not None and root.startswith('C:'):
    self.assertEqual(events,[('export',25),('cleanup',None)]);self.assertEqual(capture.read_bytes(),raw);self.assertEqual(state['status'],'failed or interrupted; see retained scope and error')
   else:
    self.assertEqual(events,[('cleanup',None)]);self.assertFalse(capture.exists())
   return dict(state)
 def test_partial_output_survives_before_cleanup(self):self.exercise(SystemExit(143))
 def test_success_does_not_export_partial_state(self):self.exercise(None)
 def test_foreign_root_refused_and_cleanup_still_runs(self):
  result=self.exercise(RuntimeError('fixture failure'),r'D:\user-data');self.assertIn('interrupted_export_error',result)
if __name__=='__main__':unittest.main()
