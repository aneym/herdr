# idle.ps1: writes {"idle_s":N,"at":iso} to %LOCALAPPDATA%\HerdrShell\idle.json.
# Must run inside Alex's interactive session (scheduled task with /it) for
# GetLastInputInfo to reflect real input.
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class IdleProbe {
    [StructLayout(LayoutKind.Sequential)]
    struct LASTINPUTINFO { public uint cbSize; public uint dwTime; }
    [DllImport("user32.dll")] static extern bool GetLastInputInfo(ref LASTINPUTINFO plii);
    [DllImport("kernel32.dll")] static extern uint GetTickCount();
    [DllImport("kernel32.dll")] static extern IntPtr GetConsoleWindow();
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hWnd);
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct STARTUPINFO { public int cb; public string r; public string d; public string t; public int x, y, w, h, cx, cy, fill, flags; public short show, r2; public IntPtr r3, i, o, e; }
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern void GetStartupInfo(out STARTUPINFO si);
    // The show state this process (and its console) was created with; 0 = SW_HIDE.
    public static int StartShow() { STARTUPINFO si; GetStartupInfo(out si); return (si.flags & 1) != 0 ? si.show : -1; }
    public static bool ConsoleVisible() { IntPtr h = GetConsoleWindow(); return h != IntPtr.Zero && IsWindowVisible(h); }
    public static long Seconds() {
        LASTINPUTINFO lii = new LASTINPUTINFO();
        lii.cbSize = (uint)Marshal.SizeOf(typeof(LASTINPUTINFO));
        if (!GetLastInputInfo(ref lii)) return -1;
        return (long)(GetTickCount() - lii.dwTime) / 1000;
    }
}
'@
$dir = Join-Path $env:LOCALAPPDATA 'HerdrShell'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
# console_visible proves the launcher kept this run off Alex's screen.
[ordered]@{ idle_s = [IdleProbe]::Seconds(); at = (Get-Date).ToString('o'); console_visible = [IdleProbe]::ConsoleVisible(); start_show = [IdleProbe]::StartShow() } |
    ConvertTo-Json -Compress |
    Set-Content (Join-Path $dir 'idle.json') -Encoding UTF8
