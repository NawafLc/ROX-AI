//! The one API the app's interface talks to — the same commands are used by the desktop app (Tauri)
//! and by the local web server (`roxai ui-server`, also the future iPhone app).
//!
//! Every command takes and returns JSON. Slow work (installs, sign-ins, scans, learning) runs in the
//! background; the interface polls `status` to see progress.

use crate::config::Ctx;
use crate::deps::Action;
use anyhow::{anyhow, bail, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};

#[derive(Default)]
struct State {
    /// dependency job: (id, progress text, fraction)
    job: Option<(String, String, Option<f32>)>,
    queue: Vec<(String, Action)>,
    /// last finished job result: (message, ok)
    notice: Option<(String, bool)>,
    update: Value,
    started: bool,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn st() -> std::sync::MutexGuard<'static, State> {
    STATE.get_or_init(|| Mutex::new(State { update: json!({"state": "checking"}), ..Default::default() })).lock().unwrap_or_else(|e| e.into_inner())
}

fn ctx() -> Ctx {
    Ctx::load()
}

/// Background startup: silent setup, first scan of the day, update check. Safe to call more than once.
pub fn startup() {
    {
        let mut s = st();
        if s.started {
            return;
        }
        s.started = true;
    }
    std::thread::spawn(|| {
        let c = ctx();
        crate::logx::init(&c.logs_dir());
        let api = crate::api::HttpApi::new().ok();
        let _ = crate::setup::ensure(&c, api.as_ref().map(|a| a as &dyn crate::api::RobloxApi));
        let first = c.store.get("setup_complete").is_none();
        let scanned = !c.store.chart_on(&crate::util::today(), &c.settings.sort_id).is_empty();
        if !first && !scanned && !crate::util::is_locked(&c.state_dir(), "scan") {
            bg(&["scan", "--if-needed"]);
        }
        check_update();
    });
}

fn check_update() {
    let v = match crate::update::check() {
        Ok(s) if s.update_available => json!({"state": "available", "latest": s.latest.map(|l| l.version), "current": s.current}),
        Ok(s) => json!({"state": "current", "current": s.current}),
        Err(e) => json!({"state": "failed", "error": e.to_string()}),
    };
    let mut v = v;
    v["checked_at"] = json!(crate::util::now_iso());
    st().update = v;
}

/// Run ROX-AI in the background (no window) — scans, learning runs.
pub fn bg(args: &[&str]) {
    let mut c = crate::util::hidden_command(crate::util::console_exe());
    c.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000 | 0x0000_0008); // no window + detached
    }
    let _ = c.spawn();
}

fn s(a: &Value, k: &str) -> String {
    a.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
}
fn u(a: &Value, k: &str) -> Option<u64> {
    a.get(k).and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|x| x.parse().ok())))
}

