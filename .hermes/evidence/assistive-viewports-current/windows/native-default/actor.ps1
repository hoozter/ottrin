param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$out=Join-Path $Root 'results';$result=@{status='running';scope='Current release GUI native UIAutomation under a fresh ordinary interactive token; explicit borrowed accepted portable runtime, not package or physical adapter acceptance'};$ownedProcesses=@();$app=$null;$mainStarted=$false;$finalStatus='failed native UIAutomation gate';$teardownFailed=$false
function Save {$result|ConvertTo-Json -Depth 20|Set-Content -Encoding UTF8 (Join-Path $out 'RESULT.json')}
function Phase($name){$result.phase=$name;Save}
function Require($value,$message){if(-not$value){throw $message}}
function AwaitWindow($process,$title){
 $deadline=[DateTime]::UtcNow.AddSeconds(30)
 do{
  Require (-not$process.HasExited) 'Application exited before its declared window appeared'
  $window=if($title){[PickerNative]::Window($process.Id,$title)}else{[PickerNative]::AnyWindow($process.Id)}
  if($window-ne[IntPtr]::Zero){Start-Sleep -Milliseconds 300;return $window}
  Start-Sleep -Milliseconds 100
 }while([DateTime]::UtcNow-lt$deadline)
 throw 'Owned native window did not appear'
}
function Tree($window,$name){
 $element=[System.Windows.Automation.AutomationElement]::FromHandle($window)
 $rootInfo=@{title=[PickerNative]::Title($window);name=$element.Current.Name;class=$element.Current.ClassName;type=$element.Current.ControlType.ProgrammaticName;process=$element.Current.ProcessId;framework=$element.Current.FrameworkId;provider_description_api_available=($null-ne[System.Windows.Automation.AutomationElementIdentifiers]::ProviderDescriptionProperty);actor_apartment=[Threading.Thread]::CurrentThread.GetApartmentState().ToString()}
 $rootInfo|ConvertTo-Json -Depth 6|Set-Content -Encoding UTF8 (Join-Path $out ($name+'-root.json'))
 $deadline=[DateTime]::UtcNow.AddSeconds(15)
 do{
  $nodes=$element.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
  $close=@($nodes|Where-Object {$_.Current.Name-eq'Close window'})
  if($close.Count){break}
  @($nodes|ForEach-Object {@{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;enabled=$_.Current.IsEnabled}})|ConvertTo-Json -Depth 6|Set-Content -Encoding UTF8 (Join-Path $out ($name+'-observed.json'));Require ([DateTime]::UtcNow-lt$deadline) 'Native accessibility provider did not expose window controls'
  Start-Sleep -Milliseconds 100
 }while($true)
 $rows=@();foreach($node in $nodes){$v=$node.Current;$r=$v.BoundingRectangle;$rows+=@{name=$v.Name;type=$v.ControlType.ProgrammaticName;enabled=$v.IsEnabled;focusable=$v.IsKeyboardFocusable;rect=@($r.X,$r.Y,$r.Width,$r.Height)}}
 $rows|ConvertTo-Json -Depth 8|Set-Content -Encoding UTF8 (Join-Path $out ($name+'-tree.json'))
 return $element
}
function Named($element,$name){
 $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$name)
 $found=$element.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)
 Require ($found.Count-eq1) ('Expected one named native control: '+$name+'; found='+$found.Count)
 return $found.Item(0)
}
function Invoke($element,$name){
 $node=Named $element $name;Require $node.Current.IsEnabled ('Control is disabled: '+$name)
 $pattern=$node.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern);$pattern.Invoke();Start-Sleep -Milliseconds 400
}
function CloseWindow($window){$element=[System.Windows.Automation.AutomationElement]::FromHandle($window);Invoke $element 'Close window'}
function ClickElement($window,$element){
 $rect=$element.Current.BoundingRectangle;Require ($rect.Width-gt0-and$rect.Height-gt0) 'Native element has no rendered bounds'
 $geometry=[PickerNative]::Geometry($window);$match=[regex]::Match($geometry,'origin=(-?\d+),(-?\d+) dpi=(\d+)');Require $match.Success 'Native client geometry unavailable'
 $scale=[double]$match.Groups[3].Value/96.0;$x=($rect.X+$rect.Width/2-[double]$match.Groups[1].Value)/$scale;$y=($rect.Y+$rect.Height/2-[double]$match.Groups[2].Value)/$scale
 [PickerNative]::Click($window,$x,$y);Start-Sleep -Milliseconds 300
}
function NativePath($path){return @{encoding='windows-utf16-le';units_hex=(-join $path.ToCharArray().ForEach({([int]$_).ToString('x4')}))}}
try{
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent();Require ($identity.User.Value-eq$ExpectedSid) 'Wrong owned actor SID';Require (-not([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary token required'
 $profilePath=[Environment]::GetFolderPath([Environment+SpecialFolder]::UserProfile);Require ((Split-Path $profilePath -Leaf)-like($identity.Name.Split('\')[-1]+'*')) 'Wrong owned profile';$session=(Get-Process -Id $PID).SessionId;Require ($session-gt0) 'Interactive desktop required';[Environment]::SetEnvironmentVariable('HOME',$profilePath,'Process')
 $result.identity=@{sid=$ExpectedSid;profile=$profilePath;administrator=$false;session=$session};Phase 'waiting for observed owned desktop'
 $deadline=[DateTime]::UtcNow.AddMinutes(8);$readyPath=Join-Path $out 'DESKTOP_READY.json';while(-not(Test-Path $readyPath)){Require ([DateTime]::UtcNow-lt$deadline) 'Owned desktop was not observed';Start-Sleep -Milliseconds 250}
 $ready=Get-Content $readyPath -Raw|ConvertFrom-Json;Require ($ready.sid-eq$ExpectedSid) 'Readiness belongs to another user';$result.desktop_ready=$true
 Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
 & ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $Root 'native-controls.ps1'))));[PickerNative]::Init()
 $uiaInput=Get-Content (Join-Path $Root 'uia-input.json') -Raw|ConvertFrom-Json;Require ((Get-FileHash (Join-Path $Root 'uia.zip')).Hash.ToLowerInvariant()-eq$uiaInput.archive_sha256) 'Private UIAutomation input mismatch'
 Require (@('native','opengl')-contains$uiaInput.renderer) 'Declared renderer required';$result.renderer=$uiaInput.renderer;$rendererArgs=if($uiaInput.renderer-eq'opengl'){@('--renderer','opengl')}else{@()}
 $extract=Join-Path $profilePath 'OttrinOwnedAssistivePayload';Require (-not(Test-Path $extract)) 'Existing payload must not be replaced';Expand-Archive (Join-Path $Root 'uia.zip') $extract
 $app=Join-Path $extract 'Ottrin-0.1.0-windows-x64\bin\ottrin.exe';Require ((Get-FileHash $app).Hash.ToLowerInvariant()-eq$uiaInput.application_sha256) 'Current Windows GUI identity mismatch';$result.input=$uiaInput
 $fixture=Join-Path $profilePath 'OttrinOwnedAssistive';Require (-not(Test-Path $fixture)) 'Existing user source must not be replaced';$null=New-Item -ItemType Directory $fixture;$source=Join-Path $fixture 'proof.txt';[IO.File]::WriteAllText($source,'Ottrin read-only assistive proof.');$sourceHash=(Get-FileHash $source).Hash
 $config=Get-Content (Join-Path $Root 'config-template.json') -Raw|ConvertFrom-Json;$config.theme='Dark';$config.default_view_mode='List';$config.bookmarks=@();$config.installed_office_previews_enabled=$false;$config.search.include_roots=@($fixture);$config.search.exclude_roots=@();$config.search.content_indexing_enabled=$false
 $pref=Join-Path $env:APPDATA 'ottrin\config.json';Require (-not(Test-Path $pref)) 'Existing preferences must not be replaced';$null=[IO.Directory]::CreateDirectory((Split-Path $pref));[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 60),[Text.UTF8Encoding]::new($false))
 Phase 'native main UIAutomation tree and actions'
 $main=Start-Process $app -ArgumentList (@(('"'+$fixture+'"'))+$rendererArgs) -PassThru;$ownedProcesses+=@($main);$mainStarted=$true;$window=AwaitWindow $main 'Ottrin';[PickerNative]::Place($window);[PickerNative]::Focus($window)
 [PickerNative]::Shot($window,(Join-Path $out 'main-before-provider.png'));$element=Tree $window 'main';[PickerNative]::Shot($window,(Join-Path $out 'main.png'))
 foreach($name in @('Back','Forward','Parent folder','New tab','Menu','Manage administrator helper','Manage index service','Close window')){$null=Named $element $name}
 $result.forward_disabled=-not(Named $element 'Forward').Current.IsEnabled;$result.parent_enabled=(Named $element 'Parent folder').Current.IsEnabled;Require ($result.forward_disabled-and$result.parent_enabled) 'Windows enabled-state mapping is incorrect'
 Phase 'native Settings UIAutomation subtree';Invoke $element 'Menu';$element=[System.Windows.Automation.AutomationElement]::FromHandle($window);Invoke $element 'Settings…'
 $settings=AwaitWindow $main 'Ottrin Settings';$result.settings_foreground=([PickerNative]::GetForegroundWindow()-eq$settings);Require $result.settings_foreground 'Settings did not receive native foreground focus';$settingsTree=Tree $settings 'settings';[PickerNative]::Shot($settings,(Join-Path $out 'settings.png'));$null=Named $settingsTree 'Close window';CloseWindow $settings;Start-Sleep -Milliseconds 500;Require (-not$main.HasExited) 'Settings close terminated the main window';$result.settings_native_close=$true
 Phase 'native read-only Inspector accessibility'
 $element=[System.Windows.Automation.AutomationElement]::FromHandle($window);ClickElement $window (Named $element 'proof.txt');Invoke $element 'Menu';$element=[System.Windows.Automation.AutomationElement]::FromHandle($window);Invoke $element 'Quick Inspector Selection'
 $inspector=AwaitWindow $main 'Ottrin Quick Inspector — proof.txt';$result.inspector_foreground=([PickerNative]::GetForegroundWindow()-eq$inspector);Require $result.inspector_foreground 'Inspector did not receive native foreground focus';$inspectorTree=Tree $inspector 'inspector';
 $result.inspector_value_controls=@();$editCondition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Edit)
 foreach($edit in $inspectorTree.FindAll([System.Windows.Automation.TreeScope]::Descendants,$editCondition)){
  $supported=@($edit.GetSupportedPatterns()|Where-Object {$_.Id-eq[System.Windows.Automation.ValuePattern]::Pattern.Id})
  if($supported.Count){$value=$edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current;$result.inspector_value_controls+=@{name=$edit.Current.Name;enabled=$edit.Current.IsEnabled;read_only=$value.IsReadOnly;fixture_content=($value.Value-eq'Ottrin read-only assistive proof.')};if($value.Value-eq'Ottrin read-only assistive proof.'){Require $value.IsReadOnly 'Preview content is exposed as editable before edit mode'}}
 }
 [PickerNative]::Shot($inspector,(Join-Path $out 'inspector.png'));CloseWindow $inspector;Start-Sleep -Milliseconds 500;Require (-not$main.HasExited) 'Inspector close terminated main';$result.inspector_native_close=$true
 CloseWindow $window;Require ($main.WaitForExit(15000)) 'Native main Close did not exit';$main.WaitForExit();Require ($main.ExitCode-eq0) 'Main native Close exit failed';$result.main_native_close_exit=$main.ExitCode
 Phase 'native chooser UIAutomation view and Close actions'
 $prefBefore=(Get-FileHash $pref).Hash;$start=[Diagnostics.ProcessStartInfo]::new($app,('picker '+($rendererArgs-join' ')));$start.UseShellExecute=$false;$start.RedirectStandardInput=$true;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true;$picker=[Diagnostics.Process]::Start($start);$ownedProcesses+=@($picker);$stdout=$picker.StandardOutput.ReadToEndAsync();$stderr=$picker.StandardError.ReadToEndAsync()
 $request=@{mode='open';title='Accessibility proof';initial_directory=(NativePath $fixture)};$picker.StandardInput.Write(($request|ConvertTo-Json -Depth 5 -Compress));$picker.StandardInput.Close();$pickWindow=AwaitWindow $picker $null;[PickerNative]::Place($pickWindow);[PickerNative]::Focus($pickWindow);$pickTree=Tree $pickWindow 'picker'
 foreach($name in @('Parent folder','Edit location','Grid view','List view','Show hidden files','Show read-only preview','Close window','Cancel')){$null=Named $pickTree $name}
 Invoke $pickTree 'Grid view';[PickerNative]::Shot($pickWindow,(Join-Path $out 'picker-grid.png'));Invoke $pickTree 'List view';[PickerNative]::Shot($pickWindow,(Join-Path $out 'picker-list.png'));CloseWindow $pickWindow
 Require ($picker.WaitForExit(15000)) 'Native picker Close did not exit';$picker.WaitForExit();Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Picker output stayed open';Require ($picker.ExitCode-eq0) ('Picker exit failed: '+$stderr.Result);$response=$stdout.Result|ConvertFrom-Json;Require (-not$response.accepted) 'Close accepted a file';$result.picker_response=$response;$result.picker_native_close_exit=$picker.ExitCode;$result.picker_preferences_preserved=((Get-FileHash $pref).Hash-eq$prefBefore)
 $result.source_preserved=((Get-FileHash $source).Hash-eq$sourceHash);Require ($result.source_preserved-and$result.picker_preferences_preserved) 'Read-only source or chooser preferences changed'
 $finalStatus='pass native Windows UIAutomation observations';Phase 'native Windows observation complete; service teardown follows'
}catch{$finalStatus='failed native UIAutomation gate';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{
 foreach($process in $ownedProcesses){if(-not$process.HasExited){$process.Kill();$process.WaitForExit()};$process.Dispose()}
 if($app-and$mainStarted){
  try{$start=[Diagnostics.ProcessStartInfo]::new($app,'index service-remove');$start.UseShellExecute=$false;$start.CreateNoWindow=$true;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true;$cleanup=[Diagnostics.Process]::Start($start);$co=$cleanup.StandardOutput.ReadToEndAsync();$ce=$cleanup.StandardError.ReadToEndAsync();Require ($cleanup.WaitForExit(30000)) 'Owned service cleanup did not finish';$cleanup.WaitForExit();$result.service_cleanup_exit=$cleanup.ExitCode;$result.service_cleanup_stdout=$co.Result;$result.service_cleanup_stderr=$ce.Result;Require ($cleanup.ExitCode-eq0) 'Owned service teardown failed';$cleanup.Dispose()}catch{$result.cleanup_error=$_.Exception.Message;$teardownFailed=$true}
 }
 $result.status=if($teardownFailed){'failed owned teardown'}else{$finalStatus}
 Save
}
if(-not$result.status.StartsWith('pass')){exit 1}
