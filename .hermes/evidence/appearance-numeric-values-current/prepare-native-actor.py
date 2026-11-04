"""Use semantic names for actual controls; keep DPI/region/visibility gates strict."""
from pathlib import Path
r=Path.cwd();e=Path(__file__).resolve().parent;s=(r/'.hermes/evidence/windows-dpi-current/actor.ps1').read_text()
a=s.index(' function SetCorners(');b=s.index(" Phase 'prepare native large display'",a)
s=s[:a]+r'''
 function SetCorners($editor,$value){
  [PickerNative]::Focus($editor);$window=[DpiDesktop]::Bounds($editor);$visible=$false
  for($scroll=0;$scroll-lt16;$scroll++){
   $numeric=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner);$bounds=$numeric.Current.BoundingRectangle
   if($bounds.Top-gt$window[1]+120-and$bounds.Bottom-lt$window[1]+$window[3]-100){$visible=$true;break}
   [DpiDesktop]::WheelAt([int]($bounds.Left+20),[int]($window[1]+$window[3]/2),-480);Pump
  }
  Require $visible 'Named Window corners control is not visible inside the property viewport'
  $numeric=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner);$before=$numeric.GetCurrentPattern([Windows.Automation.RangeValuePattern]::Pattern).Current.Value;$numeric.SetFocus();$deadline=[DateTime]::UtcNow.AddSeconds(5)
  do{$numeric=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner);if($numeric.Current.HasKeyboardFocus){break};Pump}while([DateTime]::UtcNow-lt$deadline)
  $focused=$numeric.Current.HasKeyboardFocus;Require $focused 'Named numeric control did not acknowledge keyboard focus before typing'
  [Windows.Forms.SendKeys]::SendWait('^a');[Windows.Forms.SendKeys]::SendWait([string]$value);[Windows.Forms.SendKeys]::SendWait('{ENTER}');AwaitCorners $editor $value 'typed commit'
  $report.numeric_actions+=@{action='typed commit';before=$before;expected=$value;keyboard_focus_acknowledged=$focused};Save
 }
 function AwaitCorners($editor,$value,$action){
  $deadline=[DateTime]::UtcNow.AddSeconds(5)
  do{$actual=(Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner)).GetCurrentPattern([Windows.Automation.RangeValuePattern]::Pattern).Current.Value;if($actual-eq$value){return};Pump}while([DateTime]::UtcNow-lt$deadline)
  $focus=[Windows.Automation.AutomationElement]::FocusedElement;$report.numeric_failure=@{action=$action;expected=$value;actual=$actual;focused_name=$focus.Current.Name;focused_type=$focus.Current.ControlType.ProgrammaticName};[PickerNative]::Shot($editor,(Join-Path $out 'numeric-mismatch.png'));Save;throw ('Named native value mismatch after '+$action+': expected='+$value+' actual='+$actual)

 }
''' +s[b:]
marker="  SetCorners $editor 24;CaptureLive $editor 'editor-before-cancel' 24;";assert marker in s;s=s.replace(marker,"  SetCorners $editor 24;Invoke $editor 'Restore Window corners';Start-Sleep -Milliseconds 700;AwaitCorners $editor 0 'named restore';CaptureLive $editor 'editor-after-restore' 0;CaptureLive $settings 'settings-after-restore' 0;CaptureLive $w 'main-after-restore' 0\n"+marker,1)
s=s.replace('Exact published installer at actual Windows144DPI; complete work-area visibility, visible numeric editing/Cancel and all six native window types; Native/OpenGL','Exact current installer at actual Windows144DPI; named visible numeric editing/restore/Cancel, all six native window types and complete work-area capture containment; Native/OpenGL')
# An editor owns foreground input; observe its existing owners without trying
# to click through it. Capture each complete owner separately once reachable.
a=s.index(' function CaptureLive(');b=s.index(' function NativePath(',a)
s=s[:a]+r'''
 function ObserveWindow($handle,$label,$radius,[bool]$capture){
  [PickerNative]::Place($handle);$enabled=[PickerNative]::IsWindowEnabled($handle);$file=$null
  if($capture){[PickerNative]::Focus($handle);$position=[DpiDesktop]::Bounds($handle);[Windows.Forms.Cursor]::Position=[Drawing.Point]::new($position[0]+80,$position[1]+15)}
  Start-Sleep -Milliseconds 700;$dpi=[DpiDesktop]::GetDpiForWindow($handle);$physical=[int]($radius*$dpi/96);$matches=[RadiusProbe]::Matches($handle,$physical);$fits=[DpiDesktop]::Fits($handle)
  if($capture){$file=$renderer+'-'+$label+'-'+$radius+'.png';[PickerNative]::Shot($handle,(Join-Path $out $file))}
  $report.cases+=@{renderer=$renderer;case=$label;logical_radius=$radius;dpi=$dpi;physical_radius=$physical;native_expected_region_match=$matches;window_fits_work_area=$fits;bounds=[DpiDesktop]::Bounds($handle);work_area=[DpiDesktop]::Work($handle);capture=$file;foreground_capture_requested=$capture;window_enabled=$enabled};Save
  Require ($dpi-eq144) ('Expected actual144DPI, got '+$dpi);Require $matches ('DPI-scaled native region mismatch '+$label);Require $fits ('Window extends outside native work area '+$label)
 }
 function CaptureLive($handle,$label,$radius){ObserveWindow $handle $label $radius $true}
 function ObserveOwner($handle,$label,$radius){ObserveWindow $handle $label $radius $false}
'''+s[b:]
for label in ['settings-live','main-live','settings-after-restore','main-after-restore']:
 target='$settings' if label.startswith('settings') else '$w'
 old='CaptureLive '+target+" '"+label+"'";assert old in s;s=s.replace(old,'ObserveOwner '+target+" '"+label+"'")
