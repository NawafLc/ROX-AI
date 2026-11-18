# ROX-AI one-click installer (re-run any time to repair or update).
# Gets roxai.exe (bundled, downloaded, or built from source), installs Obsidian, Roblox, Claude Desktop and
# Claude Code, then registers the MCP server, scheduled tasks and shortcuts, builds the vault and runs the first scan.
param(
  [switch]$CI,      # automated test mode (no app installs, no sign-in windows)
  [switch]$Update   # started by the app's Update button: no sign-in step, reopen the panel at the end
)
$ErrorActionPreference = "Continue"  # native tools write to stderr; we check exit codes ourselves
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Src    = Split-Path -Parent $PSScriptRoot
$AppDir = Join-Path $env:LOCALAPPDATA "ROX-AI"
$Bin    = Join-Path $AppDir "bin"
$Exe    = Join-Path $Bin "roxai.exe"
$ExeW   = Join-Path $Bin "roxaiw.exe"
$Log    = Join-Path $AppDir "install.log"
$ReleaseZip = "https://github.com/NawafLc/ROX-AI/releases/latest/download/ROX-AI-windows-x64.zip"
New-Item -ItemType Directory -Force -Path $Bin | Out-Null
Start-Transcript -Path $Log -Append | Out-Null

function Step($t) { Write-Host ""; Write-Host "==> $t" -ForegroundColor Cyan }
function Ok($t)   { Write-Host "    OK  $t" -ForegroundColor Green }
function Warn($t) { Write-Host "    !!  $t" -ForegroundColor Yellow }
function Have($c) { [bool](Get-Command $c -ErrorAction SilentlyContinue) }
function Refresh-Path {
  $env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User") + ";$env:USERPROFILE\.local\bin;$env:USERPROFILE\.cargo\bin"
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
  if ($LASTEXITCODE -ne 0) { winget install --id $id -e --silent --accept-package-agreements --accept-source-agreements 2>&1 | Out-Null }
  if ($LASTEXITCODE -eq 0) { Ok "$name installed"; return $true }
  Warn "$name install via winget returned $LASTEXITCODE"; return $false
}

Write-Host ""
if ($Update) { $Host.UI.RawUI.WindowTitle = "Updating ROX-AI"; Write-Host "  Updating ROX-AI - this window closes by itself." -ForegroundColor Green }
Write-Host "  ROX-AI installer" -ForegroundColor Green
Write-Host "  Everything is automatic. You'll only be asked to sign in (Roblox alt account + Claude) at the end."

# ---------------------------------------------------------------- 1. get roxai.exe
Step "ROX-AI program"
# stop running copies (e.g. the MCP server inside Claude Desktop) so the files can be replaced
Get-Process roxai, roxaiw -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500

$found = $null
foreach ($cand in @((Join-Path $Src "roxai.exe"), (Join-Path $Src "target\release\roxai.exe"))) {
  if (Test-Path $cand) { $found = Split-Path -Parent $cand; break }
}
if (-not $found -and -not $CI) {
  Write-Host "    downloading the latest release ..."
  $zip = Join-Path $env:TEMP "ROX-AI-windows-x64.zip"
  $dl = Join-Path $env:TEMP "roxai-dl"
  $okDl = Retry { Invoke-WebRequest $ReleaseZip -OutFile $zip -UseBasicParsing }
  if ($okDl -and (Test-Path $zip)) {
    if (Test-Path $dl) { Remove-Item -Recurse -Force $dl }
    Expand-Archive $zip -DestinationPath $dl -Force
    $exeFound = Get-ChildItem $dl -Recurse -Filter roxai.exe | Select-Object -First 1
    if ($exeFound) { $found = $exeFound.DirectoryName }
  }
}
if (-not $found) {
  Write-Host "    building from source (first time takes ~5-10 minutes) ..."
  Refresh-Path
  if (-not (Have cargo)) {
    $ri = Join-Path $env:TEMP "rustup-init.exe"
    Retry { Invoke-WebRequest "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe" -OutFile $ri -UseBasicParsing } | Out-Null
    # GNU toolchain: self-contained, no Visual Studio needed
    & $ri -y --profile minimal --default-host x86_64-pc-windows-gnu --default-toolchain stable | Out-Null
    Refresh-Path
  }
  if (-not (Have cargo)) { throw "Could not install Rust to build ROX-AI. Check your internet connection and run INSTALL.bat again." }
  Push-Location $Src
  cargo build --release
  $code = $LASTEXITCODE
  Pop-Location
  if ($code -ne 0) { throw "Building ROX-AI failed (see $Log)" }
  $found = Join-Path $Src "target\release"
}
# copy with retries: a just-stopped process can hold the file for a moment
$copied = Retry { Copy-Item (Join-Path $found "roxai.exe") $Exe -Force -ErrorAction Stop } 5
if (-not $copied) { throw "Could not replace roxai.exe (is ROX-AI still running?)" }
if (Test-Path (Join-Path $found "roxaiw.exe")) { Copy-Item (Join-Path $found "roxaiw.exe") $ExeW -Force } else { Copy-Item $Exe $ExeW -Force }
$ver = & $Exe --version
if ($LASTEXITCODE -ne 0) { throw "roxai.exe does not run" }
Ok "$ver installed to $Bin"

