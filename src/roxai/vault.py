"""Obsidian vault builder.

Every game note is a fixed *template* made of marked blocks:

  <!-- roxai:auto:KEY -->  ... <!-- /roxai:auto:KEY -->   rewritten by the daily scan (data)
  <!-- roxai:ai:KEY -->    ... <!-- /roxai:ai:KEY -->     written by Claude after playtests / research
  everything under "## ✍️ My Notes" is yours and is never touched.

Blocks are replaced in place, so re-running anything is always safe (idempotent).
"""
from __future__ import annotations

import json
import re
import unicodedata
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable

from . import analysis as A
from . import log
from .config import settings

L = log.get("vault")

DIRS = {
    "charts": "01 Charts",
    "games": "02 Games",
    "playtests": "03 Playtests",
    "genres": "04 Genres",
    "insights": "05 Insights",
    "templates": "Templates",
    "attach": "Attachments",
}

MY_NOTES = "## ✍️ My Notes"

# key, heading, what Claude should put there (also used as the empty placeholder hint)
AI_SECTIONS: list[tuple[str, str, str]] = [
    ("summary", "🎯 Summary & Why It's Trending",
     "Elevator pitch in 2-3 sentences, target audience, the *hook* that makes it spread, and why it is on the chart right now."),
    ("core_loop", "🔁 Core Gameplay Loop",
     "The loop as numbered steps (e.g. collect → sell → upgrade → unlock area → repeat), typical session length, what the player is optimizing."),
    ("ftue", "🚪 First 5 Minutes (Onboarding / FTUE)",
     "Exactly what happens from spawn: tutorial, arrows, first reward, first purchase prompt, how fast the first 'wow' moment arrives."),
    ("systems", "⚙️ Systems & Mechanics",
     "Every system observed: controls, tools, pets/units, combat, building, trading, rebirth/prestige, quests, zones, events. Bullet list with short notes."),
    ("progression", "📶 Progression & Economy",
     "Currencies, sources and sinks, upgrade curve, rebirth/prestige, zone gates, how long each tier takes, where the grind walls are."),
    ("ui_ux", "🖥️ UI / UX Layout",
     "HUD layout (what is on left/right/top/bottom), main buttons, shop placement, popups, readability on mobile, juice/feedback."),
    ("social", "👥 Social & Multiplayer",
     "Server size, co-op/competition, trading, leaderboards, friend invites, group/community rewards, chat usage."),
    ("retention", "🧲 Retention Hooks",
     "Daily rewards, streaks, timed chests, codes, limited events, offline earnings, update cadence, notifications, favorites/like prompts."),
    ("monetization", "💰 Monetization Analysis",
     "How and when it asks for Robux, best-value offers, pricing psychology, aggressiveness 1-10, which passes are likely best sellers and why."),
    ("art_audio", "🎨 Art, Audio & Game Feel",
     "Visual style, colors, camera, VFX/SFX, music, polish level, performance."),
    ("content_angles", "🎬 Content Angles",
     "Video hooks, 5 title ideas, thumbnail concepts, Shorts/TikTok moments, secrets/easter eggs, codes worth covering."),
    ("blueprint", "🛠️ Build Blueprint (Roblox Studio)",
     "How to build a game like this: MVP scope, ServerScriptService/ReplicatedStorage module list, DataStore schema, RemoteEvents, key Luau systems, asset list, effort estimate (days), step-by-step build order."),
    ("improve", "🚀 How To Beat It",
     "Weaknesses, player complaints, missing features, and concrete differentiation ideas for a competing game."),
    ("verdict", "🧾 Verdict & Scores",
     "Scores 1-10: Fun, Clarity, Retention, Monetization aggressiveness, Polish, Clone difficulty, Trend durability — each with one line of reasoning. Then a final verdict."),
]
AI_KEYS = [k for k, _, _ in AI_SECTIONS]


# ------------------------------------------------------------- helpers ---

def vault_root() -> Path:
    return settings().vault


def p(kind: str) -> Path:
    d = vault_root() / DIRS[kind]
    d.mkdir(parents=True, exist_ok=True)
    return d


def clean_display_name(name: str) -> str:
    """Strip chart noise like '[🎃 UPDATE]' or '(NEW!)' and emojis from a Roblox title."""
    n = re.sub(r"[\[\(【][^\]\)】]{0,30}[\]\)】]", " ", name)
    n = "".join(ch for ch in n if unicodedata.category(ch)[0] in "LNPZ" or ch in "-'&!")
    n = re.sub(r"\s+", " ", n).strip(" -|!")
    return n or name.strip() or "Untitled"


