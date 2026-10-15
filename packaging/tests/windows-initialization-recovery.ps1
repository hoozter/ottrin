param(
    [Parameter(Mandatory=$true)][string]$Script,
    [Parameter(Mandatory=$true)][string]$Report
)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$work=Join-Path ([IO.Path]::GetTempPath()) ('ottrin-init-'+[Guid]::NewGuid().ToString('N'))
$active=@();$results=@()
function Assert($value,$message) {if(-not $value){throw $message}}
function StartHelper($source,$phase,$target,$name) {
    $p=Start-Process powershell.exe -ArgumentList ('-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+$source+'" -Phase '+$phase+' -Target "'+$target+'"') -RedirectStandardOutput "$work\$name.out" -RedirectStandardError "$work\$name.err" -PassThru
    $null=$p.Handle;$script:active+=,$p
    return $p
}
function StopHelper($p) {
    if(-not $p.HasExited) {
        & "$env:WINDIR\System32\taskkill.exe" /PID $p.Id /T /F | Out-Null
        Assert ($LASTEXITCODE -eq 0) 'Cannot stop owned helper tree'
        Assert ($p.WaitForExit(10000)) 'Owned helper remains live'
    }
}
function RunHelper($source,$phase,$target,$name) {
    $p=StartHelper $source $phase $target $name
    Assert ($p.WaitForExit(30000)) "Helper still live: $name"
    $p.WaitForExit()
    @{exit=$p.ExitCode;output=([IO.File]::ReadAllText("$work\$name.out")).Trim()}
}
function Manifest($path) {
    $entries=@(Get-Item -LiteralPath $path -Force)+@(Get-ChildItem -LiteralPath $path -Force -Recurse)
    @($entries|Sort-Object FullName|ForEach-Object {
        @{name=$_.FullName;identity=[OttrinInstallIdentity]::Read($_.FullName);hash=$(if(-not $_.PSIsContainer){(Get-FileHash -LiteralPath $_.FullName).Hash}else{''})}
    })|ConvertTo-Json -Depth 5 -Compress
}
try {
    $null=[IO.Directory]::CreateDirectory($work)
    Assert (-not (New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Run this regression as an ordinary non-administrator'
    $source=Get-Content -LiteralPath $Script -Raw
    $definition=[regex]::Match($source,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@").Groups[1].Value
    Assert (-not [string]::IsNullOrEmpty($definition)) 'Identity implementation missing'
    Add-Type -TypeDefinition $definition
    foreach($point in @('root','new')) {
        $needle=if($point -eq 'root'){'$record.root=Identity $Root'}else{'$record.staged=Identity $New'}
        Assert ($source.Split(@($needle),[StringSplitOptions]::None).Count -eq 2) 'Initialization checkpoint is ambiguous'
        $instrumented=$source.Replace($needle,($needle+"`n        [IO.File]::WriteAllText(`$env:OTTRIN_INIT_CHECKPOINT,'ready');while(`$true){Start-Sleep -Milliseconds 100}"))
        $pause=Join-Path $work ('pause-'+$point+'.ps1')
        [IO.File]::WriteAllText($pause,$instrumented,[Text.UTF8Encoding]::new($true))
        foreach($existing in @($false,$true)) {
            $name="$point-$existing";$folder=Join-Path $work ($name+" Space Ω 'copy'");$null=[IO.Directory]::CreateDirectory($folder)
            $target=Join-Path $folder 'install'
            if($existing) {
                $null=[IO.Directory]::CreateDirectory("$target\bin")
                [IO.File]::WriteAllText("$target\bin\ottrin.exe",'old executable fixture')
                [IO.File]::WriteAllText("$target\Uninstall.exe",'old uninstaller fixture')
                [IO.File]::WriteAllText("$target\personal.txt",'preserve')
            }
            $prior=if($existing){Manifest $target}else{$null}
            $env:OTTRIN_INIT_CHECKPOINT=Join-Path $folder 'checkpoint'
            $p=StartHelper $pause Initialize $target $name
            $deadline=[DateTime]::UtcNow.AddSeconds(30)
            while(-not [IO.File]::Exists($env:OTTRIN_INIT_CHECKPOINT)) {
                Assert (-not $p.HasExited) 'Initializer exited before checkpoint'
                Assert ([DateTime]::UtcNow -lt $deadline) 'Checkpoint timeout'
                Start-Sleep -Milliseconds 100
            }
            StopHelper $p
            $now=RunHelper $Script RecoveryPlan $target "$name-recovery"
            Assert ($now.exit -eq 0 -and $now.output -ceq 'none') 'Empty initialization did not recover'
            Assert (-not (Test-Path -LiteralPath "$target.ottrin-setup")) 'Empty skeleton remains'
            if($existing){Assert ((Manifest $target) -ceq $prior) 'Original installation changed'}else{Assert (-not (Test-Path -LiteralPath $target)) 'Fresh target created'}
            $retry=RunHelper $Script Initialize $target "$name-retry"
            Assert ($retry.exit -eq 0) 'Ordinary initialization retry failed'
            $plan=RunHelper $Script RecoveryPlan $target "$name-plan"
            Assert ($plan.exit -eq 0 -and $plan.output -ceq 'rollback-restored') 'Retry journal is invalid'
            Assert ((RunHelper $Script Rollback $target "$name-rollback").exit -eq 0) 'Rollback failed'
            Assert ((RunHelper $Script FinishRollback $target "$name-finish").exit -eq 0) 'Rollback retirement failed'
            Assert (-not (Test-Path -LiteralPath "$target.ottrin-setup")) 'Retired transaction remains'
            if($existing){Assert ((Manifest $target) -ceq $prior) 'Original changed after retry rollback'}
            $results+=@{case=$name;status='pass';retry_and_rollback='pass'}
        }
    }
    foreach($case in @('root-file','new-file','new-content','new-subdirectory','partial-temporary-journal','torn-journal','unexpected-entry','new-junction')) {
        $folder=Join-Path $work $case;$null=[IO.Directory]::CreateDirectory($folder);$target=Join-Path $folder 'install';$root="$target.ottrin-setup"
        if($case -eq 'root-file'){[IO.File]::WriteAllText($root,'keep')}
        else {
            $null=[IO.Directory]::CreateDirectory($root)
            switch($case) {
                'new-file' {[IO.File]::WriteAllText("$root\new",'keep')}
                'new-content' {$null=[IO.Directory]::CreateDirectory("$root\new");[IO.File]::WriteAllText("$root\new\keep.txt",'keep')}
                'new-subdirectory' {$null=[IO.Directory]::CreateDirectory("$root\new\keep")}
                'partial-temporary-journal' {[IO.File]::WriteAllText("$root\journal-123",'{"version":')}
                'torn-journal' {[IO.File]::WriteAllText("$root\transaction.json",'{"version":')}
                'unexpected-entry' {[IO.File]::WriteAllText("$root\keep.txt",'keep')}
                'new-junction' {$null=[IO.Directory]::CreateDirectory("$folder\outside");[IO.File]::WriteAllText("$folder\outside\keep.txt",'keep');$null=New-Item -ItemType Junction -Path "$root\new" -Target "$folder\outside"}
            }
        }
        # Reparse refusal is checked separately: the identity helper must reject
        # the link, so a manifest intentionally cannot walk it.
        $prior=if($case -ne 'new-junction'){Manifest $root}else{[IO.File]::ReadAllText("$folder\outside\keep.txt")}
        $result=RunHelper $Script RecoveryPlan $target "$case-refusal"
        Assert ($result.exit -ne 0) "Unsafe recovery accepted: $case"
        if($case -eq 'new-junction') {
            Assert ((Get-Item -LiteralPath "$root\new" -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) 'Junction changed'
            Assert ([IO.File]::ReadAllText("$folder\outside\keep.txt") -ceq $prior) 'Junction target changed'
            [IO.Directory]::Delete("$root\new")
        } else {Assert ((Manifest $root) -ceq $prior) "Refused contents changed: $case"}
        $results+=@{case=$case;status='pass';refused_and_preserved=$true}
    }
    @{status='pass';user=[Security.Principal.WindowsIdentity]::GetCurrent().Name;administrator=(New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator);cases=$results}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $Report -Encoding UTF8
} catch {
    @{status='fail';error=$_.Exception.Message;stack=$_.ScriptStackTrace;cases=$results}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $Report -Encoding UTF8
    throw
} finally {
    foreach($p in $active){StopHelper $p}
    # Remove junctions themselves before recursive fixture teardown.
    if(Test-Path -LiteralPath $work){Get-ChildItem -LiteralPath $work -Recurse -Force -Attributes ReparsePoint|ForEach-Object {[IO.Directory]::Delete($_.FullName)};Remove-Item -LiteralPath $work -Recurse -Force}
    Remove-Item Env:\OTTRIN_INIT_CHECKPOINT -ErrorAction SilentlyContinue
}
