# game_guard.ps1: prints {"game":bool,"procs":[...],"idle_s":N|null}
# Exits 3 when a game is running. League lobby processes
# (LeagueClientUx*, "Riot Client") are NOT games.
$ErrorActionPreference = 'SilentlyContinue'
$pat = 'steamapps\\common|\\Epic Games\\|\\XboxGames\\|Riot Games\\League of Legends\\Game'
$procs = @()
Get-CimInstance Win32_Process | ForEach-Object {
    $exe = $_.ExecutablePath
    if (($exe -and ($exe -match $pat)) -or ($_.Name -match 'League of Legends')) {
        $procs += ("{0} pid={1}" -f $_.Name, $_.ProcessId)
    }
}
$idle = $null
$idleFile = Join-Path $env:LOCALAPPDATA 'HerdrShell\idle.json'
if (Test-Path $idleFile) {
    try {
        $j = Get-Content $idleFile -Raw | ConvertFrom-Json
        $idle = [int]$j.idle_s
    } catch {}
}
[ordered]@{ game = ($procs.Count -gt 0); procs = $procs; idle_s = $idle } |
    ConvertTo-Json -Compress
if ($procs.Count -gt 0) { exit 3 }
exit 0