def safe_filename(name: str) -> str:
    n = clean_display_name(name)
    n = re.sub(r'[<>:"/\\|?*#^\[\]]', "", n).strip(". ")
    return n[:80] or "Untitled"


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(text, encoding="utf-8")
    tmp.replace(path)


def rel(path: Path) -> str:
    return path.relative_to(vault_root()).as_posix()


def link(path: Path | str, alias: str | None = None) -> str:
    target = rel(path) if isinstance(path, Path) else path
    target = target[:-3] if target.endswith(".md") else target
    return f"[[{target}|{alias}]]" if alias else f"[[{target}]]"


def glink(r: dict) -> str:
    """Link to a game's note, or plain title if the note could not be created."""
    return link(r["note"], r["title"]) if r.get("note") else str(r.get("title", "?"))


def md_escape(s: Any) -> str:
    return str(s).replace("|", "\\|").replace("\n", " ").strip()


def table(headers: list[str], rows: Iterable[list[Any]]) -> str:
    out = ["| " + " | ".join(headers) + " |", "|" + "|".join("---" for _ in headers) + "|"]
    n = 0
    for r in rows:
        out.append("| " + " | ".join(md_escape(c) if not str(c).startswith("[[") else str(c).replace("|", "\\|")
                                    for c in r) + " |")
        n += 1
    if n == 0:
        out.append("| " + " | ".join("–" for _ in headers) + " |")
    return "\n".join(out)


# ---------------------------------------------------------- frontmatter ---

FM_RE = re.compile(r"\A---\n(.*?)\n---\n?", re.S)


def split_frontmatter(text: str) -> tuple[dict[str, str], str]:
    """Returns raw key->yaml-text mapping (order kept) and body."""
    m = FM_RE.match(text)
    if not m:
        return {}, text
    raw: dict[str, str] = {}
    key = None
    for line in m.group(1).splitlines():
        km = re.match(r"^([A-Za-z0-9_\-]+):(.*)$", line)
        if km and not line.startswith(" "):
            key = km.group(1)
            raw[key] = km.group(2).strip()
        elif key is not None:
            raw[key] += "\n" + line
    return raw, text[m.end():]


def yaml_value(v: Any) -> str:
    if v is None:
        return ""
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        return str(v)
    if isinstance(v, (list, tuple)):
        if not v:
            return "[]"
        return "\n" + "\n".join(f"  - {yaml_value(x).strip()}" for x in v)
    s = str(v).replace("\n", " ")
    return json.dumps(s, ensure_ascii=False)  # JSON string is valid YAML


def build_frontmatter(values: dict[str, Any], keep: dict[str, str] | None = None) -> str:
    lines = ["---"]
    for k, v in values.items():
        val = yaml_value(v)
        lines.append(f"{k}:{'' if val.startswith(chr(10)) else ' '}{val}" if val else f"{k}:")
    for k, raw in (keep or {}).items():
        if k not in values:
            lines.append(f"{k}: {raw}" if not raw.startswith("\n") else f"{k}:{raw}")
    lines.append("---")
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------- blocks ---

def block(kind: str, key: str, content: str) -> str:
    return f"<!-- roxai:{kind}:{key} -->\n{content.strip()}\n<!-- /roxai:{kind}:{key} -->"


def get_block(text: str, kind: str, key: str) -> str | None:
    m = re.search(rf"<!-- roxai:{kind}:{key} -->\n?(.*?)\n?<!-- /roxai:{kind}:{key} -->", text, re.S)
    return m.group(1) if m else None


def set_block(text: str, kind: str, key: str, content: str, before: str | None = MY_NOTES) -> str:
    new = block(kind, key, content)
    rx = re.compile(rf"<!-- roxai:{kind}:{key} -->.*?<!-- /roxai:{kind}:{key} -->", re.S)
    if rx.search(text):
        return rx.sub(lambda _m: new, text, count=1)
    if before and before in text:
        i = text.index(before)
        return text[:i] + new + "\n\n" + text[i:]
    return text.rstrip() + "\n\n" + new + "\n"


# ------------------------------------------------------------ vault init ---