# add bin to the user PATH (so `roxai` works in any terminal)
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$Bin*") { [Environment]::SetEnvironmentVariable("Path", "$userPath;$Bin", "User") }

# ---------------------------------------------------------------- 2. apps
if (-not $CI) {
Step "Obsidian"
if (-not (Winget-Install "Obsidian.Obsidian" "Obsidian")) {
  Retry {
    $rel = Invoke-RestMethod "https://api.github.com/repos/obsidianmd/obsidian-releases/releases/latest"
    $asset = $rel.assets | Where-Object { $_.name -match '^Obsidian-[\d\.]+\.exe$' } | Select-Object -First 1
    $f = Join-Path $env:TEMP $asset.name
    Invoke-WebRequest $asset.browser_download_url -OutFile $f -UseBasicParsing
    Start-Process $f -ArgumentList "/S" -Wait
  } | Out-Null
}

Step "Roblox Player"
$robloxFound = (Get-ChildItem "$env:LOCALAPPDATA\Roblox\Versions\*\RobloxPlayerBeta.exe","${env:ProgramFiles(x86)}\Roblox\Versions\*\RobloxPlayerBeta.exe" -ErrorAction SilentlyContinue | Select-Object -First 1)
if ($robloxFound) { Ok "Roblox already installed" }
elseif (-not (Winget-Install "Roblox.Roblox" "Roblox")) {
  Retry {
    $f = Join-Path $env:TEMP "RobloxPlayerInstaller.exe"
    Invoke-WebRequest "https://www.roblox.com/download/client?os=win" -OutFile $f -UseBasicParsing
    Start-Process $f -Wait
  } | Out-Null
}

Step "Claude Desktop"
$cdPkg = Get-ChildItem "$env:LOCALAPPDATA\Packages\Claude_*" -ErrorAction SilentlyContinue
if ((Test-Path "$env:LOCALAPPDATA\AnthropicClaude") -or $cdPkg) { Ok "Claude Desktop already installed" } else { Winget-Install "Anthropic.Claude" "Claude Desktop" | Out-Null }

Step "Claude Code (runs the weekly autopilot with your Claude subscription)"
Refresh-Path
if (-not (Have claude)) {
  Retry { Invoke-RestMethod https://claude.ai/install.ps1 | Invoke-Expression } | Out-Null
  Refresh-Path
}
if (Have claude) { Ok "Claude Code $(claude --version)" } else { Warn "Claude Code not installed - the weekly autopilot needs it (manual playtests from Claude Desktop still work)" }
} # end apps

# ---------------------------------------------------------------- 3. integration
Step "Vault, Claude registration, scheduled tasks, shortcuts"
& $Exe setup
if ($LASTEXITCODE -ne 0) { Warn "setup reported a problem (see log)" }

Step "Chart scan (about a minute)"
if ($Update) { & $Exe scan --if-needed } else { & $Exe scan }
if ($LASTEXITCODE -ne 0) { Warn "first scan failed - it will retry automatically every day" }

Step "Health check"
& $Exe doctor

# leftovers from the old Python version (settings, vault and history are kept)
Remove-Item -Recurse -Force (Join-Path $AppDir "venv"), (Join-Path $AppDir "app") -ErrorAction SilentlyContinue

if ($CI) { Write-Host "CI mode: done."; Stop-Transcript | Out-Null; exit 0 }

if ($Update) {
  Start-Process $ExeW -ArgumentList "panel" -WorkingDirectory $AppDir
  Write-Host ""
  Write-Host "ROX-AI updated to $ver. Your settings, vault and history were kept." -ForegroundColor Green
  Write-Host "(If Claude Desktop is open, restart it once so it uses the new version.)" -ForegroundColor Green
  Stop-Transcript | Out-Null
  Start-Sleep -Seconds 6
  exit 0
}

# ---------------------------------------------------------------- 4. sign-ins (the only manual part)
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
Write-Host "     (If Claude Desktop was already open, restart it once.)" -ForegroundColor White
Write-Host "==================================================================" -ForegroundColor Green
Start-Process "https://www.roblox.com/login"
if (Have claude) {
  Start-Process powershell -ArgumentList "-NoExit","-Command","Write-Host 'Log in to Claude Code, then type /exit' -ForegroundColor Green; claude"
}

# ---------------------------------------------------------------- 5. open
& $Exe open-vault
Start-Process $ExeW -ArgumentList "panel" -WorkingDirectory $AppDir
Write-Host ""
Write-Host "ROX-AI is installed. Desktop shortcut: 'ROX-AI'." -ForegroundColor Green
Write-Host "Charts are scanned daily; Claude playtests the top games weekly when your PC is idle." -ForegroundColor Green
Stop-Transcript | Out-Null
