 function RevealText($window,$name){
  for($scroll=0;$scroll-lt16;$scroll++){if(@(VisibleText $window $name).Count-eq1){return};[PickerNative]::Wheel($window,-480);Pump}
  throw ('Text is not visible in native viewport: '+$name)
 }
 function TypeCorners($editor,$value){
  [PickerNative]::Focus($editor);$b=[DpiDesktop]::Bounds($editor)
  for($scroll=0;$scroll-lt16;$scroll++){
   $n=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner);$bounds=$n.Current.BoundingRectangle
   if($bounds.Top-gt$b[1]+100-and$bounds.Bottom-lt$b[1]+$b[3]-100){break}
   [DpiDesktop]::WheelAt([int]($bounds.Left+20),[int]($b[1]+$b[3]/2),-480);Pump
  }
  Require ($bounds.Top-gt$b[1]+100-and$bounds.Bottom-lt$b[1]+$b[3]-100) 'Numeric property is outside the native viewport'
  $n.SetFocus();$deadline=[DateTime]::UtcNow.AddSeconds(5)
  do{$n=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner);if($n.Current.HasKeyboardFocus){break};Pump}while([DateTime]::UtcNow-lt$deadline)
  Require $n.Current.HasKeyboardFocus 'Numeric control did not acknowledge focus'
  [Windows.Forms.SendKeys]::SendWait('^a');[Windows.Forms.SendKeys]::SendWait([string]$value);[Windows.Forms.SendKeys]::SendWait('{ENTER}')
  $deadline=[DateTime]::UtcNow.AddSeconds(5)
  do{$actual=(Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner)).GetCurrentPattern([Windows.Automation.RangeValuePattern]::Pattern).Current.Value;if($actual-eq$value){return};Pump}while([DateTime]::UtcNow-lt$deadline)
  throw 'Typed numeric value did not commit'
 }
 Phase 'malformed settings reject CLI and daemon without changing bytes'
 [IO.File]::WriteAllText($pref,'{ invalid owned settings',[Text.UTF8Encoding]::new($false));$invalidHash=(Get-FileHash $pref).Hash
 foreach($probe in @(@{exe=$app;args='index status --json';name='invalid-cli'},@{exe=(Join-Path $install 'bin\ottrin-indexd.exe');args='';name='invalid-daemon'})){
  $failure=Run $probe.exe $probe.args $probe.name 1
  Require ($failure.stderr.Contains($pref)-and$failure.stderr.Contains('preserved')) 'Failure did not identify preserved settings'
  Require ((Get-FileHash $pref).Hash-eq$invalidHash) 'Invalid settings changed'
  $report.cases+=@{case=$probe.name;exit=$failure.exit;original_bytes_preserved=$true};Save
 }
 Phase 'native startup error is visible and dismissible'
 $info=[Diagnostics.ProcessStartInfo]::new($app);$info.UseShellExecute=$false;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$p=[Diagnostics.Process]::Start($info);$owned.Add($p);$stdout=$p.StandardOutput.ReadToEndAsync();$stderr=$p.StandardError.ReadToEndAsync()
 $errorWindow=Await $p.Id 'Ottrin startup error';[PickerNative]::Place($errorWindow);[PickerNative]::Focus($errorWindow)
 $texts=@((Tree $errorWindow).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|Where-Object {$_.Current.Name.Contains('Could not load Ottrin settings')-and$_.Current.Name.Contains('preserved')})
 Require ($texts.Count-ge1) 'Native startup dialog did not explain preserved settings';$visibleError=$texts[0].Current.Name;[PickerNative]::Shot($errorWindow,(Join-Path $out 'invalid-settings-native.png'))
 $report.error_dialog_controls=@((Tree $errorWindow).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|ForEach-Object {@{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;offscreen=$_.Current.IsOffscreen}});Save
 # This is a standard Win32 message box. Use its focused default button's
 # native Enter action rather than the egui-specific exact-name helper.
 Key $errorWindow '{ENTER}';Require ($p.WaitForExit(15000)-and$p.ExitCode-eq1) 'Dismissed startup failure did not exit with failure';Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Startup error streams did not close';Require ((Get-FileHash $pref).Hash-eq$invalidHash) 'Startup changed invalid settings'
 $report.cases+=@{case='native invalid-settings startup dialog';visible_error=$visibleError;exit=1;original_bytes_preserved=$true};Save
 foreach($renderer in @('Native','OpenGL')){
  Phase ($renderer+' initialize valid settings and native editor')
  $config.theme='Dark';$config.window_size=@(1000,700);$config.graphics_renderer=if($renderer-eq'Native'){'Native'}else{'OpenGlCompatibility'};$config.active_custom_theme='Settings audit';$config.custom_themes=@(@{name='Settings audit';base_preset='Ottrin';dark=@{app_radius=0};light=@{app_radius=0}});$rendererArgs=if($renderer-eq'OpenGL'){' --renderer opengl'}else{''};[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
  $p=[Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]::new($app,('"'+$corpus+'"'+$rendererArgs)));$owned.Add($p);$w=Await $p.Id 'Ottrin';$null=Node $w 'Menu' ([Windows.Automation.ControlType]::Button)
  Invoke $w 'Menu';Invoke $w ('Settings'+[char]0x2026);$settings=Await $p.Id 'Ottrin Settings';[PickerNative]::Focus($settings);(Node $settings 'Appearance' ([Windows.Automation.ControlType]::RadioButton)).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select();Pump
  $edit=Node $settings 'Edit Settings audit' ([Windows.Automation.ControlType]::Button);$b=[DpiDesktop]::Bounds($settings)
  for($scroll=0;$scroll-lt16;$scroll++){$bounds=$edit.Current.BoundingRectangle;if($bounds.Top-gt$b[1]+75-and$bounds.Bottom-lt$b[1]+$b[3]-75){break};[PickerNative]::Wheel($settings,-480);Pump;$edit=Node $settings 'Edit Settings audit' ([Windows.Automation.ControlType]::Button)}
  Require ($bounds.Top-gt$b[1]+75-and$bounds.Bottom-lt$b[1]+$b[3]-75) 'Theme edit action not visibly inside settings';$edit.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();$editor=Await $p.Id 'Ottrin Theme Editor';[PickerNative]::Place($editor);TypeCorners $editor 24
  Phase ($renderer+' actual theme save refusal retains the draft')
  $before=(Get-FileHash $pref).Hash;$originalAttributes=[IO.File]::GetAttributes($pref);$readonlyConfig=$pref;[IO.File]::SetAttributes($pref,($originalAttributes-bor[IO.FileAttributes]::ReadOnly));Invoke $editor 'Save changes'
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  do{$errors=@((Tree $editor).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|Where-Object {$_.Current.Name.Contains('Could not save Ottrin settings')-and$_.Current.Name.Contains('read-only')});if($errors.Count){break};Pump}while([DateTime]::UtcNow-lt$deadline)
  Require ($errors.Count-ge1-and[DpiDesktop]::IsWindowVisible($editor)) 'Save error/draft not retained in native editor';Require ((Get-FileHash $pref).Hash-eq$before) 'Failed save changed original preferences';Require ((Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner)).GetCurrentPattern([Windows.Automation.RangeValuePattern]::Pattern).Current.Value-eq24) 'Draft numeric value was lost'
  [PickerNative]::Shot($editor,(Join-Path $out ($renderer+'-save-refused.png')));$report.cases+=@{renderer=$renderer;case='native failed theme save retains draft';radius=24;original_bytes_preserved=$true;error=$errors[0].Current.Name};Save
  [IO.File]::SetAttributes($pref,$originalAttributes);$readonlyConfig=$null;Invoke $editor 'Save changes';$deadline=[DateTime]::UtcNow.AddSeconds(15)
  while([DpiDesktop]::IsWindowVisible($editor)){Require ([DateTime]::UtcNow-lt$deadline) 'Successful retry did not close editor';Pump}
  $saved=Get-Content $pref -Raw|ConvertFrom-Json;Require ($saved.custom_themes[0].dark.app_radius-eq24-and$saved.custom_themes[0].light.app_radius-eq0) 'Retry did not persist retained draft';[PickerNative]::Focus($settings);[PickerNative]::Shot($settings,(Join-Path $out ($renderer+'-save-retried.png')));$report.cases+=@{renderer=$renderer;case='native repaired save retry persisted';dark_radius=24;light_radius=0};Save
  Phase ($renderer+' actual Search backend and login status')
  (Node $settings 'Search' ([Windows.Automation.ControlType]::RadioButton)).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select();Pump;RevealText $settings 'Start index at login';$null=Node $settings 'active in the index daemon' ([Windows.Automation.ControlType]::Text);Require (@(Nodes $settings 'active while Ottrin is running').Count-eq0) 'Stale watcher lifetime wording remains'
  [PickerNative]::Shot($settings,(Join-Path $out ($renderer+'-search-health.png')));$report.cases+=@{renderer=$renderer;case='native Search separates daemon runtime and login';dpi=[DpiDesktop]::GetDpiForWindow($settings)};Save
  if($renderer-eq'OpenGL'){RunServicesAudit $settings}
  CloseOwned $settings;[PickerNative]::Focus($w);Invoke $w 'Close window';Require ($p.WaitForExit(15000)-and$p.ExitCode-eq0) 'Normal close failed';$saved=Get-Content $pref -Raw|ConvertFrom-Json;Require ($saved.default_view_mode-eq'List'-and$saved.custom_themes[0].dark.app_radius-eq24) 'Saved preferences changed on close';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed'
 }
 $saved=(Get-FileHash $pref).Hash;Phase 'supported uninstall';$uninstaller=Join-Path $out 'owned-uninstall.exe';Copy-Item (Join-Path $install 'Uninstall.exe') $uninstaller;$null=Run $uninstaller ('/S _?='+$install) 'uninstall';Remove-Item $uninstaller;Require (-not(Test-Path $install)) 'Installation remains';Require ((Get-FileHash $pref).Hash-eq$saved) 'Uninstall changed preferences';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed';$report.preferences_preserved=$true;$report.source_preserved=$true;$final='pass native settings rejection/save-retry and Search status workflow'
}catch{
 $report.error=$_.Exception.Message;$report.stack=$_.ScriptStackTrace
 if($settings-and[DpiDesktop]::IsWindowVisible($settings)){
  try{[PickerNative]::Shot($settings,(Join-Path $out 'failure-settings.png'));$report.failure_controls=@((Tree $settings).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|ForEach-Object {@{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;enabled=$_.Current.IsEnabled;offscreen=$_.Current.IsOffscreen}})}catch{$report.failure_capture_error=$_.Exception.Message}
 }
 try{$task=OwnedIndexTask;$report.failure_task=if($task){@{name=$task.Name;enabled=$task.Enabled;state=$task.State;last_result=$task.LastTaskResult;instances=$task.GetInstances(0).Count}}else{'absent'}}catch{$report.failure_task_error=$_.Exception.Message}
 Save
}
finally{
 if($readonlyConfig){try{[IO.File]::SetAttributes($readonlyConfig,$originalAttributes)}catch{$report.permissions_cleanup_error=$_.Exception.Message;$final='failed owned config permissions cleanup'}}
 foreach($p in $owned){if(-not$p.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}}
 if($install-and(Test-Path $app)-and$manifest-and(Get-FileHash $app).Hash.ToLowerInvariant()-eq$manifest.'bin/ottrin.exe'){try{$report.cleanup_index_remove=Run $app 'index service-remove' 'cleanup-index-remove'}catch{$report.cleanup_index_error=$_.Exception.Message;$final='failed owned index teardown'}}
 if($install){$left=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($install,[StringComparison]::OrdinalIgnoreCase)});$report.remaining_owned_installed_processes=$left.Count;if($left.Count){$final='failed owned process teardown'}}
 $report.status=$final;Save
}
if(-not$final.StartsWith('pass')){exit 1}
