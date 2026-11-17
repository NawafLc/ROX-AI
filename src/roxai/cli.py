"""`roxai` command line. Also what the scheduled tasks call."""
from __future__ import annotations

import argparse
import json
import sys

from . import log
from .config import reload, settings
from .db import get_db, today

L = log.get("cli")


def _print(o) -> None:
    try:
        print(o if isinstance(o, str) else json.dumps(o, indent=2, default=str, ensure_ascii=False))
    except (UnicodeEncodeError, AttributeError, OSError):
        pass  # pythonw has no console


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="roxai", description="ROX-AI — Roblox chart analyst & AI playtester")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("scan", help="refresh charts, trends, vault (no Claude)")
    p.add_argument("--if-needed", action="store_true", help="skip if a scan already succeeded today")
    p.add_argument("--limit", type=int, default=0)

    p = sub.add_parser("autopilot", help="weekly AI playtest run (if due)")
    p.add_argument("--force", action="store_true", help="run even if not due")
    p.add_argument("--scheduled", action="store_true", help="called by Task Scheduler (short idle wait)")
    p.add_argument("--max", type=int, default=0, help="max games")
    p.add_argument("--idle-wait", type=int, default=0, help="minutes to wait for the PC to be idle")

    p = sub.add_parser("playtest", help="playtest one game now via Claude Code")
    p.add_argument("universe_id", type=int)

    sub.add_parser("queue", help="show the playtest queue")
    sub.add_parser("doctor", help="health check")
    p = sub.add_parser("setup", help="vault, Claude registration, scheduled tasks, shortcuts")
    p.add_argument("--open", action="store_true")
    sub.add_parser("unblock", help="clear an account blocker after you fixed the login")
    sub.add_parser("stop", help="stop any running playtest immediately")
    sub.add_parser("panel", help="open the control panel")
    sub.add_parser("mcp", help="run the MCP server (stdio)")
    sub.add_parser("open-vault", help="open the vault in Obsidian")
    sub.add_parser("remove-tasks", help="delete the scheduled tasks")
    p = sub.add_parser("set", help="change a setting, e.g. roxai set playtests_per_run 5")
    p.add_argument("key")
    p.add_argument("value")
    sub.add_parser("settings", help="show settings")

    a = ap.parse_args(argv)

    if a.cmd == "scan":
        from .scan import run_scan
        if a.if_needed:
            last = get_db().last_run("scan", "ok")
            if last and (last["started"] or "").startswith(today()):
                _print("already scanned today")
                return 0
            # local date vs UTC timestamp: compare by snapshot day too
            r = get_db().one("SELECT 1 FROM snapshots WHERE day=? AND sort_id=? LIMIT 1", (today(), settings().sort_id))
            if r:
                _print("already scanned today")
                return 0
        r = run_scan(a.limit or None)
        r.pop("rows", None)
        _print(r)
    elif a.cmd == "autopilot":
        from .autopilot import run
        wait = (a.idle_wait or (50 if a.scheduled else 180)) * 60
        _print(run(force=a.force, max_games=a.max or None, idle_wait_s=wait))
    elif a.cmd == "playtest":
        from .autopilot import run
        _print(run(force=True, only=a.universe_id, idle_wait_s=60))
    elif a.cmd == "queue":
        from .scan import playtest_queue
        _print(playtest_queue(100))
    elif a.cmd == "doctor":
        from .doctor import report_text
        _print(report_text())
    elif a.cmd == "setup":
        from .setup import setup_all
        _print(setup_all(open_after=a.open))
    elif a.cmd == "unblock":
        get_db().set("blocked_reason", None)
        _print("Blocker cleared — playtests will resume at the next check.")
    elif a.cmd == "stop":
        from .session import request_stop
        from .winctl import roblox as RB
        request_stop("roxai stop")
        RB.kill_all()
        _print("Stop requested; Roblox closed.")
    elif a.cmd == "panel":
        from .panel import main as pm
        pm()
    elif a.cmd == "mcp":
        from .mcp_server import main as mm
        mm()
    elif a.cmd == "open-vault":
        from .setup import open_vault
        open_vault()
    elif a.cmd == "remove-tasks":
        from .setup import remove_tasks
        remove_tasks()
    elif a.cmd == "set":
        s = settings()
        if not hasattr(s, a.key):
            _print(f"unknown setting {a.key}")
            return 2
        cur = getattr(s, a.key)
        val: object = a.value
        if isinstance(cur, bool):
            val = a.value.lower() in ("1", "true", "yes", "on")
        elif isinstance(cur, int):
            val = int(a.value)
        elif isinstance(cur, list):
            val = [x.strip() for x in a.value.split(",") if x.strip()]
        setattr(s, a.key, val)
        s.save()
        reload()
        _print(f"{a.key} = {val!r}")
    elif a.cmd == "settings":
        from dataclasses import asdict
        _print(asdict(settings()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
