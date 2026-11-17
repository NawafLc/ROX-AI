"""Weekly autopilot: runs Claude Code headless (`claude -p`) with the ROX-AI MCP server to playtest the queue.

Safety/robustness layers:
  * runs only if due (every `playtest_interval_days`), unless forced
  * only one autopilot at a time (PID lock); stale locks are taken over
  * waits for the PC to be idle & unlocked — never hijacks your mouse while you use the PC
  * one Claude process per game (small context, an error in one game never affects the next)
  * hard per-game timeout → the whole process tree is killed, Roblox closed, session marked interrupted
  * account problems (login/ban/verification) pause playtesting and tell you, instead of looping
  * STOP file / panel button halts between and during games
  * everything is logged to %LOCALAPPDATA%/ROX-AI/logs
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
import traceback
from datetime import datetime
from pathlib import Path

import psutil

from . import log
from .config import IS_WINDOWS, app_dir, logs_dir, settings, state_dir
from .db import get_db, now_iso, today
from .util import AlreadyRunning, days_ago, single_instance

L = log.get("autopilot")
CREATE_NO_WINDOW = 0x08000000


class Skip(Exception):
    pass


# ------------------------------------------------------------- claude CLI ---

def find_claude() -> str | None:
    s = settings()
    if s.claude_cli and Path(s.claude_cli).exists():
        return s.claude_cli
    w = shutil.which("claude")
    if w:
        return w
    home = Path(os.environ.get("USERPROFILE", Path.home()))
    for c in [home / ".local" / "bin" / "claude.exe", home / ".local" / "bin" / "claude",
              Path(os.environ.get("APPDATA", "")) / "npm" / "claude.cmd",
              Path(os.environ.get("LOCALAPPDATA", "")) / "Programs" / "claude" / "claude.exe",
              home / ".claude" / "local" / "claude.exe", home / ".claude" / "local" / "claude"]:
        if c.exists():
            return str(c)
    return None


def python_exe() -> str:
    exe = Path(sys.executable)
    if exe.name.lower() == "pythonw.exe":
        cand = exe.with_name("python.exe")
        if cand.exists():
            return str(cand)
    return str(exe)


def mcp_config_path() -> Path:
    cfg = {"mcpServers": {"roxai": {"command": python_exe(), "args": ["-m", "roxai.mcp_server"],
                                     "env": {"ROXAI_HOME": str(app_dir())}}}}
    p = state_dir() / "mcp-config.json"
    p.write_text(json.dumps(cfg, indent=2), encoding="utf-8")
    return p


def workdir() -> Path:
    d = state_dir() / "claude-workdir"
    d.mkdir(exist_ok=True)
    (d / "CLAUDE.md").write_text(
        "# ROX-AI autopilot\nYou are running unattended for ROX-AI. Use only the roxai MCP tools (plus web search for "
        "public info like codes/wiki). Follow playbook() exactly. Never spend Robux, never chat. Always end each playtest "
        "with playtest_finish. Be thorough and specific in the vault sections.\n", encoding="utf-8")
    return d


def run_claude(prompt: str, timeout_s: int, tag: str) -> dict:
    claude = find_claude()
    if not claude:
        raise Skip("Claude Code CLI not found — run the installer again or install from https://claude.ai/download")
    s = settings()
    cmd = [claude, "-p", prompt, "--mcp-config", str(mcp_config_path()), "--strict-mcp-config",
           "--allowedTools", "mcp__roxai,WebSearch,WebFetch",
           "--output-format", "json", "--max-turns", str(s.max_turns)]
    if s.claude_model:
        cmd += ["--model", s.claude_model]
    out_file = logs_dir() / f"claude-{datetime.now():%Y%m%d-%H%M%S}-{tag}.json"
    L.info("starting claude for %s (timeout %ss)", tag, timeout_s)
    flags = CREATE_NO_WINDOW if IS_WINDOWS else 0
    with open(out_file, "w", encoding="utf-8") as fh:
        proc = subprocess.Popen(cmd, cwd=workdir(), stdout=fh, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
                                creationflags=flags, env={**os.environ, "ROXAI_HOME": str(app_dir())})
        start = time.monotonic()
        stopped = None
        while proc.poll() is None:
            time.sleep(2)
            if time.monotonic() - start > timeout_s:
                stopped = "timeout"
            elif (state_dir() / "STOP").exists():
                stopped = "stop requested"
            if stopped:
                kill_tree(proc.pid)
                break
    try:
        raw = out_file.read_text(encoding="utf-8", errors="ignore")
    except OSError:
        raw = ""
    result: dict = {"returncode": proc.returncode, "log": str(out_file), "stopped": stopped}
    try:
        data = json.loads(raw.strip().splitlines()[-1]) if raw.strip() else {}
        result.update({"is_error": data.get("is_error"), "result": str(data.get("result", ""))[:2000],
                       "turns": data.get("num_turns"), "cost_usd": data.get("total_cost_usd")})
    except (ValueError, IndexError):
        result["result"] = raw[-2000:]
    low = raw.lower()
    if any(k in low for k in ("invalid api key", "please run /login", "not logged in", "authentication_error", "oauth token")):
        result["auth_problem"] = True
    if "usage limit" in low or "rate_limit" in low or "limit reached" in low:
        result["usage_limit"] = True
    return result


def kill_tree(pid: int) -> None:
    try:
        parent = psutil.Process(pid)
        procs = parent.children(recursive=True) + [parent]
        for p in procs:
            try:
                p.terminate()
            except psutil.Error:
                pass
        _, alive = psutil.wait_procs(procs, timeout=8)
        for p in alive:
            try:
                p.kill()
            except psutil.Error:
                pass
    except psutil.Error:
        pass


def claude_ready() -> tuple[bool, str]:
    claude = find_claude()
    if not claude:
        return False, "Claude Code CLI not installed"
    try:
        r = subprocess.run([claude, "--version"], capture_output=True, text=True, timeout=60,
                           creationflags=CREATE_NO_WINDOW if IS_WINDOWS else 0)
        if r.returncode != 0:
            return False, f"claude --version failed: {r.stderr.strip()[:200]}"
        return True, r.stdout.strip()
    except Exception as e:  # noqa: BLE001
        return False, str(e)


# --------------------------------------------------------------- helpers ---

def notify(title: str, text: str) -> None:
    """Best-effort Windows toast so you know what happened without opening anything."""
    L.info("notify: %s — %s", title, text)
    get_db().set("last_notice", {"title": title, "text": text, "at": now_iso()})
    if not IS_WINDOWS:
        return
    t = title.replace("'", "''")
    b = text.replace("'", "''")[:250]
    ps = ("[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null;"
          "$x = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02);"
          f"$x.GetElementsByTagName('text')[0].AppendChild($x.CreateTextNode('{t}')) > $null;"
          f"$x.GetElementsByTagName('text')[1].AppendChild($x.CreateTextNode('{b}')) > $null;"
          "$n = [Windows.UI.Notifications.ToastNotification]::new($x);"
          "[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('ROX-AI').Show($n)")
    try:
        subprocess.Popen(["powershell", "-NoProfile", "-WindowStyle", "Hidden", "-Command", ps],
                         creationflags=CREATE_NO_WINDOW, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except OSError:
        pass


def wait_until_idle(max_wait_s: float) -> bool:
    """True once the PC has been idle (no human input) for idle_required_minutes and is unlocked."""
    from .winctl import window as W
    need = settings().idle_required_minutes * 60
    end = time.monotonic() + max_wait_s
    while True:
        if (state_dir() / "STOP").exists():
            return False
        if not W.is_locked() and W.idle_seconds() >= need:
            return True
        if time.monotonic() > end:
            return False
        time.sleep(30)


def is_due() -> tuple[bool, str]:
    db = get_db()
    last = db.last_run("autopilot", "ok")
    if not last:
        return True, "never ran"
    age = days_ago(last["finished"] or last["started"])
    interval = settings().playtest_interval_days
    if age is None or age >= interval - 0.25:
        return True, f"last full run {age:.1f} days ago" if age is not None else "unknown last run"
    return False, f"last full run {age:.1f} days ago (every {interval} days)"


def blocked() -> str | None:
    b = get_db().get("blocked_reason")
    if not b:
        return None
    if (days_ago(b.get("at")) or 0) >= 3:  # retry automatically after 3 days in case it got fixed
        get_db().set("blocked_reason", None)
        return None
    return b.get("reason")


# ------------------------------------------------------------------ main ---

def run(force: bool = False, max_games: int | None = None, only: int | None = None, idle_wait_s: float = 3 * 3600) -> dict:
    db = get_db()
    try:
        with single_instance("autopilot", stale_after_s=12 * 3600):
            run_id = db.run_start("autopilot")
            try:
                res = _run(force, max_games, only, idle_wait_s)
                status = res.pop("_status", "ok")
                db.run_end(run_id, status, json.dumps(res, default=str)[:3800])
                return {"status": status, **res}
            except Skip as e:
                db.run_end(run_id, "skipped", str(e))
                L.info("autopilot skipped: %s", e)
                return {"status": "skipped", "reason": str(e)}
            except Exception as e:  # noqa: BLE001
                db.run_end(run_id, "error", traceback.format_exc())
                L.error("autopilot error: %s", traceback.format_exc())
                notify("ROX-AI playtest run failed", str(e))
                return {"status": "error", "reason": str(e)}
    except AlreadyRunning as e:
        return {"status": "skipped", "reason": str(e)}


def _run(force: bool, max_games: int | None, only: int | None, idle_wait_s: float) -> dict:
    from .scan import playtest_queue, run_scan
    from .session import clear_stop

    if not IS_WINDOWS:
        raise Skip("Playtests need Windows (Roblox client).")
    clear_stop()
    if not force and not only:
        due, why = is_due()
        if not due:
            raise Skip(f"not due: {why}")
    b = blocked()
    if b and not force:
        notify("ROX-AI needs you (once)", f"Playtests paused: {b}. Open Roblox, log in with the alt account, then click 'I fixed it' in ROX-AI.")
        raise Skip(f"blocked: {b}")
    ok, info = claude_ready()
    if not ok:
        notify("ROX-AI: Claude Code missing", info)
        raise Skip(info)
    from .winctl import roblox as RB
    if not RB.is_installed():
        notify("ROX-AI: Roblox missing", "Install Roblox and log in with your alt account.")
        raise Skip("Roblox not installed")
    if not wait_until_idle(idle_wait_s):
        raise Skip("PC was busy/locked the whole time — will try again at the next check")

    try:
        run_scan()
    except Exception as e:  # noqa: BLE001
        L.warning("pre-run scan failed (continuing with last data): %s", e)

    if only:
        g = get_db().game(only)
        queue = [{"universe_id": only, "title": g["name"] if g else str(only), "rank": g["best_rank"] if g else "?"}]
    else:
        queue = playtest_queue(max_games)
    results = []
    s = settings()
    for i, q in enumerate(queue):
        if (state_dir() / "STOP").exists():
            results.append({"game": q["title"], "status": "stopped by user"})
            break
        if i and not wait_until_idle(30 * 60):
            results.append({"game": q["title"], "status": "postponed (PC in use)"})
            break
        prompt = (f"Playtest the Roblox game '{q['title']}' (universe_id {q['universe_id']}, chart rank {q.get('rank')}). "
                  f"First call playbook(universe_id={q['universe_id']}) and follow it exactly: game_info → vault_read_game → "
                  f"playtest_start → play with act/look/note/snap → playtest_finish with ALL sections, scores, one_liner, tags. "
                  f"Work autonomously; nobody will answer questions.")
        r = run_claude(prompt, s.per_game_timeout_minutes * 60, str(q["universe_id"]))
        RB.kill_all()  # never leave Roblox running
        from .session import manager
        manager.recover_stale()
        last = get_db().one("SELECT status FROM playtests WHERE universe_id=? ORDER BY id DESC LIMIT 1", (q["universe_id"],))
        status = last["status"] if last else "no session"
        results.append({"game": q["title"], "status": status, "claude": {k: r.get(k) for k in
                        ("returncode", "stopped", "is_error", "turns", "cost_usd", "log")}})
        L.info("game %s → %s", q["title"], status)
        if r.get("auth_problem"):
            get_db().set("blocked_reason", {"reason": "Claude Code is not logged in", "at": today()})
            notify("ROX-AI needs you (once)", "Claude Code is logged out. Open a terminal, run `claude`, and log in.")
            break
        if r.get("usage_limit"):
            notify("ROX-AI paused", "Claude usage limit reached — remaining games will be played at the next check.")
            return {"_status": "partial", "games": results, "reason": "usage limit"}
        if get_db().get("blocked_reason"):
            notify("ROX-AI needs you (once)", f"Playtests paused: {get_db().get('blocked_reason')['reason']}")
            return {"_status": "blocked", "games": results}

    if not only and results and not (state_dir() / "STOP").exists():
        report_prompt = ("Write this week's ROX-AI market report. Use vault_read_note('00 Dashboard.md'), vault_list('01 Charts') "
                         "and the latest chart notes, game_info for the biggest movers, and vault_read_game for games playtested "
                         "this week. Then call vault_write_insight(title='Weekly Market Report', ...) with: chart summary, what "
                         "is rising and why, genre shifts, common mechanics & monetization patterns, the 5 best opportunities "
                         "to build a game (with reasoning), and 10 content ideas — link games as [[02 Games/<name>]]. Also call "
                         "vault_write_genre_patterns for the 3 biggest genres.")
        rr = run_claude(report_prompt, 20 * 60, "weekly-report")
        results.append({"game": "Weekly Market Report", "status": "ok" if not rr.get("is_error") else "error"})
    try:
        run_scan()  # refresh dashboard with new playtest results
    except Exception:  # noqa: BLE001
        pass
    done = sum(1 for r in results if r["status"] == "completed")
    notify("ROX-AI weekly run finished", f"{done}/{len(queue)} games playtested. Open the vault to see the new notes.")
    return {"games": results, "queue": len(queue), "completed": done,
            "_status": "ok" if done or not queue else "partial"}
