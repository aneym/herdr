# install.ps1 [-Sha <sha>]: run the NSIS installer silently, then locate the
# installed exe and Start-menu shortcut and print them as JSON.
param([string]$Sha = '')
$ErrorActionPreference = 'Stop'
$outDir = 'C:\Users\aneym\winshell\out'
if ($Sha) {
    $inst = Join-Path $outDir "HerdrShell-setup-$Sha.exe"
} else {
    $inst = (Get-ChildItem $outDir -Filter 'HerdrShell-setup-*.exe' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
}
if (!$inst -or !(Test-Path $inst)) { Write-Error "installer not found ($inst)"; exit 1 }
Write-Output "installing $inst"
$p = Start-Process -FilePath $inst -ArgumentList '/S' -Wait -PassThru

$cands = @(
    (Join-Path $env:LOCALAPPDATA 'Herdr Shell\HerdrShell.exe'),
    (Join-Path $env:LOCALAPPDATA 'Programs\Herdr Shell\HerdrShell.exe')
)
$exe = $cands | Where-Object { Test-Path $_ } | Select-Object -First 1
if (!$exe) {
    $exe = (Get-ChildItem $env:LOCALAPPDATA -Recurse -Depth 3 -Filter 'HerdrShell.exe' -ErrorAction SilentlyContinue |
        Select-Object -First 1).FullName
}
$lnk = Get-ChildItem (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs') -Recurse -Filter '*Herdr*' -ErrorAction SilentlyContinue |
    Select-Object -First 1
[ordered]@{
    installer = $inst
    exitcode  = $p.ExitCode
    exe       = $exe
    shortcut  = if ($lnk) { $lnk.FullName } else { $null }
} | ConvertTo-Json -Compress
if (!$exe) { exit 1 }
exit 0
