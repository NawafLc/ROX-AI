//! End-to-end tests with an offline fake Roblox API: scans, trends across days, vault preservation,
//! queue rules, crash recovery, and the MCP protocol.

use roxai::api::{Badge, ChartEntry, GameDetails, Item, RobloxApi};
use roxai::config::Ctx;
use roxai::mcp::Server;
use roxai::vault::Vault;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

struct Fake {
    chart: Mutex<Vec<u64>>,
    playing: Mutex<HashMap<u64, u64>>,
    visits: Mutex<HashMap<u64, u64>>,
    broken: Mutex<Option<u64>>,
}

fn meta(uid: u64) -> (&'static str, u64, &'static str, &'static str) {
    match uid {
        1 => ("[🎃 UPDATE] Pet Sim Mega", 11, "Simulation", "Tycoon"),
        2 => ("Obby Rush!", 22, "Obby", "Classic"),
        3 => ("Brainrot | Battles", 33, "Action", "PvP"),
        _ => ("Newcomer", 44, "Action", "PvP"),
    }
}

impl Fake {
    fn new() -> Arc<Fake> {
        Arc::new(Fake {
            chart: Mutex::new(vec![1, 2, 3]),
            playing: Mutex::new(HashMap::from([(1, 1000), (2, 500), (3, 300), (4, 900)])),
            visits: Mutex::new(HashMap::from([(1, 10_000), (2, 5000), (3, 3000), (4, 100)])),
            broken: Mutex::new(None),
        })
    }
}

impl RobloxApi for Fake {
    fn chart(&self, sort_id: &str, limit: usize, _c: &str) -> anyhow::Result<Vec<ChartEntry>> {
        let mut ids = self.chart.lock().unwrap().clone();
        if sort_id != "top-trending" {
            ids.reverse();
        }
        let p = self.playing.lock().unwrap();
        Ok(ids
            .iter()
            .take(limit)
            .enumerate()
            .map(|(i, u)| {
                let (n, place, g, _) = meta(*u);
                ChartEntry {
                    rank: i as u32 + 1,
                    universe_id: *u,
                    place_id: place,
                    name: n.into(),
                    playing: p[u],
                    up_votes: 90,
                    down_votes: 10,
                    genre: g.into(),
                    maturity: String::new(),
                }
            })
            .collect())
    }
    fn sorts(&self, _c: &str) -> anyhow::Result<Vec<Value>> {
        Ok(vec![json!({"sortId": "top-trending"})])
    }
    fn games_info(&self, ids: &[u64]) -> HashMap<u64, Value> {
        let v = self.visits.lock().unwrap();
        let p = self.playing.lock().unwrap();
        ids.iter()
            .map(|u| {
                (
                    *u,
                    json!({"visits": v[u], "favoritedCount": 1, "playing": p[u]}),
                )
            })
            .collect()
    }
    fn game_details(&self, uid: u64, _deep: bool) -> GameDetails {
        if *self.broken.lock().unwrap() == Some(uid) {
            panic_free_empty()
        } else {
            let (n, place, g, sub) = meta(uid);
            let it = |n: &str, p: Option<u64>, d: &str| Item {
                name: n.into(),
                price: p,
                for_sale: p.is_some(),
                description: d.into(),
                ..Default::default()
            };
            GameDetails {
                universe_id: uid,
                place_id: place,
                name: n.into(),
                description: "Collect pets!\nRebirth!".into(),
                creator_name: "Dev Group".into(),
                creator_type: "Group".into(),
                playing: self.playing.lock().unwrap()[&uid],
                visits: self.visits.lock().unwrap()[&uid],
                favorites: 100,
                max_players: 12,
                created: "2026-09-18T21:29:29.437Z".into(),
                updated: "2026-11-17T00:39:26.7266667Z".into(),
                genre: g.into(),
                subgenre: sub.into(),
                up_votes: 90,
                down_votes: 10,
                game_passes: vec![
                    it("VIP", Some(299), "VIP tag"),
                    it("x2 Coins", Some(99), "double coins"),
                    it("Auto Farm", None, ""),
                ],
                dev_products: vec![it("100 Gems", Some(25), "")],
                badges: vec![Badge {
                    name: "Welcome".into(),
                    description: "joined".into(),
                    awarded: 5000,
                    win_rate: 0.9,
                }],
                badge_total: 1,
                ..Default::default()
            }
        }
    }
    fn download(&self, _u: &str) -> Option<Vec<u8>> {
        None
    }
}

/// A "broken" game: Roblox returns nothing — the vault note must not be created from garbage.
fn panic_free_empty() -> GameDetails {
    GameDetails::default()
}

