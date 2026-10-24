Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class CopyActivityDesktop {
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr e);
 [DllImport("kernel32.dll")] public static extern uint WTSGetActiveConsoleSessionId();
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,System.Text.StringBuilder text,int length);
  public delegate bool EnumCallback(IntPtr h,IntPtr p);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumCallback callback,IntPtr p);
  [StructLayout(LayoutKind.Sequential)] private struct KEYBDINPUT { public ushort key,scan;public uint flags,time;public UIntPtr extra; }
 [StructLayout(LayoutKind.Sequential)] private struct MOUSEINPUT { public int x,y;public uint data,flags,time;public UIntPtr extra; }
 [StructLayout(LayoutKind.Explicit)] private struct INPUTDATA { [FieldOffset(0)]public KEYBDINPUT keyboard;[FieldOffset(0)]public MOUSEINPUT mouse; }
 [StructLayout(LayoutKind.Sequential)] private struct INPUT { public uint type;public INPUTDATA data; }
 [DllImport("user32.dll",SetLastError=true)] private static extern uint SendInput(uint count,INPUT[] input,int size);
 [DllImport("user32.dll")] private static extern short GetAsyncKeyState(int key);
 public static void TypeText(string text) {
   foreach(int key in new[]{0x10,0x11,0x12,0x5b,0x5c})
     if((GetAsyncKeyState(key)&0x8000)!=0)throw new InvalidOperationException("A modifier remains held before literal text input");
   foreach(char character in text) {
     var down=new INPUT();down.type=1;down.data.keyboard.scan=(ushort)character;down.data.keyboard.flags=4;
     var up=down;up.data.keyboard.flags=6;
     if(SendInput(2,new[]{down,up},Marshal.SizeOf(typeof(INPUT)))!=2)
       throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(),"Unicode text input was not fully inserted");
   }
 }
 public static IntPtr[] Windows(uint pid) {
    var found=new System.Collections.Generic.List<IntPtr>();
    EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);if(owner==pid&&IsWindowVisible(h))found.Add(h);return true;},IntPtr.Zero);
    return found.ToArray();
  }
}
'@
