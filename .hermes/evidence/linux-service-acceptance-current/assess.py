"""Bind real service observations to published packages and completed teardown."""
from pathlib import Path
import ast,hashlib,json,subprocess
r=Path.cwd();root=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
manifest=read(root/'PACKAGE_INPUT.json');current=read(r/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json')
assert manifest['source_revision']==current['source_revision'] and manifest['artifacts']==current['artifacts']
controller=root/'CONTROLLER_SOURCE.py' # Historical runs bind the frozen producer below, not a later edited controller.
for p in [controller,root/'service-actor.py',Path(__file__)]:ast.parse(p.read_text())
results={}
for distro,suffix in [('ubuntu','.deb'),('fedora','.rpm')]:
 out=root/distro;owner=read(out/'VM_OWNER.json');vm=read(out/'VM_RESULT.json');cleanup=read(out/'VM_CLEANUP.json');actor=read(out/'results/RESULT.json')
 package=next(name for name in manifest['artifacts'] if name.endswith(suffix))
 assert sha(r/'dist/packages'/package)==manifest['artifacts'][package]==vm['package_sha256']
 assert vm['source']==actor['source']==manifest['source_revision']
 assert vm['status']=='pass within actor scope' and actor['status']=='pass'
 assert owner['actor_sha256']==sha(out/'ACTOR_SOURCE.py')==sha(root/'service-actor.py')
 assert owner['controller_sha256']==sha(controller)
 assert actor['source_and_preferences_preserved'] and actor['user_manager_stopped']
 assert len(actor['states'])==11
 states={s['label']:s for s in actor['states']}
 assert states['installed']['pid']==states['disabled while running']['pid']==states['enabled while running']['pid']>0
 assert states['repair disabled stopped service']['pid']==0
 assert states['repair disabled running service']['enabled']=='disabled'
 assert states['repair disabled stopped service']['enabled']=='disabled'
 assert states['first ensure after delayed start']['pid']==states['client exited; daemon remains']['pid']==states['daemon retained after client exit']['pid']>0
 assert 7<=actor['delayed_readiness_seconds']<90
 assert states['removed']['pid']==0 and not states['removed']['registered']
 assert cleanup['domain_removed'] and cleanup['volumes_removed']==owner['volumes']
 assert vm['temporary_seed_and_ssh_keys_removed'] and not list(out.glob('.native-vm-*'))
 results[distro]={'package':package,'package_sha256':vm['package_sha256'],'states':11,'delayed_readiness_seconds':actor['delayed_readiness_seconds'],'domain_removed':True,'owned_volumes_removed':cleanup['volumes_removed']}
domains=subprocess.check_output(['virsh','-c','qemu:///system','list','--all','--name'],text=True).split()
assert not any(name.startswith('ottrin-native-') for name in domains)
report={'status':'pass scoped native Linux CLI service acceptance','runtime_source':manifest['source_revision'],'results':results,'limits':['No graphical setup, authentication prompt/cancellation or checkbox acceptance','No user-login/reboot or actual binary-version upgrade acceptance','CLI client exit retention is not a GUI close test'],'cleanup':{'owned_domains_remaining':False,'temporary_credentials_and_seeds_removed':True,'result_transport_archives_removed':not list(root.rglob('VM_RESULTS.tar.gz')),'retained_evidence_root':str(root),'retained_purpose':'Compact frozen producers, package binding, original service states, provisioning logs and failed-fixture diagnosis; no VM disk or package copies','retained_bytes_before_receipt_refresh':sum(p.stat().st_size for p in root.rglob('*') if p.is_file())}}
assert report['cleanup']['result_transport_archives_removed']
(root/'ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n')
(root/'PROGRESS.json').write_text(json.dumps({'status':'progress; two published native package service matrices completed','acceptance':'ACCEPTANCE.json','next':'Graphical polkit prompt/cancel/setup/reinstall and native startup checkbox; real upgrade/login readiness; remaining roadmap queue unchanged'},indent=2)+'\n')
print(json.dumps(report,indent=2))
