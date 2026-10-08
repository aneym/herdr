# install_copy.ps1 -Sha <sha> [-Relaunch] [-Check | -MarkVerified | -Rollback]: swap the
# installed HerdrShell.exe for out\HerdrShell-<sha>.exe, built off the PC and checked by
# pc.py fetch (out\HerdrShell-<sha>.sha256 holds its SHA-256). No installer or compiler runs.
#
# The new exe is staged as HerdrShell.exe.new and hash-checked before the app stops.
# HerdrShell.exe.prev only ever holds a verified build: installed.json records the
# installed sha, its hash and whether pc.py saw it report its commit (-MarkVerified).
# Any failed copy, launch or startup restores .prev. Every launch rechecks for a game
# first; with a game running nothing launches and the script exits 76, leaving the new
# exe staged (or swapped in, not started) for the next run to finish.
#
# Exit codes: 0 done, 1 failed (rolled back where it had swapped), 76 game started,
# 77 staged only (the app is running and -Relaunch was not given).
param(
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{40}$')][string]$Sha,
    [switch]$Relaunch,
    [switch]$Check,
    [switch]$MarkVerified,
    [switch]$Rollback,
    # Overridable only so tests/test_install_copy_pc.py can run in a scratch directory.
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Herdr Shell'),
    [string]$OutDir = 'C:\Users\aneym\winshell\out',
    [string]$ExeName = 'HerdrShell.exe',
    [int]$UpSeconds = 8
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'gamecheck.ps1')

$InstallDir = [IO.Path]::GetFullPath($InstallDir)
$OutDir = [IO.Path]::GetFullPath($OutDir)
$dst = Join-Path $InstallDir $ExeName
$new = "$dst.new"
$prev = "$dst.prev"
$meta = Join-Path $InstallDir 'installed.json'
$prevMeta = Join-Path $InstallDir 'prev.json'
$procName = [IO.Path]::GetFileNameWithoutExtension($ExeName)

