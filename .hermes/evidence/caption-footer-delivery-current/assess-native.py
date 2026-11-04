"""Assess original captures, exact GUI and registered VM teardown; never infer offscreen visual acceptance."""
from pathlib import Path
import hashlib,json,subprocess
E=Path(__file__).resolve().parent;N=E/'native/ubuntu';O=N/'results'
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
w=read(O/'WORKER_RESULT.json');b=read(E/'BUILD_RESULT.json');c=read(E/'PACKAGE_CONTENT_ACCEPTANCE.json');v=read(N/'VM_RESULT.json');owner=read(N/'VM_OWNER.json');clean=read(N/'VM_CLEANUP.json');actor=read(O/'RESULT.json')
assert w['status']==actor['status']=='pass' and v['status']=='pass within actor scope'
assert b['source_revision']==c['source_revision']==v['source']==actor['package_source']
assert owner['actor_sha256']==sha(E/'native/actor.py')
assert w['pre_gui_warm_index']['status']=='Ready' and w['pre_gui_warm_index']['indexed_items']==1002
assert len(w['startup_samples'])==4 and all(x['item_labels']==['label | 1002 items'] for x in w['startup_samples']) and w['startup_samples'][-1]['seconds_after_window']>=30
for x in w['miller_cases']+w['grid_cases']+w['picker_cases']:assert x['gui_sha256']==c['runtime_sha256']['application']
assert len(w['miller_cases'])==4 and len(w['grid_cases'])==4 and len(w['list_cases'])==2 and all(x['cancelled'] for x in w['picker_cases'])
assert len([x for x in w['miller_cases'] if x.get('backend_scale')==1.5 and x['physical_width']==1200])==2
assert clean['domain_removed'] and len(clean['volumes_removed'])==3 and not list(N.glob('.native-vm-*'))
assert subprocess.run(['virsh','-c','qemu:///system','domuuid',owner['domain']],capture_output=True).returncode!=0
for vol in clean['volumes_removed']:assert subprocess.run(['virsh','-c','qemu:///system','vol-path',vol,'--pool','default'],capture_output=True).returncode!=0
captures={p.name:sha(p) for p in O.glob('*.png')};assert len(captures)==18
notes=['All18 original complete-window captures directly viewed. Miller wide filename fills its available line before ellipsis in Dark/Light100% and controlled150% backend scaling.','No-input installed/enabled Ready-index startup footer shows1002 items at all four0/2/10/30s samples; captures agree with accessibility text.','Dark/Light List headers and complete filename, Grid75/100/185 and cooperating picker layout retain their stated scope; picker Escape returns accepted=false with no paths.','Appearance captures show built-in theme cards, but the long saved-theme card is below the fold. They do not accept that change; a scrolled native supplement is required.']
review=read(E/'CAPTURE_REVIEW.json');assert review['scope']=='Original complete-window captures directly reviewed' and review['captures']==captures
archive=N/'VM_RESULTS.tar.gz';transport={'bytes':archive.stat().st_size,'sha256':sha(archive)};archive.unlink()
report={'status':'pass scoped main AppImage caption/footer/Grid/picker checks; saved-theme card supplement separate','source_revision':b['source_revision'],'artifacts':b['artifacts'],'gui_sha256':c['runtime_sha256']['application'],'original_captures':captures,'notes':notes,'warm_index':w['pre_gui_warm_index'],'no_input_footer_samples':w['startup_samples'],'miller_cases':w['miller_cases'],'cleanup':clean,'consumed_transport_removed':transport,'limits':['Original Appearance captures do not show saved-theme card; not accepted by this receipt.','Controlled software X11, no physical/Wayland/Windows/full theme audit.','Earliest native count sample is0.69s after visible window, not every intervening frame; rendered regression supplies missing-wakeup/return-to-idle proof.','No new daemon CPU, helper/UAC, reboot or binary-replacement acceptance; inherited unchanged-source/hash scopes remain explicit.']}
(E/'NATIVE_CAPTION_ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n');print(report['status'])
