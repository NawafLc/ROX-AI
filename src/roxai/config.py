"""Settings and filesystem locations for ROX-AI.

Everything lives in one per-user folder so the app never needs admin rights:
  Windows: %LOCALAPPDATA%\\ROX-AI
  other:   ~/.roxai          (used for development/tests only)
Override with the ROXAI_HOME environment variable.
"""
from __future__ import annotations

import json
import os
import sys
import threading
from dataclasses import asdict, dataclass, field, fields
from pathlib import Path

IS_WINDOWS = sys.platform == "win32"


def app_dir() -> Path:
    env = os.environ.get("ROXAI_HOME")
    if env:
        p = Path(env)
    elif IS_WINDOWS:
        p = Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData" / "Local")) / "ROX-AI"
    else:
        p = Path.home() / ".roxai"
    p.mkdir(parents=True, exist_ok=True)
    return p


def default_vault_path() -> Path:
    if IS_WINDOWS:
        docs = Path(os.environ.get("USERPROFILE", Path.home())) / "Documents"
    else:
        docs = Path.home() / "Documents"
    return docs / "ROX-AI Vault"


@dataclass
class Settings:
    # --- vault ---
    vault_path: str = field(default_factory=lambda: str(default_vault_path()))

    # --- charts ---
    sort_id: str = "top-trending"            # main chart the app follows
    chart_limit: int = 25                    # games per run
    extra_sorts: list[str] = field(default_factory=lambda: ["top-playing-now", "up-and-coming"])  # tracked for trends only
    country: str = "all"

    # --- playtesting ---
    playtest_minutes: int = 12               # in-game time budget per game
    playtests_per_run: int = 8               # max games per weekly run (protects your Claude usage limits)
    replay_after_days: int = 28              # re-playtest a game after this many days
    replay_if_updated: bool = True           # re-playtest sooner if the dev pushed an update since last playtest
    playtest_interval_days: int = 7          # "weekly"
    idle_required_minutes: int = 3           # don't take over mouse/keyboard unless PC idle this long
    screenshot_width: int = 1280
    jpeg_quality: int = 70
    window_width: int = 1280                 # Roblox window client size we try to set
    window_height: int = 720

    # --- Claude Code CLI (autopilot) ---
    claude_cli: str = ""                     # auto-detected when empty
    claude_model: str = ""                   # empty = your Claude Code default
    max_turns: int = 160
    per_game_timeout_minutes: int = 35

    # --- schedule (Windows Task Scheduler, local time) ---
    scan_time: str = "10:17"
    playtest_check_time: str = "14:23"

    # --- safety ---
    kill_hotkey: str = "ctrl+shift+q"        # hold to abort a running playtest instantly
    allow_chat: bool = False                 # Claude may not type into public chat unless enabled

    @classmethod
    def path(cls) -> Path:
        return app_dir() / "settings.json"

    @classmethod
    def load(cls) -> "Settings":
        p = cls.path()
        s = cls()
        if p.exists():
            try:
                raw = json.loads(p.read_text(encoding="utf-8"))
                known = {f.name for f in fields(cls)}
                for k, v in raw.items():
                    if k in known:
                        setattr(s, k, v)
            except Exception:
                # Corrupt settings must never brick the app: keep a backup, use defaults.
                p.replace(p.with_suffix(".corrupt.json"))
        s.save()
        return s

    def save(self) -> None:
        p = self.path()
        tmp = p.with_suffix(".tmp")
        tmp.write_text(json.dumps(asdict(self), indent=2), encoding="utf-8")
        tmp.replace(p)

    @property
    def vault(self) -> Path:
        return Path(self.vault_path)


_lock = threading.Lock()
_settings: Settings | None = None


def settings() -> Settings:
    global _settings
    with _lock:
        if _settings is None:
            _settings = Settings.load()
        return _settings


def reload() -> Settings:
    global _settings
    with _lock:
        _settings = Settings.load()
        return _settings


def logs_dir() -> Path:
    p = app_dir() / "logs"
    p.mkdir(exist_ok=True)
    return p


def state_dir() -> Path:
    p = app_dir() / "state"
    p.mkdir(exist_ok=True)
    return p


def db_path() -> Path:
    return app_dir() / "roxai.db"
