$ErrorActionPreference='Stop'
$r='C:\OttrinCopyCleanupQA\msi-authoring-compare'
$manifest=Get-Content "$r\INPUTS.json" -Raw|ConvertFrom-Json
$installer=New-Object -ComObject WindowsInstaller.Installer
$result=@{}
$wanted=@('Property','InstallExecuteSequence','InstallUISequence','CustomAction','Component','Registry','Directory','Media','Upgrade','LaunchCondition','LockPermissions','MsiLockPermissionsEx')
foreach($item in $manifest.PSObject.Properties){
 $path=Join-Path $r ($item.Name+'.msi')
 if((Get-FileHash $path).Hash.ToLowerInvariant()-ne$item.Value.msi_sha256){throw ('Input changed: '+$item.Name)}
 $db=$null;$summary=$null;$view=$null;$record=$null
 try{
  $db=$installer.OpenDatabase($path,0)
  $summary=$installer.SummaryInformation($path,0)
  $properties=@{};foreach($number in @(1,7,14,15,19)){$properties[$number.ToString()]=$summary.Property($number)}
  $view=$db.OpenView('SELECT `Name` FROM `_Tables`');$view.Execute();$tables=@()
  while($record=$view.Fetch()){$tables+=,$record.StringData(1);[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($record);$record=$null}
  $view.Close();[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($view);$view=$null
  $export=@{}
  foreach($table in $wanted){
   if($tables-notcontains$table){continue}
   $view=$db.OpenView('SELECT * FROM `'+$table+'`');$view.Execute();$rows=@()
   while($record=$view.Fetch()){
    $count=[int]$record.GetType().InvokeMember('FieldCount',[Reflection.BindingFlags]::GetProperty,$null,$record,$null);if($count-lt1){throw 'MSI record field count unavailable'};$row=@();for($index=1;$index-le$count;$index++){$row+=,$record.StringData($index)};$rows+=,@($row)
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($record);$record=$null
   }
   $view.Close();[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($view);$view=$null;$export[$table]=$rows
  }
  $result[$item.Name]=@{sha256=$item.Value.msi_sha256;summary=$properties;tables=$tables;data=$export}
 }finally{
  foreach($object in @($record,$view,$summary,$db)){if($object){[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($object)}}
 }
}
[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
$result|ConvertTo-Json -Depth 12|Set-Content "$r\TABLES.json" -Encoding UTF8
'All MSI databases opened read-only; no package installation or sample custom action executed.'
