# gamecheck.ps1, dot-sourced by gated.ps1 and ctl.ps1: Stop-IfGame exits 75 when a game
# runs or when the process list cannot be read, so a failed check never acts.
# Get-Game returns the running games ("name pid=N"), or the reason the process list
# could not be read as one entry starting with "unreadable:"; empty means no game.
function Get-Game {
    $pat = 'steamapps\\common|\\Epic Games\\|\\XboxGames\\|Riot Games\\League of Legends\\Game'
    try {
        $all = @(Get-CimInstance Win32_Process -ErrorAction Stop)
    } catch {
        return @("unreadable: cannot list processes ($($_.Exception.Message))")
    }
    if ($all.Count -eq 0) { return @('unreadable: empty process list') }
    return @($all | Where-Object {
        ($_.ExecutablePath -and $_.ExecutablePath -match $pat) -or $_.Name -match 'League of Legends'
    } | ForEach-Object { "{0} pid={1}" -f $_.Name, $_.ProcessId })
}

function Stop-IfGame([string]$What) {
    $games = @(Get-Game)
    if ($games.Count -gt 0) {
        Write-Output "GATED: game running ($($games -join ', ')); did not run $What"
        exit 75
    }
}
