# gamecheck.ps1, dot-sourced by gated.ps1 and ctl.ps1: Stop-IfGame exits 75 when a game
# runs or when the process list cannot be read, so a failed check never acts.
function Stop-IfGame([string]$What) {
    $pat = 'steamapps\\common|\\Epic Games\\|\\XboxGames\\|Riot Games\\League of Legends\\Game'
    try {
        $all = @(Get-CimInstance Win32_Process -ErrorAction Stop)
    } catch {
        Write-Output "GATED: cannot list processes ($($_.Exception.Message)); did not run $What"
        exit 75
    }
    if ($all.Count -eq 0) {
        Write-Output "GATED: empty process list; did not run $What"
        exit 75
    }
    $games = @($all | Where-Object {
        ($_.ExecutablePath -and $_.ExecutablePath -match $pat) -or $_.Name -match 'League of Legends'
    } | ForEach-Object { "{0} pid={1}" -f $_.Name, $_.ProcessId })
    if ($games.Count -gt 0) {
        Write-Output "GATED: game running ($($games -join ', ')); did not run $What"
        exit 75
    }
}
