import asyncio
import json
import os

from roxai import vault as V


def test_mcp_tools_registered():
    from roxai.mcp_server import mcp
    names = {t.name for t in asyncio.run(mcp.list_tools())}
    for n in ["charts", "game_info", "playtest_start", "look", "act", "note", "snap", "playtest_finish",
              "playtest_abort", "emergency_stop", "vault_write_game_analysis", "vault_write_insight", "system_check"]:
        assert n in names


def test_playbook_mentions_every_section():
    from roxai.mcp_server import playbook_text
    txt = playbook_text(1, "Game", 1)
    for k in V.AI_KEYS:
        assert k in txt
    assert "Never spend Robux" in txt


def test_tools_without_session_fail_gracefully(home):
    from roxai.mcp_server import mcp
    out = asyncio.run(mcp.call_tool("note", {"text": "x"}))
    assert "No playtest running" in str(out)
    out = asyncio.run(mcp.call_tool("session_status", {}))
    assert "No playtest running" in str(out)


def test_single_instance_lock(home):
    import pytest
    from roxai.util import AlreadyRunning, single_instance, is_locked
    with single_instance("t"):
        assert is_locked("t")
        # same process re-entering counts as stale/own -> allowed (pid == own)
        with single_instance("t"):
            pass
    lock = home / "state" / "t.lock"
    lock.write_text("999999,0")  # dead pid → taken over
    with single_instance("t"):
        pass
    # live foreign pid → refused
    lock.write_text(f"{os.getppid()},{__import__('time').time()}")
    with pytest.raises(AlreadyRunning):
        with single_instance("t"):
            pass
    lock.unlink()


def test_recover_stale_session(home, fake_api):
    from roxai.scan import run_scan
    from roxai.session import Manager, P
    from roxai.db import get_db
    run_scan()
    db = get_db()
    pid = db.playtest_start(1)
    note = V.playtest_note_path(1, "Pet Sim Mega")
    V.create_playtest_note(note, None, "Pet Sim Mega", 1, 10)
    P.session.write_text(json.dumps({"universe_id": 1, "name": "x", "playtest_id": pid, "note": str(note),
                                        "pid": 999999, "ended": False}))
    Manager().recover_stale()
    assert db.one("SELECT status FROM playtests WHERE id=?", (pid,))["status"] == "interrupted"
    assert db.game(1)["playtest_fail_count"] == 1
    assert not P.session.exists()
    assert 'status: "interrupted"' in note.read_text(encoding="utf-8")


def test_queue_skips_repeated_failures(home, fake_api):
    from roxai.scan import run_scan, playtest_queue
    from roxai.db import get_db
    run_scan()
    get_db().upsert_game(1, playtest_fail_count=3)
    assert 1 not in [q["universe_id"] for q in playtest_queue()]
    get_db().upsert_game(2, last_playtest="2099-01-01")
    assert 2 not in [q["universe_id"] for q in playtest_queue()]


def test_doctor_runs(home, fake_api):
    from roxai.doctor import report_text, short_status
    assert "Vault" in report_text()
    assert short_status()


def test_cli_settings(home, capsys):
    from roxai.cli import main
    assert main(["set", "playtests_per_run", "5"]) == 0
    from roxai.config import settings
    assert settings().playtests_per_run == 5
    assert main(["set", "extra_sorts", "a,b"]) == 0
    assert settings().extra_sorts == ["a", "b"]
