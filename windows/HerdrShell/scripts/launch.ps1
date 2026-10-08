# launch.ps1 -Exe <path> [-TestWindow] [-Background]: start the installed app on Alex's interactive
# desktop via a scheduled task. ScheduledTask cmdlets keep paths with spaces intact
# (schtasks /tr loses the inner quotes under PowerShell 5.1).
# The task runs wscript, which starts powershell with window style 0 (hidden from
# creation); powershell -WindowStyle Hidden alone flashes a console on Alex's desktop.
# That hidden powershell starts the app with Start-Process, so the app gets fresh
# startup info and its window opens normally.
param([Parameter(Mandatory = $true)][string]$Exe, [switch]$TestWindow, [switch]$Background,
      [string]$TaskName = 'HerdrShellLaunch', [string]$ScriptDir,
      [string]$TestArguments)
# PowerShell 5.1 may not populate $PSScriptRoot while binding param defaults.
$HelperDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $ScriptDir) { $ScriptDir = $HelperDir }
if (-not $ScriptDir -or -not (Test-Path -LiteralPath $ScriptDir -PathType Container)) {
    throw 'Cannot resolve launch script directory'
}
if (-not $HelperDir) { throw 'Cannot resolve launch helper directory' }
# Test overrides isolate the task, generated files and harmless stand-in arguments.
if ($TestArguments -and ($TaskName -eq 'HerdrShellLaunch' -or $ScriptDir -eq $HelperDir)) {
    throw 'TestArguments requires an isolated TaskName and ScriptDir'
}
$Exe = $Exe.Trim('"')
# --background: the window opens without taking focus (install relaunches).
$arg = if ($TestWindow) { '--test-window' } else { '--background' }
# Scheduled tasks do not inherit the SSH helper's environment. Set the hook-only
# override in the task process for test windows only; install and --relaunch
# restarts are Alex's own window and keep browser defaults and throttling.
$quotedExe = $Exe.Replace("'", "''")
$control = "`$ErrorActionPreference = 'Stop'`r`n"
if ($TestWindow) { $control += "`$env:HERDR_SHELL_CONTROL='1'`r`n" }
$quotedGamecheck = (Join-Path $HelperDir 'gamecheck.ps1').Replace("'", "''")
$control += ". '$quotedGamecheck'`r`nStop-IfGame 'HerdrShellLaunch'`r`n"
if ($TestArguments) { $arg = $TestArguments.Replace("'", "''") }
$start = "Start-Process -FilePath '$quotedExe' -ArgumentList '$arg'"
if ($TestArguments) { $start += ' -WindowStyle Hidden' }
$ps1 = Join-Path $ScriptDir 'launch-task.ps1'
$vbs = Join-Path $ScriptDir 'launch-task.vbs'
# UTF-16 with a BOM so non-ASCII paths survive both Windows PowerShell 5.1 and wscript.
Set-Content -LiteralPath $ps1 -Encoding Unicode -Value "$control$start"
$cmd = "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File ""$ps1"""
# The task's last result is powershell's exit code, so a failed Start-Process shows there.
Set-Content -LiteralPath $vbs -Encoding Unicode -Value ('WScript.Quit CreateObject("WScript.Shell").Run("' + $cmd.Replace('"', '""') + '", 0, True)')
$action = New-ScheduledTaskAction -Execute 'wscript.exe' -Argument "//B ""$vbs"""
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero)
Register-ScheduledTask -TaskName $TaskName -Action $action -Principal $principal -Settings $settings -Force | Out-Null
# A game may have started while the task was registered; check right before it runs.
. (Join-Path $HelperDir 'gamecheck.ps1')
Stop-IfGame 'HerdrShellLaunch'
Start-ScheduledTask -TaskName $TaskName
Write-Output "launched: $Exe $arg"