README = """# 🤖 ROX-AI Vault

This vault is built and updated automatically by **ROX-AI**: it follows the Roblox charts every day, has Claude
playtest the top games once a week, and turns each game into a detailed **template / blueprint**.

| Folder | What's inside |
|---|---|
| [[00 Dashboard]] | Today's chart, risers, new entries, playtest queue, everything at a glance |
| `01 Charts` | One note per day per chart, with rank changes and movers |
| `02 Games` | **One template note per game**: stats, trends, monetization data + Claude's full analysis & Studio blueprint |
| `03 Playtests` | Logs of every AI playtest session with screenshots |
| `04 Genres` | Genre overviews: which genres are winning, patterns |
| `05 Insights` | Weekly market reports and cross-game insights written by Claude |

**Legend in game notes:** 📊 = data refreshed daily · 🤖 = written by Claude · ✍️ = yours (never overwritten).

Tip: install the **Dataview** community plugin (ROX-AI tries to pre-install it, you just click *Trust*) for live tables.
"""

GAME_TEMPLATE_DOC = "# 🎮 Game Template\n\nEvery note in `02 Games` follows this structure.\n\n" + "\n".join(
    f"- **{h}** — {g}" for _, h, g in AI_SECTIONS
) + "\n\nData blocks (📊) are filled from the Roblox APIs: snapshot, chart trend, description, game passes, developer products, badges, playtest history.\n"


def init_vault() -> Path:
    root = vault_root()
    root.mkdir(parents=True, exist_ok=True)
    for k in DIRS:
        p(k)
    if not (root / "README.md").exists():
        write(root / "README.md", README)
    write(p("templates") / "Game Template.md", GAME_TEMPLATE_DOC)
    write(p("templates") / "Playtest Template.md",
          "# 🕹️ Playtest Template\n\n- Timeline of observations (auto-saved during play)\n- Screenshots\n- Final report\n- Problems / blockers\n")
    obs = root / ".obsidian"
    obs.mkdir(exist_ok=True)
    app = obs / "app.json"
    if not app.exists():
        write(app, json.dumps({"attachmentFolderPath": DIRS["attach"], "newFileLocation": "folder",
                               "newFileFolderPath": DIRS["insights"], "showLineNumber": False,
                               "readableLineLength": False}, indent=2))
    cp = obs / "community-plugins.json"
    if not cp.exists():
        write(cp, json.dumps(["dataview"]))
    _install_dataview(obs)
    if not (root / "00 Dashboard.md").exists():
        write(root / "00 Dashboard.md", "# 🏠 ROX-AI Dashboard\n\n_Waiting for the first chart scan…_\n")
    return root


def _install_dataview(obs: Path) -> None:
    d = obs / "plugins" / "dataview"
    if (d / "main.js").exists():
        return
    try:
        from .roblox_api import download
        base = "https://github.com/blacksmithgu/obsidian-dataview/releases/latest/download/"
        files = {f: download(base + f) for f in ("main.js", "manifest.json", "styles.css")}
        if files["main.js"] and files["manifest.json"]:
            d.mkdir(parents=True, exist_ok=True)
            for f, b in files.items():
                if b:
                    (d / f).write_bytes(b)
            L.info("Dataview plugin installed into vault")
    except Exception as e:  # noqa: BLE001
        L.warning("Could not pre-install Dataview (vault still works with static tables): %s", e)


# ------------------------------------------------------------ game notes ---

def game_note_path(universe_id: int, name: str, existing: str | None = None) -> Path:
    if existing:
        ep = vault_root() / existing
        if ep.exists():
            return ep
    base = safe_filename(name)
    path = p("games") / f"{base}.md"
    if path.exists():
        raw, _ = split_frontmatter(path.read_text(encoding="utf-8"))
        if raw.get("universe_id", "").strip('"') != str(universe_id):
            path = p("games") / f"{base} ({universe_id}).md"
    return path


def find_game_note(universe_id: int) -> Path | None:
    for f in p("games").glob("*.md"):
        try:
            head = f.read_text(encoding="utf-8")[:600]
        except OSError:
            continue
        if re.search(rf"^universe_id: {universe_id}\s*$", head, re.M):
            return f
    return None