old="CaptureLive $settings 'settings-after-cancel' 0;CaptureLive $w 'main-after-cancel' 0";assert old in s;s=s.replace(old,"CaptureLive $settings 'settings-after-cancel' 0")
old="Invoke $settings 'Close window';Invoke $w 'Close window'";assert old in s;s=s.replace(old,"Invoke $settings 'Close window';CaptureLive $w 'main-after-cancel' 0;Invoke $w 'Close window'",1)
old="$w=Await $p.Id 'Ottrin';$row=Node";assert old in s
s=s.replace(old,"$w=Await $p.Id 'Ottrin';CaptureLive $w 'main-saved-startup' 24;Invoke $w 'Menu';Invoke $w ('Settings'+[char]0x2026);$settings=Await $p.Id 'Ottrin Settings';CaptureLive $settings 'settings-saved-startup' 24;Invoke $settings 'Close window';$row=Node",1)
s=s.replace('all six native window types and complete work-area capture containment','all six native window types and complete work-area capture containment; live owners use complete native-region observation while editor owns input, with separate unobscured saved-startup captures')

s=s.replace(";cases=@()};$final=",";cases=@();numeric_actions=@()};$final=")
# Reuse the same owned software display for a separate current-source idle
# baseline. This does not waive the existing performance gate or change input.
marker="  Invoke $w 'Menu';Invoke $w ('Settings'+[char]0x2026);$settings=Await $p.Id 'Ottrin Settings'"
assert s.count(marker)==1
s=s.replace(marker,"  MeasureGuiIdle $p $w\n"+marker,1)
start=s.index(" Phase 'prepare native large display'")
s=s[:start]+r'''
 function MeasureGuiIdle($process,$window){
  Phase ($renderer+' separate GUI idle baseline');$deadline=[DateTime]::UtcNow.AddSeconds(60);$attempt=0
  do{$attempt++;$state=Run $app 'index status --json' ('idle-index-'+$renderer+'-'+$attempt);$index=$state.stdout|ConvertFrom-Json;if($index.status-eq'ready'-and$index.watcher_ready){break};Start-Sleep -Milliseconds 500}while([DateTime]::UtcNow-lt$deadline)
  if($index.status-ne'ready'-or-not$index.watcher_ready){$report.gui_idle_baseline+=@{renderer=$renderer;status='unmeasured: index not ready';index=$index};Save;return}
  [PickerNative]::Place($window);[PickerNative]::Focus($window);$area=[DpiDesktop]::Work($window);[Windows.Forms.Cursor]::Position=[Drawing.Point]::new($area[2]-2,$area[3]-2);Start-Sleep -Seconds 5
  $process.Refresh();$before=$process.TotalProcessorTime.TotalSeconds;$rssBefore=$process.WorkingSet64;$gdiBefore=[RadiusProbe]::GetGuiResources($process.Handle,0);$clock=[Diagnostics.Stopwatch]::StartNew();Start-Sleep -Seconds 30;$process.Refresh();$seconds=$clock.Elapsed.TotalSeconds;$cpu=100*($process.TotalProcessorTime.TotalSeconds-$before)/$seconds;$rssAfter=$process.WorkingSet64;$gdiAfter=[RadiusProbe]::GetGuiResources($process.Handle,0)
  $after=(Run $app 'index status --json' ('idle-index-'+$renderer+'-after')).stdout|ConvertFrom-Json;$idle=$after.status-eq'ready'-and$after.watcher_ready
  $report.gui_idle_baseline+=@{renderer=$renderer;status=if(-not$idle){'unverified: index state changed'}elseif($cpu-lt2){'pass current-scope criterion'}else{'failed current-scope criterion'};criterion='StaticGUIbelow2percentonecore';cpu_one_core_percent=$cpu;seconds=$seconds;dpi=[DpiDesktop]::GetDpiForWindow($window);bounds=[DpiDesktop]::Bounds($window);work_area=$area;rss_before=$rssBefore;rss_after=$rssAfter;gdi_before=$gdiBefore;gdi_after=$gdiAfter;index_before=$index;index_after=$after;scope='Same signed VirtIO/software fixture and exact current package; Dark144DPI, initial main radius0. GUI process only; no daemon CPU sample, physical or cross-source comparison.'};Save
 }
'''+s[start:]
s=s.replace(';numeric_actions=@()};$final=',';numeric_actions=@();gui_idle_baseline=@()};$final=')

