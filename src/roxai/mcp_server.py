"""ROX-AI MCP server — the tools Claude uses to study charts, play games and write the vault.

Run with `roxai-mcp` (stdio). Registered automatically in Claude Desktop and Claude Code by the installer.
"""
from __future__ import annotations

import functools
import json
import re
import time
import traceback
from importlib import resources
from pathlib import Path
from typing import Any

import anyio
from mcp.server.fastmcp import FastMCP, Image

from . import analysis as A
from . import log, roblox_api as R, vault as V
from .config import settings
from .db import get_db, today

L = log.get("mcp")

INSTRUCTIONS = """ROX-AI studies the Roblox charts and maintains an Obsidian vault with one detailed template note per game.
Typical flows:
- "What's trending?" → charts / game_info.
- "Playtest <game>" → use the `playtest_game` prompt (or read playbook()), then playtest_start → look/act/note/snap → playtest_finish.
- "Weekly run" → playtest_queue, playtest each game (one at a time), then write a market insight with vault_write_insight.
Never spend Robux, never chat, and always finish sessions with playtest_finish (even after aborts)."""

mcp = FastMCP("roxai", instructions=INSTRUCTIONS)


def playbook_text(universe_id: int | str = "<id>", name: str = "<game>", rank: Any = "?") -> str:
    txt = resources.files("roxai").joinpath("prompts/playtest.md").read_text(encoding="utf-8")
    return txt.replace("{universe_id}", str(universe_id)).replace("{name}", str(name)).replace("{rank}", str(rank))


def threaded(fn):
    """Run blocking tool bodies in a worker thread so the MCP connection stays responsive; turn errors into text."""
    @functools.wraps(fn)
    async def wrapper(*args, **kwargs):
        try:
            return await anyio.to_thread.run_sync(functools.partial(fn, *args, **kwargs))
        except Exception as e:  # noqa: BLE001
            L.error("tool %s failed: %s", fn.__name__, traceback.format_exc())
            return f"ERROR in {fn.__name__}: {type(e).__name__}: {e}"
    return wrapper


def _js(o: Any) -> str:
    return json.dumps(o, indent=1, ensure_ascii=False, default=str)


# =============================================================== research ===

@mcp.tool()
@threaded
def charts(sort_id: str = "", limit: int = 25) -> str:
    """Live Roblox chart (default: the configured chart, usually 'top-trending'). Shows rank, players, likes, genre,
    and ROX-AI status for each game (tracked? last playtest?). Other sort_ids: top-playing-now, up-and-coming,
    fun-with-friends, top-revisited (see list_chart_sorts)."""
    s = settings()
    sid = sort_id or s.sort_id
    db = get_db()
    rows = []
    for e in R.get_chart(sid, min(limit, 100), s.country):
        g = db.game(e.universe_id)
        rows.append({"rank": e.rank, "universe_id": e.universe_id, "name": e.name, "genre": e.genre,
                     "playing": e.playing, "like_ratio": A.like_ratio(e.up_votes, e.down_votes),
                     "in_vault": bool(g and g["note_path"]), "last_playtest": (g["last_playtest"] or "")[:10] if g else ""})
    return _js({"chart": sid, "games": rows})


@mcp.tool()
@threaded
def list_chart_sorts() -> str:
    """List every chart Roblox currently shows on the home/charts page (sortId + display name)."""
    return _js(R.list_sorts(settings().country))


