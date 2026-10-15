param([switch]$WithRipgrep, [ValidatePattern('^[A-Za-z0-9]{1,16}$')][string]$Namespace='Final', [ValidatePattern('^[a-z_0-9:]*$')][string]$Filter='')
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinSearchQA';$r="$b\results";$p=$null;$tempRoot=$null;$result=@{status='running';scope='Full native search-library tests as a disposable ordinary user; no installed-app claim'}
try {
 if($env:USERNAME-ne'OttrinSearchQA'){throw 'Wrong test account'}
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();if(([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Elevated test actor'}
 if($WithRipgrep){$env:PATH="$b\ripgrep\ripgrep-15.2.0-x86_64-pc-windows-msvc;"+$env:PATH}
 $tempRoot=Join-Path $env:TEMP ('ottrin-search-owned-'+$Namespace);if(Test-Path $tempRoot){throw 'Owned temporary namespace exists'};New-Item -ItemType Directory $tempRoot|Out-Null;$env:TEMP=$tempRoot;$env:TMP=$tempRoot
 $rg=Get-Command rg -CommandType Application -ErrorAction SilentlyContinue;$result.ripgrep_available=($null-ne$rg);if($rg){$result.ripgrep=$rg.Source}
 if($Filter){$result.scope='Selected native search-library regression as ordinary user'};$result.child_working_directory="$env:WINDIR\System32"
 $arguments=if($Filter){$Filter+' --exact --nocapture --test-threads=1'}else{'--nocapture --test-threads=1'};$result.filter=$Filter
 $p=Start-Process "$b\search.exe" -ArgumentList $arguments -WorkingDirectory "$env:WINDIR\System32" -PassThru -RedirectStandardOutput "$r\search.stdout" -RedirectStandardError "$r\search.stderr";$null=$p.Handle
 if(-not$p.WaitForExit(180000)){throw "Owned tests still live: $($p.Id)"}
 $result.temporary_entries_after_tests=@(Get-ChildItem -LiteralPath $tempRoot -Force).Count;if($result.temporary_entries_after_tests){throw 'Test threads left temporary stores behind'}
 $expected=if($Filter){1}else{207};if($p.ExitCode-eq0-and[IO.File]::ReadAllText("$r\search.stdout")-notmatch("test result: ok\. $expected passed; 0 failed;")){throw 'Expected test selection was not executed'}
 $result.exit=$p.ExitCode;$result.status=if($p.ExitCode-eq0){'pass'}else{'fail'}
} catch {$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally {if($p-and-not$p.HasExited){& "$env:WINDIR\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;if(-not$p.WaitForExit(10000)){$result.cleanup_error='Owned tests remain live'}};if($tempRoot-and(Test-Path $tempRoot)){Remove-Item -LiteralPath $tempRoot -Recurse -Force};$result|ConvertTo-Json -Depth 8|Set-Content "$r\RESULTS.json" -Encoding UTF8}
if($result.status-ne'pass'){exit 1}
