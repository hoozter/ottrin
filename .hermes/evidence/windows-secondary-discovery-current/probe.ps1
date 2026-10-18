$ErrorActionPreference='Stop'
$r='C:\OttrinCopyCleanupQA';$v=Get-Content "$r\SECONDARY_VOLUME.json" -Raw|ConvertFrom-Json
$result=@{ordinary_user=($env:USERNAME-eq'OttrinCopyQA');probes=@()}
foreach($parent in @($r,$v.root)){
 $path=Join-Path $parent 'secondary-acl-probe'
 if(Test-Path $path){throw 'Probe exists'}
 try {
  New-Item -ItemType Directory $path|Out-Null
  $item=@{parent=$parent;create='pass';before_sddl=(Get-Acl -LiteralPath $path).Sddl}
  try {$acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetSecurityDescriptorSddlForm('D:P(A;OICI;FA;;;OW)',[Security.AccessControl.AccessControlSections]::Access);Set-Acl -LiteralPath $path -AclObject $acl;$item.protect='pass'}catch{$item.protect='fail';$item.error=$_.Exception.Message}
  $result.probes+=$item
 }finally{if(Test-Path $path){Remove-Item -LiteralPath $path -Recurse -Force}}
}
$result|ConvertTo-Json -Depth 5|Set-Content "$r\ACL_PROBE.json"