@mcp.tool()
@threaded
def game_info(universe_id: int) -> str:
    """Everything public about one game: description, creator, stats, game passes & developer products (with prices),
    badges (with award counts = how far players get), server sample, chart trend history, monetization breakdown,
    and which vault sections are already written. Also refreshes its vault note."""
    from .scan import refresh_one
    db = get_db()
    d = R.game_details(universe_id, deep=True)
    if not d.name:
        return f"No Roblox experience found for universe id {universe_id}."
    try:
        path = refresh_one(universe_id)
    except Exception as e:  # noqa: BLE001
        path = None
        L.warning("refresh_one failed: %s", e)
    hist = [dict(h) for h in db.history(universe_id, settings().sort_id)]
    filled = []
    if path and path.exists():
        filled = [k for k, v in V.read_note_sections(path).items() if k in V.AI_KEYS and "Not analyzed yet" not in v]
    out = {
        "universe_id": d.universe_id, "place_id": d.place_id, "name": d.name, "url": d.url,
        "creator": f"{d.creator_name} ({d.creator_type}{', verified' if d.creator_verified else ''})",
        "genre": f"{d.genre} / {d.subgenre}", "playing": d.playing, "visits": d.visits, "favorites": d.favorites,
        "likes": d.up_votes, "dislikes": d.down_votes, "like_ratio": d.like_ratio, "max_players": d.max_players,
        "created": d.created, "updated": d.updated, "description": d.description[:3000],
        "monetization_summary": A.monetization(d.game_passes, d.dev_products),
        "game_passes": d.game_passes, "dev_products": d.dev_products[:60],
        "badges_top": sorted(d.badges, key=lambda b: -(b.get("awarded") or 0))[:25], "badge_total": d.badge_total,
        "server_sample": d.server_sample, "trend": A.trend(hist) if hist else {},
        "vault_note": V.rel(path) if path else None, "ai_sections_written": filled,
        "ai_sections_missing": [k for k in V.AI_KEYS if k not in filled],
    }
    return _js(out)


@mcp.tool()
@threaded
def scan_charts(limit: int = 0) -> str:
    """Run the daily chart scan right now: refresh stats/trends for every chart game, write today's chart note,
    genre notes and the dashboard. Takes ~1-3 minutes."""
    from .scan import run_scan
    r = run_scan(limit or None)
    r.pop("rows", None)
    return _js(r)


@mcp.tool()
@threaded
def playtest_queue(limit: int = 0) -> str:
    """Games that should be playtested next (never played / stale / updated since last playtest), best first."""
    from .scan import playtest_queue as q
    return _js(q(limit or None))


@mcp.tool()
def playbook(universe_id: int = 0) -> str:
    """The full playtest playbook (how to play, rules, what to write). Read it before your first playtest."""
    if universe_id:
        g = get_db().game(universe_id)
        return playbook_text(universe_id, g["name"] if g else universe_id, g["best_rank"] if g else "?")
    return playbook_text()


# ================================================================== vault ===

def _note_for(universe_id: int) -> Path:
    db = get_db()
    g = db.game(universe_id)
    if g and g["note_path"] and (V.vault_root() / g["note_path"]).exists():
        return V.vault_root() / g["note_path"]
    found = V.find_game_note(universe_id)
    if found:
        return found
    from .scan import refresh_one
    return refresh_one(universe_id)


@mcp.tool()
@threaded
def vault_read_game(universe_id: int) -> str:
    """Read the 🤖 analysis sections and ✍️ user notes currently stored for a game (so you extend rather than overwrite)."""
    path = _note_for(universe_id)
    secs = V.read_note_sections(path)
    return _js({"note": V.rel(path), "sections": secs})


@mcp.tool()
@threaded
def vault_write_game_analysis(universe_id: int, sections: dict[str, str], scores: dict[str, int] | None = None,
                              one_liner: str = "", tags: list[str] | None = None) -> str:
    """Write/replace 🤖 sections of a game's template note without a playtest (e.g. from research).
    Valid section keys: summary, core_loop, ftue, systems, progression, ui_ux, social, retention, monetization,
    art_audio, content_angles, blueprint, improve, verdict. Scores 1-10 keys: fun, clarity, retention,
    monetization_aggressiveness, polish, clone_difficulty, trend_durability."""
    path = _note_for(universe_id)
    warns = V.write_ai_sections(path, sections, scores, one_liner or None, tags, status="analyzed")
    saved = [k for k in sections if k in V.AI_KEYS]
    return f"Saved {len(saved)} section(s) to {V.rel(path)}." + (" Warnings: " + "; ".join(warns) if warns else "")