def new_game_body(title: str, uid: int, icon_rel: str | None) -> str:
    parts = [f"# {title}\n"]
    if icon_rel:
        parts.append(f"![[{icon_rel}|180]]\n")
    parts.append("> [!info] ROX-AI game template\n> 📊 sections refresh daily from Roblox · 🤖 sections are written by Claude"
                 " after playtests · ✍️ **My Notes** is yours and never overwritten.\n")
    for kind, key, content in [
        ("auto", "snapshot", "## 📊 Snapshot\n_pending_"),
        ("auto", "trend", "## 📈 Chart Trend\n_pending_"),
    ]:
        parts.append(block(kind, key, content) + "\n")
    for key, heading, hint in AI_SECTIONS:
        parts.append(block("ai", key, f"## {heading}\n> [!todo] Not analyzed yet\n> {hint}") + "\n")
        if key == "monetization":
            parts.append(block("auto", "monetization_data", "## 📊 Store Data\n_pending_") + "\n")
    for key, content in [("description", "## 📊 Official Description\n_pending_"),
                         ("badges", "## 📊 Badges\n_pending_"),
                         ("playtests", "## 📊 Playtest History\n_No playtests yet._")]:
        parts.append(block("auto", key, content) + "\n")
    parts.append(f"{MY_NOTES}\n\n- \n")
    return "\n".join(parts)


