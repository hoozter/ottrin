from pathlib import Path
r=Path.cwd();e=Path(__file__).resolve().parent
s=(r/'.hermes/evidence/windows-rounded-resize-current/live-radius-actor.ps1').read_text();cs=(e/'desktop-probe.cs').read_text()
a=s.index(' function Await(')
s=s[:a]+" Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing @'\n"+cs+"\n'@\n"+s[a:]
a=s.index(' function CaptureLive(');b=s.index(' function SetCorners(',a)
s=s[:a]+r'''
 function CaptureLive($handle,$label,$radius){
  [PickerNative]::Place($handle);[PickerNative]::Focus($handle);Start-Sleep -Milliseconds 700;$dpi=[DpiDesktop]::GetDpiForWindow($handle);$physical=[int]($radius*$dpi/96);$matches=[RadiusProbe]::Matches($handle,$physical);$fits=[DpiDesktop]::Fits($handle);$capture=$renderer+'-'+$label+'-'+$radius+'.png';[PickerNative]::Shot($handle,(Join-Path $out $capture));$report.cases+=@{renderer=$renderer;case=$label;logical_radius=$radius;dpi=$dpi;physical_radius=$physical;native_expected_region_match=$matches;window_fits_work_area=$fits;bounds=[DpiDesktop]::Bounds($handle);work_area=[DpiDesktop]::Work($handle);capture=$capture};Save;Require ($dpi-eq144) ('Expected actual144DPI, got '+$dpi);Require $matches ('DPI-scaled native region mismatch '+$label);Require $fits ('Window extends outside native work area '+$label)
 }
 function NativePath($path){return @{encoding='windows-utf16-le';units_hex=(-join $path.ToCharArray().ForEach({([int]$_).ToString('x4')}))}}
''' +s[b:]
# Scroll the actual right-hand property list and require the numeric control to be
# inside the captured window before typing. This does not accept hidden controls.
s=s.replace("$label=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Text);$bounds=$label.Current.BoundingRectangle",r'''
  [PickerNative]::Focus($editor);$window=[DpiDesktop]::Bounds($editor);$visible=$false
  for($scroll=0;$scroll-lt16;$scroll++){
   $label=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Text);$bounds=$label.Current.BoundingRectangle
   if($bounds.Top-gt$window[1]+120-and$bounds.Bottom-lt$window[1]+$window[3]-100){$visible=$true;break}
   [DpiDesktop]::WheelAt([int]($bounds.Left+20),[int]($window[1]+$window[3]/2),-480);Pump
  }
  Require $visible 'Window corners label is not visibly inside the property viewport'
''')
s=s.replace("$numeric[0].SetFocus();", "$controlBounds=$numeric[0].Current.BoundingRectangle;Require ($controlBounds.Top-gt$window[1]+120-and$controlBounds.Bottom-lt$window[1]+$window[3]-100) 'Numeric control is clipped';$numeric[0].SetFocus();")
s=s.replace("[RadiusProbe]::Resize($editor,1180,760)","[RadiusProbe]::Resize($editor,1770,1140)")
# Pause only on the owned IPC file while the host observes and changes the normal
# Settings UI. The product windows must prove actual144DPI after continuation.
a=s.index(" foreach($renderer in @('Native','OpenGL')){")
s=s[:a]+r'''
 Phase 'prepare native large display';$report.display_modes=[DpiDesktop]::Modes();Save;$report.display_mode=[DpiDesktop]::LargeMode();Save

 $null=Start-Process 'ms-settings:display';$deadline=[DateTime]::UtcNow.AddSeconds(90);$systemWindow=[IntPtr]::Zero;$settingsCandidates=@();$actorSession=[Diagnostics.Process]::GetCurrentProcess().SessionId
 do{
  $hosts=@(Get-Process SystemSettings,ApplicationFrameHost -ErrorAction SilentlyContinue|Where-Object {$_.SessionId-eq$actorSession});$ids=@($hosts|ForEach-Object {$_.Id});$nameCondition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Settings')
  $settingsCandidates=@([Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children,$nameCondition)|Where-Object {$_.Current.ProcessId-in$ids-and-not$_.Current.IsOffscreen-and$_.Current.NativeWindowHandle-ne0})
  if($settingsCandidates.Count-eq1){$systemWindow=[IntPtr]::new([int64]$settingsCandidates[0].Current.NativeWindowHandle);$report.settings_host=@{pid=$settingsCandidates[0].Current.ProcessId;type=$settingsCandidates[0].Current.ControlType.ProgrammaticName;title=$settingsCandidates[0].Current.Name};Save;break};Pump
 }while([DateTime]::UtcNow-lt$deadline)
 if($systemWindow-eq[IntPtr]::Zero){$report.settings_discovery=@{hosts=@($hosts|Select-Object Name,Id,SessionId);visible_top_windows=@([Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children,[Windows.Automation.Condition]::TrueCondition)|ForEach-Object {@{name=$_.Current.Name;pid=$_.Current.ProcessId;type=$_.Current.ControlType.ProgrammaticName;native_handle=$_.Current.NativeWindowHandle}})};Save}
 Require ($systemWindow-ne[IntPtr]::Zero) 'Native Windows Display Settings did not open';[PickerNative]::Place($systemWindow);[PickerNative]::Focus($systemWindow);[PickerNative]::Shot($systemWindow,(Join-Path $out 'windows-display-settings.png'))
 $nodes=@((Tree $systemWindow).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|Where-Object {-not$_.Current.IsOffscreen}|ForEach-Object {@{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;bounds=$_.Current.BoundingRectangle.ToString()}});[IO.File]::WriteAllText((Join-Path $out 'windows-display-tree.json'),($nodes|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))

 Phase 'select normal Windows150percent Scale';$scaleCombo=Node $systemWindow 'Scale' ([Windows.Automation.ControlType]::ComboBox);$settingsPid=$scaleCombo.Current.ProcessId;$scaleCombo.GetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern).Expand();Pump
 $deadline=[DateTime]::UtcNow.AddSeconds(20);$choice=@()
 do{$condition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'150%');$choice=@([Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)|Where-Object {$_.Current.ProcessId-eq$settingsPid-and$_.Current.ControlType-eq[Windows.Automation.ControlType]::ListItem-and-not$_.Current.IsOffscreen});if($choice.Count-ne1){Pump}}while($choice.Count-ne1-and[DateTime]::UtcNow-lt$deadline)
 Require ($choice.Count-eq1) 'Native150percent option not uniquely visible';$choice[0].GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select();$deadline=[DateTime]::UtcNow.AddSeconds(20)
 while([DpiDesktop]::GetDpiForWindow($systemWindow)-ne144){Require ([DateTime]::UtcNow-lt$deadline) 'Native Settings did not reach144DPI';Pump}
 [DpiDesktop]::PerMonitor();[PickerNative]::Shot($systemWindow,(Join-Path $out 'windows-display-settings-150.png'));$report.dpi_setup=@{method='Native Scale ComboBox Expand, visible150percent ListItem Select';actual_settings_dpi=[DpiDesktop]::GetDpiForWindow($systemWindow);selected_label='150%'};Save
 [DpiDesktop]::Close($systemWindow);Start-Sleep -Milliseconds 500
''' +s[a:]
s=s.replace("$config.graphics_renderer=if($renderer", "$config.window_size=@(1000,700);$config.graphics_renderer=if($renderer",1)
# Keep default saved0 and Cancel semantics. A separate seeded24 saved-theme launch
# checks all other window kinds at the same real DPI without repeating live edits.
a=s.index(" $saved=(Get-FileHash $pref).Hash;Phase 'supported uninstall'")
s=s[:a]+r'''
 foreach($renderer in @('Native','OpenGL')){
  Phase ($renderer+'144DPI other native windows');$config.graphics_renderer=if($renderer-eq'Native'){'Native'}else{'OpenGlCompatibility'};$config.custom_themes[0].dark.app_radius=24;$config.custom_themes[0].light.app_radius=24;$rendererArgs=if($renderer-eq'OpenGL'){' --renderer opengl'}else{''};[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
  $s=[Diagnostics.ProcessStartInfo]::new($app,('"'+$corpus+'"'+$rendererArgs));$s.UseShellExecute=$false;$p=[Diagnostics.Process]::Start($s);$owned.Add($p);$w=Await $p.Id 'Ottrin';$row=Node $w 'preserve.txt' ([Windows.Automation.ControlType]::Button);$toggle=$row.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern);if($toggle.Current.ToggleState-ne[Windows.Automation.ToggleState]::On){$toggle.Toggle()};Start-Sleep -Milliseconds 500
  Invoke $w 'Menu';Invoke $w 'Quick Inspector Selection';$inspector=Await $p.Id ('Ottrin Quick Inspector '+[char]0x2014+' preserve.txt');CaptureLive $inspector 'inspector-scaled' 24;Invoke $inspector 'Close window';Invoke $w 'Close window';Require ($p.WaitForExit(15000)-and$p.ExitCode-eq0) 'Normal main close failed'
  $s=[Diagnostics.ProcessStartInfo]::new($app,('picker'+$rendererArgs));$s.UseShellExecute=$false;$s.RedirectStandardInput=$true;$s.RedirectStandardOutput=$true;$s.RedirectStandardError=$true;$picker=[Diagnostics.Process]::Start($s);$owned.Add($picker);$stdout=$picker.StandardOutput.ReadToEndAsync();$stderr=$picker.StandardError.ReadToEndAsync();$picker.StandardInput.Write((@{mode='open';title='DPI probe';initial_directory=(NativePath $corpus)}|ConvertTo-Json -Depth 5 -Compress));$picker.StandardInput.Close();$pickWindow=Await $picker.Id ('Ottrin '+[char]0x2014+' DPI probe');CaptureLive $pickWindow 'picker-scaled' 24;Invoke $pickWindow 'Cancel';Require ($picker.WaitForExit(15000)-and$picker.ExitCode-eq0) 'Picker cancel failed';Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Picker streams incomplete';Require (-not($stdout.Result|ConvertFrom-Json).accepted) 'Picker cancel accepted'
  $s=[Diagnostics.ProcessStartInfo]::new($app,('--properties "'+$source+'"'+$rendererArgs));$s.UseShellExecute=$false;$properties=[Diagnostics.Process]::Start($s);$owned.Add($properties);$propWindow=Await $properties.Id ('Ottrin '+[char]0x2014+' Properties '+[char]0x2014+' preserve.txt');CaptureLive $propWindow 'properties-scaled' 24;Invoke $propWindow 'Close window';Require ($properties.WaitForExit(15000)-and$properties.ExitCode-eq0) 'Properties close failed'
 }
''' +s[a:]
s=s.replace('Exact installed native live editor radius and Cancel propagation;0/24radius,Native/OpenGL; source/preferences preservation','Exact published installer at actual Windows144DPI; complete work-area visibility, visible numeric editing/Cancel and all six native window types; Native/OpenGL').replace('pass native live radius propagation and Cancel; originalsrequire review','pass native144DPI shape/visibility/live editing; originalsrequire review')
s=s.replace("$final='failed native pointer-resize audit'","$final='failed native DPI/layout audit'")
(e/'actor.ps1').write_text(s)
