# Keeps Studio's herdr sockets reachable as local named pipes:
# ssh -L forwards Studio's unix sockets to loopback TCP, and herdr-pipe-relay
# serves them as the pipes herdr-studio.cmd points HERDR_SOCKET_PATH at.
param([string]$Host_ = 'studio', [int]$ApiPort = 47461, [int]$ClientPort = 47462)
$here = $PSScriptRoot
$dir = Join-Path $env:LOCALAPPDATA 'herdr-fork\studio'
New-Item -ItemType Directory -Force $dir | Out-Null
$remote = '/Users/aneyman/.config/herdr'
$log = Join-Path $dir 'link.log'

$relay = Start-Process -FilePath "$here\herdr-pipe-relay.exe" -NoNewWindow -PassThru `
    -ArgumentList @("$dir\herdr.sock=127.0.0.1:$ApiPort", "$dir\herdr-client.sock=127.0.0.1:$ClientPort") `
    -RedirectStandardError "$dir\relay.log"
try {
    while ($true) {
        "$(Get-Date -Format o) ssh up" | Add-Content $log
        & ssh.exe -N -T -o ExitOnForwardFailure=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=3 `
            -o BatchMode=yes `
            -L "127.0.0.1:${ApiPort}:$remote/herdr.sock" -L "127.0.0.1:${ClientPort}:$remote/herdr-client.sock" $Host_ 2>> $log
        "$(Get-Date -Format o) ssh exited $LASTEXITCODE" | Add-Content $log
        if ($relay.HasExited) { break }
        Start-Sleep -Seconds 5
    }
} finally {
    if (-not $relay.HasExited) { Stop-Process -Id $relay.Id -Force }
}
