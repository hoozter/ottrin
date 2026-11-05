"""Accept exact NSIS native density/caption/control scope after direct review."""
from pathlib import Path
import hashlib,json,subprocess
E=Path(__file__).resolve().parent;F=E/'native-captions'
read=lambda p:json.loads(p.read_text())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
r=read(F/'GUEST_RESULT.json');o=read(F/'OWNER.json');i=read(F/'APPLICATION_INPUT.json');b=read(E/'windows/BUILD_RESULT.json')
assert r['status']=='pass native caption/default-view, service startup and uninstall scope'
assert r['package']==i and i['source_revision']==b['source_revision']
assert i['files']==[{'name':'current-setup.exe','sha256':b['artifacts']['Ottrin-0.1.0-windows-x64-setup.exe']}]
assert sha(F/'actor.ps1')==i['actor_sha256']==sha(E/'native-captions.ps1')
assert len(read(E/'PAYLOAD.json'))==58
assert r['preferences_preserved'] and r['sources_preserved']
assert len(r['cases'])==18
assert {(c['theme'],c['view'],c['view_scale']) for c in r['cases']}=={(t,v,s) for t in ['Dark','Light'] for v in ['Miller','List','Grid'] for s in [.75,1,1.85]}
assert all(c['actual_view_toggle_verified'] and len(c['captions'])==2 for c in r['cases'])
expected={'Miller':[23,25,34],'List':[26,28,36],'Grid':[81,98,150]}
assert len(r['density_proof'])==6
for proof in r['density_proof']:
 assert proof['scales']==[.75,1,1.85] and proof['heights']==expected[proof['view']]
 cases=sorted([c for c in r['cases'] if c['theme']==proof['theme'] and c['view']==proof['view']],key=lambda c:c['view_scale'])
 assert [c['captions'][0]['height'] for c in cases]==proof['heights']
assert len(r['no_input_footer_samples'])==3 and all(c['label']=='3 items' for c in r['no_input_footer_samples'])
assert r['no_input_footer_samples'][-1]['seconds']>=10
service=r['service_round_trip'];assert service['registration']=='On -> Off -> On' and service['os_task_inspected'] and service['running_pid_preserved']>0 and service['normal_close_keeps_daemon']
assert o['cleanup']['domain_removed'] and o['cleanup']['fixture_removed'] and len(o['cleanup']['owned_volumes_removed'])==2
assert not Path(o['private_console']['home']).parent.exists()
identity=subprocess.run(['virsh','-c','qemu:///system','domuuid',o['domain']],capture_output=True,text=True);assert identity.returncode or identity.stdout.strip()!=o['uuid']
names=[c['capture'] for c in r['cases']]+['services-on.png','services-off.png']
assert len(names)==20 and set(names)=={p.name for p in (F/'results').glob('*.png')}
captures={name:sha(F/'results'/name) for name in names}
review={'reviewed':True,'status':'all20 original native Windows PNG captures directly reviewed','captures':captures,'findings':['All six List cases contain wide filenames within Name and clear of Type/Size/Modified','Column filenames stay inside their columns; Grid labels remain within actual tiles','Own-density size changes are visible in both themes at75/100/185 percent','Settings shows installed-service actions and Ready state independently of checked On/unchecked Off startup','High-density Column/List icon-to-text padding is visibly too tight; remains full UI audit work','Light footer/logo contrast is weak; remains full theme audit work'],'limits':['No full theme/property/role, physical graphics, MSIX, index upgrade or administrator-Copy acceptance']}
(F/'CAPTURE_REVIEW.json').write_text(json.dumps(review,indent=2)+'\n')
receipt={'status':'pass exact native NSIS filename/density/default-view/startup/uninstall scope; full UI audit incomplete','source_revision':b['source_revision'],'artifacts':b['artifacts'],'native_input':i,'density_proof':r['density_proof'],'service_round_trip':service,'no_input_footer_samples':r['no_input_footer_samples'],'preferences_preserved':True,'sources_preserved':True,'capture_review':review,'cleanup':o['cleanup'],'limits':review['limits']+review['findings'][-2:]}
(E/'NATIVE_WINDOWS_CAPTION_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('Accepted bounded exact native NSIS scope; twenty originals reviewed; full UI audit remains open')