fn setup() -> (tempfile::TempDir, Ctx, Arc<Fake>) {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("settings.json"), json!({"vault_path": d.path().join("vault"), "chart_limit": 3, "extra_sorts": ["up-and-coming"]}).to_string()).unwrap();
    let ctx = Ctx::at(&home);
    (d, ctx, Fake::new())
}

#[test]
fn scan_builds_vault() {
    let (_d, ctx, api) = setup();
    let r = roxai::scan::run_scan(&ctx, api.as_ref(), None, Some("2026-11-17".into())).unwrap();
    assert_eq!((r.games, r.errors, r.new), (3, 0, 3));
    let v = Vault::new(ctx.vault());
    let note = v.root.join("02 Games/Pet Sim Mega.md");
    let t = std::fs::read_to_string(&note).unwrap();
    assert!(
        t.contains("universe_id: 1\n") && t.contains("chart_rank: 1\n"),
        "{t}"
    );
    assert!(t.contains("| VIP | 299 | vip/status |"));
    assert!(t.contains("off-sale"));
    assert!(
        std::fs::read_to_string(v.root.join("00 Dashboard.md"))
            .unwrap()
            .matches("[[02 Games/")
            .count()
            >= 3
    );
    assert!(v.root.join("01 Charts/2026-11-17 Top Trending.md").exists());
    assert!(v.root.join("04 Genres/Obby.md").exists());
    let q = roxai::scan::playtest_queue(&ctx, None, None);
    assert_eq!(
        q.iter().map(|x| x.universe_id).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
}

#[test]
fn second_day_trends_and_ai_preserved() {
    let (_d, ctx, api) = setup();
    roxai::scan::run_scan(&ctx, api.as_ref(), None, Some("2026-11-17".into())).unwrap();
    let v = Vault::new(ctx.vault());
    let note = v.root.join("02 Games/Pet Sim Mega.md");
    v.write_ai_sections(
        &note,
        json!({"core_loop": "LOOP-TEXT"}).as_object().unwrap(),
        json!({"fun": 7}).as_object(),
        None,
        None,
        "analyzed",
    )
    .unwrap();
    let t = std::fs::read_to_string(&note)
        .unwrap()
        .replace("- \n", "- my idea\n");
    std::fs::write(&note, t).unwrap();

    *api.chart.lock().unwrap() = vec![2, 1, 4];
    api.playing.lock().unwrap().insert(1, 2000);
    api.visits.lock().unwrap().insert(1, 20_000);
    let r = roxai::scan::run_scan(&ctx, api.as_ref(), None, Some("2026-11-18".into())).unwrap();
    assert_eq!((r.new, r.dropped), (1, 1));
    let t = std::fs::read_to_string(&note).unwrap();
    assert!(t.contains("LOOP-TEXT") && t.contains("score_fun: 7") && t.contains("my idea"));
    assert!(
        t.contains("chart_rank: 2\n") && t.contains("days_on_chart: 2\n"),
        "{t}"
    );
    assert!(t.contains("+100.0%"));
    let chart =
        std::fs::read_to_string(v.root.join("01 Charts/2026-11-18 Top Trending.md")).unwrap();
    assert!(chart.contains("Brainrot Battles (was #3)"), "{chart}");
    assert!(chart.contains("🆕"));
}

#[test]
fn scan_survives_one_broken_game() {
    let (_d, ctx, api) = setup();
    *api.broken.lock().unwrap() = Some(2);
    let r = roxai::scan::run_scan(&ctx, api.as_ref(), None, Some("2026-11-17".into())).unwrap();
    assert_eq!(r.games, 3);
    assert_eq!(ctx.store.last_run("scan", None).unwrap().status, "ok");
    // the broken game still gets a note built from chart data
    assert!(Vault::new(ctx.vault())
        .root
        .join("02 Games/Obby Rush.md")
        .exists());
}

#[test]
fn queue_rules() {
    let (_d, ctx, api) = setup();
    roxai::scan::run_scan(&ctx, api.as_ref(), None, Some("2026-11-17".into())).unwrap();
    ctx.store
        .upsert_game(1, |g| {
            g.playtest_fail_count = 3;
            g.last_fail = Some(roxai::util::now_iso());
        })
        .unwrap();
    ctx.store
        .upsert_game(2, |g| g.last_playtest = Some(roxai::util::now_iso()))
        .unwrap();
    let q: Vec<u64> = roxai::scan::playtest_queue(&ctx, None, None)
        .iter()
        .map(|x| x.universe_id)
        .collect();
    assert_eq!(q, vec![3]);
}

#[test]
fn recover_stale_session() {
    let (_d, ctx, api) = setup();
    roxai::scan::run_scan(&ctx, api.as_ref(), None, Some("2026-11-17".into())).unwrap();
    let pid = ctx.store.playtest_start(1).unwrap();
    let v = Vault::new(ctx.vault());
    let note = v.playtest_note_path("Pet Sim Mega");
    v.create_playtest_note(&note, None, "Pet Sim Mega", 1, 10)
        .unwrap();
    std::fs::write(ctx.session_file(), json!({"universe_id": 1, "name": "x", "playtest_id": pid, "note": note, "pid": 999_999, "ended": false}).to_string()).unwrap();
    roxai::session::Manager::new(&ctx).recover_stale();
    assert_eq!(ctx.store.playtest(pid).unwrap().status, "interrupted");
    assert_eq!(ctx.store.game(1).unwrap().playtest_fail_count, 1);
    assert!(!ctx.session_file().exists());
    assert!(std::fs::read_to_string(&note)
        .unwrap()
        .contains("status: \"interrupted\""));
}

fn rpc(s: &Server, method: &str, params: Value) -> Value {
    s.handle(&json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params}))
        .unwrap()
}