/// Dispatch one command.
pub fn call(cmd: &str, a: Value) -> Result<Value> {
    let mut c = ctx();
    Ok(match cmd {
        "status" => status(&c),
        "deps" => deps_json(&c),
        "dep_act" => {
            let id = s(&a, "id");
            let action = if s(&a, "action") == "signin" { Action::SignIn } else { Action::Install };
            enqueue(vec![(id, action)]);
            json!({"ok": true})
        }
        "dep_act_all" => {
            let ids: Vec<String> = a.get("ids").and_then(|v| v.as_array()).map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            let list = crate::deps::status(&c);
            let items = list.iter().filter(|d| ids.iter().any(|i| i == d.id) && !d.ok).filter_map(|d| d.action.map(|(act, _)| (d.id.to_string(), act))).collect();
            enqueue(items);
            json!({"ok": true})
        }
        "games" => games_json(&c),
        "home" => home_json(&c),
        "my_games" => crate::team::my_games_json(&c),
        "my_game" => crate::team::game_json(&c, &s(&a, "id"))?,
        "team" => team_json(&c),
        "team_start" => {
            c.settings.team_enabled = true;
            c.settings.save(&c.home)?;
            crate::session::clear_stop(&c);
            let _ = c.store.set("team_paused_until", Value::Null);
            if !crate::util::is_locked(&c.state_dir(), "studio") {
                bg(&["team", "run"]);
            }
            json!({"ok": true})
        }
        "team_pause" => {
            // the current agent finishes its step, then the team stops
            c.settings.team_enabled = false;
            c.settings.save(&c.home)?;
            json!({"ok": true})
        }
        "chat" => json!({"messages": crate::team::chat(&c), "typing": crate::util::is_locked(&c.state_dir(), "lead-chat")}),
        "chat_send" => {
            let text = s(&a, "text");
            if text.trim().is_empty() {
                bail!("empty message");
            }
            crate::team::chat_push(&c, "owner", &text)?;
            bg(&["team", "chat"]);
            json!({"ok": true})
        }
        "game_play" => {
            let dir = crate::team::game_dir(&c, &s(&a, "id"))?;
            let place = crate::pipeline::play_upload(&c, &dir)?;
            // straight into the Roblox app (the test place is private: sign in to Roblox with the account that owns it)
            crate::util::open_path(&format!("roblox://experiences/start?placeId={place}"));
            json!({"ok": true, "place": place, "url": format!("https://www.roblox.com/games/{place}")})
        }
        "game_feedback" => {
            crate::team::add_feedback(&c, &s(&a, "id"), &s(&a, "text"))?;
            if c.settings.team_enabled && !crate::util::is_locked(&c.state_dir(), "studio") {
                bg(&["team", "run"]);
            }
            json!({"ok": true})
        }
        "game" => game_json(&c, u(&a, "uid").ok_or_else(|| anyhow!("uid required"))?)?,
        "icon" => icon(&c, u(&a, "uid").unwrap_or(0)),
        "vault_image" => vault_image(&c, &s(&a, "path")),
        "chart" => chart_json(&c),
        "learn_queue" => serde_json::to_value(crate::scan::learn_queue(&c, Some(10)))?,
        "playbook" => {
            let st = crate::playbook::load(&c);
            json!({"chapters": crate::playbook::status(&c), "due": crate::playbook::due(&c).iter().map(|x| x.id).collect::<Vec<_>>(),
                "rules": st.chapters.values().map(|x| x.rules.len()).sum::<usize>(), "sources": st.sources.len(),
                "writing": c.store.get("playbook_now").and_then(|n| n["chapter"].as_str().map(String::from))})
        }
        "vault_tree" => json!(crate::gamesview::vault_tree(&c).into_iter().map(|(f, files)| json!({"folder": f, "files": files})).collect::<Vec<_>>()),
        "note" => note_json(&c, &s(&a, "path"))?,
        "resolve" => json!({"path": crate::gamesview::resolve(&c, &s(&a, "target"))}),
        "settings_get" => serde_json::to_value(&c.settings)?,
        "settings_set" => {
            let before = (c.settings.auto_run, c.settings.mode.clone());
            let mut v = serde_json::to_value(&c.settings)?;
            if let (Some(obj), Some(patch)) = (v.as_object_mut(), a.get("patch").and_then(|p| p.as_object())) {
                for (k, val) in patch {
                    if obj.contains_key(k) {
                        obj.insert(k.clone(), val.clone());
                    }
                }
            }
            c.settings = serde_json::from_value(v)?;
            c.settings.save(&c.home)?;
            if before != (c.settings.auto_run, c.settings.mode.clone()) {
                let c2 = c.clone();
                std::thread::spawn(move || crate::setup::apply_auto_run(&c2));
            }
            serde_json::to_value(&c.settings)?
        }
        "start" => {
            match s(&a, "what").as_str() {
                "scan" => bg(&["scan"]),
                "learn" => bg(&["autopilot", "--force"]),
                "learn_game" => bg(&["learn", &u(&a, "uid").ok_or_else(|| anyhow!("uid required"))?.to_string()]),
                "playbook" => match a.get("chapter").and_then(|v| v.as_str()) {
                    Some(ch) => bg(&["playbook", "--chapter", ch]),
                    None => bg(&["playbook", "--force"]),
                },
                "stop" => {
                    crate::session::request_stop(&c, "Stop button");
                    bg(&["stop"]);
                }
                w => bail!("unknown action {w}"),
            }
            json!({"ok": true})
        }
        "claude_token_save" => {
            let t = a.get("token").and_then(|v| v.as_str()).unwrap_or("");
            crate::autopilot::save_claude_token(&c, t)?;
            json!({"ok": true})
        }
        "claude_token_window" => {
            // a visible console where the owner signs in once and copies the long-lived token
            let Some(claude) = crate::autopilot::find_claude(&c) else { bail!("Install Claude Code first (Settings → Run setup)") };
            std::process::Command::new("cmd")
                .args(["/c", "start", "Claude sign-in for ROX-AI", "cmd", "/k"])
                .arg(&claude)
                .arg("setup-token")
                .spawn()?;
            json!({"ok": true})
        }
        "claude_token_clear" => {
            crate::autopilot::clear_claude_token(&c)?;
            json!({"ok": true})
        }
        "unblock" => {
            c.store.set("blocked_reason", Value::Null)?;
            json!({"ok": true})
        }
        "update_check" => {
            st().update = json!({"state": "checking"});
            std::thread::spawn(check_update);
            json!({"ok": true})
        }
        "update_run" => {
            st().update = json!({"state": "installing"});
            std::thread::spawn(move || match crate::update::run(&c, false) {
                Ok(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    std::process::exit(0);
                }
                Err(e) => st().update = json!({"state": "failed", "error": e.to_string()}),
            });
            json!({"ok": true})
        }
        "location" => {
            let here = crate::config::app_folder();
            json!({"here": here, "temporary": crate::relocate::is_temporary(&here), "suggested": crate::relocate::suggested_folder()})
        }
        "move_app" => {
            let dest = std::path::PathBuf::from(s(&a, "dest"));
            let exe = crate::relocate::move_app(&dest)?;
            let _ = crate::relocate::create_shortcuts(&exe, a["desktop"].as_bool().unwrap_or(false), a["start_menu"].as_bool().unwrap_or(false));
            let old = crate::config::app_folder();
            std::process::Command::new(&exe).arg("--moved-from").arg(&old).current_dir(&dest).spawn()?;
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(400));
                std::process::exit(0);
            });
            json!({"ok": true, "moved_to": dest})
        }
        "shortcuts" => {
            let exe = crate::util::bg_exe();
            crate::relocate::create_shortcuts(&exe, a["desktop"].as_bool().unwrap_or(false), a["start_menu"].as_bool().unwrap_or(false))?;
            json!({"ok": true})
        }
        "finish_setup" => {
            c.store.set("setup_complete", json!(crate::util::now_iso()))?;
            let c2 = c.clone();
            std::thread::spawn(move || crate::setup::apply_auto_run(&c2));
            if !crate::util::is_locked(&c.state_dir(), "scan") {
                bg(&["scan", "--if-needed"]);
            }
            json!({"ok": true})
        }
        "restart_setup" => {
            c.store.set("setup_complete", Value::Null)?;
            json!({"ok": true})
        }
        "open" => {
            match s(&a, "what").as_str() {
                "obsidian" => {
                    let c2 = c.clone();
                    std::thread::spawn(move || crate::setup::open_vault(&c2));
                }
                "note" => crate::setup::open_note(&c, &s(&a, "path")),
                "folder" => crate::util::open_path(&crate::config::app_folder().to_string_lossy()),
                "logs" => crate::util::open_path(&c.logs_dir().to_string_lossy()),
                "url" => {
                    let url = s(&a, "url");
                    if url.starts_with("https://") {
                        crate::util::open_path(&url);
                    }
                }
                w => bail!("unknown target {w}"),
            }
            json!({"ok": true})
        }
        "roblox_status" => roblox_status(&c),
        "roblox_connect" => {
            let conn = crate::cloud::connect(&c, &s(&a, "key"), &s(&a, "place"))?;
            // prove the whole pipeline right away: build the starter game, upload it, test it on Roblox
            if !crate::util::is_locked(&c.state_dir(), "pipeline") {
                bg(&["pipeline", "selftest"]);
            }
            serde_json::to_value(conn)?
        }
        "roblox_disconnect" => {
            crate::cloud::disconnect(&c)?;
            json!({"ok": true})
        }
        "pipeline_test" => {
            if !crate::util::is_locked(&c.state_dir(), "pipeline") {
                c.store.set("pipeline_now", json!({"project": "_pipeline-check", "step": "Starting", "since": crate::util::now_iso()}))?;
                bg(&["pipeline", "selftest"]);
            }
            json!({"ok": true})
        }
        "remove_from_pc" => {
            crate::relocate::remove_shortcuts();
            json!({"message": crate::setup::remove_from_pc(&c)})
        }
        other => bail!("unknown command {other}"),
    })
}