def update_game_note(details: Any, trend: dict, chart_rank: int | None, sort_id: str,
                     history: list[dict], icon_rel: str | None, db_row: Any | None,
                     playtests: list[dict] | None = None) -> Path:
    """Create or refresh the data parts of a game note. Never touches 🤖 or ✍️ sections."""
    d = details
    existing = db_row["note_path"] if db_row is not None and "note_path" in db_row.keys() else None
    path = game_note_path(d.universe_id, d.name, existing)
    title = clean_display_name(d.name)
    if path.exists():
        raw_fm, body = split_frontmatter(path.read_text(encoding="utf-8"))
    else:
        raw_fm, body = {}, new_game_body(title, d.universe_id, icon_rel)

    mon = A.monetization(d.game_passes, d.dev_products)
    lr = d.like_ratio
    age = A.days_since(d.created)
    upd = A.days_since(d.updated)

    fm: dict[str, Any] = {
        "type": "roblox-game",
        "universe_id": d.universe_id,
        "place_id": d.place_id,
        "name": d.name,
        "url": d.url,
        "creator": d.creator_name,
        "creator_type": d.creator_type,
        "genre": d.genre,
        "subgenre": d.subgenre,
        "chart": sort_id,
        "chart_rank": chart_rank if chart_rank is not None else "",
        "best_rank": trend.get("best_rank", chart_rank or ""),
        "days_on_chart": trend.get("days_tracked", 0),
        "momentum": trend.get("momentum", 50),
        "playing": d.playing,
        "visits": d.visits,
        "favorites": d.favorites,
        "likes": d.up_votes,
        "dislikes": d.down_votes,
        "like_ratio": lr,
        "max_players": d.max_players,
        "game_age_days": age if age is not None else "",
        "days_since_update": upd if upd is not None else "",
        "roblox_created": d.created[:10],
        "roblox_updated": d.updated[:10],
        "gamepass_count": mon["passes"].get("count", 0),
        "gamepass_median": mon["passes"].get("median", ""),
        "gamepass_max": mon["passes"].get("max", ""),
        "devproduct_count": mon["products"].get("count", 0),
        "buy_all_passes": mon.get("buy_all_passes_robux", 0),
        "flags": trend.get("flags", []),
        "last_scan": datetime.now().strftime("%Y-%m-%d"),
    }
    # keep AI/user-owned frontmatter keys (scores, playtest info, tags…)
    keep = {k: v for k, v in raw_fm.items() if k not in fm}
    if "tags" not in keep:
        keep["tags"] = yaml_value(["roblox", "game", re.sub(r"\W+", "-", (d.genre or "unknown").lower())])
    if "status" not in keep:
        keep["status"] = '"tracked"'

    # --- snapshot
    snap = [
        "## 📊 Snapshot",
        f"[▶ Play on Roblox]({d.url}) · Creator: **{md_escape(d.creator_name)}**{' ✅' if d.creator_verified else ''} ({d.creator_type})",
        "",
        table(["Metric", "Value", "Metric", "Value"], [
            ["Chart rank", f"#{chart_rank}" if chart_rank else "off chart", "Playing now", A.fmt_num(d.playing)],
            ["Best rank", f"#{trend.get('best_rank', '–')}", "Visits", A.fmt_num(d.visits)],
            ["Momentum", f"{trend.get('momentum', 50)}/100", "Favorites", A.fmt_num(d.favorites)],
            ["Likes", f"{lr}% ({A.fmt_num(d.up_votes)}👍 / {A.fmt_num(d.down_votes)}👎)", "Server size", d.max_players],
            ["Genre", f"{d.genre} › {d.subgenre}".strip(" ›"), "Avatar", d.avatar_type],
            ["Released", f"{d.created[:10]} ({age}d ago)" if age is not None else "–",
             "Last update", f"{d.updated[:10]} ({upd}d ago)" if upd is not None else "–"],
            ["Private servers", "yes" if d.vip_servers else "no", "Servers sampled",
             (f"{d.server_sample.get('servers_sampled')} · {d.server_sample.get('avg_fps')} fps · "
              f"{d.server_sample.get('avg_ping')} ms · {d.server_sample.get('full_pct')}% full") if d.server_sample else "–"],
        ]),
    ]
    if trend.get("flags"):
        snap.append("\n" + " ".join(f"`{f}`" for f in trend["flags"]))
    body = set_block(body, "auto", "snapshot", "\n".join(snap))

    # --- trend
    tr = ["## 📈 Chart Trend",
          table(["Δ rank 1d", "Δ rank 7d", "CCU Δ 1d", "CCU Δ 7d", "Visits/day", "Favs/day", "Days tracked"], [[
              A.fmt_delta(trend.get("rank_change_1d")), A.fmt_delta(trend.get("rank_change_7d")),
              A.fmt_pct(trend.get("ccu_change_1d_pct")), A.fmt_pct(trend.get("ccu_change_7d_pct")),
              A.fmt_num(trend.get("visits_per_day")), A.fmt_num(trend.get("favorites_per_day")),
              trend.get("days_tracked", 0)]]),
          "",
          "<details><summary>Daily history</summary>\n",
          table(["Day", "Rank", "Playing", "Visits", "Like %"],
                [[h["day"], h["rank"], A.fmt_num(h["playing"]), A.fmt_num(h["visits"]),
                  A.like_ratio(h["up_votes"], h["down_votes"])] for h in reversed(history[-30:])]),
          "\n</details>"]
    body = set_block(body, "auto", "trend", "\n".join(tr))

    # --- monetization data
    pc = mon["pass_categories"]
    md = ["## 📊 Store Data",
          f"**{mon['passes'].get('count', 0)} game passes** (median R${mon['passes'].get('median', '–')}, "
          f"max R${mon['passes'].get('max', '–')}, buy-all R${mon.get('buy_all_passes_robux', 0)}) · "
          f"**{mon['products'].get('count', 0)} developer products** "
          f"(median R${mon['products'].get('median', '–')}, max R${mon['products'].get('max', '–')})",
          "",
          "Pass mix: " + (", ".join(f"{k} ×{v}" for k, v in pc.items()) or "–"),
          "",
          "### Game passes",
          table(["Pass", "R$", "Type", "Description"],
                [[g["name"], g["price"] if g["price"] is not None else "off-sale", g.get("category", ""),
                  (g.get("description") or "")[:140]] for g in sorted(d.game_passes, key=lambda x: x["price"] or 0)]),
          "",
          "<details><summary>Developer products</summary>\n",
          table(["Product", "R$", "Type", "Description"],
                [[g["name"], g["price"] or "–", g.get("category", ""), (g.get("description") or "")[:120]]
                 for g in sorted(d.dev_products, key=lambda x: x["price"] or 0)]),
          "\n</details>"]
    body = set_block(body, "auto", "monetization_data", "\n".join(md))

    # --- description
    desc = d.description.strip() or "_No description._"
    body = set_block(body, "auto", "description", "## 📊 Official Description\n" +
                     "\n".join("> " + line for line in desc.splitlines()))

    # --- badges
    if d.badges:
        bl = sorted(d.badges, key=lambda b: -(b.get("awarded") or 0))
        btxt = ["## 📊 Badges", f"{d.badge_total} badges (sorted by times awarded — shows how far players actually get):", "",
                table(["Badge", "Awarded", "Win rate", "Description"],
                      [[b["name"], A.fmt_num(b.get("awarded")), f"{round((b.get('win_rate') or 0) * 100, 2)}%",
                        (b.get("description") or "")[:100]] for b in bl[:40]])]
    else:
        btxt = ["## 📊 Badges", "_No public badges._"]
    body = set_block(body, "auto", "badges", "\n".join(btxt))

    if playtests is not None:
        body = set_block(body, "auto", "playtests", playtest_history_md(playtests))

    write(path, build_frontmatter(fm, keep) + body)
    return path


def playtest_history_md(playtests: list[dict]) -> str:
    if not playtests:
        return "## 📊 Playtest History\n_No playtests yet._"
    return "## 📊 Playtest History\n" + table(
        ["Date", "Status", "Log", "Summary"],
        [[(r.get("started") or "")[:10], r.get("status", ""),
          link(r["note_path"], "log") if r.get("note_path") else "–", (r.get("summary") or "")[:160]]
         for r in playtests])


