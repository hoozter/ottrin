$ErrorActionPreference='Stop';$result=@{status='running';cases=@{}};$owned=@();$work=Join-Path $env:USERPROFILE 'PreviewPackageQA';$binary='C:\OttrinContentQA\payload\Ottrin-0.1.0-windows-x64\bin\ottrin.exe';$b='C:\OttrinContentQA';$r="$b\results"
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;using System.Drawing;using System.Drawing.Imaging;using System.Runtime.InteropServices;
public static class PreviewCapture {
 private delegate bool Visitor(IntPtr h,IntPtr l);
 [DllImport("user32.dll")] private static extern bool EnumWindows(Visitor v,IntPtr l);
 [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr h,System.Text.StringBuilder b,int n);
 public static string Title(IntPtr h){var b=new System.Text.StringBuilder(512);GetWindowText(h,b,b.Capacity);return b.ToString();}
 public static IntPtr Find(int pid,string label){IntPtr found=IntPtr.Zero;EnumWindows((h,l)=>{uint p;GetWindowThreadProcessId(h,out p);if(p==(uint)pid&&IsWindowVisible(h)&&Title(h).IndexOf(label,StringComparison.OrdinalIgnoreCase)>=0){found=h;return false;}return true;},IntPtr.Zero);return found;}

 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h,int x,int y,int w,int z,bool repaint);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 public struct RECT{public int left,top,right,bottom;}
 public static int Capture(IntPtr h,string path,bool green,bool eps){RECT r;if(!GetWindowRect(h,out r))throw new Exception("No window rectangle");using(var image=new Bitmap(r.right-r.left,r.bottom-r.top,PixelFormat.Format32bppArgb)){
 using(var g=Graphics.FromImage(image)){g.CopyFromScreen(r.left,r.top,0,0,image.Size);}image.Save(path,ImageFormat.Png);
  var data=image.LockBits(new Rectangle(0,0,image.Width,image.Height),ImageLockMode.ReadOnly,PixelFormat.Format32bppArgb);try{
 int stride=Math.Abs(data.Stride);byte[] bytes=new byte[stride*image.Height];Marshal.Copy(data.Scan0,bytes,0,bytes.Length);
 int mincx=image.Width,mincy=image.Height,maxcx=-1,maxcy=-1;
 if(!green){int[] cols=new int[image.Width],rows=new int[image.Height];for(int y=0;y<image.Height;y++)for(int x=0;x<image.Width;x++){int i=y*stride+x*4;int b=bytes[i],g=bytes[i+1],r0=bytes[i+2];if(Math.Min(r0,Math.Min(g,b))>235&&Math.Max(r0,Math.Max(g,b))-Math.Min(r0,Math.Min(g,b))<12){cols[x]++;rows[y]++;}}for(int x=0;x<cols.Length;x++)if(cols[x]>150){mincx=Math.Min(mincx,x);maxcx=x;}for(int y=0;y<rows.Length;y++)if(rows[y]>150){mincy=Math.Min(mincy,y);maxcy=y;}if(maxcx-mincx<200||maxcy-mincy<200)return 0;}
 int redPixels=0;int n=0,minx=int.MaxValue,miny=int.MaxValue,maxx=0,maxy=0;
 for(int y=0;y<image.Height;y++)for(int x=0;x<image.Width;x++){if(!green&&(x<mincx||x>maxcx||y<mincy||y>maxcy))continue;int i=y*stride+x*4;int b=bytes[i],g=bytes[i+1],red=bytes[i+2];if(eps&&red>140&&g<80&&b<90)redPixels++;if(green?(g>140&&red<60&&b<140):(red+20<g&&red+20<b&&red<220&&g>70&&b>100)){n++;minx=Math.Min(minx,x);miny=Math.Min(miny,y);maxx=Math.Max(maxx,x);maxy=Math.Max(maxy,y);}}
 return green?((eps&&redPixels<1000)?0:n):((n>80&&maxx-minx>.65*(maxcx-mincx)&&maxy-miny>.55*(maxcy-mincy))?n:0);
 }finally{image.UnlockBits(data);}}

 }
}
'@
try {
 if($env:USERNAME-ne'OttrinContentQA'){throw 'Wrong account'};$identity=[Security.Principal.WindowsIdentity]::GetCurrent();if(([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Elevated GUI actor'}
 if(Test-Path $work){throw 'Owned GUI fixture exists'};New-Item -ItemType Directory $work|Out-Null
 Copy-Item "$b\model.ply","$b\composite.afphoto","$b\drawing.eps" $work
 $config=Get-Content "$b\gui-config.json" -Raw|ConvertFrom-Json;$config.search.include_roots=@($work);$config.window_size=@(1000,680);$pref="$env:APPDATA\ottrin\config.json";New-Item -ItemType Directory -Force (Split-Path $pref)|Out-Null;[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 25),[Text.UTF8Encoding]::new($false))
 $env:OTTRIN_RENDERER='opengl';$result.ordinary_user=$true;$result.screen=[Windows.Forms.Screen]::PrimaryScreen.Bounds.ToString()
 foreach($case in @(@{label='ply';name='model.ply'},@{label='affinity';name='composite.afphoto'},@{label='eps';name='drawing.eps'})){
  [Windows.Forms.SendKeys]::SendWait('{ESC}');$file=Join-Path $work $case.name;$before=(Get-FileHash $file).Hash
  $p=Start-Process $binary -ArgumentList "preview `"$file`"" -PassThru -RedirectStandardOutput "$r\GUI_$($case.label).stdout" -RedirectStandardError "$r\GUI_$($case.label).stderr";$null=$p.Handle;$owned+=,$p
  $until=[DateTime]::UtcNow.AddSeconds(40);$pixels=0;$window=[IntPtr]::Zero
  do {
   Start-Sleep -Milliseconds 250;$p.Refresh();if($p.HasExited){throw "Early GUI exit $($p.ExitCode)"}
   $window=[PreviewCapture]::Find($p.Id,$case.name)
   if($window-ne[IntPtr]::Zero){
    $null=[PreviewCapture]::MoveWindow($window,20,10,1000,680,$true);$null=[PreviewCapture]::SetForegroundWindow($window)
    if(-not(Test-Path "$r\GUI_WINDOW_READY.json")){@{pid=$p.Id;window=$window.ToInt64();name=$case.name;position='20,10,1000,680'}|ConvertTo-Json|Set-Content "$r\GUI_WINDOW_READY.json"}
    $pixels=[PreviewCapture]::Capture($window,"$r\GUI_$($case.label).png",($case.label-ne'ply'),($case.label-eq'eps'))
    if(($case.label-ne'ply'-and$pixels-gt1000)-or($case.label-eq'ply'-and$pixels-gt80)){break}
   }
  }while([DateTime]::UtcNow-lt$until)
  if(($case.label-ne'ply'-and$pixels-le1000)-or($case.label-eq'ply'-and$pixels-le80)){throw "Rendered fixture surface missing: $($case.label) $pixels"}
  $title=[PreviewCapture]::Title($window);$null=[PreviewCapture]::PostMessage($window,0x10,[IntPtr]::Zero,[IntPtr]::Zero)
  if(-not$p.WaitForExit(15000)){throw 'GUI clean close timed out'};if($p.ExitCode-ne0){throw "Unclean GUI exit $($p.ExitCode)"}
  if((Get-FileHash $file).Hash-ne$before){throw 'Source bytes changed'}
  $result.cases[$case.label]=@{title=$title;rendered_fixture_pixels=$pixels;clean_exit=$p.ExitCode;source_unchanged=$true}
 }
 $result.status='pass'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally {
 foreach($p in $owned){if(-not$p.HasExited){& C:\Windows\System32\taskkill.exe /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}}
 try {$index=Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-eq($binary.Replace('ottrin.exe','ottrin-indexd.exe'))};if($index){$stop=Start-Process ($binary.Replace('ottrin.exe','ottrin-indexd.exe')) -ArgumentList '--stop' -PassThru -Wait -WindowStyle Hidden;$result.index_stop_exit=$stop.ExitCode;if($stop.ExitCode-ne0){throw 'Owned daemon stop failed'}}else{$result.index_stop='not running'}}catch{$result.cleanup_error=$_.Exception.Message;$result.status='fail'}
 if(Test-Path $work){Remove-Item $work -Recurse -Force};$result.fixture_removed=-not(Test-Path $work)
 $result|ConvertTo-Json -Depth 12|Set-Content "$r\GUI_PREVIEWS.json"
}
if($result.status-ne'pass'){exit 1}
