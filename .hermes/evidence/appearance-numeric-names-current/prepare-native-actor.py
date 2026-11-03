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
  $numeric=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner);$numeric.SetFocus();[Windows.Forms.SendKeys]::SendWait('^a');[Windows.Forms.SendKeys]::SendWait([string]$value);[Windows.Forms.SendKeys]::SendWait('{ENTER}');Start-Sleep -Milliseconds 700;Pump
  $actual=(Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner)).GetCurrentPattern([Windows.Automation.RangeValuePattern]::Pattern).Current.Value;Require ($actual-eq$value) 'Named native value did not match typed value'
 }
''' +s[b:]
marker="  SetCorners $editor 24;CaptureLive $editor 'editor-before-cancel' 24;";assert marker in s;s=s.replace(marker,"  SetCorners $editor 24;Invoke $editor 'Restore Window corners';Start-Sleep -Milliseconds 700;$restored=(Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner)).GetCurrentPattern([Windows.Automation.RangeValuePattern]::Pattern).Current.Value;Require ($restored-eq0) 'Named restore did not restore opening value';CaptureLive $editor 'editor-after-restore' 0;CaptureLive $settings 'settings-after-restore' 0;CaptureLive $w 'main-after-restore' 0\n"+marker,1)
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

(e/'native-actor.ps1').write_text(s)
