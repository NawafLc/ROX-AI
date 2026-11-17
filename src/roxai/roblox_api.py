"""Roblox public web APIs (no login needed).

Every call is retried with backoff and rate-limit handling, and every function
degrades gracefully (returns empty data) instead of crashing a run, because
Roblox endpoints change and occasionally fail.
"""
from __future__ import annotations

import random
import time
import uuid
from dataclasses import dataclass, field
from typing import Any, Iterable

import httpx

from . import log

L = log.get("api")

UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ROX-AI/1.0"
_client: httpx.Client | None = None


def client() -> httpx.Client:
    global _client
    if _client is None:
        _client = httpx.Client(timeout=25, headers={"User-Agent": UA, "Accept": "application/json"},
                               follow_redirects=True)
    return _client


def get_json(url: str, params: dict | None = None, tries: int = 5) -> Any:
    """GET with retries. Raises the last error only if every attempt failed."""
    last: Exception | None = None
    for attempt in range(tries):
        try:
            r = client().get(url, params=params)
            if r.status_code == 429 or r.status_code >= 500:
                wait = float(r.headers.get("retry-after") or 0) or (2 ** attempt + random.random())
                L.warning("HTTP %s from %s, retrying in %.1fs", r.status_code, url, wait)
                time.sleep(min(wait, 30))
                last = httpx.HTTPStatusError(f"{r.status_code}", request=r.request, response=r)
                continue
            r.raise_for_status()
            return r.json()
        except (httpx.TransportError, ValueError) as e:  # network / bad JSON
            last = e
            time.sleep(min(2 ** attempt + random.random(), 30))
        except httpx.HTTPStatusError as e:
            # 4xx other than 429: not worth retrying
            raise e
    assert last is not None
    raise last


def safe(fn, default):
    try:
        return fn()
    except Exception as e:  # noqa: BLE001
        L.warning("API call failed (%s): %s", getattr(fn, "__name__", "call"), e)
        return default


def chunks(seq: list, n: int) -> Iterable[list]:
    for i in range(0, len(seq), n):
        yield seq[i:i + n]


# ---------------------------------------------------------------- charts ---

@dataclass
class ChartEntry:
    rank: int
    universe_id: int
    place_id: int
    name: str
    playing: int = 0
    up_votes: int = 0
    down_votes: int = 0
    genre: str = ""
    maturity: str = ""
    sponsored: bool = False


def list_sorts(country: str = "all") -> list[dict]:
    sid = str(uuid.uuid4())
    data = get_json("https://apis.roblox.com/explore-api/v1/get-sorts",
                    {"sessionId": sid, "device": "computer", "country": country})
    return [
        {"sortId": s.get("sortId"), "name": s.get("sortDisplayName"), "count": len(s.get("games") or [])}
        for s in data.get("sorts", []) if s.get("contentType") == "Games"
    ]


def get_chart(sort_id: str = "top-trending", limit: int = 25, country: str = "all") -> list[ChartEntry]:
    sid = str(uuid.uuid4())
    games: list[dict] = []
    try:
        data = get_json("https://apis.roblox.com/explore-api/v1/get-sort-content",
                        {"sessionId": sid, "sortId": sort_id, "device": "computer", "country": country})
        games = data.get("games") or []
    except Exception as e:  # noqa: BLE001
        L.warning("get-sort-content failed for %s (%s); falling back to get-sorts", sort_id, e)
    if not games:
        data = get_json("https://apis.roblox.com/explore-api/v1/get-sorts",
                        {"sessionId": sid, "device": "computer", "country": country})
        for s in data.get("sorts", []):
            if s.get("sortId") == sort_id:
                games = s.get("games") or []
    out: list[ChartEntry] = []
    for g in games:
        if g.get("isSponsored"):
            continue  # ads are not real chart positions
        out.append(ChartEntry(
            rank=len(out) + 1,
            universe_id=int(g["universeId"]),
            place_id=int(g.get("rootPlaceId") or 0),
            name=str(g.get("name") or "").strip(),
            playing=int(g.get("playerCount") or 0),
            up_votes=int(g.get("totalUpVotes") or 0),
            down_votes=int(g.get("totalDownVotes") or 0),
            genre=str(g.get("genreL1") or ""),
            maturity=str(g.get("contentMaturity") or ""),
        ))
        if len(out) >= limit:
            break
    return out


