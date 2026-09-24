@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0..\builders\build-win7-offline.ps1" %*
exit /b %errorlevel%
