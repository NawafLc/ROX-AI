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
        "game" => game_json(&c, u(&a, "uid").ok_or_else(|| anyhow!("uid required"))?)?,
        "icon" => icon(&c, u(&a, "uid").unwrap_or(0)),
        "vault_image" => vault_image(&c, &s(&a, "path")),
        "chart" => chart_json(&c),
        "learn_queue" => serde_json::to_value(crate::scan::learn_queue(&c, Some(10)))?,
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
                "stop" => {
                    crate::session::request_stop(&c, "Stop button");
                    bg(&["stop"]);
                }
                w => bail!("unknown action {w}"),
            }
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
    let activity = if learning {
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