def write_ai_sections(path: Path, sections: dict[str, str], scores: dict[str, int] | None = None,
                      one_liner: str | None = None, tags: list[str] | None = None, status: str = "analyzed") -> list[str]:
    """Write Claude's analysis into the 🤖 blocks. Unknown keys are ignored (returned as warnings)."""
    text = path.read_text(encoding="utf-8")
    raw_fm, body = split_frontmatter(text)
    warnings: list[str] = []
    headings = {k: h for k, h, _ in AI_SECTIONS}
    for key, content in sections.items():
        if key not in headings:
            warnings.append(f"unknown section '{key}' ignored (valid: {', '.join(AI_KEYS)})")
            continue
        content = content.strip()
        if not content.startswith("## "):
            content = f"## {headings[key]}\n{content}"
        stamp = f"\n\n<sub>🤖 updated {datetime.now().strftime('%Y-%m-%d')}</sub>"
        body = set_block(body, "ai", key, content + stamp)
    fm_vals: dict[str, Any] = {}
    if scores:
        for k, v in scores.items():
            try:
                fm_vals[f"score_{re.sub(r'[^a-z0-9]+', '_', k.lower()).strip('_')}"] = max(0, min(10, int(v)))
            except (TypeError, ValueError):
                warnings.append(f"score {k}={v!r} is not a number")
    if one_liner:
        fm_vals["one_liner"] = one_liner
    if tags:
        existing = re.findall(r"-\s*(.+)", raw_fm.get("tags", ""))
        merged = list(dict.fromkeys([t.strip().strip('"') for t in existing] +
                                    [re.sub(r"[^\w\-/]+", "-", t.lower()).strip("-") for t in tags]))
        fm_vals["tags"] = merged
    fm_vals["status"] = status
    fm_vals["last_analysis"] = datetime.now().strftime("%Y-%m-%d")
    # rebuild frontmatter: existing keys in place, replaced where updated
    merged_fm: dict[str, str] = {}
    for k, v in raw_fm.items():
        merged_fm[k] = yaml_value(fm_vals.pop(k)) if k in fm_vals else v
    for k, v in fm_vals.items():
        merged_fm[k] = yaml_value(v)
    fm_text = "---\n" + "\n".join(f"{k}:{'' if v.startswith(chr(10)) or not v else ' '}{v}" for k, v in merged_fm.items()) + "\n---\n"
    write(path, fm_text + body)
    return warnings


def read_note_sections(path: Path) -> dict[str, str]:
    text = path.read_text(encoding="utf-8")
    out = {}
    for key in AI_KEYS:
        b = get_block(text, "ai", key)
        if b is not None:
            out[key] = b
    notes = text.split(MY_NOTES, 1)
    out["my_notes"] = notes[1].strip() if len(notes) > 1 else ""
    return out


# ---------------------------------------------------------- chart notes ---

def chart_note(day: str, sort_name: str, rows: list[dict], new_ids: set[int], dropped: list[dict],
               extra: dict[str, list[dict]]) -> Path:
    path = p("charts") / f"{day} {sort_name}.md"
    risers = sorted([r for r in rows if (r.get("rank_change_1d") or 0) > 0], key=lambda r: -r["rank_change_1d"])[:5]
    lines = [
        build_frontmatter({"type": "chart-snapshot", "day": day, "chart": sort_name, "games": len(rows)}),
        f"# 📈 {sort_name} — {day}\n",
        table(["#", "Δ", "Game", "Genre", "Playing", "CCU Δ1d", "Like %", "Visits/day", "Flags"],
              [[r["rank"], "🆕" if r["universe_id"] in new_ids else A.fmt_delta(r.get("rank_change_1d")),
                glink(r), r.get("genre", ""),
                A.fmt_num(r.get("playing")), A.fmt_pct(r.get("ccu_change_1d_pct")), r.get("like_ratio", "–"),
                A.fmt_num(r.get("visits_per_day")), " ".join(r.get("flags") or [])] for r in rows]),
        "",
        "## 🆕 New entries",
        "\n".join(f"- {glink(r)} at **#{r['rank']}** ({r.get('genre', '')})"
                  for r in rows if r["universe_id"] in new_ids) or "_None today._",
        "",
        "## 🚀 Biggest climbers",
        "\n".join(f"- {glink(r)} {A.fmt_delta(r.get('rank_change_1d'))} → #{r['rank']}" for r in risers)
        or "_None._",
        "",
        "## 📉 Dropped off",
        "\n".join(f"- {r['title']} (was #{r['rank']})" for r in dropped) or "_None._",
    ]
    for sname, srows in extra.items():
        lines += ["", f"## Also: {sname}",
                  table(["#", "Game", "Genre", "Playing"],
                        [[r["rank"], glink(r), r.get("genre", ""),
                          A.fmt_num(r.get("playing"))] for r in srows])]
    write(path, "\n".join(lines) + "\n")
    return path


