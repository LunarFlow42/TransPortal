$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class TransPortalWindowInspector {
    public delegate bool EnumProc(IntPtr h, IntPtr p);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr p);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr h, EnumProc f, IntPtr p);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
    [DllImport("user32.dll")] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] static extern bool IsHungAppWindow(IntPtr h);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out Rect r);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out Rect r);
    [StructLayout(LayoutKind.Sequential)] public struct Rect {
        public int Left, Top, Right, Bottom;
        public override string ToString() { return Left+","+Top+","+(Right-Left)+"x"+(Bottom-Top); }
    }
    static void Print(IntPtr h, string kind) {
        uint p; uint thread = GetWindowThreadProcessId(h, out p);
        var t = new StringBuilder(256); var c = new StringBuilder(256);
        GetWindowText(h, t, 256); GetClassName(h, c, 256);
        Rect r, client; GetWindowRect(h, out r); GetClientRect(h, out client);
        Console.WriteLine(kind+" hwnd="+h+" pid="+p+" thread="+thread+" title="+t+" class="+c+
            " visible="+IsWindowVisible(h)+" minimized="+IsIconic(h)+" hung="+IsHungAppWindow(h)+
            " rect="+r+" client="+client);
    }
    public static void Inspect(uint pid) {
        EnumWindows((h, p) => {
            uint id; GetWindowThreadProcessId(h, out id);
            if (id == pid) {
                Print(h, "host");
                EnumChildWindows(h, (ch, cp) => { Print(ch, "child"); return true; }, IntPtr.Zero);
            }
            return true;
        }, IntPtr.Zero);
    }
}
'@
# Reads only TransPortal window metadata; never shows/hides windows or captures pixels.
Get-Process transportal -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Output "TransPortal process $($_.Id), started $($_.StartTime)"
    [TransPortalWindowInspector]::Inspect($_.Id)
}
