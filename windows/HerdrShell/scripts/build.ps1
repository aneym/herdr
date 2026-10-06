# build.ps1 -Sha <sha>: run build_inner.ps1 as a job at BelowNormal priority,
# stream its output, and kill the whole tree if a game starts mid-build.
param([string]$Sha = 'dev')
$ErrorActionPreference = 'Stop'
[System.Diagnostics.Process]::GetCurrentProcess().PriorityClass = 'BelowNormal'
$env:Path = 'C:\Users\aneym\.cargo\bin;' + $env:Path
$env:CARGO_TARGET_DIR = 'C:\Users\aneym\winshell\target-shell'
$env:HERDR_SHELL_COMMIT = $Sha

$cache = 'C:\Users\aneym\winshell\cache'
New-Item -ItemType Directory -Force -Path $cache | Out-Null
$flag = Join-Path $cache 'game.flag'
$rcFile = Join-Path $cache 'build.rc'
Remove-Item $flag, $rcFile -Force -ErrorAction SilentlyContinue

$guard = Start-Job {
    param($f)
    $pat = 'steamapps\\common|\\Epic Games\\|\\XboxGames\\|Riot Games\\League of Legends\\Game'
    while ($true) {
        $hit = $false
        Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | ForEach-Object {
            $e = $_.ExecutablePath
            if (($e -and $e -match $pat) -or ($_.Name -match 'League of Legends')) { $hit = $true }
        }
        if ($hit) { Set-Content $f 'game'; break }
        Start-Sleep -Seconds 10
    }
} -ArgumentList $flag

$build = Start-Job -FilePath 'C:\Users\aneym\winshell\scripts\build_inner.ps1' -ArgumentList $Sha

$killed = $false
while ($build.State -eq 'Running') {
    Receive-Job $build | ForEach-Object { Write-Output $_ }
    if (Test-Path $flag) {
        Write-Output 'GAME DETECTED mid-build; stopping build'
        Get-CimInstance Win32_Process -Filter "ParentProcessId=$PID" -ErrorAction SilentlyContinue |
            Select-Object -ExpandProperty ProcessId |
            ForEach-Object { cmd /c "taskkill /T /F /PID $_" | Out-Null }
        Stop-Job $build -ErrorAction SilentlyContinue
        $killed = $true
        break
    }
    Start-Sleep -Seconds 2
}
Receive-Job $build | ForEach-Object { Write-Output $_ }
Stop-Job $guard, $build -ErrorAction SilentlyContinue
Remove-Job $guard, $build -Force -ErrorAction SilentlyContinue

if ($killed) { exit 75 }
$rc = 1
if (Test-Path $rcFile) { $rc = [int](Get-Content $rcFile -Raw).Trim() }
if ($rc -ne 0) { exit $rc }

$nsisDir = 'C:\Users\aneym\winshell\target-shell\release\bundle\nsis'
$exe = Get-ChildItem $nsisDir -Filter *.exe -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (!$exe) { Write-Error "no NSIS installer under $nsisDir"; exit 1 }
$outDir = 'C:\Users\aneym\winshell\out'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$dst = Join-Path $outDir "HerdrShell-setup-$Sha.exe"
Copy-Item $exe.FullName $dst -Force
Write-Output "INSTALLER: $dst"
exit 0