function Hash($path) {
    if (!(Test-Path -LiteralPath $path -PathType Leaf)) { return $null }
    (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToUpperInvariant()
}
function Read-Meta($path) {
    if (!(Test-Path -LiteralPath $path)) { return $null }
    try { Get-Content -LiteralPath $path -Raw | ConvertFrom-Json } catch { $null }
}
function Write-Meta($path, $sha, $hash, $verified) {
    $json = [ordered]@{ sha = $sha; sha256 = $hash; verified = [bool]$verified } | ConvertTo-Json -Compress
    [IO.File]::WriteAllText($path, $json, (New-Object Text.UTF8Encoding($false)))
}
function Get-App {
    @(Get-Process $procName -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $dst })
}
function Stop-App {
    Get-App | Stop-Process -Force -ErrorAction SilentlyContinue
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while ((Get-App).Count) {
        if ([DateTime]::UtcNow -ge $deadline) { throw "$ExeName did not stop within 10 s" }
        Start-Sleep -Milliseconds 100
    }
}
# Launch only when no game runs; returns $true when the app stayed up $UpSeconds.
function Start-App {
    $games = @(Get-Game)
    if ($games.Count) { return 'game' }
    $since = Get-Date
    & (Join-Path $PSScriptRoot 'launch.ps1') -Exe $dst -Background | Out-Null
    $deadline = [DateTime]::UtcNow.AddSeconds($UpSeconds + 12)
    while ([DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 500
        $p = Get-App | Where-Object { $_.StartTime -ge $since } | Select-Object -First 1
        if ($p -and ((Get-Date) - $p.StartTime).TotalSeconds -ge $UpSeconds) { return 'up' }
    }
    'down'
}
function Undo([string]$why, [bool]$launch) {
    try { Stop-App } catch {}
    if (Test-Path -LiteralPath $prev -PathType Leaf) {
        Copy-Item -LiteralPath $prev -Destination $dst -Force
        if (Test-Path -LiteralPath $prevMeta) { Copy-Item -LiteralPath $prevMeta -Destination $meta -Force }
        else { Remove-Item -LiteralPath $meta -Force -ErrorAction SilentlyContinue }
        $note = 'previous exe restored'
        if ($launch) {
            $state = 'down'
            try { $state = Start-App } catch {}
            if ($state -eq 'game') { $note = "$note; not relaunched: game running" } else { $note = "$note; relaunch $state" }
        }
    } else {
        $note = 'no previous exe to restore'
    }
    Write-Output "rolled back: $why; $note"
    exit 1
}

$expected = $null
$sumFile = Join-Path $OutDir "HerdrShell-$Sha.sha256"
if (Test-Path -LiteralPath $sumFile) { $expected = (Get-Content -LiteralPath $sumFile -Raw).Trim().ToUpperInvariant() }

if ($Check) {
    $m = Read-Meta $meta
    $h = Hash $dst
    if ($expected -and $h -eq $expected -and $m -and $m.sha -eq $Sha) { Write-Output "installed: $Sha"; exit 0 }
    Write-Output "installed exe is not $Sha (hash $h, recorded $($m.sha))"
    exit 1
}
if ($MarkVerified) {
    $h = Hash $dst
    if (!$expected -or $h -ne $expected) { Write-Output 'installed exe does not match; not marked'; exit 1 }
    Write-Meta $meta $Sha $h $true
    Write-Output "verified: $Sha"
    exit 0
}
if ($Rollback) { Undo "commit check failed for $Sha" ([bool]$Relaunch) }

$src = Join-Path $OutDir "HerdrShell-$Sha.exe"
if (!$expected) { Write-Output "no checksum for $Sha; fetch it first"; exit 1 }
if ((Hash $src) -ne $expected) { Write-Output "artifact hash mismatch for $src"; exit 1 }
if (!(Test-Path -LiteralPath $dst -PathType Leaf)) { Write-Output "no installed exe at $dst; run the NSIS install once"; exit 1 }

$swapped = (Hash $dst) -eq $expected
if (!$swapped) {
    Copy-Item -LiteralPath $src -Destination $new -Force
    if ((Hash $new) -ne $expected) {
        Remove-Item -LiteralPath $new -Force -ErrorAction SilentlyContinue
        Write-Output 'staged copy hash mismatch; nothing changed'
        exit 1
    }
    Write-Output "staged: $new"
    $running = (Get-App).Count
    if ($running -and !$Relaunch) { Write-Output "app running; staged only (use --relaunch)"; exit 77 }
    $games = @(Get-Game)
    if ($games.Count) { Write-Output "game started ($($games -join ', ')); new exe staged, app untouched"; exit 76 }

    # .prev keeps the last verified build; an unverified install never replaces it.
    $m = Read-Meta $meta
    $current = Hash $dst
    $good = (-not $m) -or ($m.verified -and $m.sha256 -eq $current)
    if ($good -or !(Test-Path -LiteralPath $prev -PathType Leaf)) {
        Copy-Item -LiteralPath $dst -Destination $prev -Force
        if ($m) { Write-Meta $prevMeta $m.sha $current $true } else { Write-Meta $prevMeta 'unknown' $current $true }
    }
    try {
        Stop-App
        Move-Item -LiteralPath $new -Destination $dst -Force
    } catch {
        Undo "swap failed: $($_.Exception.Message)" ($running -gt 0)
    }
    if ((Hash $dst) -ne $expected) { Undo 'installed exe hash mismatch after swap' ($running -gt 0) }
    Write-Meta $meta $Sha $expected $false
    Write-Output "swapped: $dst"
} else {
    Write-Output "already swapped: $Sha"
    if ((Get-App).Count) {
        if ($Relaunch) { Write-Output "running: $Sha" }
        exit 0
    }
}
if (!$Relaunch) { exit 0 }

$state = $null
try { $state = Start-App } catch { Undo "launch failed: $($_.Exception.Message)" $true }
if ($state -eq 'game') { Write-Output 'game started; swapped in, not launched'; exit 76 }
if ($state -ne 'up') {
    $block = Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-CodeIntegrity/Operational'; Id = 3077, 3033; StartTime = (Get-Date).AddMinutes(-2) } -ErrorAction SilentlyContinue |
        Where-Object { $_.Message -match [regex]::Escape($ExeName) } | Select-Object -First 1
    $why = if ($block) { "Application Control blocked it (event $($block.Id))" } else { 'it did not stay running' }
    Undo $why $true
}
Write-Output "running: $Sha"
exit 0
