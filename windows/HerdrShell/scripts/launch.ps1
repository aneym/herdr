# launch.ps1 -Exe <path>: start the installed app on Alex's interactive
# desktop via a scheduled task.
param([Parameter(Mandatory = $true)][string]$Exe)
$tr = '"' + $Exe + '"'
schtasks /create /tn HerdrShellLaunch /sc once /st 00:00 /it /f /tr $tr | Out-Null
schtasks /run /tn HerdrShellLaunch | Out-Null
Write-Output "launched: $Exe"
