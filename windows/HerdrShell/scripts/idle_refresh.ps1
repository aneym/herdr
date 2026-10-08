# idle_refresh.ps1: (re)create the HerdrShellIdle interactive task, run it,
# and print the resulting idle.json. The task starts wscript, which runs
# powershell with window style 0 (hidden from creation), so no console ever
# shows on Alex's desktop; powershell -WindowStyle Hidden alone flashes one.
$scripts = 'C:\Users\aneym\winshell\scripts'
$vbs = Join-Path $scripts 'idle.vbs'
$cmd = "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $scripts\idle.ps1"
Set-Content -LiteralPath $vbs -Encoding ASCII -Value "CreateObject(""WScript.Shell"").Run ""$cmd"", 0, True"
schtasks /create /tn HerdrShellIdle /sc once /st 00:00 /it /f /tr "wscript.exe //B $vbs" | Out-Null
# gated.ps1 checked for a game already; check again right before the task starts.
. (Join-Path $PSScriptRoot 'gamecheck.ps1')
Stop-IfGame 'HerdrShellIdle'
schtasks /run /tn HerdrShellIdle | Out-Null
Start-Sleep -Milliseconds 3000
$f = Join-Path $env:LOCALAPPDATA 'HerdrShell\idle.json'
if (Test-Path $f) { Get-Content $f -Raw } else { Write-Output '{}' }
