from pathlib import Path
import hashlib,json,subprocess,shutil
r=Path.cwd();e=Path(__file__).resolve().parent;read=lambda p:json.loads(p.read_text())
base=read(e/'BASELINE.json');domains=set(subprocess.check_output(['virsh','-c','qemu:///system','list','--all','--uuid'],text=True).split());volumes={x.split()[0] for x in subprocess.check_output(['virsh','-c','qemu:///system','vol-list','default'],text=True).splitlines() if x.strip() and not x.strip().startswith(('Name','-'))};cases=[];profiles=[]
for name in ['assembly','windows-fields','windows-fields-canonical','windows-fields-native-path']:
 p=e/name;o=read(p/'OWNER.json');c=o['cleanup'];assert c['domain_removed'] and c['fixture_removed'] and o['uuid'] not in domains and not(set(c['owned_volumes_removed'])&volumes);assert not(p/'fixture').exists()
 cases.append({'case':name,'uuid':o['uuid'],'removed_volumes':c['owned_volumes_removed']})
 if o.get('private_console'):
  t=read(p/'viewer/TEARDOWN.json');assert t['profile_removed'] and not Path(t['profile']).exists();profiles.append(t['profile'])
for name in ['shortcut-control','linux-fields-dark','linux-fields-dark-observed','linux-fields-light']:
 t=read(e/name/'TEARDOWN.json');assert t['profile_removed'] and not Path(t['profile']).exists();profiles.append(t['profile'])
retired=['.hermes/packaging-tmp/.linux-desktop-extract','.hermes/packaging-tmp/.windows-msix-source','.hermes/evidence/editable-fields-current/source.zip']
assert all(not(r/p).exists() for p in retired)
inc=read(e/'INCREMENTAL_CLEANUP_RESULT.json');assert inc['result']['removed_sessions']==len(inc['retired_paths'])==15 and all(not Path(x).exists() for x in inc['retired_paths']) and all(Path(x).is_dir() for x in inc['retained_newest_paths'])
for p in list(e.rglob('__pycache__')):
 assert not p.is_symlink() and all(x.suffix=='.pyc' and x.stat().st_nlink==1 for x in p.iterdir());shutil.rmtree(p)
protected={'win11ltsc':'4e160378-2610-4072-b528-048e4c9f509e','ottrin-copy-cleanup-qa':'9b2e795f-fb04-4adb-95a7-8832f8894c67'}
for name,uid in protected.items():assert subprocess.check_output(['virsh','-c','qemu:///system','domuuid',name],text=True).strip()==uid
current={p:int(subprocess.check_output(['du','-s','-B1',p]).split()[0]) for p in base['sizes']};stages=[]
for sub in ['', 'windows']:
 p=e/sub/'BUILD_SOURCE.json';d=read(p);stage=Path(d['stage']);pending=stage.exists()
 if pending:
  files=[x for x in stage.rglob('*') if x.is_file()];assert all(not x.is_symlink() and x.stat().st_nlink==1 for x in files);assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
  d.update(pending_consumers=['Exact-source CI41639409624 completion; then guarded stable publication'],cleanup_complete=False);p.write_text(json.dumps(d,indent=2)+'\n');stages.append({'path':str(stage),'purpose':'Verified current candidate awaiting exact-source hosted gate and stable publication','pending_consumer':'CI41639409624 and promote-'+('windows' if sub else 'linux')+'.py','allocated_bytes':int(subprocess.check_output(['du','-s','-B1',str(stage)]).split()[0])})
receipt={'status':'All owned native resources, disposable extraction/source inputs and obsolete compiler sessions removed; verified stages await publication' if stages else 'All owned native resources, disposable stages/source inputs, superseded packages and obsolete compiler sessions removed','native_cases':cases,'removed_volumes':sum(len(x['removed_volumes']) for x in cases),'removed_private_profiles':profiles,'retired_paths':retired,'compiler_storage':{p:{'baseline':int(n),'current':current[p],'delta':current[p]-int(n)} for p,n in base['sizes'].items()},'compiler_cleanup':'Guarded helper removed15 obsolete sessions after explicit same-process identity reviews, exclusive locks and newest-session checks. Current binaries, dependencies and newest finalized caches retained. Same compiler/configuration test executable ottrin_ui-11a85ca5198af5b6 replaced its earlier version; no extra target configuration or blanket clean. Physical reclaim is not inferred from allocation due concurrent VM writes.','remaining_task_created_storage':[{'path':str(e),'purpose':'Compact source/native checks, failed-attempt diagnostics, producers and original reviewed captures; no live QA consumer'},*stages,*([] if stages else [{'path':str(r/'dist/packages'),'purpose':'Current four-format verified usable Linux delivery'},{'path':str(r/'dist/windows-candidate'),'purpose':'Current verified Windows ZIP/NSIS/MSIX9 and58-file payload'}])],'protected_bases_preserved':protected,'personal_service':'Normal personal index service/data preserved; native Linux daemon bytes unchanged','limits':('Whole roadmap not complete; artifact publication depends on exact-source hosted gate.' if stages else 'Whole roadmap not complete; exact verified cohort published locally.')+' Historical native scopes retain original source/artifact hashes.'}
(e/'CLEANUP_FINAL.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps({'native_cases':len(cases),'removed_volumes':receipt['removed_volumes'],'removed_private_profiles':len(profiles),'pending_stages':stages},indent=2))
