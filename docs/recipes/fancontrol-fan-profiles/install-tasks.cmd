@echo off
rem Double-click this rather than install-tasks.ps1: Windows does not run
rem PowerShell scripts by default, and marks the ones it downloaded. This
rem runs the script beside it with that rule set aside for this one run;
rem nothing on the machine changes.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-tasks.ps1" %*
echo.
pause
