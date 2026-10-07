# gated.ps1 -Script <helper.ps1> [-ArgsB64 <base64 JSON string array>]: run one
# helper only when no game runs. The check and the action share this one
# process, so a match that starts between a separate guard call and the action
# cannot slip through. Exits 75 without running the helper when a game is up.
# League lobby processes (LeagueClientUx*, "Riot Client") are not games.
param([Parameter(Mandatory = $true)][string]$Script, [string]$ArgsB64 = '')
. (Join-Path $PSScriptRoot 'gamecheck.ps1')
Stop-IfGame $Script
$helper = Join-Path $PSScriptRoot $Script
if (!(Test-Path -LiteralPath $helper -PathType Leaf)) { Write-Error "no helper $Script"; exit 1 }
$list = @()
if ($ArgsB64) {
    $json = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($ArgsB64))
    $list = @($json | ConvertFrom-Json)
}
# A child process binds the helper's named parameters exactly as a direct -File call would.
& powershell -NoProfile -ExecutionPolicy Bypass -File $helper @list
exit $LASTEXITCODE
