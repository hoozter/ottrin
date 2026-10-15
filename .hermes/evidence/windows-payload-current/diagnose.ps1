$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\payload-current'
$env:OTTRIN_INSTALLER_TARGET="$r\wrapper-probe"
$env:OTTRIN_INSTALLER_PAYLOAD_SCRIPT="$r\payload-transaction.qa.ps1"
$env:OTTRIN_INSTALLER_PAYLOAD_PHASE='Initialize'
try {
 & ([scriptblock]::Create([IO.File]::ReadAllText($env:OTTRIN_INSTALLER_PAYLOAD_SCRIPT))) -Phase $env:OTTRIN_INSTALLER_PAYLOAD_PHASE
 'initialize passed'|Set-Content "$r\diagnose.log"
} catch {$_|Format-List * -Force|Out-String|Set-Content "$r\diagnose.log";exit 1}