def genre_notes(games: list[dict]) -> None:
    by: dict[str, list[dict]] = {}
    for g in games:
        by.setdefault(g.get("genre") or "Unknown", []).append(g)
    for genre, gs in by.items():
        path = p("genres") / f"{safe_filename(genre)}.md"
        text = path.read_text(encoding="utf-8") if path.exists() else (
            build_frontmatter({"type": "genre", "genre": genre}) + f"# 🧩 {genre}\n\n" +
            block("ai", "patterns", "## 🤖 Genre Patterns\n> [!todo] Claude writes cross-game patterns here during weekly runs.") +
            f"\n\n{MY_NOTES}\n\n- \n")
        avg_like = round(sum(g.get("like_ratio") or 0 for g in gs) / len(gs), 1)
        content = [f"## 📊 Games on the chart now ({len(gs)})",
                   f"Total playing: **{A.fmt_num(sum(g.get('playing') or 0 for g in gs))}** · avg like ratio **{avg_like}%**", "",
                   table(["#", "Game", "Sub-genre", "Playing", "Like %", "Passes", "Momentum"],
                         [[g["rank"], link(g["note"], g["title"]), g.get("subgenre", ""), A.fmt_num(g.get("playing")),
                           g.get("like_ratio"), g.get("passes", 0), g.get("momentum", 50)]
                          for g in sorted(gs, key=lambda x: x["rank"])])]
        text = set_block(text, "auto", "current", "\n".join(content))
        write(path, text)


def dashboard(day: str, sort_name: str, rows: list[dict], new_ids: set[int], dropped: list[dict],
              queue: list[dict], recent_playtests: list[dict], status_lines: list[str]) -> Path:
    path = vault_root() / "00 Dashboard.md"
    genres: dict[str, int] = {}
    for r in rows:
        genres[r.get("genre") or "Unknown"] = genres.get(r.get("genre") or "Unknown", 0) + 1
    hot = sorted(rows, key=lambda r: -(r.get("momentum") or 0))[:5]
    text = "\n".join([
        f"# 🏠 ROX-AI Dashboard\n",
        f"_Last scan: **{day}** · chart: **{sort_name}** · {len(rows)} games_\n",
        "> [!abstract] System status\n" + "\n".join(f"> {s}" for s in status_lines) + "\n",
        f"## 🏆 Today's {sort_name}",
        table(["#", "Δ", "Game", "Genre", "Playing", "Like %", "Momentum", "Playtested"],
              [[r["rank"], "🆕" if r["universe_id"] in new_ids else A.fmt_delta(r.get("rank_change_1d")),
                glink(r), r.get("genre", ""), A.fmt_num(r.get("playing")), r.get("like_ratio"),
                r.get("momentum", 50), r.get("last_playtest") or "—"] for r in rows]),
        "",
        "## 🔥 Hottest momentum",
        "\n".join(f"- {glink(r)} — momentum **{r.get('momentum')}**, CCU {A.fmt_pct(r.get('ccu_change_1d_pct'))}"
                  for r in hot) or "_Needs 2+ days of data._",
        "",
        "## 🆕 New today",
        "\n".join(f"- {glink(r)} (#{r['rank']})" for r in rows if r["universe_id"] in new_ids) or "_None._",
        "",
        "## 📉 Fell off the chart",
        "\n".join(f"- {r['title']}" for r in dropped) or "_None._",
        "",
        "## 🧩 Genre mix",
        table(["Genre", "Games"], sorted(([g, n] for g, n in genres.items()), key=lambda x: -x[1])),
        "",
        "## 🕹️ Next playtest queue",
        "\n".join(f"{i + 1}. {glink(q)} — {q['reason']}"
                  for i, q in enumerate(queue)) or "_Queue empty — everything is up to date._",
        "",
        "## 📝 Recent playtests",
        "\n".join(f"- {r['started'][:10]} · {link(r['note_path'], r['title']) if r.get('note_path') else r['title']} · {r['status']}"
                  for r in recent_playtests) or "_None yet._",
        "",
        "## 🔎 Live queries (Dataview)",
        "```dataview\nTABLE chart_rank AS \"#\", playing AS \"Playing\", like_ratio AS \"Like %\", momentum, score_fun AS \"Fun\", "
        "score_retention AS \"Retention\", last_analysis AS \"Analyzed\"\nFROM \"02 Games\"\nWHERE chart_rank\nSORT chart_rank ASC\n```",
        "",
        "Best clone targets (high fun, low clone difficulty):",
        "```dataview\nTABLE score_fun AS Fun, score_clone_difficulty AS \"Clone diff.\", genre, one_liner\nFROM \"02 Games\"\n"
        "WHERE score_fun\nSORT (score_fun - score_clone_difficulty) DESC\nLIMIT 15\n```",
        "",
        "Monetization leaders:",
        "```dataview\nTABLE gamepass_count AS Passes, gamepass_median AS \"Median R$\", buy_all_passes AS \"Buy-all R$\", "
        "devproduct_count AS Products, score_monetization_aggressiveness AS Aggro\nFROM \"02 Games\"\nSORT buy_all_passes DESC\nLIMIT 15\n```",
        "",
        "Latest insights:",
        "```dataview\nLIST FROM \"05 Insights\" SORT file.ctime DESC LIMIT 10\n```",
    ])
    write(path, text + "\n")
    return path


