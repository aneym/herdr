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
$running = @(Get-Process HerdrShell -ErrorAction SilentlyContinue)
$running | Stop-Process -Force
$deadline = [DateTime]::UtcNow.AddSeconds(10)
while (Get-Process HerdrShell -ErrorAction SilentlyContinue) {
    if ([DateTime]::UtcNow -ge $deadline) {
        Write-Error 'HerdrShell did not stop within 10 s'
        exit 1
    }
    Start-Sleep -Milliseconds 100
}
Write-Output "stopped: $($running.Count)"
Write-Output "installing $inst"
$p = Start-Process -FilePath $inst -ArgumentList '/S' -Wait -PassThru
if ($p.ExitCode -ne 0) {
    Write-Error "installer failed: exit code $($p.ExitCode)"
    exit 1
}

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
