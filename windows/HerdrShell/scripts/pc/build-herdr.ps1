# Builds the fork's herdr.exe and the pipe relay on the PC, then installs them
# per-user under %LOCALAPPDATA%\Programs\herdr-fork and puts that dir on PATH
# in place of the stock standalone release. Exits 75 when a game is running.
param(
    [Parameter(Mandatory = $true)][string]$Src,
    [Parameter(Mandatory = $true)][string]$Sha,
    [int]$Jobs = 8
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'guard.ps1')

$root = 'C:\Users\aneym\winshell'
$install = Join-Path $env:LOCALAPPDATA 'Programs\herdr-fork'
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
$env:ZIG = "$env:USERPROFILE\.cache\zig-0.15.2\zig.exe"
$env:HERDR_BUILD_CHANNEL = 'fork'
$env:HERDR_BUILD_ID = $Sha
$env:LIBGHOSTTY_VT_OPTIMIZE = 'ReleaseFast'

function Stop-Tree([int]$ProcessId) { & taskkill.exe /T /F /PID $ProcessId 2>&1 | Out-Null }

function Invoke-Guarded([string]$Dir, [string[]]$CargoArgs, [string]$Log) {
    $games = Test-Game
    if ($games) { Write-Output "game running ($($games -join ', ')); not building"; exit 75 }
    $p = Start-Process -FilePath cargo.exe -ArgumentList $CargoArgs -WorkingDirectory $Dir `
        -NoNewWindow -PassThru -RedirectStandardError $Log -RedirectStandardOutput "$Log.out"
    $null = $p.Handle  # keeps ExitCode readable after exit
    try { $p.PriorityClass = 'BelowNormal' } catch {}
    while (-not $p.WaitForExit(10000)) {
        $games = Test-Game
        if ($games) {
            Stop-Tree $p.Id
            Write-Output "game started ($($games -join ', ')); build stopped, resume later"
            exit 75
        }
        # Children (rustc, zig) inherit nothing from the parent's class; lower them too.
        Get-CimInstance Win32_Process -Filter "ParentProcessId=$($p.Id)" -ErrorAction SilentlyContinue | ForEach-Object {
            try { (Get-Process -Id $_.ProcessId).PriorityClass = 'BelowNormal' } catch {}
        }
    }
    if ($p.ExitCode -ne 0) {
        Get-Content $Log -Tail 40
        throw "cargo $($CargoArgs -join ' ') failed with exit $($p.ExitCode)"
    }
}

New-Item -ItemType Directory -Force "$root\logs" | Out-Null
$target = 'x86_64-pc-windows-msvc'
$env:CARGO_TARGET_DIR = "$root\target-herdr"
Invoke-Guarded $Src @('build', '--release', '--locked', '--target', $target, '-j', "$Jobs") "$root\logs\herdr-build.log"
$herdrExe = "$root\target-herdr\$target\release\herdr.exe"

$env:CARGO_TARGET_DIR = "$root\target-relay"
Invoke-Guarded "$Src\windows\HerdrShell\relay" @('build', '--release', '-j', "$Jobs") "$root\logs\relay-build.log"
$relayExe = "$root\target-relay\release\herdr-pipe-relay.exe"

# Stage herdr.exe with its app-local ConPTY runtime exactly as release CI does.
$stage = "$root\stage-herdr"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
& "$Src\scripts\package_windows_conpty.ps1" -HerdrExe $herdrExe `
    -PackagePath "$root\cache\Microsoft.Windows.Console.ConPTY.nupkg" `
    -StageDir $stage -OutputPath "$root\out\herdr-windows-x86_64-$Sha.zip"

# Install: stop anything running from the install dir first (relay, link).
Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($install, 'OrdinalIgnoreCase') } |
    ForEach-Object { Stop-Process -Id $_.Id -Force }
New-Item -ItemType Directory -Force $install | Out-Null
Copy-Item -Recurse -Force "$stage\*" $install
Copy-Item -Force $relayExe $install
Copy-Item -Force "$Src\windows\HerdrShell\scripts\pc\studio-link.ps1", "$Src\windows\HerdrShell\scripts\pc\herdr-studio.cmd" $install
@{ commit = $Sha; built_at = (Get-Date).ToString('o'); version = (& "$install\herdr.exe" --version) } |
    ConvertTo-Json | Set-Content -Encoding UTF8 "$install\BUILD.json"

# PATH: the fork dir replaces the stock standalone release entry (files kept for rollback).
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User') -split ';' | Where-Object { $_ }
$userPath = @($install) + @($userPath | Where-Object { $_ -ne $install -and $_ -notlike '*\.herdr\packages\standalone\*' })
[Environment]::SetEnvironmentVariable('Path', ($userPath -join ';'), 'User')

Write-Output "installed $install"
& "$install\herdr.exe" --version