# ------------------------------------------------------------ game info ---

@dataclass
class GameDetails:
    universe_id: int
    place_id: int = 0
    name: str = ""
    description: str = ""
    creator_name: str = ""
    creator_id: int = 0
    creator_type: str = ""
    creator_verified: bool = False
    playing: int = 0
    visits: int = 0
    favorites: int = 0
    max_players: int = 0
    created: str = ""
    updated: str = ""
    genre: str = ""
    subgenre: str = ""
    avatar_type: str = ""
    vip_servers: bool = False
    up_votes: int = 0
    down_votes: int = 0
    icon_url: str = ""
    thumbnail_urls: list[str] = field(default_factory=list)
    game_passes: list[dict] = field(default_factory=list)
    dev_products: list[dict] = field(default_factory=list)
    badges: list[dict] = field(default_factory=list)
    badge_total: int = 0
    server_sample: dict = field(default_factory=dict)

    @property
    def like_ratio(self) -> float:
        t = self.up_votes + self.down_votes
        return round(self.up_votes / t * 100, 1) if t else 0.0

    @property
    def url(self) -> str:
        return f"https://www.roblox.com/games/{self.place_id}"


def games_info(universe_ids: list[int]) -> dict[int, dict]:
    out: dict[int, dict] = {}
    for c in chunks(universe_ids, 50):
        data = get_json("https://games.roblox.com/v1/games", {"universeIds": ",".join(map(str, c))})
        for g in data.get("data", []):
            out[int(g["id"])] = g
    return out


def votes(universe_ids: list[int]) -> dict[int, tuple[int, int]]:
    out: dict[int, tuple[int, int]] = {}
    for c in chunks(universe_ids, 50):
        data = get_json("https://games.roblox.com/v1/games/votes", {"universeIds": ",".join(map(str, c))})
        for v in data.get("data", []):
            out[int(v["id"])] = (int(v.get("upVotes") or 0), int(v.get("downVotes") or 0))
    return out


def icons(universe_ids: list[int], size: str = "512x512") -> dict[int, str]:
    out: dict[int, str] = {}
    for c in chunks(universe_ids, 50):
        data = get_json("https://thumbnails.roblox.com/v1/games/icons",
                        {"universeIds": ",".join(map(str, c)), "size": size, "format": "Png", "isCircular": "false"})
        for d in data.get("data", []):
            if d.get("state") == "Completed" and d.get("imageUrl"):
                out[int(d["targetId"])] = d["imageUrl"]
    return out


def thumbnails(universe_id: int, count: int = 3) -> list[str]:
    data = get_json("https://thumbnails.roblox.com/v1/games/multiget/thumbnails",
                    {"universeIds": universe_id, "size": "768x432", "format": "Png", "countPerUniverse": count})
    urls: list[str] = []
    for u in data.get("data", []):
        for t in u.get("thumbnails") or []:
            if t.get("state") == "Completed" and t.get("imageUrl"):
                urls.append(t["imageUrl"])
    return urls


def game_passes(universe_id: int) -> list[dict]:
    out: list[dict] = []
    token = ""
    for _ in range(10):
        params = {"passView": "Full", "pageSize": 100}
        if token:
            params["pageToken"] = token
        data = get_json(f"https://apis.roblox.com/game-passes/v1/universes/{universe_id}/game-passes", params)
        for p in data.get("gamePasses", []):
            out.append({
                "id": p.get("id"),
                "name": p.get("displayName") or p.get("name"),
                "price": p.get("price") if p.get("isForSale") else None,
                "for_sale": bool(p.get("isForSale")),
                "description": (p.get("displayDescription") or "").strip(),
                "created": p.get("created", ""),
            })
        token = data.get("nextPageToken") or ""
        if not token:
            break
    return out


