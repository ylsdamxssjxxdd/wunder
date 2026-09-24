@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "bash '%~dp0build-win32-arm64-offline.sh' --docker"
exit /b %ERRORLEVEL%
