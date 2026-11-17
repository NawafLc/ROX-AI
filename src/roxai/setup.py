"""One-shot system integration (run by the installer, safe to re-run any time):
  * create the vault (+ Dataview) and register it in Obsidian
  * register the MCP server in Claude Desktop (all install variants) and Claude Code
  * create Windows scheduled tasks (daily scan, hourly-retrying playtest check)
  * desktop + Start-menu shortcuts with an app icon
"""
from __future__ import annotations

import json
import os
import secrets
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from xml.sax.saxutils import escape

import psutil

from . import log
from . import vault as V
from .autopilot import CREATE_NO_WINDOW, find_claude, python_exe
from .config import IS_WINDOWS, app_dir, settings

L = log.get("setup")
TASK_FOLDER = "ROX-AI"


def pythonw_exe() -> str:
    exe = Path(python_exe())
    w = exe.with_name("pythonw.exe")
    return str(w if w.exists() else exe)


def _run(cmd: list[str], timeout: int = 120) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout,
                          creationflags=CREATE_NO_WINDOW if IS_WINDOWS else 0)


# ------------------------------------------------------------ Claude ------

def claude_desktop_config_paths() -> list[Path]:
    paths: list[Path] = []
    appdata = os.environ.get("APPDATA")
    if appdata:
        paths.append(Path(appdata) / "Claude" / "claude_desktop_config.json")
    la = os.environ.get("LOCALAPPDATA")
    if la:
        for pkg in (Path(la) / "Packages").glob("Claude_*"):
            paths.append(pkg / "LocalCache" / "Roaming" / "Claude" / "claude_desktop_config.json")
    if sys.platform == "darwin":
        paths.append(Path.home() / "Library" / "Application Support" / "Claude" / "claude_desktop_config.json")
    return paths


def mcp_entry() -> dict:
    return {"command": python_exe(), "args": ["-m", "roxai.mcp_server"], "env": {"ROXAI_HOME": str(app_dir())}}


def register_claude_desktop() -> list[str]:
    done = []
    paths = claude_desktop_config_paths()
    existing_dirs = [p for p in paths if p.parent.exists()]
    for p in existing_dirs or paths[:1]:
        try:
            p.parent.mkdir(parents=True, exist_ok=True)
            cfg = {}
            if p.exists():
                try:
                    cfg = json.loads(p.read_text(encoding="utf-8") or "{}")
                except ValueError:
                    p.with_suffix(".backup.json").write_text(p.read_text(encoding="utf-8"), encoding="utf-8")
                    cfg = {}
            cfg.setdefault("mcpServers", {})["roxai"] = mcp_entry()
            p.write_text(json.dumps(cfg, indent=2), encoding="utf-8")
            done.append(str(p))
        except OSError as e:
            L.warning("could not write %s: %s", p, e)
    return done


def register_claude_code() -> str:
    claude = find_claude()
    if not claude:
        return "Claude Code CLI not found (skipped)"
    try:
        _run([claude, "mcp", "remove", "roxai", "-s", "user"], 60)
        e = mcp_entry()
        r = _run([claude, "mcp", "add", "-s", "user", "-e", f"ROXAI_HOME={app_dir()}", "roxai", "--", e["command"], *e["args"]], 60)
        return "registered" if r.returncode == 0 else f"failed: {(r.stderr or r.stdout).strip()[:300]}"
    except Exception as ex:  # noqa: BLE001
        return f"failed: {ex}"


# ---------------------------------------------------------- scheduler -----

TASK_XML = """<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>{desc}</Description><Author>ROX-AI</Author></RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>{start}</StartBoundary>
      <Enabled>true</Enabled>
      {repetition}
      <ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay>
    </CalendarTrigger>
    <LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId><Delay>{logon_delay}</Delay></LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>true</RunOnlyIfNetworkAvailable>
    <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>{limit}</ExecutionTimeLimit>
    <Priority>7</Priority>
    <RestartOnFailure><Interval>PT15M</Interval><Count>3</Count></RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec><Command>{cmd}</Command><Arguments>{args}</Arguments><WorkingDirectory>{cwd}</WorkingDirectory></Exec>
  </Actions>
</Task>
"""


def _current_user() -> str:
    dom = os.environ.get("USERDOMAIN", "")
    usr = os.environ.get("USERNAME", "")
    return f"{dom}\\{usr}" if dom else usr


def _create_task(name: str, desc: str, time_hhmm: str, args: str, limit: str, logon_delay: str,
                 repetition: str = "") -> str:
    start = time.strftime("%Y-%m-%d") + f"T{time_hhmm}:00"
    xml = TASK_XML.format(desc=escape(desc), start=start, repetition=repetition, user=escape(_current_user()),
                          logon_delay=logon_delay, limit=limit, cmd=escape(pythonw_exe()), args=escape(args),
                          cwd=escape(str(app_dir())))
    with tempfile.NamedTemporaryFile("w", suffix=".xml", delete=False, encoding="utf-16") as f:
        f.write(xml)
        tmp = f.name
    try:
        r = _run(["schtasks", "/Create", "/TN", f"{TASK_FOLDER}\\{name}", "/XML", tmp, "/F"])
        return "ok" if r.returncode == 0 else f"failed: {(r.stderr or r.stdout).strip()[:300]}"
    finally:
        os.unlink(tmp)