def dev_products(universe_id: int) -> list[dict]:
    out: list[dict] = []
    cursor = ""
    for _ in range(10):
        params: dict[str, Any] = {"limit": 50}
        if cursor:
            params["cursor"] = cursor
        data = get_json(f"https://apis.roblox.com/developer-products/v2/universes/{universe_id}/developerproducts",
                        params)
        for p in data.get("developerProducts", []):
            out.append({
                "id": p.get("ProductId"),
                "name": p.get("displayName") or p.get("Name"),
                "price": p.get("PriceInRobux"),
                "for_sale": bool(p.get("IsForSale")),
                "description": (p.get("displayDescription") or p.get("Description") or "").strip(),
            })
        cursor = data.get("nextPageCursor") or ""
        if not cursor:
            break
    return out


def badges(universe_id: int, max_pages: int = 3) -> tuple[list[dict], int]:
    out: list[dict] = []
    cursor = ""
    pages = 0
    while pages < max_pages:
        params: dict[str, Any] = {"limit": 100, "sortOrder": "Asc"}
        if cursor:
            params["cursor"] = cursor
        data = get_json(f"https://badges.roblox.com/v1/universes/{universe_id}/badges", params)
        for b in data.get("data", []):
            st = b.get("statistics") or {}
            out.append({
                "name": b.get("displayName") or b.get("name"),
                "description": (b.get("displayDescription") or b.get("description") or "").strip(),
                "awarded": st.get("awardedCount", 0),
                "win_rate": st.get("winRatePercentage", 0),
            })
        cursor = data.get("nextPageCursor") or ""
        pages += 1
        if not cursor:
            break
    return out, len(out)


def server_sample(place_id: int) -> dict:
    data = get_json(f"https://games.roblox.com/v1/games/{place_id}/servers/Public", {"limit": 100})
    servers = data.get("data") or []
    if not servers:
        return {}
    fps = [s.get("fps") or 0 for s in servers]
    ping = [s.get("ping") or 0 for s in servers]
    full = sum(1 for s in servers if s.get("playing") and s.get("maxPlayers") and s["playing"] >= s["maxPlayers"])
    return {
        "servers_sampled": len(servers),
        "avg_fps": round(sum(fps) / len(fps), 1),
        "avg_ping": round(sum(ping) / len(ping)),
        "full_pct": round(full / len(servers) * 100),
    }


def game_details(universe_id: int, deep: bool = True) -> GameDetails:
    """Collect everything public about one experience."""
    d = GameDetails(universe_id=universe_id)
    info = safe(lambda: games_info([universe_id]).get(universe_id, {}), {})
    if info:
        cr = info.get("creator") or {}
        d.place_id = int(info.get("rootPlaceId") or 0)
        d.name = info.get("name", "")
        d.description = info.get("description") or ""
        d.creator_name = cr.get("name", "")
        d.creator_id = int(cr.get("id") or 0)
        d.creator_type = cr.get("type", "")
        d.creator_verified = bool(cr.get("hasVerifiedBadge"))
        d.playing = int(info.get("playing") or 0)
        d.visits = int(info.get("visits") or 0)
        d.favorites = int(info.get("favoritedCount") or 0)
        d.max_players = int(info.get("maxPlayers") or 0)
        d.created = info.get("created", "")
        d.updated = info.get("updated", "")
        d.genre = info.get("genre_l1") or info.get("genre") or ""
        d.subgenre = info.get("genre_l2") or ""
        d.avatar_type = info.get("universeAvatarType", "")
        d.vip_servers = bool(info.get("createVipServersAllowed"))
    v = safe(lambda: votes([universe_id]).get(universe_id), None)
    if v:
        d.up_votes, d.down_votes = v
    d.icon_url = safe(lambda: icons([universe_id]).get(universe_id, ""), "")
    if deep:
        d.thumbnail_urls = safe(lambda: thumbnails(universe_id), [])
        d.game_passes = safe(lambda: game_passes(universe_id), [])
        d.dev_products = safe(lambda: dev_products(universe_id), [])
        d.badges, d.badge_total = safe(lambda: badges(universe_id), ([], 0))
        if d.place_id:
            d.server_sample = safe(lambda: server_sample(d.place_id), {})
    return d


def download(url: str) -> bytes | None:
    try:
        r = client().get(url)
        r.raise_for_status()
        return r.content
    except Exception as e:  # noqa: BLE001
        L.warning("download failed %s: %s", url, e)
        return None
