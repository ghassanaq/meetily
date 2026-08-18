@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0test-live-assist-voice.ps1"
exit /b %ERRORLEVEL%
