# Removes ROX-AI's scheduled tasks, shortcuts, MCP registrations and program files. Your vault and history are kept.
$AppDir = Join-Path $env:LOCALAPPDATA "ROX-AI"
$Exe = Join-Path $AppDir "bin\roxai.exe"

if (Test-Path $Exe) { & $Exe stop 2>$null; & $Exe remove-tasks 2>$null }
schtasks /Delete /TN "ROX-AI\Daily Chart Scan" /F 2>$null | Out-Null
schtasks /Delete /TN "ROX-AI\Weekly Playtests" /F 2>$null | Out-Null
Get-Process roxai, roxaiw -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

$desktop = [Environment]::GetFolderPath('Desktop')
Remove-Item (Join-Path $desktop "ROX-AI.lnk") -ErrorAction SilentlyContinue
Remove-Item "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\ROX-AI.lnk" -ErrorAction SilentlyContinue

$cfgs = @("$env:APPDATA\Claude\claude_desktop_config.json") + (Get-ChildItem "$env:LOCALAPPDATA\Packages\Claude_*\LocalCache\Roaming\Claude\claude_desktop_config.json" -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
foreach ($c in $cfgs) {
  if (Test-Path $c) {
    try {
      $j = Get-Content $c -Raw | ConvertFrom-Json
      if ($j.mcpServers -and $j.mcpServers.roxai) { $j.mcpServers.PSObject.Properties.Remove('roxai'); $j | ConvertTo-Json -Depth 20 | Set-Content $c -Encoding UTF8 }
    } catch {}
  }
}
if (Get-Command claude -ErrorAction SilentlyContinue) { claude mcp remove roxai -s user 2>$null | Out-Null }

$Bin = Join-Path $AppDir "bin"
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -like "*$Bin*") { [Environment]::SetEnvironmentVariable("Path", (($userPath -split ';') | Where-Object { $_ -ne $Bin }) -join ';', "User") }
Remove-Item -Recurse -Force $Bin, (Join-Path $AppDir "venv"), (Join-Path $AppDir "app") -ErrorAction SilentlyContinue
Write-Host "ROX-AI removed. Your vault (Documents\ROX-AI Vault) and history ($AppDir) were kept." -ForegroundColor Green
