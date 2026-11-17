@echo off
title ROX-AI Installer
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\install.ps1"
if errorlevel 1 (
  echo.
  echo Something failed. A log is in %LOCALAPPDATA%\ROX-AI\install.log
)
echo.
pause
