"""Daily chart scan — pure data, no Claude needed, safe to run any number of times per day."""
from __future__ import annotations

import json
import traceback
from dataclasses import asdict
from pathlib import Path

from . import analysis as A
from . import log, roblox_api as R, vault as V
from .config import settings
from .db import get_db, today
from .util import days_ago, single_instance

L = log.get("scan")

SORT_NAMES = {
    "top-trending": "Top Trending",
    "top-playing-now": "Top Playing Now",
    "up-and-coming": "Up-and-Coming",
    "fun-with-friends": "Fun with Friends",
    "top-revisited": "Top Revisited",
}


def sort_name(sid: str) -> str:
    return SORT_NAMES.get(sid, sid.replace("-", " ").title())


def ensure_icon(universe_id: int, url: str, refresh: bool = False) -> str | None:
    if not url:
        return None
    d = V.p("attach") / str(universe_id)
    f = d / "icon.png"
    if not f.exists() or refresh:
        data = R.download(url)
        if data:
            d.mkdir(parents=True, exist_ok=True)
            f.write_bytes(data)
    return V.rel(f) if f.exists() else None


def playtest_queue(limit: int | None = None, day: str | None = None) -> list[dict]:
    """Which games Claude should play next, best first."""
    s = settings()
    db = get_db()
    day = day or (db.one("SELECT MAX(day) d FROM snapshots WHERE sort_id=?", (s.sort_id,)) or {"d": None})["d"]
    if not day:
        return []
    out: list[dict] = []
    for snap in db.chart_on(day, s.sort_id):
        g = db.game(snap["universe_id"])
        if g is None:
            continue
        reason = None
        last = g["last_playtest"]
        age = days_ago(last)
        if (g["playtest_fail_count"] or 0) >= 3 and (age is None or age < 14):
            continue  # keeps failing (e.g. crashes, ban-wall) — retry in 2 weeks
        if not last:
            reason = "never playtested"
        elif age is not None and age >= s.replay_after_days:
            reason = f"last playtest {int(age)} days ago"
        elif s.replay_if_updated and age is not None and age >= 7 and g["roblox_updated"] and \
                (days_ago(g["roblox_updated"]) or 999) < age:
            reason = "game updated since last playtest"
        if reason:
            out.append({"universe_id": g["universe_id"], "place_id": g["place_id"], "title": V.clean_display_name(g["name"]),
                        "rank": snap["rank"], "reason": reason, "note": g["note_path"]})
    return out[: (limit if limit is not None else s.playtests_per_run)]


def recent_playtests(n: int = 8) -> list[dict]:
    db = get_db()
    rows = db.q("""SELECT p.*, g.name FROM playtests p LEFT JOIN games g ON g.universe_id=p.universe_id
                   ORDER BY p.id DESC LIMIT ?""", (n,))
    return [{**dict(r), "title": V.clean_display_name(r["name"] or str(r["universe_id"]))} for r in rows]


def status_lines() -> list[str]:
    from . import doctor
    try:
        return doctor.short_status()
    except Exception as e:  # noqa: BLE001
        return [f"status unavailable: {e}"]


def run_scan(limit: int | None = None) -> dict:
    s = settings()
    db = get_db()
    limit = limit or s.chart_limit
    with single_instance("scan"):
        run_id = db.run_start("scan")
        try:
            result = _scan(limit)
            db.run_end(run_id, "ok", json.dumps({k: v for k, v in result.items() if k != "rows"})[:3000])
            return result
        except Exception as e:
            L.error("scan failed: %s", traceback.format_exc())
            db.run_end(run_id, "error", f"{e}\n{traceback.format_exc()}")
            raise


