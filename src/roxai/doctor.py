"""Health checks — used by the panel, the dashboard status box, `roxai doctor` and the system_check MCP tool."""
from __future__ import annotations

import json
import sys
from dataclasses import dataclass

from .config import IS_WINDOWS, app_dir, settings
from .db import get_db, today
from .util import days_ago


@dataclass
class Check:
    name: str
    ok: bool | None  # None = warning / unknown
    detail: str
    fix: str = ""


def checks(network: bool = True) -> list[Check]:
    s = settings()
    db = get_db()
    out: list[Check] = []
    out.append(Check("Windows", IS_WINDOWS, sys.platform, "Playtests need Windows; scans work anywhere."))
    out.append(Check("Python", sys.version_info >= (3, 11), sys.version.split()[0]))

    # vault
    v = s.vault
    out.append(Check("Vault", v.exists(), str(v), "Run: roxai setup"))
    if v.exists():
        out.append(Check("Dataview plugin", (v / ".obsidian" / "plugins" / "dataview" / "main.js").exists(),
                         "live dashboard tables", "Optional: install Dataview in Obsidian → Community plugins"))
    if IS_WINDOWS:
        from . import setup as S
        from .autopilot import claude_ready
        from .winctl import roblox as RB

        out.append(Check("Obsidian", S.obsidian_exe() is not None, str(S.obsidian_exe() or "not found"),
                         "winget install Obsidian.Obsidian"))
        out.append(Check("Roblox Player", RB.is_installed(), RB.player_path() or "protocol only" if RB.is_installed() else "not found",
                         "Install from roblox.com/download and log in with your ALT account"))
        ok, info = claude_ready()
        out.append(Check("Claude Code CLI", ok, info, "Re-run INSTALL.bat (installs Claude Code), then run `claude` once to log in"))
        cd = [p for p in S.claude_desktop_config_paths() if p.exists() and "roxai" in p.read_text(encoding="utf-8", errors="ignore")]
        out.append(Check("Claude Desktop MCP", bool(cd), str(cd[0]) if cd else "not registered", "roxai setup"))
        for t in ("Daily Chart Scan", "Weekly Playtests"):
            st = S.task_status(t)
            out.append(Check(f"Task: {t}", st is not None and "disabled" not in (st or "").lower(), st or "missing", "roxai setup"))

    last_scan = db.last_run("scan")
    if last_scan:
        age = days_ago(last_scan["finished"] or last_scan["started"]) or 0
        out.append(Check("Last chart scan", last_scan["status"] == "ok" and age < 2,
                         f"{last_scan['status']} · {(last_scan['started'] or '')[:16]} ({age:.1f} d ago)",
                         "roxai scan" if last_scan["status"] != "ok" else ""))
    else:
        out.append(Check("Last chart scan", None, "never", "roxai scan"))
    last_ap = db.last_run("autopilot")
    if last_ap:
        out.append(Check("Last playtest run", last_ap["status"] in ("ok", "skipped", "running", "partial"),
                         f"{last_ap['status']} · {(last_ap['started'] or '')[:16]} · {(last_ap['detail'] or '')[:120]}"))
    else:
        out.append(Check("Last playtest run", None, "never ran yet"))
    b = db.get("blocked_reason")
    out.append(Check("Account blocker", not b, b["reason"] if b else "none",
                     "Fix the Roblox/Claude login, then click 'I fixed it' (roxai unblock)" if b else ""))
    if network:
        try:
            from . import roblox_api as R
            n = len(R.get_chart(s.sort_id, 3, s.country))
            out.append(Check("Roblox API", n > 0, f"{n} games returned"))
        except Exception as e:  # noqa: BLE001
            out.append(Check("Roblox API", False, str(e)[:200], "Check internet connection"))
    out.append(Check("Data folder", True, str(app_dir())))
    return out


def report_text(network: bool = True) -> str:
    lines = []
    for c in checks(network):
        icon = "✅" if c.ok else ("⚠️" if c.ok is None else "❌")
        lines.append(f"{icon} {c.name}: {c.detail}" + (f"  → {c.fix}" if c.fix and not c.ok else ""))
    return "\n".join(lines)


def short_status() -> list[str]:
    db = get_db()
    out = []
    ls = db.last_run("scan", "ok")
    out.append(f"📊 Last successful scan: **{(ls['started'] or '')[:16].replace('T', ' ') if ls else 'never'}**")
    la = db.last_run("autopilot")
    if la:
        try:
            det = json.loads(la["detail"] or "{}")
            extra = f" · {det.get('completed', '?')}/{det.get('queue', '?')} games" if isinstance(det, dict) and "queue" in det else ""
        except ValueError:
            extra = f" · {(la['detail'] or '')[:80]}"
        out.append(f"🕹️ Last playtest run: **{la['status']}** {(la['started'] or '')[:10]}{extra}")
    else:
        out.append("🕹️ Last playtest run: not yet (runs weekly when your PC is idle)")
    n = db.one("SELECT COUNT(*) c FROM games")["c"]
    p = db.one("SELECT COUNT(*) c FROM playtests WHERE status='completed'")["c"]
    out.append(f"🗂️ {n} games tracked · {p} completed playtests · today {today()}")
    b = db.get("blocked_reason")
    if b:
        out.append(f"⛔ **Needs you:** {b['reason']} → fix it, then press *I fixed it* in the ROX-AI panel.")
    return out