@mcp.tool()
@threaded
def vault_write_insight(title: str, markdown: str, tags: list[str] | None = None) -> str:
    """Create a note in 05 Insights (weekly market report, cross-game comparison, trend analysis…).
    Link games with [[02 Games/<Name>]]."""
    p = V.write_insight(title, markdown, tags)
    return f"Saved {V.rel(p)}"


@mcp.tool()
@threaded
def vault_write_genre_patterns(genre: str, markdown: str) -> str:
    """Write the 🤖 'Genre Patterns' section of a genre note (what the winning games in this genre share)."""
    p = V.set_genre_patterns(genre, markdown)
    return f"Saved {V.rel(p)}"


@mcp.tool()
@threaded
def vault_search(query: str, folder: str = "", max_results: int = 30) -> str:
    """Case-insensitive text/regex search across the vault notes. Returns matching lines with note paths."""
    root = V.vault_root() / folder if folder else V.vault_root()
    try:
        rx = re.compile(query, re.I)
    except re.error:
        rx = re.compile(re.escape(query), re.I)
    hits = []
    for f in sorted(root.rglob("*.md")):
        if ".obsidian" in f.parts:
            continue
        for i, line in enumerate(f.read_text(encoding="utf-8", errors="ignore").splitlines(), 1):
            if rx.search(line):
                hits.append(f"{V.rel(f)}:{i}: {line.strip()[:200]}")
                if len(hits) >= max_results:
                    return "\n".join(hits)
    return "\n".join(hits) or "No matches."


@mcp.tool()
@threaded
def vault_read_note(path: str, max_chars: int = 20000) -> str:
    """Read any vault note by its path relative to the vault (e.g. '00 Dashboard.md' or '01 Charts/2026-01-01 Top Trending.md')."""
    root = V.vault_root().resolve()
    p = (root / path).resolve()
    if root not in p.parents and p != root:
        return "Path must be inside the vault."
    if not p.exists():
        cands = [V.rel(f) for f in root.rglob("*.md") if path.lower() in f.name.lower()][:10]
        return f"Not found. Similar: {cands}"
    return p.read_text(encoding="utf-8")[:max_chars]


@mcp.tool()
@threaded
def vault_list(folder: str = "02 Games") -> str:
    """List notes in a vault folder."""
    root = V.vault_root() / folder
    if not root.exists():
        return "Folder not found."
    return "\n".join(sorted(V.rel(f) for f in root.glob("*.md")))


# =============================================================== playtest ===

def _manager():
    from .session import manager
    return manager


def _shot_result(text: str, grid: bool = False) -> list:
    m = _manager()
    shot = m.look(grid=grid)
    s = m.s
    dark = " (screen is almost black — loading, or a fade)" if shot.mean_brightness < 12 else ""
    return [Image(data=shot.jpeg, format="jpeg"),
            f"{text}\nScreenshot {shot.width}x{shot.height}px{dark}. {s.status_line() if s else ''}"]


@mcp.tool()
async def playtest_start(universe_id: int, minutes: int = 0) -> Any:
    """Close any running Roblox, join the game with the logged-in alt account and start a timed AI playtest
    (default budget from settings, ~12 min). Returns the first screenshot. Hands off the mouse/keyboard while it runs:
    any physical input or Ctrl+Shift+Q stops the AI instantly."""
    def run():
        m = _manager()
        s = m.start(universe_id, minutes or None)
        # let the game load a bit
        for _ in range(12):
            time.sleep(1.5)
            shot = m.look()
            if shot.mean_brightness > 15:
                break
        return _shot_result(f"🎮 Playtest started: {s.name} (place {s.place_id}). Vault log: {V.rel(s.note)}.\n"
                            f"Read playbook() if you haven't. Use act/look/note/snap, then playtest_finish.")
    try:
        return await anyio.to_thread.run_sync(run)
    except Exception as e:  # noqa: BLE001
        L.error("playtest_start: %s", traceback.format_exc())
        return f"ERROR playtest_start: {e}"