def _scan(limit: int) -> dict:
    s = settings()
    db = get_db()
    V.init_vault()
    day = today()
    main = R.get_chart(s.sort_id, limit, s.country)
    if not main:
        raise RuntimeError("Roblox returned an empty chart (API change or outage) — will retry next run")
    L.info("chart %s: %d games", s.sort_id, len(main))

    from concurrent.futures import ThreadPoolExecutor

    def one(e: R.ChartEntry) -> dict:
        try:
            return _process_game(e, day)
        except Exception as ex:  # noqa: BLE001 - one bad game never kills the scan
            L.error("game %s failed: %s", e.universe_id, traceback.format_exc())
            return {"universe_id": e.universe_id, "rank": e.rank, "title": V.clean_display_name(e.name),
                    "genre": e.genre, "playing": e.playing, "note": None, "error": str(ex)}

    # a few games in parallel: Roblox APIs are slow per call but tolerate light concurrency
    with ThreadPoolExecutor(max_workers=4) as pool:
        rows = sorted(pool.map(one, main), key=lambda r: r["rank"])

    # extra charts — snapshots for trend history + short tables
    extra: dict[str, list[dict]] = {}
    for sid in s.extra_sorts:
        try:
            ch = R.get_chart(sid, limit, s.country)
            info = R.safe(lambda ch=ch: R.games_info([c.universe_id for c in ch]), {})
            srows = []
            for c in ch:
                gi = info.get(c.universe_id, {})
                db.add_snapshot(day, sid, c.rank, c.universe_id, c.playing, int(gi.get("visits") or 0),
                                int(gi.get("favoritedCount") or 0), c.up_votes, c.down_votes)
                g = db.game(c.universe_id)
                srows.append({"rank": c.rank, "title": V.clean_display_name(c.name), "genre": c.genre,
                              "playing": c.playing, "note": g["note_path"] if g else None})
            extra[sort_name(sid)] = srows
        except Exception:  # noqa: BLE001
            L.warning("extra sort %s failed: %s", sid, traceback.format_exc())

    prev = db.previous_day(day, s.sort_id)
    today_ids = {r["universe_id"] for r in rows}
    new_ids = {r["universe_id"] for r in rows if r.get("days_tracked", 1) <= 1}
    dropped: list[dict] = []
    if prev:
        for snap in db.chart_on(prev, s.sort_id):
            if snap["universe_id"] not in today_ids:
                g = db.game(snap["universe_id"])
                dropped.append({"title": V.clean_display_name(g["name"]) if g else str(snap["universe_id"]),
                                "rank": snap["rank"]})
    sname = sort_name(s.sort_id)
    chart_path = V.chart_note(day, sname, rows, new_ids, dropped, extra)
    V.genre_notes([r for r in rows if r.get("note")])
    queue = playtest_queue(day=day)
    V.dashboard(day, sname, [r for r in rows if r.get("note")], new_ids, dropped, queue, recent_playtests(), status_lines())
    return {"day": day, "chart": sname, "games": len(rows), "new": len(new_ids), "dropped": len(dropped),
            "errors": sum(1 for r in rows if r.get("error")), "chart_note": str(chart_path),
            "queue": [q["title"] for q in queue], "rows": rows}


def _process_game(e: R.ChartEntry, day: str) -> dict:
    s = settings()
    db = get_db()
    d = R.game_details(e.universe_id, deep=True)
    if not d.name:
        d.name = e.name
        d.place_id = d.place_id or e.place_id
        d.playing = d.playing or e.playing
        d.up_votes, d.down_votes = d.up_votes or e.up_votes, d.down_votes or e.down_votes
        d.genre = d.genre or e.genre
    db.add_snapshot(day, s.sort_id, e.rank, e.universe_id, d.playing or e.playing, d.visits, d.favorites,
                    d.up_votes, d.down_votes)
    hist = [dict(h) for h in db.history(e.universe_id, s.sort_id)]
    tr = A.trend(hist)
    row = db.game(e.universe_id)
    refresh_icon = row is None or (row["last_seen"] and (days_ago(row["last_seen"]) or 0) > 7)
    icon_rel = ensure_icon(e.universe_id, d.icon_url, refresh=bool(refresh_icon) and row is not None)
    pts = [dict(r) for r in db.q("SELECT * FROM playtests WHERE universe_id=? ORDER BY id DESC LIMIT 10", (e.universe_id,))]
    path = V.update_game_note(d, tr, e.rank, s.sort_id, hist, icon_rel, row, pts)
    best = min([x for x in [tr.get("best_rank"), row["best_rank"] if row else None, e.rank] if x])
    db.upsert_game(e.universe_id, place_id=d.place_id, name=d.name, creator=d.creator_name, genre=d.genre,
                   subgenre=d.subgenre, last_seen=day, best_rank=best, roblox_created=d.created,
                   roblox_updated=d.updated, note_path=V.rel(path),
                   details_json=json.dumps({k: v for k, v in asdict(d).items()
                                            if k not in ("badges",)}, default=str)[:200000])
    g = db.game(e.universe_id)
    mon = A.price_stats(d.game_passes)
    return {"universe_id": e.universe_id, "rank": e.rank, "title": V.clean_display_name(d.name), "genre": d.genre,
            "subgenre": d.subgenre, "playing": d.playing, "like_ratio": d.like_ratio, "note": V.rel(path),
            "passes": mon.get("count", 0), "last_playtest": (g["last_playtest"] or "")[:10] if g else "",
            **{k: tr.get(k) for k in ("rank_change_1d", "ccu_change_1d_pct", "visits_per_day", "momentum", "flags",
                                      "days_tracked")}}


def refresh_one(universe_id: int) -> Path:
    """Refresh a single game note on demand (used by MCP tools)."""
    s = settings()
    db = get_db()
    V.init_vault()
    d = R.game_details(universe_id, deep=True)
    if not d.name:
        raise ValueError(f"Roblox has no experience with universe id {universe_id}")
    hist = [dict(h) for h in db.history(universe_id, s.sort_id)]
    tr = A.trend(hist) if hist else {}
    row = db.game(universe_id)
    icon_rel = ensure_icon(universe_id, d.icon_url)
    pts = [dict(r) for r in db.q("SELECT * FROM playtests WHERE universe_id=? ORDER BY id DESC LIMIT 10", (universe_id,))]
    path = V.update_game_note(d, tr, tr.get("rank") if hist and hist[-1]["day"] == today() else None, s.sort_id,
                              hist, icon_rel, row, pts)
    db.upsert_game(universe_id, place_id=d.place_id, name=d.name, creator=d.creator_name, genre=d.genre,
                   subgenre=d.subgenre, roblox_created=d.created, roblox_updated=d.updated, note_path=V.rel(path))
    return path
