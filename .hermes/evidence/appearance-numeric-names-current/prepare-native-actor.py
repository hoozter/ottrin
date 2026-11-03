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
(e/'native-actor.ps1').write_text(s)
