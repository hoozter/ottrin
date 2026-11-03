using System;using System.Collections.Generic;using System.Runtime.InteropServices;using System.Threading;using System.Drawing;using System.Windows.Forms;
public static class DpiDesktop {
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)]struct Mode {
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=32)]public string device;
  public ushort spec,driver,size,extra;public uint fields;public int x,y;public uint orientation,fixedOutput;public short color,duplex,yResolution,ttOption,collate;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=32)]public string form;
  public ushort logPixels;public uint bits,width,height,flags,frequency,icmMethod,icmIntent,media,dither,reserved1,reserved2,panWidth,panHeight;
 }
 [StructLayout(LayoutKind.Sequential)]struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)]struct MonitorInfo {public int size;public Rect monitor,work;public uint flags;}
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern bool EnumDisplaySettingsW(string name,int mode,ref Mode data);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern int ChangeDisplaySettingsExW(string name,ref Mode mode,IntPtr hwnd,uint flags,IntPtr parameter);
 [DllImport("user32.dll")]public static extern uint GetDpiForWindow(IntPtr window);
 [DllImport("user32.dll")]static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")]static extern bool GetWindowRect(IntPtr window,out Rect rect);
 [DllImport("user32.dll")]static extern IntPtr MonitorFromWindow(IntPtr window,uint flags);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern bool GetMonitorInfoW(IntPtr monitor,ref MonitorInfo info);
 [DllImport("user32.dll")]static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 public static string[] Modes(){var modes=new List<string>();for(int i=0;i<1024;i++){Mode m=new Mode();m.size=220;if(!EnumDisplaySettingsW(null,i,ref m))break;string v=m.width+"x"+m.height+"x"+m.bits;if(!modes.Contains(v))modes.Add(v);}return modes.ToArray();}
 public static string LargeMode(){
  if(Marshal.SizeOf(typeof(Mode))!=220)throw new Exception("Unexpected native DEVMODEW size");
  Mode best=new Mode();ulong area=ulong.MaxValue;
  for(int i=0;i<1024;i++){Mode mode=new Mode();mode.size=220;if(!EnumDisplaySettingsW(null,i,ref mode))break;ulong a=(ulong)mode.width*mode.height;if(mode.bits>=32&&mode.width>=1920&&mode.height>=1440&&a<area){best=mode;area=a;}}
  if(area==ulong.MaxValue)throw new Exception("Driver has no supported1920x1440-or-larger mode");
  int result=ChangeDisplaySettingsExW(null,ref best,IntPtr.Zero,0,IntPtr.Zero);if(result!=0)throw new Exception("Native display change failed: "+result);Thread.Sleep(1500);return best.width+"x"+best.height;
 }
 [DllImport("user32.dll")]static extern bool PostMessageW(IntPtr window,uint message,IntPtr wparam,IntPtr lparam);
 public static void Close(IntPtr window){if(!PostMessageW(window,0x10,IntPtr.Zero,IntPtr.Zero))throw new Exception("Normal Settings close request failed");}
 public static void PerMonitor(){if(SetThreadDpiAwarenessContext(new IntPtr(-4))==IntPtr.Zero)throw new Exception("Cannot enter native per-monitor V2 observation context");}
 public static int[] Bounds(IntPtr window){Rect r;if(!GetWindowRect(window,out r))throw new Exception("Window bounds unavailable");return new int[]{r.left,r.top,r.right-r.left,r.bottom-r.top};}
 public static int[] Work(IntPtr window){MonitorInfo m=new MonitorInfo();m.size=Marshal.SizeOf(typeof(MonitorInfo));if(!GetMonitorInfoW(MonitorFromWindow(window,2),ref m))throw new Exception("Work area unavailable");return new int[]{m.work.left,m.work.top,m.work.right-m.work.left,m.work.bottom-m.work.top};}
 public static bool Fits(IntPtr window){int[] b=Bounds(window),a=Work(window);return b[0]>=a[0]&&b[1]>=a[1]&&b[0]+b[2]<=a[0]+a[2]&&b[1]+b[3]<=a[1]+a[3];}
 public static void WheelAt(int x,int y,int delta){Cursor.Position=new Point(x,y);mouse_event(0x800,0,0,unchecked((uint)delta),UIntPtr.Zero);Thread.Sleep(200);}
}
