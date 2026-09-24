@echo off
setlocal
set "TARGET_ARGS=%*"
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "bash '%~dp0build-linux-arm64-offline.sh' %TARGET_ARGS% --docker"
exit /b %ERRORLEVEL%