fn enqueue(items: Vec<(String, Action)>) {
    let start = {
        let mut g = st();
        for it in items {
            if !g.queue.contains(&it) && g.job.as_ref().map(|j| &j.0) != Some(&it.0) {
                g.queue.push(it);
            }
        }
        g.job.is_none()
    };
    if !start {
        return;
    }
    std::thread::spawn(|| loop {
        let (id, act) = {
            let mut g = st();
            if g.queue.is_empty() {
                g.job = None;
                break;
            }
            let (id, act) = g.queue.remove(0);
            g.job = Some((id.clone(), "Starting…".into(), None));
            (id, act)
        };
        let id2 = id.clone();
        let report = move |text: &str, frac: Option<f32>| {
            st().job = Some((id2.clone(), text.to_string(), frac));
        };
        let r = crate::deps::act(&ctx(), &id, act, &report);
        st().notice = Some(match r {
            Ok(m) => (m, true),
            Err(e) => (e.to_string(), false),
        });
    });
}

/// The Team page: every agent, the game being built, and how the team is set up.
fn team_json(c: &Ctx) -> Value {
    let cur = crate::team::current(c).map(|(_, s)| {
        let (stage, text, p) = crate::team::stage_of(&s);
        let doing: Vec<Value> = s.tasks.iter().filter(|t| t.status == "doing").map(|t| json!({"id": t.id, "title": t.title, "owner": t.owner})).collect();
        let (d, t) = s.progress();
        json!({"slug": s.game.slug, "title": s.game.title, "pitch": s.game.pitch, "genre": s.game.genre, "art_style": s.game.art_style, "size": s.game.size,
            "stage": stage, "stage_text": text, "progress": (p * 100.0).round(), "done": d, "total": t, "doing": doing,
            "activity": s.activity.iter().rev().take(12).collect::<Vec<_>>(), "eta": crate::team::eta_json(c, &s)})
    });
    let ready = crate::team::all_games(c).iter().filter(|(_, s)| s.game.status == "ready").count();
    json!({
        "enabled": c.settings.team_enabled,
        "running": crate::util::is_locked(&c.state_dir(), "studio"),
        "paused_until": crate::team::paused_until(c),
        "claude_ready": crate::autopilot::find_claude(c).is_some(),
        "blocked": crate::autopilot::blocked(c),
        "roblox_connected": crate::cloud::connection(c).is_some(),
        "claude_token": crate::autopilot::has_claude_token(c),
        "agents": crate::team::agents_json(c),
        "current": cur,
        "ready": ready,
        "games_to_build": c.settings.games_to_build,
        "parallel": c.settings.team_parallel,
        "max_game_size": c.settings.max_game_size,
    })
}

