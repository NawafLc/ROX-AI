import datetime as dt

from roxai import analysis as A
from roxai import db as D
from roxai import vault as V


def test_scan_builds_vault(home, fake_api):
    from roxai.scan import run_scan, playtest_queue
    r = run_scan()
    assert r["games"] == 3 and r["errors"] == 0 and r["new"] == 3
    root = V.vault_root()
    note = root / "02 Games" / "Pet Sim Mega.md"
    assert note.exists()
    t = note.read_text(encoding="utf-8")
    assert "universe_id: 1\n" in t and "chart_rank: 1" in t
    assert "| VIP | 299 | vip/status |" in t
    assert "off-sale" in t
    assert (root / "00 Dashboard.md").read_text(encoding="utf-8").count("[[02 Games/") >= 3
    assert any(f.name.endswith("Top Trending.md") for f in (root / "01 Charts").iterdir())
    assert (root / "04 Genres" / "Obby.md").exists()
    q = playtest_queue()
    assert [x["universe_id"] for x in q] == [1, 2, 3]


def test_second_day_trends_and_ai_preserved(home, fake_api, monkeypatch):
    from roxai import scan
    scan.run_scan()
    note = V.vault_root() / "02 Games" / "Pet Sim Mega.md"
    V.write_ai_sections(note, {"core_loop": "LOOP-TEXT"}, {"fun": 7})
    txt = note.read_text(encoding="utf-8").replace("- \n", "- my idea\n")
    V.write(note, txt)

    # simulate next day: game 3 drops out, game 4 enters, game 2 climbs, player counts change
    tomorrow = (dt.date.today() + dt.timedelta(days=1)).isoformat()
    monkeypatch.setattr(scan, "today", lambda: tomorrow)
    fake_api["chart"] = [2, 1, 4]
    fake_api["playing"][1] = 2000
    fake_api["visits"][1] = 20_000
    r = scan.run_scan()
    assert r["new"] == 1 and r["dropped"] == 1
    t = note.read_text(encoding="utf-8")
    assert "LOOP-TEXT" in t and "score_fun: 7" in t and "my idea" in t
    assert "chart_rank: 2" in t and "days_on_chart: 2" in t
    assert "+100.0%" in t  # CCU doubled
    chart = (V.vault_root() / "01 Charts" / f"{tomorrow} Top Trending.md").read_text(encoding="utf-8")
    assert "Brainrot Battles (was #3)" in chart
    assert "🆕" in chart


def test_scan_survives_one_broken_game(home, fake_api, monkeypatch):
    import roxai.roblox_api as R
    good = R.game_details

    def flaky(uid, deep=True):
        if uid == 2:
            raise RuntimeError("boom")
        return good(uid, deep)
    monkeypatch.setattr(R, "game_details", flaky)
    from roxai.scan import run_scan
    r = run_scan()
    assert r["errors"] == 1 and r["games"] == 3
    assert D.get_db().last_run("scan")["status"] == "ok"


def test_trend_math():
    h = [dict(day="2026-01-01", rank=10, playing=100, visits=1000, favorites=10),
         dict(day="2026-01-02", rank=4, playing=200, visits=3000, favorites=30)]
    t = A.trend(h)
    assert t["rank_change_1d"] == 6 and t["ccu_change_1d_pct"] == 100.0 and t["visits_per_day"] == 2000
    assert "rising-fast" in t["flags"] and t["momentum"] > 60


def test_monetization():
    m = A.monetization([{"name": "x2 Cash", "price": 99}, {"name": "VIP", "price": 400}, {"name": "Auto Collect", "price": 150}],
                       [{"name": "Starter Pack", "price": 49}])
    assert m["passes"]["median"] == 150 and m["buy_all_passes_robux"] == 649
    assert m["has_vip"] and m["has_auto"] and m["has_starter_pack"]
    assert A.fmt_num(290_707_003) == "290.7M" and A.fmt_num(1000) == "1K"


def test_parse_roblox_dates():
    assert A.days_since("2026-11-17T00:39:26.7266667Z") is not None
    assert A.days_since("") is None
