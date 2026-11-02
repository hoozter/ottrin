"""Retire one unused older preview test executable after a matching verified replacement."""
from pathlib import Path
import hashlib,json,subprocess,shutil
r=Path.cwd();e=Path(__file__).resolve().parent
old='95903c79075b572c';new='89d8d7d374b9faf4'
old_meta=r/f'target/debug/.fingerprint/ottrin-preview-{old}'
new_meta=r/f'target/debug/.fingerprint/ottrin-preview-{new}'
a=json.loads((old_meta/'test-lib-ottrin_preview.json').read_text());b=json.loads((new_meta/'test-lib-ottrin_preview.json').read_text())
keys=['rustc','features','declared_features','target','profile','path','rustflags','config','compile_kind']
assert all(a.get(key)==b.get(key) for key in keys)
older=r/f'target/debug/deps/ottrin_preview-{old}';newer=r/f'target/debug/deps/ottrin_preview-{new}'
assert newer.stat().st_mtime_ns>older.stat().st_mtime_ns
assert '3 passed; 0 failed; 1 ignored' in (e/'HTML_FOCUSED.log').read_text()
# Historical prose/logs are receipts, not consumers. Maintained executable producers
# must not contain a command or input pointing to the retired test binary.
refs=subprocess.run(['rg','-l',f'ottrin_preview-{old}|ottrin-preview-{old}','scripts','packaging','.hermes/evidence','--glob','*.py','--glob','*.sh','--glob','*.json','--glob','*.ps1'],capture_output=True,text=True)
assert refs.returncode==1,(refs.stdout,refs.stderr)
paths=[older,older.with_name(older.name+'.d'),*old_meta.iterdir()]
assert all(p.is_file() and not p.is_symlink() and p.stat().st_nlink==1 for p in paths)
consumers=subprocess.run(['fuser',*map(str,paths)],capture_output=True,text=True);assert consumers.returncode==1,(consumers.stdout,consumers.stderr)
receipt={'status':'pass','replacement':str(newer),'replacement_sha256':hashlib.file_digest(newer.open('rb'),'sha256').hexdigest(),'retired':str(older),'retired_sha256':hashlib.file_digest(older.open('rb'),'sha256').hexdigest(),'matching_compiler_settings':{key:b.get(key) for key in keys},'removed_bytes':sum(p.stat().st_size for p in paths),'removed_files':[str(p) for p in paths],'verification':'HTML protocol, workspace admission, real kernel positive/denial and owned child controls pass; strict all-target clippy passes. No maintained producer references or active consumers; all link counts one.'}
for p in paths:p.unlink()
old_meta.rmdir();(e/'RETIRED_PREVIEW_TEST.json').write_text(json.dumps(receipt,indent=2)+'\n');print(receipt['removed_bytes'])
