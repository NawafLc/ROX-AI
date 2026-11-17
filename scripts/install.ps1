# ROX-AI one-click installer (re-run any time to repair or update).
# Installs: uv + Python 3.12 (private), ROX-AI, Obsidian, Roblox, Claude Desktop, Claude Code,
# then registers the MCP server, scheduled tasks, shortcuts, builds the vault and runs the first scan.
param([switch]$CI)  # -CI: automated test mode (no app installs, no sign-in windows)
$ErrorActionPreference = "Continue"  # native tools write to stderr; we check exit codes ourselves
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Src     = Split-Path -Parent $PSScriptRoot
$AppDir  = Join-Path $env:LOCALAPPDATA "ROX-AI"
$Code    = Join-Path $AppDir "app"
$Venv    = Join-Path $AppDir "venv"
$Py      = Join-Path $Venv "Scripts\python.exe"
$Log     = Join-Path $AppDir "install.log"
New-Item -ItemType Directory -Force -Path $AppDir | Out-Null
Start-Transcript -Path $Log -Append | Out-Null

function Step($t) { Write-Host ""; Write-Host "==> $t" -ForegroundColor Cyan }
function Ok($t)   { Write-Host "    OK  $t" -ForegroundColor Green }
function Warn($t) { Write-Host "    !!  $t" -ForegroundColor Yellow }
function Have($c) { [bool](Get-Command $c -ErrorAction SilentlyContinue) }
function Refresh-Path {
  $env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User") + ";$env:USERPROFILE\.local\bin"
}
function Retry([scriptblock]$Block, [int]$Times = 3) {
  for ($i = 1; $i -le $Times; $i++) {
    try { & $Block; return $true } catch { Warn "attempt $i failed: $($_.Exception.Message)"; Start-Sleep -Seconds (5 * $i) }
  }
  return $false
}
function Winget-Install($id, $name) {
  if (-not (Have winget)) { Warn "winget not available - skipping $name"; return $false }
  $listed = winget list --id $id -e --accept-source-agreements 2>$null | Out-String
  if ($listed -match [regex]::Escape($id)) { Ok "$name already installed"; return $true }
  Write-Host "    installing $name ..."
  winget install --id $id -e --silent --accept-package-agreements --accept-source-agreements --scope user 2>&1 | Out-Null
  if ($LASTEXITCODE -ne 0) {
    winget install --id $id -e --silent --accept-package-agreements --accept-source-agreements 2>&1 | Out-Null
  }
  if ($LASTEXITCODE -eq 0) { Ok "$name installed"; return $true }
  Warn "$name install via winget returned $LASTEXITCODE"; return $false
}

Write-Host ""
Write-Host "  ROX-AI installer" -ForegroundColor Green
Write-Host "  Everything is automatic. You'll only be asked to sign in (Roblox alt account + Claude) at the end."

