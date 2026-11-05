"""Accept exact NSIS native density/caption/control scope after direct review."""
from pathlib import Path
import hashlib,json,subprocess
E=Path(__file__).resolve().parent;F=E/'native-windows'
read=lambda p:json.loads(p.read_text())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
r=read(F/'GUEST_RESULT.json');o=read(F/'OWNER.json');i=read(F/'APPLICATION_INPUT.json');b=read(E/'windows/BUILD_RESULT.json')
assert r['status']=='pass native row/thumbnail/footer/status appearance and uninstall scope'
assert r['package']==i and i['source_revision']==b['source_revision']
assert i['files']==[{'name':'current-setup.exe','sha256':b['artifacts']['Ottrin-0.1.0-windows-x64-setup.exe']}]
assert sha(F/'actor.ps1')==i['actor_sha256']==sha(E/'native-windows.ps1')
assert len(read(E/'PAYLOAD.json'))==58
assert r['preferences_preserved'] and r['sources_preserved']
assert len(r['cases'])==12
assert {(c['theme'],c['view'],c['view_scale']) for c in r['cases']}=={(t,v,s) for t in ['Dark','Light'] for v in ['Miller','List'] for s in [.75,1,1.85]}
assert all(c['actual_view_toggle_verified'] and len(c['captions'])==2 for c in r['cases'])
expected={'Miller':[23,25,34],'List':[26,28,36],'Grid':[81,98,150]}
assert len(r['density_proof'])==4
for proof in r['density_proof']:
 assert proof['scales']==[.75,1,1.85] and proof['heights']==expected[proof['view']]
 cases=sorted([c for c in r['cases'] if c['theme']==proof['theme'] and c['view']==proof['view']],key=lambda c:c['view_scale'])
 assert [c['captions'][0]['height'] for c in cases]==proof['heights']
assert len(r['no_input_footer_samples'])==3 and all(c['label']=='4 items' for c in r['no_input_footer_samples'])
assert r['no_input_footer_samples'][-1]['seconds']>=10
service=r['installed_service_scope'];assert service['startup']=='On' and service['ready'] and service['normal_close_keeps_pid']>0
assert len(r['thumbnail_cases'])==len(r['services'])==2
assert o['cleanup']['domain_removed'] and o['cleanup']['fixture_removed'] and len(o['cleanup']['owned_volumes_removed'])==2
assert not Path(o['private_console']['home']).parent.exists()
identity=subprocess.run(['virsh','-c','qemu:///system','domuuid',o['domain']],capture_output=True,text=True);assert identity.returncode or identity.stdout.strip()!=o['uuid']
names=[c['capture'] for c in r['cases']+r['thumbnail_cases']+r['services']]
assert len(names)==16 and set(names)=={p.name for p in (F/'results').glob('*.png')}
captures={name:sha(F/'results'/name) for name in names}
review=read(F/'CAPTURE_REVIEW.json')
assert review['reviewed'] and review['captures']==captures and len(captures)==16
assert not (F/'RESULTS.zip').exists(), 'Retire verified consumed transport after direct review'
receipt={'status':'pass exact native NSIS row/thumbnail/footer/status/default-view/uninstall scope; full UI audit incomplete','source_revision':b['source_revision'],'artifacts':b['artifacts'],'native_input':i,'density_proof':r['density_proof'],'installed_service_scope':service,'no_input_footer_samples':r['no_input_footer_samples'],'preferences_preserved':True,'sources_preserved':True,'capture_review':review,'cleanup':o['cleanup'],'limits':['Software guest/default DPI only; no full theme/property/role, physical graphics, MSIX lifecycle or administrator-Copy acceptance','Current installed service Ready and normal close verified; earlier actual On/Off/On proof retains original source scope']}
(E/'NATIVE_WINDOWS_UI_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('Accepted bounded exact native NSIS scope;16 originals reviewed; full UI audit remains open')
