import json
import os
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))


@pytest.fixture()
def home(tmp_path, monkeypatch):
    """Isolated ROX-AI home + vault for every test."""
    h = tmp_path / "home"
    h.mkdir()
    (h / "settings.json").write_text(json.dumps({"vault_path": str(tmp_path / "vault"), "chart_limit": 3,
                                                 "extra_sorts": ["up-and-coming"]}))
    monkeypatch.setenv("ROXAI_HOME", str(h))
    import roxai.roblox_api as R
    monkeypatch.setattr(R, "download", lambda url: None)  # never hit the network in tests
    import roxai.config as C
    import roxai.db as D
    C._settings = None
    D._db = None
    yield h
    if D._db is not None:
        D._db.conn.close()
    D._db = None
    C._settings = None


GAMES = {
    1: dict(name="[🎃 UPDATE] Pet Sim Mega", place=11, genre="Simulation", sub="Tycoon"),
    2: dict(name="Obby Rush!", place=22, genre="Obby", sub="Classic"),
    3: dict(name="Brainrot | Battles", place=33, genre="Action", sub="PvP"),
    4: dict(name="Newcomer", place=44, genre="Action", sub="PvP"),
}


@pytest.fixture()
def fake_api(monkeypatch):
    """Offline stand-in for the Roblox web APIs. `state['chart']` controls today's chart order."""
    import roxai.roblox_api as R

    state = {"chart": [1, 2, 3], "playing": {1: 1000, 2: 500, 3: 300, 4: 900}, "visits": {1: 10_000, 2: 5000, 3: 3000, 4: 100}}

    def get_chart(sort_id="top-trending", limit=25, country="all"):
        ids = state["chart"] if sort_id == "top-trending" else list(reversed(state["chart"]))
        return [R.ChartEntry(rank=i + 1, universe_id=u, place_id=GAMES[u]["place"], name=GAMES[u]["name"],
                             playing=state["playing"][u], up_votes=90, down_votes=10, genre=GAMES[u]["genre"])
                for i, u in enumerate(ids[:limit])]

    def game_details(uid, deep=True):
        g = GAMES[uid]
        d = R.GameDetails(universe_id=uid, place_id=g["place"], name=g["name"], description="Collect pets!\nRebirth!",
                          creator_name="Dev Group", creator_type="Group", playing=state["playing"][uid],
                          visits=state["visits"][uid], favorites=100, max_players=12, created="2026-09-18T21:29:29.437Z",
                          updated="2026-11-17T00:39:26.7266667Z", genre=g["genre"], subgenre=g["sub"], up_votes=90, down_votes=10,
                          icon_url="")
        d.game_passes = [{"name": "VIP", "price": 299, "for_sale": True, "description": "VIP tag"},
                         {"name": "x2 Coins", "price": 99, "for_sale": True, "description": "double coins"},
                         {"name": "Auto Farm", "price": None, "for_sale": False, "description": ""}]
        d.dev_products = [{"name": "100 Gems", "price": 25, "for_sale": True, "description": ""}]
        d.badges = [{"name": "Welcome", "description": "joined", "awarded": 5000, "win_rate": 0.9}]
        d.badge_total = 1
        return d

    monkeypatch.setattr(R, "get_chart", get_chart)
    monkeypatch.setattr(R, "game_details", game_details)
    monkeypatch.setattr(R, "games_info", lambda ids: {u: {"visits": state["visits"][u], "favoritedCount": 1} for u in ids})
    monkeypatch.setattr(R, "download", lambda url: None)
    return state