/// Settings → Roblox connection: the test place, the build tools and the last pipeline run. Never the key.
fn roblox_status(c: &Ctx) -> Value {
    let running = crate::util::is_locked(&c.state_dir(), "pipeline");
    json!({
        "connection": crate::cloud::connection(c),
        "tools": crate::pipeline::tools_status(c),
        "tools_ready": crate::pipeline::tools_ready(c),
        "running": running,
        "now": if running { c.store.get("pipeline_now").unwrap_or(Value::Null) } else { Value::Null },
        "last": c.store.get("pipeline_last").unwrap_or(Value::Null),
    })
}

fn deps_json(c: &Ctx) -> Value {
    json!(crate::deps::status(c)
        .into_iter()
        .map(|d| json!({"id": d.id, "name": d.name, "why": d.why, "ok": d.ok, "required": d.required,
            "action": d.action.map(|(a, label)| json!({"kind": if a == Action::Install { "install" } else { "signin" }, "label": label}))}))
        .collect::<Vec<_>>())
}

fn status(c: &Ctx) -> Value {
    let stdir = c.state_dir();
    let games = c.store.games();
    let learned = games.values().filter(|g| g.analysis_source.is_some()).count();
    let chart_day = c.store.latest_day(&c.settings.sort_id);
    let on_chart = chart_day.as_ref().map(|d| c.store.chart_on(d, &c.settings.sort_id).len()).unwrap_or(0);
    let last_scan = c.store.last_run("scan", Some("ok")).map(|r| r.finished.unwrap_or(r.started));
    let learning = crate::util::is_locked(&stdir, "autopilot");
    let scanning = crate::util::is_locked(&stdir, "scan");
    let learning_game = c.store.get("learning_now").and_then(|n| n["game"].as_str().map(|g| crate::vault::clean_display_name(g)));
    let blocked = c.store.get("blocked_reason").and_then(|b| b["reason"].as_str().map(String::from));
    let writing = c.store.get("playbook_now").and_then(|n| n["chapter"].as_str().map(String::from));
    let activity = if learning && writing.is_some() {
        json!({"kind": "learning", "title": format!("Writing the playbook: {}", writing.unwrap_or_default()), "detail": "Claude is reading developer devlogs, Roblox's docs and your game notes."})
    } else if learning {
        match &learning_game {
            Some(g) => json!({"kind": "learning", "title": format!("Learning {g}"), "detail": "Claude is reading YouTube videos about it. This takes a few minutes per game."}),
            None => json!({"kind": "learning", "title": "Learning new games", "detail": "Picking the next games to learn."}),
        }
    } else if scanning {
        json!({"kind": "scanning", "title": "Updating the charts", "detail": "Reading today's top Roblox games. About a minute."})
    } else if let Some(b) = &blocked {
        json!({"kind": "attention", "title": "Needs you", "detail": b})
    } else {
        json!({"kind": "idle", "title": "All set", "detail": if !c.settings.auto_run { "Automatic mode is off." } else if c.is_active() { "Charts update daily and new games are learned automatically." } else { "Charts update daily." }})
    };
    let g = st();
    json!({
        "version": crate::VERSION,
        "mode": c.settings.mode,
        "auto_run": c.settings.auto_run,
        "setup_complete": c.store.get("setup_complete").is_some(),
        "activity": activity,
        "busy": learning || scanning,
        "stats": {"games": games.len(), "learned": learned, "on_chart": on_chart, "chart_day": chart_day, "last_scan": last_scan},
        "blocked": blocked,
        "job": g.job.as_ref().map(|(id, text, frac)| json!({"id": id, "text": text, "progress": frac})),
        "queued": g.queue.iter().map(|q| q.0.clone()).collect::<Vec<_>>(),
        "notice": g.notice.as_ref().map(|(m, ok)| json!({"message": m, "ok": ok})),
        "update": g.update,
        "platform": std::env::consts::OS,
    })
}

