# idle_refresh.ps1: (re)create the HerdrShellIdle interactive task, run it,
# and print the resulting idle.json.
$tr = 'powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File C:\Users\aneym\winshell\scripts\idle.ps1'
schtasks /create /tn HerdrShellIdle /sc once /st 00:00 /it /f /tr $tr | Out-Null
schtasks /run /tn HerdrShellIdle | Out-Null
Start-Sleep -Milliseconds 3000
$f = Join-Path $env:LOCALAPPDATA 'HerdrShell\idle.json'
if (Test-Path $f) { Get-Content $f -Raw } else { Write-Output '{}' }
