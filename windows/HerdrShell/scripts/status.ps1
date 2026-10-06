# status.ps1: print {"running":bool,"installed":bool,"exe":..,"version":..}
$ErrorActionPreference = 'SilentlyContinue'
$cands = @(
    (Join-Path $env:LOCALAPPDATA 'Herdr Shell\HerdrShell.exe'),
    (Join-Path $env:LOCALAPPDATA 'Programs\Herdr Shell\HerdrShell.exe')
)
$exe = $cands | Where-Object { Test-Path $_ } | Select-Object -First 1
if (!$exe) {
    $exe = (Get-ChildItem $env:LOCALAPPDATA -Recurse -Depth 3 -Filter 'HerdrShell.exe' | Select-Object -First 1).FullName
}
$proc = Get-Process HerdrShell -ErrorAction SilentlyContinue
$ver = $null
if ($exe) { $ver = (Get-Item $exe).VersionInfo.ProductVersion }
[ordered]@{
    running   = ($null -ne $proc)
    installed = ($null -ne $exe)
    exe       = $exe
    version   = $ver
} | ConvertTo-Json -Compress
exit 0
