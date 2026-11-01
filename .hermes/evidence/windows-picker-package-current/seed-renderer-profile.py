"""Initialize the owned, not-yet-launched retry profile; never touch a real user's config."""
from pathlib import Path
import base64,json
import native_compiler as n
p=Path(__file__).resolve().parent;e=p.parent/'windows-picker-gui-current'
s=json.loads((e/'SESSION.json').read_text());assert s['status']=='waiting for owned input'
script=r'''$ErrorActionPreference='Stop';$root='C:\OttrinWindowQA';$owner=Get-Content "$root\actor-config.json" -Raw|ConvertFrom-Json;if($owner.account-ne'OttrinWindowQA'){throw 'Wrong owned account'};$u=Get-LocalUser $owner.account;if($u.SID.Value-ne$owner.sid-or$u.Description-ne'Ottrin owned packaged workflow QA'){throw 'Owned account identity changed'};if(@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-like"$root\payload\*"}).Count){throw 'Payload must not be running'};$path="$root\cases\ordinary\roaming\ottrin\config.json";$c=Get-Content $path -Raw|ConvertFrom-Json;$c|Add-Member -NotePropertyName graphics_renderer -NotePropertyValue Native -Force;$c|Add-Member -NotePropertyName default_view_mode -NotePropertyValue Miller -Force;[IO.File]::WriteAllText($path,($c|ConvertTo-Json -Depth 20),[Text.UTF8Encoding]::new($false));@{status='pass';graphics_renderer=$c.graphics_renderer;default_view_mode=$c.default_view_mode;config_sha256=(Get-FileHash $path).Hash.ToLowerInvariant();scope='Explicit initial private fixture preferences, not user-click acceptance'}|ConvertTo-Json'''
try:response=n.wait(n.start(script))
finally:n.cancel_active()
d=json.loads(base64.b64decode(response['out-data']).decode('utf-8-sig'));assert d['status']=='pass';(e/'SEEDED_PREFERENCES.json').write_text(json.dumps(d,indent=2)+'\n')
