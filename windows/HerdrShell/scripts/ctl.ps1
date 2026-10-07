# ctl.ps1 -JsonB64 <b64>: send one JSON line to the HerdrShell control pipe,
# print the reply line.
param([Parameter(Mandatory = $true)][string]$JsonB64)
$json = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($JsonB64))
$pipeName = 'herdr-shell-control-' + $env:USERNAME
$client = New-Object System.IO.Pipes.NamedPipeClientStream('.', $pipeName, [System.IO.Pipes.PipeDirection]::InOut)
try {
    $client.Connect(15000)
} catch {
    Write-Error "cannot connect to \\.\pipe\$pipeName : $($_.Exception.Message)"
    exit 1
}
# Connecting can wait up to 15 s; check again right before the command is sent.
. (Join-Path $PSScriptRoot 'gamecheck.ps1')
Stop-IfGame 'ctl'
$writer = New-Object System.IO.StreamWriter($client)
$writer.NewLine = "`n"
$writer.AutoFlush = $true
$reader = New-Object System.IO.StreamReader($client)
$writer.WriteLine($json)
$reply = $reader.ReadLine()
$client.Dispose()
if ($null -eq $reply) { exit 1 }
Write-Output $reply
