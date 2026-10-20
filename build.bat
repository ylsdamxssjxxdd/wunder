@echo off
setlocal
if /I "%~1"=="pack" goto :pack

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0builders\build.ps1" %*
set "RC=%ERRORLEVEL%"
if not "%RC%"=="0" pause
exit /b %RC%

:pack
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0builders\pack.ps1" %*
set "RC=%ERRORLEVEL%"
if not "%RC%"=="0" pause
exit /b %RC%