/// Home page: a very short summary of today and what each member of the team is doing.
fn home_json(c: &Ctx) -> Value {
    let today = crate::util::today();
    let stdir = c.state_dir();
    let games = c.store.games();
    let learned_today = games.values().filter(|g| g.last_analysis.as_deref().is_some_and(|d| d.starts_with(&today))).count();
    let new_today = games.values().filter(|g| g.first_seen.starts_with(&today)).count();
    let charts_today = !c.store.chart_on(&today, &c.settings.sort_id).is_empty();
    let scanning = crate::util::is_locked(&stdir, "scan");
    let learning = crate::util::is_locked(&stdir, "autopilot");
    let last_scan = c.store.last_run("scan", Some("ok")).map(|r| r.finished.unwrap_or(r.started));
    let learning_game = c.store.get("learning_now").and_then(|n| n["game"].as_str().map(crate::vault::clean_display_name));
    let next = crate::scan::learn_queue(c, Some(3));
    let my_games = c.store.get("my_games").and_then(|v| v.as_array().map(|a| a.len())).unwrap_or(0);

    let mut summary: Vec<String> = vec![];
    summary.push(if scanning { "Charts updating now".into() } else if charts_today { "Charts updated".into() } else { "Charts not updated yet".into() });
    if c.is_active() || learned_today > 0 {
        summary.push(match learned_today {
            0 => "no games learned yet".into(),
            1 => "1 game learned".into(),
            n => format!("{n} games learned"),
        });
    }
    if new_today > 0 {
        summary.push(format!("{new_today} new on the chart"));
    }

    let model_name = |m: &str| match m {
        "" => "Default model".to_string(),
        m if m.contains("opus") => "Opus 5.5".into(),
        m if m.contains("sonnet") => "Sonnet 5.5".into(),
        m if m.contains("haiku") => "Haiku 5.5".into(),
        m => m.to_string(),
    };
    let writing = c.store.get("playbook_now").and_then(|n| n["chapter"].as_str().map(String::from));
    let researcher = if learning && writing.is_some() {
        json!({"state": "working", "text": format!("Writing the playbook: {}", writing.unwrap_or_default())})
    } else if learning {
        json!({"state": "working", "text": match &learning_game { Some(g) => format!("Learning {g} from YouTube"), None => "Picking games to learn".into() }})
    } else if !c.is_active() {
        json!({"state": "paused", "text": "Paused — Passive mode"})
    } else if let Some(n) = next.first() {
        json!({"state": "idle", "text": format!("Next up: {}", n.title)})
    } else {
        json!({"state": "idle", "text": "Every chart game is learned"})
    };
    let scout = if scanning {
        json!({"state": "working", "text": "Reading today's Roblox charts"})
    } else {
        json!({"state": "idle", "text": if charts_today { "Today's charts are in" } else { "Updates the charts daily" }, "since": last_scan})
    };
    let mut s1 = scout;
    s1["id"] = json!("scout");
    s1["name"] = json!("Scout");
    s1["role"] = json!("Watches the Roblox charts");
    s1["model"] = json!("No AI needed");
    let mut r1 = researcher;
    r1["id"] = json!("researcher");
    r1["name"] = json!("Researcher");
    r1["role"] = json!("Learns how top games work");
    r1["model"] = json!(model_name(&c.settings.claude_model));
    // the game-building team (active members), then the market research pair
    let mut agents: Vec<Value> = crate::team::agents_json(c).into_iter().filter(|a| a["active"] == true).collect();
    agents.push(s1);
    agents.push(r1);
    json!({
        "summary": summary,
        "learned_today": learned_today, "new_today": new_today, "charts_today": charts_today,
        "my_games": my_games,
        "agents": agents,
        "working": agents.iter().filter(|a| a["state"] == "working").count(),
        "next": next.iter().map(|q| json!({"uid": q.universe_id, "title": q.title, "rank": q.rank, "reason": q.reason})).collect::<Vec<_>>(),
    })
}

