# gated.ps1 -Script <helper.ps1> [-ArgsB64 <base64 JSON string array>]: run one
# helper only when no game runs. The check and the action share this one
# process, which shrinks the window a match could start in to the helper's own
# run; ctl.ps1 checks again right before it sends. Exits 75 without running the
# helper when a game is up or the process list cannot be read.
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
