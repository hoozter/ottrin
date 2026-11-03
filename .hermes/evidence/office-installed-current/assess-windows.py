"""Accept exact native conversion/fallback scopes without erasing visual defects."""
from pathlib import Path
import hashlib,json,ntpath
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
current=read(r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json');payload=read(r/'.hermes/evidence/assistive-delivery-current/PAYLOAD.json');reports={}
for name in ['windows-observed','windows-worksheet']:
 case=e/name;a=read(case/'GUEST_RESULT.json');owner=read(case/'OWNER.json');input=read(case/'APPLICATION_INPUT.json');review=read(case/'VISUAL_REVIEW.json')
 assert a==json.loads((case/'results/RESULT.json').read_bytes().decode('utf-8-sig'))
 assert a['status']=='pass within installed Office render/fallback scope; direct capture review remains required'
 assert a['package']==input and input['source_revision']==current['source_commit'] and a['payload_files']==58
 for item in input['files']:assert item['name']=='current-setup.exe' and item['sha256']==current['artifacts']['Ottrin-0.1.0-windows-x64-setup.exe']
 for key in ['desktop_ready','owned_processes_removed','office_temp_removed','source_preserved','preferences_preserved_by_removal']:assert a[key] is True,key
 assert a['uninstall']=='pass' and not a['identity']['administrator'] and a['identity']['session']>0
 assert a['provider']['mergedlo_sha256']=='017D0C21B6E45E54C0194B58017853D8C653059CC643643C3EB831A4D309331F' and a['provider']['vcl_windows_sha256']=='BBA69CA1047FB46866F8982E90C995B559E2C95FA36258CE8ABFE339BFD53715'
 for renderer in ['native','opengl']:
  actual=a['cases'][renderer]
  for surface in (['main','inspector'] if name=='windows-worksheet' else ['main','inspector','presentation']):
   view=actual[surface];assert view['conversions']
   for child in view['conversions']:
    assert child['pid']>0 and child['parent']>0 and ntpath.basename(child['executable']).casefold()=='office-render.exe'
    assert '--version' not in child['arguments'] and 'ottrin-office-preview-' in child['executable'] and 'LibreOffice' in child['arguments']
   if name=='windows-worksheet':assert view['blue']>30 and view['green']>30
   else:assert view['orange']>100 and view['cyan']>100
   dlls={v['name'].casefold():v for v in view['modules']};assert dlls['opengl32.dll']['sha256']==payload['bin/opengl32.dll'] and dlls['libgallium_wgl.dll']['sha256']==payload['bin/libgallium_wgl.dll'];assert 'd3d12.dll' in dlls
  if name=='windows-observed':assert actual['inspector_fallback']['title'].endswith('02-office.xlsx') and actual['main_fallback']['injection']
 for filename,meta in read(case/'RESULT_MEMBERS.json').items():
  raw=(case/'results'/filename).read_bytes();assert len(raw)==meta['bytes'] and hashlib.sha256(raw).hexdigest()==meta['sha256']
 assert review['status'].startswith('reviewed')
 for filename,digest in review['captures'].items():assert sha(case/'results'/filename)==digest
 cleanup=owner['cleanup'];assert cleanup['domain_removed'] and cleanup['fixture_removed'] and cleanup['base_still_off'] and len(cleanup['owned_volumes_removed'])==2
 viewer=read(case/'viewer/TEARDOWN.json');assert viewer['profile_removed'] and not Path(viewer['profile']).exists()
 reports[name]={'native_result_sha256':sha(case/'results/RESULT.json'),'inputs':input,'provider':a['provider'],'native_identity':a['identity'],'review':'VISUAL_REVIEW.json','cleanup':cleanup}
for name,digest in current['artifacts'].items():assert sha(r/'dist/windows-candidate'/name)==digest
receipt={'status':'pass within current installed Office conversion/fallback scope; shared fallback typography correction remains required','source_revision':current['source_commit'],'current_packages':current['artifacts'],'native_cases':reports,'accepted':['Exact ordinary-user current NSIS installation,58 matching payload files and normal uninstall preserving preferences','Representative DOCX main/Inspector, XLSX main/Inspector and PPTX Inspector rendered through actual installed LibreOffice conversion children','Saved Native preference and explicit OpenGL override, software VM/default DPI; captured module hashes retain actual provenance','Helper-withheld XLSX fallback and visible explanation in main/Inspector','Unchanged authored source bytes, no remaining Office temp staging or installed processes, owned clones/media/consoles removed'],'reviewed_native_captures':14,'open_visual_issue':{'path':'crates/ottrin-ui/src/preview_render.rs','function':'render_spreadsheet_row','evidence':'windows-observed/results/native-main-fallback.png','finding':'Ui::put forces centered-and-justified layout; wrapped native worksheet cells visibly stretch letters in a narrow Miller preview. Correct shared cell layout before claiming full preview/UI acceptance.'},'limits':['No arbitrary Office fidelity, every format or full roadmap acceptance','Native preference maps to WGPU in exact application source, while explicit override selects OpenGL; both DLL families load, so module presence alone is not exclusive API/device proof','Physical GPUs, extended DPI and final owner acceptance remain separate','The known narrow fallback-cell typography defect is not waived by successful conversion/data-preservation checks']}
(e/'WINDOWS_OFFICE_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n');print(receipt['status'])
