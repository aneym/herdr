# launch.ps1 -Exe <path> [-TestWindow]: start the installed app on Alex's interactive
# desktop via a scheduled task. ScheduledTask cmdlets keep paths with spaces intact
# (schtasks /tr loses the inner quotes under PowerShell 5.1).
param([Parameter(Mandatory = $true)][string]$Exe, [switch]$TestWindow)
$Exe = $Exe.Trim('"')
$arg = if ($TestWindow) { '--test-window' } else { '' }
$action = if ($arg) { New-ScheduledTaskAction -Execute $Exe -Argument $arg } else { New-ScheduledTaskAction -Execute $Exe }
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero)
Register-ScheduledTask -TaskName HerdrShellLaunch -Action $action -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName HerdrShellLaunch
Write-Output "launched: $Exe $arg"