fn games_json(c: &Ctx) -> Value {
    json!(crate::gamesview::rows(c)
        .into_iter()
        .map(|g| json!({"uid": g.uid, "rank": g.rank, "title": g.title, "genre": g.genre, "playing": g.playing, "like_ratio": g.like_ratio,
            "momentum": g.momentum, "source": g.source, "source_label": g.source_label(), "last_analysis": g.last_analysis, "fun": g.fun,
            "one_liner": g.one_liner, "note": g.note}))
        .collect::<Vec<_>>())
}

fn game_json(c: &Ctx, uid: u64) -> Result<Value> {
    let row = crate::gamesview::rows(c).into_iter().find(|r| r.uid == uid).ok_or_else(|| anyhow!("unknown game"))?;
    let g = c.store.game(uid).unwrap_or_default();
    let note = row.note.as_ref().map(|p| note_json(c, p)).transpose()?;
    let hist = c.store.history(uid, &c.settings.sort_id, 60);
    Ok(json!({
        "uid": uid, "title": row.title, "genre": row.genre, "subgenre": g.subgenre, "rank": row.rank, "best_rank": g.best_rank,
        "playing": row.playing, "like_ratio": row.like_ratio, "momentum": row.momentum, "fun": row.fun, "one_liner": row.one_liner,
        "source": row.source, "source_label": row.source_label(), "last_analysis": row.last_analysis, "videos": g.videos_used,
        "url": format!("https://www.roblox.com/games/{}", g.place_id), "creator": g.creator, "first_seen": g.first_seen,
        "history": hist.iter().map(|h| json!({"day": h.day, "rank": h.rank, "playing": h.playing})).collect::<Vec<_>>(),
        "note": note,
    }))
}