# ------------------------------------------------------------ playtests ---

def playtest_note_path(universe_id: int, name: str, day: str | None = None) -> Path:
    day = day or datetime.now().strftime("%Y-%m-%d")
    base = p("playtests") / f"{day} {safe_filename(name)}.md"
    i = 2
    path = base
    while path.exists():
        path = base.with_name(f"{base.stem} #{i}.md")
        i += 1
    return path


def create_playtest_note(path: Path, game_note: Path | None, name: str, universe_id: int, minutes: int) -> None:
    fm = build_frontmatter({"type": "playtest", "universe_id": universe_id, "game": name,
                            "date": datetime.now().strftime("%Y-%m-%d %H:%M"), "budget_minutes": minutes,
                            "status": "running", "tags": ["playtest"]})
    body = "\n".join([
        f"# 🕹️ Playtest — {clean_display_name(name)}",
        f"Game note: {link(game_note) if game_note else '–'}\n",
        block("auto", "timeline", "## ⏱️ Timeline\n"),
        "",
        block("auto", "shots", "## 📸 Screenshots\n"),
        "",
        block("ai", "report", "## 🤖 Final Report\n_pending_"),
        "",
        block("auto", "problems", "## ⚠️ Problems\n_None._"),
        "",
        MY_NOTES, "", "- ",
    ])
    write(path, fm + body + "\n")


def append_to_block(path: Path, kind: str, key: str, line: str) -> None:
    text = path.read_text(encoding="utf-8")
    cur = get_block(text, kind, key) or ""
    if "_None._" in cur:
        cur = cur.replace("_None._", "").rstrip()
    text = set_block(text, kind, key, cur.rstrip() + "\n" + line)
    write(path, text)


def set_fm_value(path: Path, key: str, value: Any) -> None:
    text = path.read_text(encoding="utf-8")
    raw, body = split_frontmatter(text)
    raw[key] = yaml_value(value)
    fm_text = "---\n" + "\n".join(f"{k}:{'' if v.startswith(chr(10)) or not v else ' '}{v}" for k, v in raw.items()) + "\n---\n"
    write(path, fm_text + body)


def write_insight(title: str, content: str, tags: list[str] | None = None) -> Path:
    day = datetime.now().strftime("%Y-%m-%d")
    path = p("insights") / f"{day} {safe_filename(title)}.md"
    fm = build_frontmatter({"type": "insight", "date": day, "tags": ["insight", *(tags or [])]})
    body = content.strip()
    if not body.startswith("# "):
        body = f"# {title}\n\n{body}"
    write(path, fm + body + "\n")
    return path


def set_genre_patterns(genre: str, content: str) -> Path:
    path = p("genres") / f"{safe_filename(genre)}.md"
    text = path.read_text(encoding="utf-8") if path.exists() else (
        build_frontmatter({"type": "genre", "genre": genre}) + f"# 🧩 {genre}\n\n{MY_NOTES}\n\n- \n")
    if not content.strip().startswith("## "):
        content = "## 🤖 Genre Patterns\n" + content
    text = set_block(text, "ai", "patterns", content + f"\n\n<sub>🤖 updated {datetime.now().strftime('%Y-%m-%d')}</sub>")
    write(path, text)
    return path
