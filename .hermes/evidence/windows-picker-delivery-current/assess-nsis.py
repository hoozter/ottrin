"""Bind ordinary native NSIS/chooser observations to exact packages and completed teardown."""
from pathlib import Path
import argparse,hashlib,json,ntpath
r=Path.cwd();root=Path(__file__).resolve().parent;parser=argparse.ArgumentParser();parser.add_argument('--case',type=Path,required=True);e=parser.parse_args().case.resolve();assert e.parent==root
native_bytes=(e/'results/RESULT.json').read_bytes();result=json.loads(native_bytes.decode('utf-8-sig'));expected=json.loads((e/'APPLICATION_INPUT.json').read_text());owner=json.loads((e/'OWNER.json').read_text());viewer=json.loads((e/'viewer/TEARDOWN.json').read_text())
assert result['status']=='pass' and result['desktop_ready'] and result['package']==expected
assert result['identity']['administrator'] is False and result['identity']['session']>0
prepared=json.loads((e/'GUEST_PREPARATION.json').read_text());assert result['identity']['sid']==prepared['actor_sid']
assert ntpath.basename(result['identity']['profile']).casefold()==prepared['account'].casefold()
current=json.loads((root/'HOST_PAYLOAD.json').read_text());assert expected['source_revision']==current['source_revision']
stage=r/'.hermes/packaging-tmp/.windows-picker-stage'
paths={'portable.zip':stage/'Ottrin-0.1.0-windows-x64-portable.zip','current-setup.exe':stage/'Ottrin-0.1.0-windows-x64-setup.exe','previous-setup.exe':r/'dist/windows-candidate/Ottrin-0.1.0-windows-x64-setup.exe'}
for item in expected['files']:assert hashlib.sha256(paths[item['name']].read_bytes()).hexdigest()==item['sha256']
manifest=json.loads((root/'PAYLOAD.json').read_text());binary=manifest['bin/ottrin.exe'];selection=ntpath.join(result['identity']['profile'],'OttrinOwnedSelection')
def decode(wire):
 assert wire['encoding']=='windows-utf16-le';units=wire['units_hex'];assert len(units)%4==0
 data=b''.join(int(units[i:i+4],16).to_bytes(2,'little') for i in range(0,len(units),4));path=data.decode('utf-16-le');assert '\0' not in path
 return path[4:] if path.startswith('\\\\?\\') else path
expected_cases={}
for theme in ['dark','light']:
 for suffix,names in [('baseline',['beta.txt']),('up-single',['alpha.txt']),('up-range',['alpha.txt','beta.txt']),('down-single',['gamma.txt']),('down-range',['beta.txt','gamma.txt'])]:expected_cases['picker-'+theme+'-'+suffix]=names
cases={case['case']:case for case in result['picker_cases']};assert len(cases)==len(result['picker_cases'])==10 and cases.keys()==expected_cases.keys()
for name,names in expected_cases.items():
 case=cases[name];assert case['status']=='pass' and case['exit']==0 and case['binary_sha256']==binary
 response=case['response'];assert response['accepted']
 actual=[ntpath.normcase(decode(wire)) for wire in response['paths']];targets=[ntpath.normcase(ntpath.join(selection,n)) for n in names]
 assert len(actual)==len(set(actual)) and sorted(actual)==sorted(targets)
for key in ['portable_picker_without_index','fresh_install_uninstall','running_index_upgrade','uninstall_retry']:assert result[key]=='pass',key
assert result['fresh_install_exit']==0 and result['payload_files']==58 and result['owned_portable_extraction_removed'] is True
for key in ['fresh_main','current_main']:
 main=result[key];assert main['status']=='pass' and main['exit']==0 and main['saved_view']=='Miller' and main['binary_sha256']==binary
before=result['before_upgrade'];after=result['after_upgrade'];assert before['database_identity']==after['database_identity'] and before['preferences']==after['preferences'] and before['live']['pid']!=after['live']['pid']
assert result['runtime_cases']==[{'enabled':False,'running':True,'status':'pass'},{'enabled':True,'running':False,'status':'pass'},{'enabled':False,'running':False,'status':'pass'}]
refusal=result['refused_uninstall'];assert refusal['status']=='pass' and refusal['exit']!=0 and refusal['before']==refusal['after'] and refusal['database_identity']==before['database_identity']
for key in ['task_and_security_preserved','payload_preserved','preferences_preserved','source_preserved']:assert refusal[key] is True
cleanup=owner['cleanup'];assert cleanup['domain_removed'] and cleanup['fixture_removed'] and cleanup['base_still_off'] and len(cleanup['owned_volumes_removed'])==2
assert viewer['profile_removed'] and not Path(viewer['profile']).exists()
members=json.loads((e/'RESULT_MEMBERS.json').read_text())
for member in members:assert hashlib.sha256((e/'results'/member['path']).read_bytes()).hexdigest()==member['sha256']
for filename,exitcode in [('fresh-install-process.json',0),('fresh-uninstall-process.json',0),('running-upgrade-process.json',0),('uninstall-retry-process.json',0)]:
 raw=(e/'results'/filename).read_bytes();assert json.loads(raw.decode('utf-8-sig'))['exit']==exitcode
assert result['last_picker_window']['title']=='Ottrin \u2014 Selection proof'
assert result['last_picker_window']['enabled'] is True and result['last_picker_window']['input_idle'] is True
report={'status':'pass within exact ordinary-user current portable chooser and NSIS lifecycle scope','source_revision':expected['source_revision'],'package_inputs':expected,'native_identity':result['identity'],'native_result_sha256':hashlib.sha256(native_bytes).hexdigest(),'result_authority':'Exact UTF8 native results/RESULT.json; console progress snapshot is not an acceptance input','picker_cases':list(cases),'themes':['Dark','Light'],'display_scope':'Native Windows software desktop at its default DPI; no150percent or physical-GPU claim','fresh_current_install_uninstall':'pass','accepted_baseline_running_index_upgrade':'pass','saved_miller_configuration_and_native_database_identity':'pass','independent_startup_running_intent_matrix':result['runtime_cases'],'real_cache_lock_refusal_preserves_live_index':refusal,'supported_uninstall_retry':'pass','owned_cleanup':cleanup,'scope_limit':'No MSI rollback, Store certification or whole-roadmap acceptance. Current installed/portable GUI captures still need direct rendered review.'}
(e/'ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'status':report['status'],'picker_cases':len(cases),'cleanup':'verified'}))