# Invoke is asynchronous. Wait for the closed native viewport to disappear
# before attempting to focus its owner; never click through a closing window.
marker=' [DllImport("user32.dll")]static extern bool PostMessageW('
assert marker in s
s=s.replace(marker,' [DllImport("user32.dll")]public static extern bool IsWindowVisible(IntPtr window);\n'+marker,1)
marker=' function CaptureLive('
i=s.index(marker)
s=s[:i]+r'''
 function AwaitClosed($handle){
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  do{if(-not[DpiDesktop]::IsWindowVisible($handle)){return};Pump}while([DateTime]::UtcNow-lt$deadline)
  [PickerNative]::Shot($handle,(Join-Path $out 'close-not-completed.png'));throw 'Invoked Close window did not close its native viewport'
 }
 function CloseOwned($handle){Invoke $handle 'Close window';AwaitClosed $handle}
'''+s[i:]
s=s.replace("Invoke $settings 'Close window'",'CloseOwned $settings')
s=s.replace("Invoke $inspector 'Close window'",'CloseOwned $inspector')
s=s.replace("Invoke $editor 'Cancel';Start-Sleep -Milliseconds 700", "Invoke $editor 'Cancel';AwaitClosed $editor")
s=s.replace('  [PickerNative]::Place($handle);$enabled=', '  Phase ($renderer+\' observe \'+$label+\' radius\'+$radius);[PickerNative]::Place($handle);$enabled=')

(e/'native-actor.ps1').write_text(s)
