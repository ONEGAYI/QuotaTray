@echo off
setlocal
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\clean.ps1" %*
set "QUOTATRAY_CLEAN_EXIT=%ERRORLEVEL%"
endlocal & exit /b %QUOTATRAY_CLEAN_EXIT%