@mcp.tool()
async def look(grid: bool = False) -> Any:
    """Screenshot of the game. grid=true overlays a labelled 100px grid to help pick click coordinates."""
    try:
        return await anyio.to_thread.run_sync(lambda: _shot_result("👀", grid))
    except Exception as e:  # noqa: BLE001
        return f"ERROR look: {e}"


@mcp.tool()
async def act(actions: list[dict[str, Any]], look_after: bool = True, grid: bool = False) -> Any:
    """Perform up to 25 game actions in order, then (by default) return a screenshot.
    Action types: walk{dir:'w'|'a'|'s'|'d' combos, seconds, sprint?, jump?} · jump{times} · key{key, hold?, times?}
    · keys{keys:[..], seconds} (hold combo) · click{x,y, button?, double?} · hold_click{x,y,seconds}
    · move_mouse{x,y} · camera{dx,dy} · scroll{clicks} (neg = zoom in) · type{text, enter?} · wait{seconds}.
    x/y are pixels in the most recent screenshot."""
    def run():
        m = _manager()
        done = m.act(actions)
        txt = "✅ " + "; ".join(done)
        if look_after:
            return _shot_result(txt, grid)
        return f"{txt}\n{m.s.status_line() if m.s else ''}"
    try:
        return await anyio.to_thread.run_sync(run)
    except Exception as e:  # noqa: BLE001
        msg = f"⚠️ {type(e).__name__}: {e}"
        try:
            return await anyio.to_thread.run_sync(lambda: _shot_result(msg))
        except Exception:  # noqa: BLE001
            return msg


@mcp.tool()
@threaded
def note(text: str) -> str:
    """Save a timestamped observation to the playtest log (crash-safe). Include numbers, prices, timers."""
    _manager().timeline(text)
    s = _manager().s
    return f"noted. {s.status_line() if s and not s.ended else ''}"


@mcp.tool()
@threaded
def snap(caption: str) -> str:
    """Save the current screen into the vault (shown in the playtest note) with a caption."""
    rel = _manager().snap(caption)
    return f"📸 saved {rel}"


@mcp.tool()
@threaded
def playtest_finish(report: str, sections: dict[str, str] | None = None, scores: dict[str, int] | None = None,
                    one_liner: str = "", tags: list[str] | None = None) -> str:
    """End the playtest: closes Roblox, saves your report to the playtest log and writes the 🤖 sections of the
    game's template note (keys: summary, core_loop, ftue, systems, progression, ui_ux, social, retention,
    monetization, art_audio, content_angles, blueprint, improve, verdict). Works even after an abort."""
    m = _manager()
    s = m.s
    if not s:
        return "No playtest session to finish."
    aborted = s.abort_reason
    status = "completed" if not aborted or s.elapsed > s.budget_s * 0.5 else "partial"
    m.end(status, (one_liner or report[:200]))
    text = s.note.read_text(encoding="utf-8")
    V.write(s.note, V.set_block(text, "ai", "report", "## 🤖 Final Report\n" + report.strip()))
    warns: list[str] = []
    if s.game_note and sections:
        warns = V.write_ai_sections(s.game_note, sections, scores, one_liner or None, tags, status="playtested")
        V.set_fm_value(s.game_note, "last_playtest", today())
    elif sections:
        warns = V.write_ai_sections(_note_for(s.universe_id), sections, scores, one_liner or None, tags, status="playtested")
    if status == "completed":
        get_db().upsert_game(s.universe_id, last_playtest=today())
    missing = [k for k in V.AI_KEYS if k not in (sections or {})]
    out = (f"✅ Playtest saved ({status}). Log: {V.rel(s.note)} · Game note: "
           f"{V.rel(s.game_note) if s.game_note else '-'} · {s.shots_saved} screenshots · {int(s.elapsed // 60)} min.")
    if aborted:
        out += f"\nSession had ended early: {aborted}."
    if missing:
        out += f"\nSections not provided (kept as before): {', '.join(missing)}. Use vault_write_game_analysis to add them."
    if warns:
        out += "\nWarnings: " + "; ".join(warns)
    m.s = None
    return out