#[test]
fn mcp_protocol() {
    let (_d, ctx, api) = setup();
    roxai::scan::run_scan(&ctx, api.as_ref(), None, Some("2026-11-17".into())).unwrap();
    let s = Server::new(ctx.clone(), api.clone());
    let init = rpc(
        &s,
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "roxai");
    assert!(s
        .handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .is_none());
    let tools = rpc(&s, "tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for n in [
        "charts",
        "game_info",
        "playtest_start",
        "look",
        "act",
        "note",
        "snap",
        "playtest_finish",
        "playtest_abort",
        "emergency_stop",
        "vault_write_game_analysis",
        "vault_write_insight",
        "system_check",
    ] {
        assert!(names.contains(&n), "missing {n}");
    }
    for t in tools["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object");
    }
    // no session → graceful
    let r = rpc(
        &s,
        "tools/call",
        json!({"name": "note", "arguments": {"text": "x"}}),
    );
    assert!(r["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("No playtest running"));
    assert_eq!(r["result"]["isError"], true);
    let r = rpc(
        &s,
        "tools/call",
        json!({"name": "session_status", "arguments": {}}),
    );
    assert!(r["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("No playtest running"));
    // vault writing via MCP
    let r = rpc(
        &s,
        "tools/call",
        json!({"name": "vault_write_game_analysis", "arguments": {"universe_id": 1, "sections": {"summary": "MCP summary", "bogus": "x"}, "scores": {"fun": 8}, "one_liner": "Pets!", "tags": ["Pets"]}}),
    );
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        txt.contains("Saved 1 section") && txt.contains("bogus"),
        "{txt}"
    );
    let r = rpc(
        &s,
        "tools/call",
        json!({"name": "vault_search", "arguments": {"query": "MCP summary"}}),
    );
    assert!(r["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("02 Games/Pet Sim Mega.md"));
    let r = rpc(
        &s,
        "tools/call",
        json!({"name": "playtest_queue", "arguments": {"limit": 2}}),
    );
    assert!(r["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("never playtested"));
    let r = rpc(
        &s,
        "tools/call",
        json!({"name": "vault_read_note", "arguments": {"path": "../secret"}}),
    );
    assert_eq!(r["result"]["isError"], true);
    // prompts
    let p = rpc(
        &s,
        "prompts/get",
        json!({"name": "playtest_game", "arguments": {"universe_id": "1"}}),
    );
    let text = p["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(text.contains("Pet Sim Mega") && text.contains("Never spend Robux"));
    for k in roxai::vault::ai_keys() {
        assert!(text.contains(k), "playbook missing {k}");
    }
    assert_eq!(rpc(&s, "nope", json!({}))["error"]["code"], -32601);
    assert_eq!(rpc(&s, "ping", json!({}))["result"], json!({}));
}

#[test]
fn settings_set_from_str() {
    let (_d, mut ctx, _api) = setup();
    ctx.settings.set_from_str("playtests_per_run", "5").unwrap();
    ctx.settings.set_from_str("extra_sorts", "a, b").unwrap();
    ctx.settings.set_from_str("allow_chat", "yes").unwrap();
    ctx.settings
        .set_from_str("replay_after_days", "14.5")
        .unwrap();
    assert!(ctx.settings.set_from_str("nope", "1").is_err());
    ctx.settings.save(&ctx.home).unwrap();
    ctx.reload();
    assert_eq!(ctx.settings.playtests_per_run, 5);
    assert_eq!(ctx.settings.extra_sorts, vec!["a", "b"]);
    assert!(ctx.settings.allow_chat);
    assert_eq!(ctx.settings.replay_after_days, 14.5);
}
