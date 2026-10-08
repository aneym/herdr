# install_copy.ps1 -Sha <sha> [-Relaunch]: replace the installed HerdrShell.exe
# with out\HerdrShell-<sha>.exe, built off the PC (pc.py fetch). No installer and
# no compiler runs here. The previous exe stays beside it as HerdrShell.exe.prev;
# when the new exe does not stay up (an Application Control block included) the
# previous one goes back and relaunches, and the script exits 1 with the reason.
param([Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{40}$')][string]$Sha, [switch]$Relaunch)
$ErrorActionPreference = 'Stop'
$src = "C:\Users\aneym\winshell\out\HerdrShell-$Sha.exe"
if (!(Test-Path -LiteralPath $src -PathType Leaf)) { Write-Error "artifact not found ($src)"; exit 1 }
$dir = Join-Path $env:LOCALAPPDATA 'Herdr Shell'
$dst = Join-Path $dir 'HerdrShell.exe'
$prev = "$dst.prev"
if (!(Test-Path -LiteralPath $dst -PathType Leaf)) { Write-Error "no installed exe at $dst; run the NSIS install once"; exit 1 }

$running = @(Get-Process HerdrShell -ErrorAction SilentlyContinue)
$running | Stop-Process -Force -ErrorAction SilentlyContinue
$deadline = [DateTime]::UtcNow.AddSeconds(10)
while (Get-Process HerdrShell -ErrorAction SilentlyContinue) {
    if ([DateTime]::UtcNow -ge $deadline) { Write-Error 'HerdrShell did not stop within 10 s'; exit 1 }
    Start-Sleep -Milliseconds 100
}
Write-Output "stopped: $($running.Count)"
Copy-Item -LiteralPath $dst -Destination $prev -Force
Copy-Item -LiteralPath $src -Destination $dst -Force
Write-Output "copied $src -> $dst"
if (!$Relaunch -and !$running.Count) { exit 0 }

$since = Get-Date
& (Join-Path $PSScriptRoot 'launch.ps1') -Exe $dst
# The app must still be alive after 8 s; a blocked image never starts.
$up = $false
$deadline = [DateTime]::UtcNow.AddSeconds(20)
while ([DateTime]::UtcNow -lt $deadline) {
    Start-Sleep -Seconds 1
    $p = Get-Process HerdrShell -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -ge $since }
    if ($p -and (([DateTime]::Now - ($p | Select-Object -First 1).StartTime).TotalSeconds -ge 8)) { $up = $true; break }
}
if ($up) { Write-Output "running: $Sha"; exit 0 }

$block = Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-CodeIntegrity/Operational'; Id = 3077, 3033; StartTime = $since } -ErrorAction SilentlyContinue |
    Where-Object { $_.Message -match 'HerdrShell\.exe' } | Select-Object -First 1
$why = if ($block) { "Application Control blocked it (event $($block.Id))" } else { 'it did not stay running' }
Get-Process HerdrShell -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1
Copy-Item -LiteralPath $prev -Destination $dst -Force
& (Join-Path $PSScriptRoot 'launch.ps1') -Exe $dst | Out-Null
Write-Error "new exe rolled back: $why"
exit 1