fn icon(c: &Ctx, uid: u64) -> Value {
    let p = c.vault().join(crate::vault::DIR_ATTACH).join(uid.to_string()).join("icon.png");
    match std::fs::read(p) {
        Ok(b) => json!({"data": format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b))}),
        Err(_) => json!({"data": null}),
    }
}

/// An image inside the vault (for pictures in notes), as a data URL.
fn vault_image(c: &Ctx, rel: &str) -> Value {
    if rel.contains("..") {
        return json!({"data": null});
    }
    let root = c.vault();
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let mut path = root.join(rel);
    if !path.is_file() {
        // Obsidian-style bare file name: look in the attachments folder
        path = walkdir_find(&root.join(crate::vault::DIR_ATTACH), name).unwrap_or(path);
    }
    let mime = match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/png",
    };
    match std::fs::read(&path) {
        Ok(b) => json!({"data": format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(b))}),
        Err(_) => json!({"data": null}),
    }
}

fn walkdir_find(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = walkdir_find(&p, name) {
                return Some(f);
            }
        } else if p.file_name().is_some_and(|n| n == name) {
            return Some(p);
        }
    }
    None
}

fn chart_json(c: &Ctx) -> Value {
    let sort = &c.settings.sort_id;
    let Some(day) = c.store.latest_day(sort) else { return json!({"day": null, "rows": []}) };
    let games = c.store.games();
    let rows: Vec<Value> = c
        .store
        .chart_on(&day, sort)
        .into_iter()
        .map(|s| {
            let hist = c.store.history(s.universe_id, sort, 30);
            let t = crate::analysis::trend(&hist);
            let g = games.get(&s.universe_id);
            json!({"uid": s.universe_id, "rank": s.rank, "title": g.map(|g| crate::vault::clean_display_name(&g.name)).unwrap_or_default(),
                "genre": g.map(|g| g.genre.clone()).unwrap_or_default(), "playing": s.playing,
                "like_ratio": crate::analysis::like_ratio(s.up_votes, s.down_votes), "momentum": t.momentum, "rank_change": t.rank_change_1d,
                "ccu_change": t.ccu_change_1d_pct, "flags": t.flags, "days": t.days_tracked, "learned": g.and_then(|g| g.analysis_source.clone())})
        })
        .collect();
    let mut genres: std::collections::BTreeMap<String, (usize, u64)> = Default::default();
    for r in &rows {
        let e = genres.entry(r["genre"].as_str().unwrap_or("Other").to_string()).or_default();
        e.0 += 1;
        e.1 += r["playing"].as_u64().unwrap_or(0);
    }
    let mut gv: Vec<Value> = genres.into_iter().map(|(g, (n, p))| json!({"genre": g, "games": n, "playing": p})).collect();
    gv.sort_by_key(|g| std::cmp::Reverse(g["playing"].as_u64().unwrap_or(0)));
    json!({"day": day, "chart": crate::scan::sort_name(sort), "rows": rows, "genres": gv})
}