def create_tasks() -> dict:
    if not IS_WINDOWS:
        return {"skipped": "not Windows"}
    s = settings()
    return {
        "Daily Chart Scan": _create_task(
            "Daily Chart Scan", "ROX-AI: refresh Roblox charts, trends and the Obsidian vault (no Claude needed).",
            s.scan_time, "-m roxai.cli scan --if-needed", "PT1H", "PT5M"),
        "Weekly Playtests": _create_task(
            "Weekly Playtests", "ROX-AI: checks hourly if the weekly AI playtest run is due and the PC is idle, then runs it.",
            s.playtest_check_time, "-m roxai.cli autopilot --scheduled", "PT11H", "PT20M",
            repetition="<Repetition><Interval>PT1H</Interval><Duration>PT12H</Duration><StopAtDurationEnd>false</StopAtDurationEnd></Repetition>"),
    }


def remove_tasks() -> None:
    for n in ("Daily Chart Scan", "Weekly Playtests"):
        _run(["schtasks", "/Delete", "/TN", f"{TASK_FOLDER}\\{n}", "/F"])


def task_status(name: str) -> str | None:
    if not IS_WINDOWS:
        return None
    r = _run(["schtasks", "/Query", "/TN", f"{TASK_FOLDER}\\{name}", "/FO", "LIST"])
    if r.returncode != 0:
        return None
    for line in r.stdout.splitlines():
        if line.strip().lower().startswith("status:"):
            return line.split(":", 1)[1].strip()
    return "exists"


def run_task_now(name: str) -> bool:
    return _run(["schtasks", "/Run", "/TN", f"{TASK_FOLDER}\\{name}"]).returncode == 0


# ---------------------------------------------------------- shortcuts -----

def make_icon() -> Path:
    from PIL import Image, ImageDraw, ImageFont

    p = app_dir() / "roxai.ico"
    if p.exists():
        return p
    img = Image.new("RGBA", (256, 256), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([8, 8, 248, 248], 56, fill=(20, 24, 38, 255), outline=(0, 220, 170, 255), width=10)
    try:
        font = ImageFont.truetype("arialbd.ttf", 110)
    except OSError:
        font = ImageFont.load_default()
    d.text((128, 132), "RX", fill=(0, 230, 180, 255), font=font, anchor="mm")
    img.save(p, sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    return p


def _shortcut(lnk: Path, target: str, args: str, icon: Path, desc: str) -> bool:
    ps = (f"$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{lnk}');"
          f"$s.TargetPath='{target}';$s.Arguments='{args}';$s.WorkingDirectory='{app_dir()}';"
          f"$s.IconLocation='{icon}';$s.Description='{desc}';$s.Save()")
    r = _run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", ps])
    return r.returncode == 0


def create_shortcuts() -> list[str]:
    if not IS_WINDOWS:
        return []
    icon = make_icon()
    out = []
    desktop = Path(os.environ.get("USERPROFILE", Path.home())) / "Desktop"
    try:
        r = _run(["powershell", "-NoProfile", "-Command", "[Environment]::GetFolderPath('Desktop')"])
        if r.stdout.strip():
            desktop = Path(r.stdout.strip())
    except Exception:  # noqa: BLE001
        pass
    start = Path(os.environ.get("APPDATA", "")) / "Microsoft" / "Windows" / "Start Menu" / "Programs"
    for folder in (desktop, start):
        if folder.exists():
            lnk = folder / "ROX-AI.lnk"
            if _shortcut(lnk, pythonw_exe(), "-m roxai.panel", icon, "ROX-AI control panel"):
                out.append(str(lnk))
    return out


# ------------------------------------------------------------- Obsidian ---

def obsidian_exe() -> Path | None:
    for c in [Path(os.environ.get("LOCALAPPDATA", "")) / "Programs" / "Obsidian" / "Obsidian.exe",
              Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "Obsidian" / "Obsidian.exe"]:
        if c.exists():
            return c
    return None


def register_obsidian_vault() -> str:
    cfg = Path(os.environ.get("APPDATA", "")) / "obsidian" / "obsidian.json"
    vault = str(settings().vault)
    running = any((p.info.get("name") or "").lower() == "obsidian.exe" for p in psutil.process_iter(["name"]))
    if running:
        return "Obsidian is running — the vault will be opened via link instead"
    try:
        cfg.parent.mkdir(parents=True, exist_ok=True)
        data = json.loads(cfg.read_text(encoding="utf-8")) if cfg.exists() else {}
        vaults = data.setdefault("vaults", {})
        if any(os.path.normcase(v.get("path", "")) == os.path.normcase(vault) for v in vaults.values()):
            return "already registered"
        for v in vaults.values():
            v.pop("open", None)
        vaults[secrets.token_hex(8)] = {"path": vault, "ts": int(time.time() * 1000), "open": True}
        cfg.write_text(json.dumps(data), encoding="utf-8")
        return "registered"
    except Exception as e:  # noqa: BLE001
        return f"failed: {e}"


def open_vault() -> None:
    from urllib.parse import quote
    path = str(settings().vault)
    if IS_WINDOWS:
        try:
            os.startfile(f"obsidian://open?path={quote(path)}")  # type: ignore[attr-defined]
            return
        except OSError:
            os.startfile(path)  # type: ignore[attr-defined]


# ---------------------------------------------------------------- all -----

def setup_all(open_after: bool = False) -> dict:
    res: dict = {}
    V.init_vault()
    res["vault"] = str(settings().vault)
    res["obsidian"] = register_obsidian_vault() if IS_WINDOWS else "skipped"
    res["claude_desktop"] = register_claude_desktop()
    res["claude_code"] = register_claude_code()
    res["tasks"] = create_tasks()
    res["shortcuts"] = create_shortcuts()
    L.info("setup: %s", res)
    if open_after:
        open_vault()
    return res
