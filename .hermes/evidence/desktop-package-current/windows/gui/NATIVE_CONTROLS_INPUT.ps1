Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing -TypeDefinition @'
using System;using System.Text;using System.Runtime.InteropServices;using System.Windows.Forms;using System.Drawing;using System.Drawing.Imaging;
public static class PickerNative {
 public delegate bool EnumProc(IntPtr hwnd,IntPtr parameter);
 [StructLayout(LayoutKind.Sequential)]public struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)]public struct Point {public int x,y;}
 [DllImport("user32.dll")]static extern bool EnumWindows(EnumProc callback,IntPtr param);
 [DllImport("user32.dll")]static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
 [DllImport("user32.dll")]static extern bool IsWindowVisible(IntPtr hwnd);
 [DllImport("user32.dll")]public static extern bool IsWindowEnabled(IntPtr hwnd);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern int GetWindowText(IntPtr hwnd,StringBuilder text,int count);
 public static string Title(IntPtr hwnd){var text=new StringBuilder(1024);GetWindowText(hwnd,text,1024);return text.ToString();}
 [DllImport("user32.dll")]public static extern IntPtr GetWindow(IntPtr hwnd,uint command);
 [DllImport("user32.dll")]static extern bool GetClientRect(IntPtr hwnd,out Rect rect);
 [DllImport("user32.dll")]static extern bool ClientToScreen(IntPtr hwnd,ref Point point);
 [DllImport("user32.dll")]static extern bool GetWindowRect(IntPtr hwnd,out Rect rect);
 [DllImport("user32.dll")]public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr hwnd);
 [DllImport("user32.dll")]static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 [DllImport("user32.dll")]static extern uint GetDpiForWindow(IntPtr hwnd);
 [DllImport("user32.dll")]static extern bool SetProcessDPIAware();
 public static void Init(){SetProcessDPIAware();}
 public static IntPtr AnyWindow(int wanted){IntPtr found=IntPtr.Zero;EnumWindows(delegate(IntPtr h,IntPtr p){uint id;GetWindowThreadProcessId(h,out id);if(id==wanted&&IsWindowVisible(h)){found=h;return false;}return true;},IntPtr.Zero);return found;}
 public static IntPtr Window(int wanted,string title){IntPtr found=IntPtr.Zero;EnumWindows(delegate(IntPtr h,IntPtr p){uint id;GetWindowThreadProcessId(h,out id);if(id==wanted&&IsWindowVisible(h)&&Title(h)==title){found=h;return false;}return true;},IntPtr.Zero);return found;}
 [DllImport("user32.dll")]static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int width,int height,uint flags);
 public static void Place(IntPtr h){var area=Screen.FromHandle(h).WorkingArea;Rect rect;if(!GetWindowRect(h,out rect))throw new Exception("Cannot read owned test window bounds");int width=Math.Min(rect.right-rect.left,area.Width),height=Math.Min(rect.bottom-rect.top,area.Height);if(width<=0||height<=0||!SetWindowPos(h,IntPtr.Zero,area.Left,area.Top,width,height,0x14))throw new Exception("Cannot fit owned test window within the work area");System.Threading.Thread.Sleep(150);if(!GetWindowRect(h,out rect)||rect.left<area.Left||rect.top<area.Top||rect.right>area.Right||rect.bottom>area.Bottom)throw new Exception("Owned test window remains outside the work area");}
 public static void Focus(IntPtr h){SetForegroundWindow(h);System.Threading.Thread.Sleep(100);if(GetForegroundWindow()!=h){Rect r;GetClientRect(h,out r);var p=new Point();p.x=r.right/2;p.y=(int)(16*GetDpiForWindow(h)/96.0);ClientToScreen(h,ref p);Cursor.Position=new System.Drawing.Point(p.x,p.y);mouse_event(2,0,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(60);mouse_event(4,0,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(150);}if(GetForegroundWindow()!=h)throw new Exception("Owned window did not acquire foreground focus");}
 public static void Click(IntPtr h,double x,double y){Focus(h);Rect r;GetClientRect(h,out r);double scale=GetDpiForWindow(h)/96.0;var p=new Point();p.x=(int)(x<0?r.right+x*scale:x*scale);p.y=(int)(y<0?r.bottom+y*scale:y*scale);ClientToScreen(h,ref p);Cursor.Position=new System.Drawing.Point(p.x,p.y);if(Cursor.Position.X!=p.x||Cursor.Position.Y!=p.y)throw new Exception("Test click lies outside the active desktop: requested="+p.x+","+p.y+" actual="+Cursor.Position);System.Threading.Thread.Sleep(100);mouse_event(2,0,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(60);mouse_event(4,0,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(100);}
 public static void Wheel(IntPtr h,int delta){Focus(h);Rect r;GetClientRect(h,out r);var p=new Point();p.x=r.right/2;p.y=r.bottom/2;ClientToScreen(h,ref p);Cursor.Position=new System.Drawing.Point(p.x,p.y);mouse_event(0x800,0,0,unchecked((uint)delta),UIntPtr.Zero);}
 public static double BottomButtonRow(IntPtr h){Rect r;GetClientRect(h,out r);var p=new Point();ClientToScreen(h,ref p);double scale=GetDpiForWindow(h)/96.0;using(var b=new Bitmap(r.right,r.bottom))using(var g=Graphics.FromImage(b)){g.CopyFromScreen(p.x,p.y,0,0,b.Size);int start=-1;for(int y=b.Height-12;y>b.Height/2;y--){var a=b.GetPixel((int)(201*scale),y);var z=b.GetPixel((int)(205*scale),y);bool fill=a.R==49&&a.G==56&&a.B==66&&z.R==49&&z.G==56&&z.B==66;if(fill){if(start<0)start=y;}else if(start>=0){if(start-y>=12*scale)return ((start+y)/2.0)/scale;start=-1;}}}throw new Exception("The rendered folder action button row was not found");}
 public static int GreenStatusDots(IntPtr h){Rect r;GetClientRect(h,out r);var p=new Point();ClientToScreen(h,ref p);double scale=GetDpiForWindow(h)/96.0;int count=0,run=0;using(var b=new Bitmap(r.right,r.bottom))using(var g=Graphics.FromImage(b)){g.CopyFromScreen(p.x,p.y,0,0,b.Size);for(int y=(int)(350*scale);y<Math.Min(b.Height-20,(int)(610*scale));y++){var c=b.GetPixel((int)(201*scale),y);if(c.R==80&&c.G==185&&c.B==110){run++;}else{if(run>=5*scale)count++;run=0;}}}return count;}
 public static string Geometry(IntPtr h){Rect r;GetClientRect(h,out r);var p=new Point();ClientToScreen(h,ref p);return "client="+r.right+"x"+r.bottom+" origin="+p.x+","+p.y+" dpi="+GetDpiForWindow(h);}
 public static void Shot(IntPtr h,string path){Rect r;GetWindowRect(h,out r);using(var b=new Bitmap(r.right-r.left,r.bottom-r.top))using(var g=Graphics.FromImage(b)){g.CopyFromScreen(r.left,r.top,0,0,b.Size);b.Save(path,ImageFormat.Png);}}
}
'@
