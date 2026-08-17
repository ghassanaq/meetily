@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0start-live-assist.ps1"
if errorlevel 1 pause
