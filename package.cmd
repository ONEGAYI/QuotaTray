@echo off
setlocal
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\package.ps1" %*
set "QUOTATRAY_PACKAGE_EXIT=%ERRORLEVEL%"
endlocal & exit /b %QUOTATRAY_PACKAGE_EXIT%