/// A note for the reader: frontmatter as key/values, body with ROX-AI's internal markers removed.
fn note_json(c: &Ctx, rel: &str) -> Result<Value> {
    if rel.contains("..") {
        bail!("bad path");
    }
    let text = std::fs::read_to_string(c.vault().join(rel)).map_err(|_| anyhow!("note not found: {rel}"))?;
    let (fm, body) = crate::vault::split_frontmatter(&text);
    let rx = regex::Regex::new(r"(?m)^\s*<!--.*?-->\s*\n?").unwrap();
    let body = rx.replace_all(&body, "").to_string();
    let props: serde_json::Map<String, Value> = fm.into_iter().map(|(k, v)| (k, json!(v.replace("\n  - ", ", ").trim_start_matches(", ").trim_matches('"').to_string()))).collect();
    Ok(json!({"path": rel, "title": rel.rsplit('/').next().unwrap_or(rel).trim_end_matches(".md"), "props": props, "body": body}))
}

// ---------------------------------------------------------------- web server

/// Serve the interface + API over HTTP on localhost (development, previews, and later the iPhone app).
pub fn serve(port: u16, dist: std::path::PathBuf) -> Result<()> {
    let server = tiny_http::Server::http(("127.0.0.1", port)).map_err(|e| anyhow!("can't listen on {port}: {e}"))?;
    startup();
    crate::info!("UI server on http://127.0.0.1:{port}");
    println!("ROX-AI interface: http://127.0.0.1:{port}");
    for mut req in server.incoming_requests() {
        let url = req.url().split('?').next().unwrap_or("/").to_string();
        if let Some(cmd) = url.strip_prefix("/api/") {
            let mut body = String::new();
            let _ = req.as_reader().read_to_string(&mut body);
            let args: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let cmd = cmd.to_string();
            let (code, out) = match call(&cmd, args) {
                Ok(v) => (200, v),
                Err(e) => (400, json!({"error": e.to_string()})),
            };
            let resp = tiny_http::Response::from_string(out.to_string())
                .with_status_code(code)
                .with_header(tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap());
            let _ = req.respond(resp);
            continue;
        }
        let rel = if url == "/" { "index.html".to_string() } else { url.trim_start_matches('/').to_string() };
        let path = if rel.contains("..") { dist.join("index.html") } else { dist.join(&rel) };
        let path = if path.is_file() { path } else { dist.join("index.html") };
        match std::fs::read(&path) {
            Ok(b) => {
                let mime = match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
                    "html" => "text/html; charset=utf-8",
                    "js" => "text/javascript",
                    "css" => "text/css",
                    "svg" => "image/svg+xml",
                    "png" => "image/png",
                    "woff2" => "font/woff2",
                    "json" => "application/json",
                    _ => "application/octet-stream",
                };
                let _ = req.respond(tiny_http::Response::from_data(b).with_header(tiny_http::Header::from_bytes("Content-Type", mime).unwrap()));
            }
            Err(_) => {
                let _ = req.respond(tiny_http::Response::from_string("interface not built — run npm run build in ui/").with_status_code(404));
            }
        }
    }
    Ok(())
}

