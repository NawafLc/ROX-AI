//! Weekly autopilot: runs Claude Code headless (`claude -p`) with the ROX-AI MCP server to playtest the queue.
//!
//! Safety/robustness:
//!   * runs only when due (every `playtest_interval_days`), unless forced
//!   * one autopilot at a time (OS lock, released automatically if the process dies)
//!   * waits for the PC to be idle & unlocked — never hijacks your mouse while you use the PC
//!   * one Claude process per game (small context; one game's error never affects the next)
//!   * hard per-game timeout → whole process tree killed, Roblox closed, session marked interrupted
//!   * account problems (login/ban/verification) pause playtesting and notify you instead of looping
//!   * STOP file / panel button halts between and during games

use crate::api::RobloxApi;
use crate::config::Ctx;
use crate::session::{clear_stop, Manager};
use crate::util::{days_since, hidden_command, now_iso, single_instance};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub fn find_claude(ctx: &Ctx) -> Option<PathBuf> {
    if !ctx.settings.claude_cli.is_empty()
        && std::path::Path::new(&ctx.settings.claude_cli).exists()
    {
        return Some(PathBuf::from(&ctx.settings.claude_cli));
    }
    let exe = if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    };
    if let Some(paths) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&paths) {
            for n in [exe, "claude.cmd"] {
                let p = d.join(n);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    let home = crate::config::home_dir();
    let env = |k: &str| std::env::var(k).map(PathBuf::from).unwrap_or_default();
    [
        home.join(".local").join("bin").join(exe),
        env("APPDATA").join("npm").join("claude.cmd"),
        env("LOCALAPPDATA")
            .join("Programs")
            .join("claude")
            .join("claude.exe"),
        home.join(".claude").join("local").join(exe),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

pub fn claude_ready(ctx: &Ctx) -> (bool, String) {
    let Some(c) = find_claude(ctx) else {
        return (false, "Claude Code CLI not installed".into());
    };
    match crate::util::run_capture(
        &c.to_string_lossy(),
        &["--version"],
        Duration::from_secs(60),
    ) {
        Ok((0, out)) => (true, out.trim().to_string()),
        Ok((code, out)) => (
            false,
            format!(
                "claude --version failed ({code}): {}",
                out.chars().take(200).collect::<String>()
            ),
        ),
        Err(e) => (false, e.to_string()),
    }
}

pub fn mcp_entry(ctx: &Ctx) -> Value {
    json!({"command": crate::util::console_exe().to_string_lossy(), "args": ["mcp"], "env": {"ROXAI_HOME": ctx.home.to_string_lossy()}})
}

fn mcp_config(ctx: &Ctx) -> Result<PathBuf> {
    let p = ctx.state_dir().join("mcp-config.json");
    crate::util::atomic_write(
        &p,
        serde_json::to_string_pretty(&json!({"mcpServers": {"roxai": mcp_entry(ctx)}}))?.as_bytes(),
    )?;
    Ok(p)
}

fn workdir(ctx: &Ctx) -> Result<PathBuf> {
    let d = ctx.state_dir().join("claude-workdir");
    std::fs::create_dir_all(&d)?;
    std::fs::write(d.join("CLAUDE.md"), "# ROX-AI autopilot\nYou are running unattended for ROX-AI. Use only the roxai MCP tools (plus web search for public info like codes/wiki). Follow playbook() exactly. Never spend Robux, never chat. Always end each playtest with playtest_finish. Be thorough and specific in the vault sections.\n")?;
    Ok(d)
}

#[derive(Debug, Default, serde::Serialize)]
pub struct ClaudeRun {
    pub returncode: Option<i32>,
    pub stopped: Option<String>,
    pub is_error: Option<bool>,
    pub turns: Option<u64>,
    pub cost_usd: Option<f64>,
    pub result: String,
    pub log: String,
    pub auth_problem: bool,
    pub usage_limit: bool,
}

pub fn run_claude(ctx: &Ctx, prompt: &str, timeout: Duration, tag: &str) -> Result<ClaudeRun> {
    run_claude_opts(
        ctx,
        prompt,
        &ClaudeOpts { model: ctx.settings.claude_model.clone(), timeout, tag: tag.to_string(), ..ClaudeOpts::new(ctx) },
    )
}

/// How to run one headless Claude Code session.
#[derive(Clone, Debug)]
pub struct ClaudeOpts {
    /// "" = the account default; otherwise an alias ("opus", "sonnet", "haiku") or a full model id
    pub model: String,
    pub tools: String,
    /// extra environment for ROX-AI's MCP server (e.g. which game an agent works on)
    pub mcp_env: Vec<(String, String)>,
    pub timeout: Duration,
    pub tag: String,
    pub max_turns: u32,
    /// working folder (holds the CLAUDE.md the session reads); None = the learning autopilot's
    pub workdir: Option<PathBuf>,
}

impl ClaudeOpts {
    pub fn new(ctx: &Ctx) -> ClaudeOpts {
        ClaudeOpts {
            model: String::new(),
            tools: "mcp__roxai,WebSearch,WebFetch".into(),
            mcp_env: vec![],
            timeout: Duration::from_secs(30 * 60),
            tag: "run".into(),
            max_turns: ctx.settings.max_turns,
            workdir: None,
        }
    }
}

fn mcp_config_with(ctx: &Ctx, env: &[(String, String)], tag: &str) -> Result<PathBuf> {
    if env.is_empty() {
        return mcp_config(ctx);
    }
    let mut entry = mcp_entry(ctx);
    for (k, v) in env {
        entry["env"][k] = json!(v);
    }
    let p = ctx.state_dir().join(format!("mcp-{tag}.json"));
    crate::util::atomic_write(&p, serde_json::to_string_pretty(&json!({"mcpServers": {"roxai": entry}}))?.as_bytes())?;
    Ok(p)
}

pub fn run_claude_opts(ctx: &Ctx, prompt: &str, o: &ClaudeOpts) -> Result<ClaudeRun> {
    let Some(claude) = find_claude(ctx) else {
        bail!("Claude Code isn't installed — Settings → Run setup")
    };
    let (timeout, tag) = (o.timeout, o.tag.as_str());
    let log = ctx.logs_dir().join(format!(
        "claude-{}-{tag}.json",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ));
    let mcp = mcp_config_with(ctx, &o.mcp_env, tag)?;
    let mut cmd = hidden_command(&claude);
    // the prompt goes in on stdin (no command-line length limit)
    cmd.args([
        "-p",
        "--mcp-config",
        &mcp.to_string_lossy(),
        "--strict-mcp-config",
        "--allowedTools",
        &o.tools,
        "--output-format",
        "json",
        "--max-turns",
        &o.max_turns.to_string(),
    ]);
    if !o.model.is_empty() {
        cmd.args(["--model", &o.model]);
    }
    let f = std::fs::File::create(&log)?;
    let wd = match &o.workdir {
        Some(d) => d.clone(),
        None => workdir(ctx)?,
    };
    cmd.current_dir(wd)
        .env("ROXAI_HOME", &ctx.home)
        .stdin(std::process::Stdio::piped())
        .stdout(f.try_clone()?)
        .stderr(f);
    crate::info!("starting claude for {tag} (timeout {}s)", timeout.as_secs());
    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(prompt.as_bytes());
    }
    let start = Instant::now();
    let mut stopped = None;
    let code = loop {
        if let Some(st) = child.try_wait()? {
            break st.code();
        }
        if start.elapsed() > timeout {
            stopped = Some("timeout".to_string());
        } else if ctx.stop_file().exists() {
            stopped = Some("stop requested".to_string());
        }
        if stopped.is_some() {
            crate::win::proc::kill_tree(child.id());
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_secs(2));
    };
    let raw = std::fs::read_to_string(&log).unwrap_or_default();
    let mut r = ClaudeRun {
        returncode: code,
        stopped,
        log: log.to_string_lossy().into_owned(),
        ..Default::default()
    };
    if let Some(last) = raw.trim().lines().last() {
        if let Ok(v) = serde_json::from_str::<Value>(last) {
            r.is_error = v["is_error"].as_bool();
            r.turns = v["num_turns"].as_u64();
            r.cost_usd = v["total_cost_usd"].as_f64();
            r.result = v["result"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(2000)
                .collect();
        } else {
            r.result = raw
                .chars()
                .rev()
                .take(2000)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
        }
    }
    let low = raw.to_lowercase();
    r.auth_problem = [
        "invalid api key",
        "please run /login",
        "not logged in",
        "authentication_error",
        "oauth token",
        "failed to authenticate",
        "oauth session expired",
        "could not be refreshed",
    ]
    .iter()
    .any(|k| low.contains(k));
    r.usage_limit = ["usage limit", "rate_limit", "limit reached", "session limit", "weekly limit", "hit your limit", "\"api_error_status\":429"]
        .iter()
        .any(|k| low.contains(k));
    Ok(r)
}

/// Best-effort Windows toast so you know what happened without opening anything.
pub fn notify(ctx: &Ctx, title: &str, text: &str) {
    crate::info!("notify: {title} — {text}");
    let _ = ctx.store.set(
        "last_notice",
        json!({"title": title, "text": text, "at": now_iso()}),
    );
    #[cfg(windows)]
    {
        let t = title.replace('\'', "''");
        let b: String = text.replace('\'', "''").chars().take(250).collect();
        let ps = format!("[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null;$x = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02);$x.GetElementsByTagName('text')[0].AppendChild($x.CreateTextNode('{t}')) > $null;$x.GetElementsByTagName('text')[1].AppendChild($x.CreateTextNode('{b}')) > $null;$n = [Windows.UI.Notifications.ToastNotification]::new($x);[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('ROX-AI').Show($n)");
        let _ = hidden_command("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps])
            .spawn();
    }
}

fn wait_until_idle(ctx: &Ctx, max_wait: Duration) -> bool {
    use crate::win::window as W;
    let need = ctx.settings.idle_required_minutes * 60.0;
    let end = Instant::now() + max_wait;
    loop {
        if ctx.stop_file().exists() {
            return false;
        }
        if !W::is_locked() && W::idle_seconds() >= need {
            return true;
        }
        if Instant::now() > end {
            return false;
        }
        std::thread::sleep(Duration::from_secs(30));
    }
}

pub fn is_due(ctx: &Ctx) -> (bool, String) {
    let Some(last) = ctx.store.last_run("autopilot", Some("ok")) else {
        return (true, "never ran".into());
    };
    let age = days_since(last.finished.as_deref().unwrap_or(&last.started));
    let iv = if ctx.settings.playtesting_enabled {
        ctx.settings.playtest_interval_days
    } else {
        ctx.settings.learn_interval_days
    };
    match age {
        Some(a) if a < iv - 0.25 => (
            false,
            format!("last full run {a:.1} days ago (every {iv} days)"),
        ),
        Some(a) => (true, format!("last full run {a:.1} days ago")),
        None => (true, "unknown last run".into()),
    }
}

pub fn blocked(ctx: &Ctx) -> Option<String> {
    let b = ctx.store.get("blocked_reason")?;
    if days_since(b["at"].as_str().unwrap_or("")).unwrap_or(0.0) >= 3.0 {
        // retry automatically after 3 days in case it got fixed
        let _ = ctx.store.set("blocked_reason", Value::Null);
        return None;
    }
    Some(b["reason"].as_str().unwrap_or("unknown").to_string())
}

pub fn run(
    ctx: &Ctx,
    api: &dyn RobloxApi,
    force: bool,
    max_games: Option<usize>,
    only: Option<u64>,
    idle_wait: Duration,
) -> Value {
    let lock = match single_instance(&ctx.state_dir(), "autopilot") {
        Ok(l) => l,
        Err(e) => return json!({"status": "skipped", "reason": e.to_string()}),
    };
    let run_id = ctx.store.run_start("autopilot").unwrap_or(0);
    let res = if ctx.settings.playtesting_enabled {
        inner(ctx, api, force, max_games, only, idle_wait)
    } else {
        learn_inner(ctx, api, force, max_games, only)
    };
    let out = match res {
        Ok((status, v)) => {
            let _ = ctx.store.run_end(run_id, &status, &v.to_string());
            json!({"status": status, "result": v})
        }
        Err(e) => {
            let msg = format!("{e:#}");
            let skip = msg.starts_with("SKIP: ");
            let status = if skip { "skipped" } else { "error" };
            let _ = ctx.store.run_end(run_id, status, &msg);
            if !skip {
                crate::error!("autopilot error: {msg}");
                notify(ctx, "ROX-AI playtest run failed", &msg);
            }
            json!({"status": status, "reason": msg.trim_start_matches("SKIP: ")})
        }
    };
    drop(lock);
    out
}

/// Active mode: Claude learns each queued game from public data + YouTube videos (no screen, no Roblox).
fn learn_inner(
    ctx: &Ctx,
    api: &dyn RobloxApi,
    force: bool,
    max_games: Option<usize>,
    only: Option<u64>,
) -> Result<(String, Value)> {
    if !ctx.is_active() && only.is_none() {
        bail!("SKIP: passive mode — only chart data is collected");
    }
    if only.is_none() && crate::util::is_locked(&ctx.state_dir(), "studio") {
        bail!("SKIP: the team is building a game (building comes first)");
    }
    clear_stop(ctx);
    if !force && only.is_none() {
        let (due, why) = is_due(ctx);
        if !due {
            bail!("SKIP: not due: {why}");
        }
    }
    if let Some(b) = blocked(ctx) {
        if !force {
            bail!("SKIP: blocked: {b}");
        }
    }
    let (ok, info) = claude_ready(ctx);
    if !ok {
        notify(ctx, "ROX-AI: Claude Code missing", &info);
        bail!("SKIP: {info}");
    }
    if ctx
        .store
        .chart_on(&crate::util::today(), &ctx.settings.sort_id)
        .is_empty()
    {
        if let Err(e) = crate::scan::run_scan(ctx, api, None, None) {
            crate::warn!("pre-run scan failed (continuing with last data): {e:#}");
        }
    }
    let queue: Vec<(u64, String, String)> = match only {
        Some(uid) => {
            let g = ctx.store.game(uid);
            vec![(
                uid,
                g.as_ref()
                    .map(|g| g.name.clone())
                    .unwrap_or(uid.to_string()),
                g.and_then(|g| g.best_rank)
                    .map(|r| r.to_string())
                    .unwrap_or("?".into()),
            )]
        }
        None => crate::scan::learn_queue(ctx, max_games)
            .into_iter()
            .map(|q| (q.universe_id, q.title, q.rank.to_string()))
            .collect(),
    };
    let _ = ctx.store.set(
        "learning_now",
        json!({"queue": queue.iter().map(|q| q.1.clone()).collect::<Vec<_>>(), "at": now_iso()}),
    );
    let mut results = vec![];
    for (uid, title, rank) in &queue {
        if ctx.stop_file().exists() {
            results.push(json!({"game": title, "status": "stopped by user"}));
            break;
        }
        let _ = ctx
            .store
            .set("learning_now", json!({"game": title, "at": now_iso()}));
        let before = ctx.store.game(*uid).and_then(|g| g.last_analysis);
        let prompt = format!("Learn the Roblox game '{title}' (universe_id {uid}, chart rank {rank}) from public data and YouTube videos — do not play it. First call learn_playbook(universe_id={uid}) and follow it exactly: game_info → vault_read_game → youtube_search → youtube_video for the best videos → vault_write_game_analysis with ALL sections, scores, one_liner, tags, method \"videos\" and sources. Work autonomously; nobody will answer questions.");
        let r = run_claude(
            ctx,
            &prompt,
            Duration::from_secs(ctx.settings.per_game_timeout_minutes * 60),
            &uid.to_string(),
        )?;
        let after = ctx.store.game(*uid).and_then(|g| g.last_analysis);
        let status = if after.is_some() && after != before {
            "learned"
        } else {
            "not finished"
        };
        crate::info!("learn {title} → {status}");
        results.push(json!({"game": title, "status": status, "claude": {"returncode": r.returncode, "stopped": r.stopped, "turns": r.turns, "log": r.log}}));
        if r.auth_problem {
            ctx.store.set(
                "blocked_reason",
                json!({"reason": "Claude Code is not signed in", "at": now_iso()}),
            )?;
            notify(
                ctx,
                "ROX-AI needs you (once)",
                "Claude is signed out. Open ROX-AI and click Sign in.",
            );
            break;
        }
        if r.usage_limit {
            notify(
                ctx,
                "ROX-AI paused",
                "Claude usage limit reached — the rest will be learned at the next run.",
            );
            let _ = ctx.store.set("learning_now", Value::Null);
            return Ok((
                "partial".into(),
                json!({"games": results, "reason": "usage limit"}),
            ));
        }
    }
    let _ = ctx.store.set("learning_now", Value::Null);
    // weekly market report
    let report_due = ctx
        .store
        .get("last_report")
        .and_then(|v| v.as_str().map(String::from))
        .and_then(|d| days_since(&d))
        .is_none_or(|d| d >= 7.0);
    if only.is_none()
        && report_due
        && !ctx.stop_file().exists()
        && results.iter().any(|r| r["status"] == "learned")
    {
        let p = "Write this week's ROX-AI market report. Use vault_read_note('00 Dashboard.md'), vault_list('01 Charts') and the latest chart notes, game_info for the biggest movers, and vault_read_game for the games learned recently. Then call vault_write_insight(title='Weekly Market Report', ...) with: chart summary, what is rising and why, genre shifts, common mechanics & monetization patterns, the 5 best opportunities to build a game (with reasoning), and 10 content ideas — link games as [[02 Games/<name>]]. Also call vault_write_genre_patterns for the 3 biggest genres.";
        if run_claude(ctx, p, Duration::from_secs(20 * 60), "weekly-report")
            .is_ok_and(|r| r.is_error != Some(true))
        {
            let _ = ctx.store.set("last_report", json!(now_iso()));
        }
    }
    // the playbook: rewrite the most outdated chapter(s) from everything learned so far
    let mut playbook = vec![];
    if only.is_none() && !ctx.stop_file().exists() {
        playbook = crate::playbook::learn_due(ctx, ctx.settings.playbook_chapters_per_run);
    }
    let done = results.iter().filter(|r| r["status"] == "learned").count();
    if !queue.is_empty() {
        notify(
            ctx,
            "ROX-AI learned new games",
            &format!(
                "{done}/{} games analyzed from YouTube. Open ROX-AI → Games to read them.",
                queue.len()
            ),
        );
        let _ = crate::scan::run_scan(ctx, api, None, None); // refresh dashboard
    }
    Ok((
        if done > 0 || queue.is_empty() {
            "ok"
        } else {
            "partial"
        }
        .into(),
        json!({"games": results, "queue": queue.len(), "completed": done, "playbook": playbook}),
    ))
}

fn inner(
    ctx: &Ctx,
    api: &dyn RobloxApi,
    force: bool,
    max_games: Option<usize>,
    only: Option<u64>,
    idle_wait: Duration,
) -> Result<(String, Value)> {
    use crate::win::roblox as RB;
    if !cfg!(windows) {
        bail!("SKIP: Playtests need Windows (Roblox client).");
    }
    clear_stop(ctx);
    if !force && only.is_none() {
        let (due, why) = is_due(ctx);
        if !due {
            bail!("SKIP: not due: {why}");
        }
    }
    if let Some(b) = blocked(ctx) {
        if !force {
            notify(ctx, "ROX-AI needs you (once)", &format!("Playtests paused: {b}. Log in to Roblox with the alt account, then click 'I fixed the login' in ROX-AI."));
            bail!("SKIP: blocked: {b}");
        }
    }
    let (ok, info) = claude_ready(ctx);
    if !ok {
        notify(ctx, "ROX-AI: Claude Code missing", &info);
        bail!("SKIP: {info}");
    }
    if !RB::is_installed() {
        notify(
            ctx,
            "ROX-AI: Roblox missing",
            "Install Roblox and log in with your alt account.",
        );
        bail!("SKIP: Roblox not installed");
    }
    if !wait_until_idle(ctx, idle_wait) {
        bail!("SKIP: PC was busy/locked the whole time — will try again at the next check");
    }
    if let Err(e) = crate::scan::run_scan(ctx, api, None, None) {
        crate::warn!("pre-run scan failed (continuing with last data): {e:#}");
    }
    let queue: Vec<(u64, String, String)> = match only {
        Some(uid) => {
            let g = ctx.store.game(uid);
            vec![(
                uid,
                g.as_ref()
                    .map(|g| g.name.clone())
                    .unwrap_or(uid.to_string()),
                g.and_then(|g| g.best_rank)
                    .map(|r| r.to_string())
                    .unwrap_or("?".into()),
            )]
        }
        None => crate::scan::playtest_queue(ctx, max_games, None)
            .into_iter()
            .map(|q| (q.universe_id, q.title, q.rank.to_string()))
            .collect(),
    };
    let mut results = vec![];
    let mgr = Manager::new(ctx);
    for (i, (uid, title, rank)) in queue.iter().enumerate() {
        if ctx.stop_file().exists() {
            results.push(json!({"game": title, "status": "stopped by user"}));
            break;
        }
        if i > 0 && !wait_until_idle(ctx, Duration::from_secs(30 * 60)) {
            results.push(json!({"game": title, "status": "postponed (PC in use)"}));
            break;
        }
        let prompt = format!("Playtest the Roblox game '{title}' (universe_id {uid}, chart rank {rank}). First call playbook(universe_id={uid}) and follow it exactly: game_info → vault_read_game → playtest_start → play with act/look/note/snap → playtest_finish with ALL sections, scores, one_liner, tags. Work autonomously; nobody will answer questions.");
        let r = run_claude(
            ctx,
            &prompt,
            Duration::from_secs(ctx.settings.per_game_timeout_minutes * 60),
            &uid.to_string(),
        )?;
        RB::kill_all(); // never leave Roblox running
        mgr.recover_stale();
        let status = ctx
            .store
            .playtests_for(*uid, 1)
            .first()
            .map(|p| p.status.clone())
            .unwrap_or("no session".into());
        crate::info!("game {title} → {status}");
        results.push(json!({"game": title, "status": status, "claude": {"returncode": r.returncode, "stopped": r.stopped, "is_error": r.is_error, "turns": r.turns, "cost_usd": r.cost_usd, "log": r.log}}));
        if r.auth_problem {
            ctx.store.set(
                "blocked_reason",
                json!({"reason": "Claude Code is not logged in", "at": now_iso()}),
            )?;
            notify(
                ctx,
                "ROX-AI needs you (once)",
                "Claude Code is logged out. Open a terminal, run `claude`, and log in.",
            );
            break;
        }
        if r.usage_limit {
            notify(
                ctx,
                "ROX-AI paused",
                "Claude usage limit reached — remaining games will be played at the next check.",
            );
            return Ok((
                "partial".into(),
                json!({"games": results, "reason": "usage limit"}),
            ));
        }
        if let Some(b) = ctx.store.get("blocked_reason") {
            notify(
                ctx,
                "ROX-AI needs you (once)",
                &format!("Playtests paused: {}", b["reason"]),
            );
            return Ok(("blocked".into(), json!({"games": results})));
        }
    }
    if only.is_none() && !results.is_empty() && !ctx.stop_file().exists() {
        let p = "Write this week's ROX-AI market report. Use vault_read_note('00 Dashboard.md'), vault_list('01 Charts') and the latest chart notes, game_info for the biggest movers, and vault_read_game for games playtested this week. Then call vault_write_insight(title='Weekly Market Report', ...) with: chart summary, what is rising and why, genre shifts, common mechanics & monetization patterns, the 5 best opportunities to build a game (with reasoning), and 10 content ideas — link games as [[02 Games/<name>]]. Also call vault_write_genre_patterns for the 3 biggest genres.";
        let rr = run_claude(ctx, p, Duration::from_secs(20 * 60), "weekly-report")?;
        results.push(json!({"game": "Weekly Market Report", "status": if rr.is_error == Some(true) { "error" } else { "ok" }}));
    }
    let _ = crate::scan::run_scan(ctx, api, None, None); // refresh dashboard with playtest results
    let done = results
        .iter()
        .filter(|r| r["status"] == "completed")
        .count();
    notify(
        ctx,
        "ROX-AI weekly run finished",
        &format!(
            "{done}/{} games playtested. Open the vault to see the new notes.",
            queue.len()
        ),
    );
    Ok((
        if done > 0 || queue.is_empty() {
            "ok"
        } else {
            "partial"
        }
        .into(),
        json!({"games": results, "queue": queue.len(), "completed": done}),
    ))
}
