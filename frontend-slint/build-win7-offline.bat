@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\build-offline.ps1" %*
exit /b %errorlevel%
