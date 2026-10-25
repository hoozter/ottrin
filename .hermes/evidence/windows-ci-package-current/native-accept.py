from pathlib import Path
import json,base64,hashlib
import native_compiler as n
p=Path(__file__).parent
script=r'''$r='C:\OttrinCIPackageQA';if(-not(Test-Path "$r\MSI_PAYLOAD.json")){throw 'Verified MSI receipt missing'}
$runs=@();foreach($spec in @(@('ottrin.exe','--help',0),@('ottrin-indexd.exe','--help',2),@('ottrin-priv-helper.exe','--help',2))){$exe=$spec[0];$expected=$spec[2];$info=New-Object System.Diagnostics.ProcessStartInfo;$info.FileName="$r\payload\Ottrin-0.1.0-windows-x64\bin\$exe";$info.Arguments=$spec[1];$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$child=New-Object System.Diagnostics.Process;$child.StartInfo=$info;if(-not$child.Start()){throw 'Process start refused'};$stdout=$child.StandardOutput.ReadToEndAsync();$stderr=$child.StandardError.ReadToEndAsync();if(-not$child.WaitForExit(10000)){$child.Kill();throw "CLI timeout $exe"};$child.WaitForExit();$code=$child.ExitCode;[IO.File]::WriteAllText("$r\$exe.stdout",$stdout.Result);[IO.File]::WriteAllText("$r\$exe.stderr",$stderr.Result);$child.Dispose();if($code-ne$expected){throw "CLI exit mismatch $exe : $code"};$runs+=@{executable=$exe;exit=$code;stdout_sha256=(Get-FileHash "$r\$exe.stdout").Hash.ToLowerInvariant();stderr_sha256=(Get-FileHash "$r\$exe.stderr").Hash.ToLowerInvariant()}};@{status='pass';identity=[Security.Principal.WindowsIdentity]::GetCurrent().Name;scope='Native loader and CLI argument paths only; no GUI or service lifecycle';runs=$runs}|ConvertTo-Json -Depth 5|Set-Content "$r\CLI_SMOKE.json"'''
pid=n.start(script);(p/'accept.pid').write_text(str(pid));n.wait(pid)
for name in ['MSI_PAYLOAD.json','CLI_SMOKE.json','administrative.log']:
 n.download(name,p/name)
for name in ['MSI_PAYLOAD.json','CLI_SMOKE.json']:
 print((p/name).read_text(encoding='utf-8-sig'))
