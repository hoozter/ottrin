"""Collect live owned-GUI module identity; read redirected output only after Close."""
from pathlib import Path, PureWindowsPath
import argparse, base64, json
import native_compiler as n

p = Path(__file__).resolve().parent
e = p.parent / 'windows-preview-scroll-gui-current'
parser = argparse.ArgumentParser()
parser.add_argument('name', choices=['default', 'opengl'])
args = parser.parse_args()
session = json.loads((e / 'SESSION.json').read_text())
assert session['status'] == 'waiting for owned input'
assert session['guest_root'] == r'C:\OttrinWindowQA'
script = r'''$app='C:\OttrinWindowQA\payload\Ottrin-0.1.0-windows-x64\bin\ottrin.exe';
$owned=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-eq$app});
if($owned.Count-ne1){throw 'Owned GUI missing or ambiguous'};
$names=@('d3d12.dll','d3d11.dll','d3d10warp.dll','dxgi.dll','opengl32.dll','libgallium_wgl.dll','libegl.dll','libglesv2.dll','dxil.dll','vulkan-1.dll','d3dcompiler_47.dll');
$modules=@((Get-Process -Id $owned[0].ProcessId).Modules|Where-Object{$_.ModuleName.ToLowerInvariant()-in$names}|ForEach-Object{@{name=$_.ModuleName;path=$_.FileName;sha256=(Get-FileHash -LiteralPath $_.FileName).Hash.ToLowerInvariant()}});
@{pid=$owned[0].ProcessId;command_line=$owned[0].CommandLine;modules=$modules}|ConvertTo-Json -Depth 6'''
try:
    response = n.wait(n.start(script))
finally:
    n.cancel_active()
value = json.loads(base64.b64decode(response['out-data']).decode('utf-8-sig'))
manifest = json.loads((p / 'PAYLOAD_GUI_MANIFEST.json').read_text())
entries = {key.lower(): digest for key, digest in manifest['entries'].items()}
payload = PureWindowsPath(r'C:\OttrinWindowQA\payload')
assert value['modules']
for module in value['modules']:
    path = PureWindowsPath(module['path'])
    if path.is_relative_to(payload):
        key = str(path.relative_to(payload)).lower()
        assert entries[key] == module['sha256']
        module['matches_exact_portable_payload'] = True
if args.name == 'opengl':
    assert '--renderer opengl' in value['command_line']
    for name in ['opengl32.dll', 'libgallium_wgl.dll']:
        module = next(module for module in value['modules'] if module['name'].lower() == name)
        assert module.get('matches_exact_portable_payload'), module
else:
    assert '--renderer' not in value['command_line']
value.update(source=manifest['source_commit'], launch=args.name, nonce=session['nonce'],
             boundary='Loaded module provenance and rendered ordinary virtual desktop; physical-adapter acceptance remains separate',
             log_collection='Read app.stdout/app.stderr after normal Close; the redirect writer holds the active files without read sharing')
(e / (args.name.upper() + '_RENDERER.json')).write_text(json.dumps(value, indent=2) + '\n')
print(json.dumps({'launch': args.name, 'pid': value['pid'], 'modules': [module['name'] for module in value['modules']]}, indent=2))