# ---------------------------------------------------------------- 1. uv + python
Step "Python toolchain (uv)"
Refresh-Path
if (-not (Have uv)) {
  $ok = Retry { Invoke-RestMethod https://astral.sh/uv/install.ps1 | Invoke-Expression }
  Refresh-Path
  if (-not (Have uv)) { throw "Could not install uv (needed for Python). Check your internet connection and run INSTALL.bat again." }
}
Ok "uv $(uv --version)"

# ---------------------------------------------------------------- 2. app copy + venv
Step "Installing ROX-AI into $AppDir"
if (Test-Path $Code) { Remove-Item -Recurse -Force $Code }
New-Item -ItemType Directory -Force -Path $Code | Out-Null
robocopy $Src $Code /E /XD .git .venv __pycache__ .pytest_cache node_modules /NFL /NDL /NJH /NJS /NP | Out-Null
if (-not (Test-Path $Py)) {
  uv venv --python 3.12 $Venv
  if ($LASTEXITCODE -ne 0) { throw "Could not create the Python environment" }
}
$ok = Retry { uv pip install --python $Py --upgrade $Code; if ($LASTEXITCODE -ne 0) { throw "pip failed" } }
if (-not $ok) { throw "Could not install ROX-AI's Python packages" }
& $Py -c "import roxai, mcp, mss, PIL, psutil, httpx; print('roxai', roxai.__version__)"
if ($LASTEXITCODE -ne 0) { throw "ROX-AI import check failed" }
Ok "ROX-AI installed"

# ---------------------------------------------------------------- 3. apps
if (-not $CI) {
Step "Obsidian"
if (-not (Winget-Install "Obsidian.Obsidian" "Obsidian")) {
  Retry {
    $rel = Invoke-RestMethod "https://api.github.com/repos/obsidianmd/obsidian-releases/releases/latest"
    $asset = $rel.assets | Where-Object { $_.name -match '^Obsidian-[\d\.]+\.exe$' } | Select-Object -First 1
    $f = Join-Path $env:TEMP $asset.name
    Invoke-WebRequest $asset.browser_download_url -OutFile $f
    Start-Process $f -ArgumentList "/S" -Wait
  } | Out-Null
}

Step "Roblox Player"
$robloxFound = (Get-ChildItem "$env:LOCALAPPDATA\Roblox\Versions\*\RobloxPlayerBeta.exe","${env:ProgramFiles(x86)}\Roblox\Versions\*\RobloxPlayerBeta.exe" -ErrorAction SilentlyContinue | Select-Object -First 1)
if ($robloxFound) { Ok "Roblox already installed" }
else {
  if (-not (Winget-Install "Roblox.Roblox" "Roblox")) {
    Retry {
      $f = Join-Path $env:TEMP "RobloxPlayerInstaller.exe"
      Invoke-WebRequest "https://www.roblox.com/download/client?os=win" -OutFile $f
      Start-Process $f -Wait
    } | Out-Null
  }
}

Step "Claude Desktop"
$cd = Test-Path "$env:LOCALAPPDATA\AnthropicClaude" -PathType Container
$cdPkg = Get-ChildItem "$env:LOCALAPPDATA\Packages\Claude_*" -ErrorAction SilentlyContinue
if ($cd -or $cdPkg) { Ok "Claude Desktop already installed" } else { Winget-Install "Anthropic.Claude" "Claude Desktop" | Out-Null }

Step "Claude Code (runs the weekly autopilot with your Claude subscription)"
Refresh-Path
if (-not (Have claude)) {
  Retry { Invoke-RestMethod https://claude.ai/install.ps1 | Invoke-Expression } | Out-Null
  Refresh-Path
}
if (Have claude) { Ok "Claude Code $(claude --version)" } else { Warn "Claude Code not installed - the weekly autopilot needs it (manual playtests from Claude Desktop still work)" }

} # end apps

# ---------------------------------------------------------------- 4. integration
Step "Vault, Claude registration, scheduled tasks, shortcuts"
& $Py -m roxai.cli setup
if ($LASTEXITCODE -ne 0) { Warn "setup reported a problem (see log)" }

Step "First chart scan (1-3 minutes)"
& $Py -m roxai.cli scan
if ($LASTEXITCODE -ne 0) { Warn "first scan failed - it will retry automatically every day" }

Step "Health check"
& $Py -m roxai.cli doctor

if ($CI) { Write-Host "CI mode: done."; Stop-Transcript | Out-Null; exit 0 }

# ---------------------------------------------------------------- 5. sign-ins (the only manual part)
Write-Host ""
Write-Host "==================================================================" -ForegroundColor Green
Write-Host "  Almost done - just 2 sign-ins (one time only):" -ForegroundColor Green
Write-Host ""
Write-Host "  1) ROBLOX: a browser opens. Log in with your *ALT* account" -ForegroundColor White
Write-Host "     (never your main), then click Play on any game once so the" -ForegroundColor White
Write-Host "     Roblox app remembers the login. Close the game afterwards." -ForegroundColor White
Write-Host ""
Write-Host "  2) CLAUDE: a window opens running 'claude'. Log in with your" -ForegroundColor White
Write-Host "     Claude account, then type /exit. Also open Claude Desktop" -ForegroundColor White
Write-Host "     once and sign in - the 'roxai' tools will be there." -ForegroundColor White
Write-Host "==================================================================" -ForegroundColor Green
Start-Process "https://www.roblox.com/login"
if (Have claude) {
  Start-Process powershell -ArgumentList "-NoExit","-Command","Write-Host 'Log in to Claude Code, then type /exit' -ForegroundColor Green; claude"
}

# ---------------------------------------------------------------- 6. open
& $Py -m roxai.cli open-vault
Start-Process (Join-Path $Venv "Scripts\pythonw.exe") -ArgumentList "-m","roxai.panel" -WorkingDirectory $AppDir
Write-Host ""
Write-Host "ROX-AI is installed. Desktop shortcut: 'ROX-AI'." -ForegroundColor Green
Write-Host "Charts are scanned daily; Claude playtests the top games weekly when your PC is idle." -ForegroundColor Green
Stop-Transcript | Out-Null
