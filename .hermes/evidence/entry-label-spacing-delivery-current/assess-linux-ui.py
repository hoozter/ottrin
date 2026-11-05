"""Bind directly reviewed original frames to this exact packaged UI and retire transport."""
from pathlib import Path
import hashlib,json,subprocess
E=Path(__file__).resolve().parent;N=E/'native/ubuntu';R=N/'results'
def read(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def save(p,v):p.write_text(json.dumps(v,indent=2)+'\n')
content=read(E/'PACKAGE_CONTENT_ACCEPTANCE.json');source=content['source_revision'];result=read(R/'RESULT.json');w=result['worker'];vm=read(N/'VM_RESULT.json');cleanup=read(N/'VM_CLEANUP.json');transport=read(N/'TRANSPORT_CONTENT.json')
assert result['status']==w['status']=='pass' and result['package_source']==vm['source']==source
assert vm['status']=='pass within actor scope' and vm['temporary_seed_and_ssh_keys_removed'] and cleanup['domain_removed'] and len(cleanup['volumes_removed'])==3
assert vm['package_sha256']==content['artifacts']['ottrin_0.1.0_amd64.deb']
assert result['appimage_input']['sha256']==content['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
assert result['appimage_input']['daemon_sha256']==content['runtime_sha256']['index_daemon']
assert result['sources_preserved'] and len(result['original_fixture_hashes'])==4
assert w['database_identity_preserved'] and w['daemon_ready_after_normal_close'] and w['initial_index']['indexed_items']==5
assert len(w['footer_samples'])==3 and all(s['visible_items']==4 and s['index_total']==5 for s in w['footer_samples'])
assert len(w['cases'])==14 and len(w['services'])==2
frames={}
for case in w['cases']+w['services']:
 assert case['gui_sha256']==content['runtime_sha256']['application']
 name=case['capture'];assert name not in frames;frames[name]=sha(R/name)
assert len(frames)==16 and set(frames)=={p.name for p in R.glob('*.png')}
for m in transport['members']:assert sha(R/m['name'])==m['sha256']
review={'status':'pass within directly reviewed scenes','source_revision':source,'original_frames':frames,'review':['All16 original unmodified1920x1200 frames directly opened using view_image.','Dark/Light Miller and List .75/1.85 density at backend1.0, and1.85 at backend1.5: visible filename/icon gaps, directory-chevron clearance, List Name/Type separation and aligned filename starts.','Dark/Light thumbnail scenes show actual blue PNG plus text thumbnail surfaces; all four filename starts align. Text thumbnails are pale in Light, but are actual miniature documents; no blanket thumbnail-contrast acceptance.','Both Services frames show readable green helper and index Ready status, enabled startup and Update/reinstall rather than Install service.','All main scenes show readable4items footer and wordmark; no first-frame0-items flash in native accessibility samples.'],'limits':['Software X11 on owned Ubuntu guest; no physical/Wayland or full interface acceptance.','Partially visible ancestor columns at horizontal scroll edge are intentional clipping; no acceptance of unrelated navigation behavior.','No new startup-toggle/reboot, protected Copy, personal profile or sustained CPU claim.']}
save(E/'LINUX_CAPTURE_REVIEW.json',review)
archive=N/'VM_RESULTS.tar.gz'
if archive.exists():
 assert archive.is_file() and not archive.is_symlink() and archive.stat().st_nlink==1
 assert sha(archive)==transport['archive_sha256'] and archive.stat().st_size==transport['archive_bytes']
 p=subprocess.run(['fuser',str(archive)],capture_output=True);assert p.returncode==1,p.stdout
 archive.unlink()
assert not archive.exists()
save(E/'NATIVE_UI_ACCEPTANCE.json',{'status':'pass scoped current Linux row/thumbnail/footer/status appearance','source_revision':source,'appimage_input':result['appimage_input'],'original_captures_directly_reviewed':frames,'review_receipt':str((E/'LINUX_CAPTURE_REVIEW.json').relative_to(Path.cwd())),'worker_result_sha256':sha(R/'RESULT.json'),'visible_top_level_items':4,'indexed_items_including_nested_file':5,'daemon_pid_preserved':w['running_pid_preserved'],'database_identity_preserved':True,'sources_preserved':True,'owned_domain_uuid':vm['uuid'],'owned_domain_and_volumes_removed':True,'consumed_transport_removed':True,'retired_transport_sha256':transport['archive_sha256'],'retired_transport_bytes':transport['archive_bytes'],'scope':review['limits']})
print('Linux exact-source native UI assessment passed; consumed transport removed')
