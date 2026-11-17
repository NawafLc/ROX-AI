"""Pure-data analytics (no AI): trends, momentum, monetization breakdowns."""
from __future__ import annotations

import re
import statistics
from datetime import datetime, timezone
from typing import Any, Sequence


def pct(a: float, b: float) -> float | None:
    if not b:
        return None
    return round((a - b) / b * 100, 1)


def parse_dt(s: str) -> datetime | None:
    if not s:
        return None
    try:
        s = s.replace("Z", "+00:00")
        if "." in s:  # Roblox sometimes sends 7-digit fractions
            head, tail = s.split(".", 1)
            frac = re.match(r"\d+", tail).group(0)  # type: ignore[union-attr]
            rest = tail[len(frac):]
            s = f"{head}.{frac[:6]}{rest}"
        dt = datetime.fromisoformat(s)
        return dt if dt.tzinfo else dt.replace(tzinfo=timezone.utc)
    except Exception:  # noqa: BLE001
        return None


def days_since(s: str) -> int | None:
    dt = parse_dt(s)
    if not dt:
        return None
    return max(0, (datetime.now(timezone.utc) - dt).days)


def trend(history: Sequence[Any]) -> dict:
    """history: snapshot rows oldest->newest with keys day, rank, playing, visits, favorites."""
    if not history:
        return {}
    h = [dict(r) for r in history]
    cur = h[-1]
    out: dict[str, Any] = {
        "rank": cur["rank"],
        "playing": cur["playing"],
        "days_tracked": len(h),
        "best_rank": min(r["rank"] for r in h if r["rank"]),
    }
    if len(h) >= 2:
        prev = h[-2]
        out["rank_change_1d"] = (prev["rank"] - cur["rank"]) if prev["rank"] and cur["rank"] else None  # + = climbing
        out["ccu_change_1d_pct"] = pct(cur["playing"], prev["playing"])
        dd = _day_gap(prev["day"], cur["day"]) or 1
        out["visits_per_day"] = max(0, int((cur["visits"] - prev["visits"]) / dd)) if cur["visits"] and prev["visits"] else None
        out["favorites_per_day"] = max(0, int((cur["favorites"] - prev["favorites"]) / dd)) if cur["favorites"] and prev["favorites"] else None
    week = [r for r in h if (_day_gap(r["day"], cur["day"]) or 0) <= 7]
    if len(week) >= 2:
        out["ccu_change_7d_pct"] = pct(cur["playing"], week[0]["playing"])
        out["rank_change_7d"] = (week[0]["rank"] - cur["rank"]) if week[0]["rank"] and cur["rank"] else None
    out["momentum"] = momentum(out)
    out["flags"] = flags(out, len(h))
    return out


def _day_gap(a: str, b: str) -> int | None:
    try:
        return (datetime.strptime(b, "%Y-%m-%d") - datetime.strptime(a, "%Y-%m-%d")).days
    except Exception:  # noqa: BLE001
        return None


def momentum(t: dict) -> int:
    """0-100 score mixing CCU growth and rank climbing. 50 = flat."""
    s = 50.0
    if t.get("ccu_change_1d_pct") is not None:
        s += max(-20, min(20, t["ccu_change_1d_pct"] / 2))
    if t.get("ccu_change_7d_pct") is not None:
        s += max(-20, min(20, t["ccu_change_7d_pct"] / 5))
    if t.get("rank_change_1d") is not None:
        s += max(-10, min(10, t["rank_change_1d"]))
    return int(max(0, min(100, round(s))))


def flags(t: dict, days_tracked: int) -> list[str]:
    f: list[str] = []
    if days_tracked == 1:
        f.append("new-entry")
    if (t.get("rank_change_1d") or 0) >= 5 or (t.get("ccu_change_1d_pct") or 0) >= 40:
        f.append("rising-fast")
    if (t.get("rank_change_1d") or 0) <= -5 or (t.get("ccu_change_1d_pct") or 0) <= -35:
        f.append("falling")
    if t.get("momentum", 50) >= 70:
        f.append("hot")
    return f


def like_ratio(up: int, down: int) -> float:
    t = (up or 0) + (down or 0)
    return round(up / t * 100, 1) if t else 0.0


PASS_CATEGORIES = [
    ("multiplier", r"\b(x\s?\d|\d\s?x|double|triple|2x|3x|boost|multiplier)\b"),
    ("vip/status", r"\b(vip|premium|mvp|elite|supporter|exclusive|tag|title)\b"),
    ("automation", r"\b(auto|afk|idle|offline)\b"),
    ("luck/gacha", r"\b(luck|lucky|egg|hatch|spin|crate|roll|chest|gacha|pet)\b"),
    ("currency", r"\b(coins?|cash|gems?|money|bucks|robux|currency|tokens?)\b"),
    ("speed/qol", r"\b(speed|fast|walk|teleport|tp|skip|instant|storage|inventory|slot|backpack|capacity)\b"),
    ("cosmetic", r"\b(skin|trail|aura|color|cosmetic|emote|effect|glow|title)\b"),
    ("power/item", r"\b(sword|gun|weapon|tool|power|ability|item|admin|god|carpet|coil)\b"),
]


def categorize(name: str, desc: str = "") -> str:
    text = f"{name} {desc}".lower()
    for cat, rx in PASS_CATEGORIES:
        if re.search(rx, text):
            return cat
    return "other"


def price_stats(items: list[dict]) -> dict:
    prices = [int(i["price"]) for i in items if i.get("price")]
    if not prices:
        return {"count": len(items), "for_sale": 0}
    return {
        "count": len(items),
        "for_sale": len(prices),
        "min": min(prices),
        "max": max(prices),
        "median": int(statistics.median(prices)),
        "avg": int(sum(prices) / len(prices)),
        "total": sum(prices),
    }


def monetization(passes: list[dict], products: list[dict]) -> dict:
    cats: dict[str, int] = {}
    for p in passes:
        c = categorize(p.get("name", ""), p.get("description", ""))
        p["category"] = c
        cats[c] = cats.get(c, 0) + 1
    for p in products:
        p["category"] = categorize(p.get("name", ""), p.get("description", ""))
    ps, ds = price_stats(passes), price_stats(products)
    # "Whale ceiling": cost of buying every pass once
    return {
        "passes": ps,
        "products": ds,
        "pass_categories": dict(sorted(cats.items(), key=lambda kv: -kv[1])),
        "buy_all_passes_robux": ps.get("total", 0),
        "has_starter_pack": any(re.search(r"starter|beginner|newbie", (p.get("name") or "").lower()) for p in passes + products),
        "has_vip": any(p.get("category") == "vip/status" for p in passes),
        "has_auto": any(p.get("category") == "automation" for p in passes),
        "has_luck": any(p.get("category") == "luck/gacha" for p in passes + products),
    }


def fmt_num(n: float | int | None) -> str:
    if n is None:
        return "–"
    n = float(n)
    for unit, div in (("B", 1e9), ("M", 1e6), ("K", 1e3)):
        if abs(n) >= div:
            return f"{n / div:.1f}{unit}".replace(".0", "")
    return str(int(n))


def fmt_pct(p: float | None) -> str:
    if p is None:
        return "–"
    return f"{'+' if p > 0 else ''}{p}%"


def fmt_delta(d: int | None) -> str:
    if d is None:
        return "–"
    if d > 0:
        return f"▲{d}"
    if d < 0:
        return f"▼{-d}"
    return "="
