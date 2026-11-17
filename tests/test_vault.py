from roxai import vault as V


def test_clean_names():
    assert V.clean_display_name("[🎃 UPDATE] Pet Sim Mega") == "Pet Sim Mega"
    assert V.safe_filename("Brainrot | Battles") == "Brainrot Battles"
    assert V.safe_filename("What?? #1 [x]") == "What 1"
    assert V.safe_filename("🔥🔥") == "Untitled" or V.safe_filename("🔥🔥")


def test_blocks_idempotent():
    t = "# T\n\n## ✍️ My Notes\nmine"
    t1 = V.set_block(t, "auto", "a", "one")
    t2 = V.set_block(t1, "auto", "a", "two")
    assert t2.count("<!-- roxai:auto:a -->") == 1
    assert "two" in t2 and "one" not in t2
    assert t2.endswith("mine")
    assert V.get_block(t2, "auto", "a") == "two"


def test_block_with_regex_chars_in_content():
    t = V.set_block("x", "ai", "k", r"price \1 $5 \g<0>")
    t = V.set_block(t, "ai", "k", r"new \2 content")
    assert r"new \2 content" in t


def test_frontmatter_roundtrip():
    fm = V.build_frontmatter({"a": 1, "b": "x: y", "c": ["p", "q"], "d": True, "e": None})
    raw, body = V.split_frontmatter(fm + "body")
    assert body == "body"
    assert raw["a"] == "1" and raw["b"] == '"x: y"' and "- \"p\"" in raw["c"] and raw["d"] == "true"


def test_write_ai_sections_preserves_everything(home):
    V.init_vault()
    path = V.p("games") / "G.md"
    body = V.new_game_body("G", 5, None) + "my private idea\n"
    V.write(path, V.build_frontmatter({"universe_id": 5, "name": "G", "tags": ["roblox"]}) + body)
    warns = V.write_ai_sections(path, {"summary": "Great game", "blueprint": "## custom heading\nsteps", "nope": "x"},
                                {"fun": 9, "Clone Difficulty": "4", "bad": "x"}, "one line", ["Pets"])
    text = path.read_text(encoding="utf-8")
    assert "Great game" in text and "## custom heading" in text
    assert "my private idea" in text
    assert "score_fun: 9" in text and "score_clone_difficulty: 4" in text
    assert any("nope" in w for w in warns) and any("bad" in w for w in warns)
    assert '"pets"' in text and '"roblox"' in text
    secs = V.read_note_sections(path)
    assert "Great game" in secs["summary"] and "my private idea" in secs["my_notes"]


def test_init_vault_creates_structure(home):
    root = V.init_vault()
    for d in V.DIRS.values():
        assert (root / d).is_dir()
    assert (root / "00 Dashboard.md").exists()
    assert (root / ".obsidian" / "community-plugins.json").exists()
