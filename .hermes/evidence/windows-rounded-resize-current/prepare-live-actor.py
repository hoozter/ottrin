from pathlib import Path
r=Path.cwd();e=Path(__file__).resolve().parent
s=(e/'pointer-actor.ps1').read_text();old=(r/'.hermes/evidence/windows-window-shape-fix-current/native-complete/actor.ps1').read_text()
a=old.index(" Add-Type @'\nusing System;using System.Runtime.InteropServices;");b=old.index(' function NativePath',a);probe=old[a:b]
a=s.index(' Add-Type -ReferencedAssemblies');b=s.index(' function Await',a);s=s[:a]+probe+s[b:]
a=s.index(' function Gesture(');b=s.index(" $saved=(Get-FileHash $pref).Hash",a)
s=s[:a]+r'''
 function CaptureLive($handle,$label,$radius){[PickerNative]::Focus($handle);Start-Sleep -Milliseconds 500;$matches=[RadiusProbe]::Matches($handle,$radius);$capture=$renderer+'-'+$label+'-'+$radius+'.png';[PickerNative]::Shot($handle,(Join-Path $out $capture));$report.cases+=@{renderer=$renderer;case=$label;radius=$radius;native_expected_region_match=$matches;native_region=[RadiusProbe]::Region($handle);capture=$capture};Save;Require $matches ('Live radius mismatch '+$label+' expected '+$radius)}
 function SetCorners($editor,$value){
  $label=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Text);$bounds=$label.Current.BoundingRectangle
  $all=@((Tree $editor).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|Where-Object {-not$_.Current.IsOffscreen})
  $numeric=@($all|Where-Object {$b=$_.Current.BoundingRectangle;($_.Current.ControlType-eq[Windows.Automation.ControlType]::Spinner-or$_.Current.ControlType-eq[Windows.Automation.ControlType]::Edit)-and$b.Left-ge$bounds.Right-and[Math]::Abs(($b.Top+$b.Height/2)-($bounds.Top+$bounds.Height/2))-lt16})
  if($numeric.Count-ne1){$all|ForEach-Object {@{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;bounds=$_.Current.BoundingRectangle.ToString()}}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $out 'numeric-tree.json') -Encoding UTF8;throw ('Window corners numeric control count='+$numeric.Count)}
  $numeric[0].SetFocus();[Windows.Forms.SendKeys]::SendWait('^a');[Windows.Forms.SendKeys]::SendWait([string]$value);[Windows.Forms.SendKeys]::SendWait('{ENTER}');Start-Sleep -Milliseconds 700;Pump
 }
 foreach($renderer in @('Native','OpenGL')){
  Phase ($renderer+' live editor radius');$config.theme='Dark';$config.graphics_renderer=if($renderer-eq'Native'){'Native'}else{'OpenGlCompatibility'};$config.active_custom_theme='Radius audit';$config.custom_themes=@(@{name='Radius audit';base_preset='Ottrin';dark=@{app_radius=0};light=@{app_radius=0}});$rendererArgs=if($renderer-eq'OpenGL'){' --renderer opengl'}else{''};[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
  $s=[Diagnostics.ProcessStartInfo]::new($app,('"'+$corpus+'"'+$rendererArgs));$s.UseShellExecute=$false;$p=[Diagnostics.Process]::Start($s);$owned.Add($p);$w=Await $p.Id 'Ottrin';$null=Node $w 'Menu' ([Windows.Automation.ControlType]::Button)
  Invoke $w 'Menu';Invoke $w ('Settings'+[char]0x2026);$settings=Await $p.Id 'Ottrin Settings'
  (Node $settings 'Appearance' ([Windows.Automation.ControlType]::RadioButton)).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select();Pump
  $buttons=@();for($i=0;$i-lt12;$i++){$buttons=@(Nodes $settings 'Edit Radius audit'|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::Button});if($buttons.Count-eq1){break};[PickerNative]::Wheel($settings,-480);Pump};Require ($buttons.Count-eq1) 'Saved theme edit not visible';$buttons[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();$editor=Await $p.Id 'Ottrin Theme Editor';[RadiusProbe]::Resize($editor,1180,760);Pump
  foreach($radius in @(24,0)){SetCorners $editor $radius;CaptureLive $editor 'editor-live' $radius;CaptureLive $settings 'settings-live' $radius;CaptureLive $w 'main-live' $radius}
  SetCorners $editor 24;CaptureLive $editor 'editor-before-cancel' 24;Invoke $editor 'Cancel';Start-Sleep -Milliseconds 700;CaptureLive $settings 'settings-after-cancel' 0;CaptureLive $w 'main-after-cancel' 0
  Invoke $settings 'Close window';Invoke $w 'Close window';Require ($p.WaitForExit(15000)-and$p.ExitCode-eq0) 'Normal close failed';$savedConfig=Get-Content $pref -Raw|ConvertFrom-Json;Require ($savedConfig.custom_themes[0].dark.app_radius-eq0) 'Cancel persisted unsaved radius';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed';Save
 }
''' +s[b:]
s=s.replace('Exact installed native pointer resize at visible rounded arcs and exclusive titlebar ownership;0/24radius,Native/OpenGL; source/preferences preservation','Exact installed native live editor radius and Cancel propagation;0/24radius,Native/OpenGL; source/preferences preservation').replace('pass native installed pointer resize andexclusive titlebarownership; originalsrequire review','pass native live radius propagation and Cancel; originalsrequire review')
(e/'live-radius-actor.ps1').write_text(s)
