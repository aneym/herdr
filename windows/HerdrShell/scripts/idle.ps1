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
[ordered]@{ idle_s = [IdleProbe]::Seconds(); at = (Get-Date).ToString('o') } |
    ConvertTo-Json -Compress |
    Set-Content (Join-Path $dir 'idle.json') -Encoding UTF8
