@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "bash '%~dp0build-linux-amd64-offline.sh' --docker"
exit /b %ERRORLEVEL%