@mcp.tool()
@threaded
def playtest_abort(reason: str) -> str:
    """Stop the playtest right away because it cannot continue (login/age-verification screen, ban, game broken).
    Closes Roblox. Use playtest_finish afterwards only if you learned something worth saving."""
    m = _manager()
    if not m.s:
        return "No session running."
    m.s.abort_reason = f"blocked: {reason}"
    blocked = re.search(r"login|log in|sign in|verif|captcha|ban", reason, re.I)
    m.end("blocked", reason)
    if blocked:
        get_db().set("blocked_reason", {"reason": reason, "at": today()})
    return (f"Session stopped: {reason}. "
            + ("⚠️ This looks account-related: the autopilot will pause playtests until the user fixes the Roblox login."
               if blocked else "You can call playtest_finish to save partial findings."))


@mcp.tool()
@threaded
def session_status() -> str:
    """Current playtest status (time left, actions, problems)."""
    m = _manager()
    s = m.s
    if not s:
        return "No playtest running."
    return _js({"game": s.name, "elapsed_s": int(s.elapsed), "remaining_s": int(s.remaining), "actions": s.actions,
                "screens_saved": s.shots_saved, "ended": s.ended, "abort_reason": s.abort_reason, "log": V.rel(s.note)})


@mcp.tool()
@threaded
def emergency_stop() -> str:
    """Immediately release all keys, close Roblox and end the session."""
    from .winctl import input as IN, roblox as RB
    IN.release_all()
    m = _manager()
    if m.s:
        m.s.abort_reason = "emergency_stop"
        m.end("stopped", "emergency stop")
    RB.kill_all()
    return "Stopped. All inputs released and Roblox closed."


@mcp.tool()
@threaded
def system_check() -> str:
    """Health check of the whole setup (Roblox installed, vault, schedule, Claude CLI, last runs, blockers)."""
    from . import doctor
    return doctor.report_text()


# ================================================================ prompts ===

@mcp.prompt()
def playtest_game(universe_id: str) -> str:
    """Playtest one game and fill its vault template."""
    g = get_db().game(int(universe_id)) if str(universe_id).isdigit() else None
    return playbook_text(universe_id, g["name"] if g else universe_id, g["best_rank"] if g else "?") + \
        "\n\nStart now with game_info, then vault_read_game, then playtest_start."


@mcp.prompt()
def weekly_run() -> str:
    """Playtest everything in the queue, then write the weekly market report."""
    return ("Run the ROX-AI weekly cycle:\n1. scan_charts()\n2. playtest_queue() and playtest every listed game one "
            "after another, following playbook() exactly (always playtest_finish each).\n3. Then write a weekly market "
            "report with vault_write_insight (title 'Weekly Market Report'): genre shifts, rising mechanics, monetization "
            "patterns, best opportunities to build, with [[links]] to game notes; and update vault_write_genre_patterns "
            "for the top 3 genres.")


@mcp.prompt()
def market_report() -> str:
    """Write a market/trend report from the vault data without playing."""
    return ("Read '00 Dashboard.md' and the latest notes in '01 Charts' (vault_read_note / vault_list), use game_info for "
            "the top movers, then write a detailed report with vault_write_insight: what's trending and why, genre mix "
            "changes, monetization patterns, 5 concrete game ideas with reasoning, and content ideas.")


def main() -> None:
    try:
        from .winctl import window as W
        W.make_dpi_aware()
    except Exception:  # noqa: BLE001
        pass
    try:
        from .session import manager
        manager.recover_stale()
    except Exception:  # noqa: BLE001
        L.warning("recover_stale failed: %s", traceback.format_exc())
    try:
        V.init_vault()
    except Exception:  # noqa: BLE001
        L.warning("vault init failed: %s", traceback.format_exc())
    mcp.run()


if __name__ == "__main__":
    main()
