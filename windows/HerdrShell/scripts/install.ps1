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
# Read the path while the process lives; an exited process no longer reports it.
$wasRunning = @($running | ForEach-Object { $_.Path } | Where-Object { $_ })
$running | Stop-Process -Force -ErrorAction SilentlyContinue
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
# A blocked or failed installer (Smart App Control refused one on 2026-10-06) must not leave
# Alex without the app: start the copy that was running before.
$restart = { if ($wasRunning.Count) { & (Join-Path $PSScriptRoot 'launch.ps1') -Exe $wasRunning[0] } }
try {
    $p = Start-Process -FilePath $inst -ArgumentList '/S' -Wait -PassThru -ErrorAction Stop
} catch {
    & $restart
    Write-Error "installer did not start: $($_.Exception.Message)"
    exit 1
}
if ($p.ExitCode -ne 0) {
    & $restart
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
