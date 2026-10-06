@echo off
rem Runs herdr against Studio's server through studio-link.ps1.
set "HERDR_SOCKET_PATH=%LOCALAPPDATA%\herdr-fork\studio\herdr.sock"
set "HERDR_CLIENT_SOCKET_PATH=%LOCALAPPDATA%\herdr-fork\studio\herdr-client.sock"
"%~dp0herdr.exe" %*
