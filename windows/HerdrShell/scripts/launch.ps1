# launch.ps1 -Exe <path> [-TestWindow]: start the installed app on Alex's interactive
# desktop via a scheduled task. ScheduledTask cmdlets keep paths with spaces intact
# (schtasks /tr loses the inner quotes under PowerShell 5.1).
param([Parameter(Mandatory = $true)][string]$Exe, [switch]$TestWindow)
$Exe = $Exe.Trim('"')
$arg = if ($TestWindow) { '--test-window' } else { '' }
# Scheduled tasks do not inherit the SSH helper's environment. Set the hook-only
# override in the task process for test windows only; install and --relaunch
# restarts are Alex's own window and keep browser defaults and throttling.
$quotedExe = $Exe.Replace("'", "''")
$control = if ($TestWindow) { "`$env:HERDR_SHELL_CONTROL='1'; " } else { '' }
$command = "$control& '$quotedExe' $arg"
$encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
$action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -WindowStyle Hidden -EncodedCommand $encoded"
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero)
Register-ScheduledTask -TaskName HerdrShellLaunch -Action $action -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName HerdrShellLaunch
Write-Output "launched: $Exe $arg"
