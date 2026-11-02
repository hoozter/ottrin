param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$Nonce)
$ErrorActionPreference='Stop'
# Independent MSI authoring via the native Windows Installer database API.
# No compiler-generated database, WiX, SDK runtime, embedded program or policy fix.
$installer=New-Object -ComObject WindowsInstaller.Installer
$database=$null;$summary=$null;$tables=$null
function ImportTable([string]$Name,[string]$Columns,[string]$Types,[string]$Keys,[string[]]$Rows){
 $text=(($Columns,$Types,($Name+'|'+$Keys))+$Rows)-join "`r`n"
 [IO.File]::WriteAllText((Join-Path $tables ($Name+'.idt')),($text.Replace('|',"`t")+"`r`n"),[Text.Encoding]::ASCII)
 $database.Import($tables,($Name+'.idt'))
}
try{
 [IO.File]::WriteAllText((Join-Path $Root 'control.txt'),'Ottrin native database rollback control',[Text.UTF8Encoding]::new($false))
 $builds=@()
 foreach($kind in @('install','uninstall')){
  $path=Join-Path $Root ("control-$kind.msi");if(Test-Path $path){throw 'Existing native control retained'}
  $tables=Join-Path $Root ("tables-$kind");New-Item -ItemType Directory $tables|Out-Null
  $product='{'+[Guid]::NewGuid().ToString().ToUpperInvariant()+'}';$component='{'+[Guid]::NewGuid().ToString().ToUpperInvariant()+'}'
  $database=$installer.OpenDatabase($path,3)
  ImportTable 'Property' 'Property|Value' 's72|l0' 'Property' @(
   "ProductCode|$product",'ProductLanguage|1033',"ProductName|Ottrin Native Database Control $kind",'ProductVersion|1.0.0','Manufacturer|Ottrin QA','INSTALLLEVEL|1','SecureCustomProperties|OTTRIN_QA_FAIL'
  )
  ImportTable 'Directory' 'Directory|Directory_Parent|DefaultDir' 's72|S72|l255' 'Directory' @(
   'TARGETDIR||SourceDir','LocalAppDataFolder|TARGETDIR|.',"CONTROLFOLDER|LocalAppDataFolder|OttrinHostedRollback-${kind}:."
  )
  ImportTable 'Component' 'Component|ComponentId|Directory_|Attributes|Condition|KeyPath' 's72|S38|s72|i2|S255|S72' 'Component' @("ControlComponent|$component|CONTROLFOLDER|260||OwnedRegistry")
  ImportTable 'Feature' 'Feature|Feature_Parent|Title|Description|Display|Level|Directory_|Attributes' 's38|S38|L64|L255|I2|i2|S72|I2' 'Feature' @('Main||Control||1|1||0')
  ImportTable 'FeatureComponents' 'Feature_|Component_' 's38|s72' 'Feature_|Component_' @('Main|ControlComponent')
  $bytes=(Get-Item (Join-Path $Root 'control.txt')).Length
  ImportTable 'File' 'File|Component_|FileName|FileSize|Version|Language|Attributes|Sequence' 's72|s72|l255|i4|S72|S20|I2|i2' 'File' @("ControlFile|ControlComponent|control.txt|$bytes|||8192|1")
  ImportTable 'Registry' 'Registry|Root|Key|Name|Value|Component_' 's72|i2|l255|L255|L0|s72' 'Registry' @("OwnedRegistry|1|Software\OttrinQA\HostedRollback\$Nonce\$kind|Owned|#1|ControlComponent")
  ImportTable 'RemoveFile' 'FileKey|Component_|FileName|DirProperty|InstallMode' 's72|s72|L255|s72|i2' 'FileKey' @('ControlFolderCleanup|ControlComponent||CONTROLFOLDER|2')
  ImportTable 'Media' 'DiskId|LastSequence|DiskPrompt|Cabinet|VolumeLabel|Source' 'i2|i2|L64|S255|S32|S72' 'DiskId' @('1|1||||')
  ImportTable 'CustomAction' 'Action|Type|Source|Target' 's72|i2|S72|S255' 'Action' @('DeliberateFailure|1025|OwnedFailureLibrary|FailAfterExecution')
  ImportTable 'InstallExecuteSequence' 'Action|Condition|Sequence' 's72|S255|I2' 'Action' @(
   'CostInitialize||800','FileCost||900','CostFinalize||1000','InstallValidate||1400','InstallInitialize||1500','ProcessComponents||1600',
   'UnpublishFeatures||1800','RemoveRegistryValues||2600','RemoveFiles||3500','RemoveFolders||3600','CreateFolders||3700',
   'InstallFiles||4000','WriteRegistryValues||5000','RegisterUser||6000','RegisterProduct||6100','PublishFeatures||6300','PublishProduct||6400',
   'DeliberateFailure|OTTRIN_QA_FAIL=1|6450','InstallExecute||6500','InstallFinalize||6600'
  )
  $view=$database.OpenView('CREATE TABLE `Binary` (`Name` CHAR(72) NOT NULL, `Data` OBJECT NOT NULL PRIMARY KEY `Name`)');$null=$view.Execute();$view.Close();$null=[Runtime.InteropServices.Marshal]::FinalReleaseComObject($view)
  $disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});if($disc.Count-ne1){throw 'Owned failure library media missing'};$library=Join-Path ($disc[0].DeviceID+'\') 'failure.dll'
  $record=$installer.CreateRecord(2);$record.StringData(1)='OwnedFailureLibrary';$record.SetStream(2,$library);$view=$database.OpenView('INSERT INTO `Binary` (`Name`, `Data`) VALUES (?, ?)');$null=$view.Execute($record);$view.Close();foreach($value in @($view,$record)){$null=[Runtime.InteropServices.Marshal]::FinalReleaseComObject($value)}
  $database.Commit()
  $summary=$database.SummaryInformation(20)
  $summary.Property(1)=1252;$summary.Property(2)='Installation Database';$summary.Property(3)='Independent native rollback control';$summary.Property(4)='Ottrin QA'
  $summary.Property(7)='x64;1033';$summary.Property(9)='{'+[Guid]::NewGuid().ToString().ToUpperInvariant()+'}'
  $summary.Property(14)=400;$summary.Property(15)=8;$summary.Property(18)='Native Windows Installer table import; no WiX'
  $summary.Persist();$null=[Runtime.InteropServices.Marshal]::FinalReleaseComObject($summary);$summary=$null
  $database.Commit();$null=[Runtime.InteropServices.Marshal]::FinalReleaseComObject($database);$database=$null
  $builds+=@{kind=$kind;product=$product;sha256=(Get-FileHash $path).Hash;payload_bytes=$bytes;source='uncompressed control.txt';summary_word_count=8;tables=@(Get-ChildItem $tables -File|ForEach-Object {$_.Name})}
  Copy-Item $tables (Join-Path $Root 'results') -Recurse
 }
 @{status='authored; runtime acceptance separate';api='WindowsInstaller.Installer.OpenDatabase(Create), Database.Import and SummaryInformation';compiler_inputs=@();builds=$builds}|ConvertTo-Json -Depth 8|Set-Content -Encoding UTF8 (Join-Path $Root 'results\NATIVE_BUILD.json')
}finally{
 foreach($value in @($summary,$database,$installer)){if($value){$null=[Runtime.InteropServices.Marshal]::FinalReleaseComObject($value)}}
}
