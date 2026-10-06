# Dot-source: Test-Game returns the names of running game processes.
# League client lobby (LeagueClientUx, Riot Client) is not a game.
function Test-Game {
    Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $_.Name -eq 'League of Legends' -or
        ($_.Path -and $_.Path -match 'steamapps\\common|\\Epic Games\\|\\XboxGames\\|Riot Games\\League of Legends\\Game')
    } | Select-Object -ExpandProperty Name -Unique
}
