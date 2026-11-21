//! The AI team: a Lead and specialist agents that build complete Roblox games, one game at a time.
//!
//! Everything is a task on the game's board (`<game>/studio.json`, shared through a locked store so
//! several agents can work at once):
//!
//!   Lead picks an idea ─► Designer writes design.md ─► Lead plans the build into tasks ─► specialists build
//!     ─► QA pass ─► code review ─► Lead's final verdict ─► (more work ↺) ─► ready in My Games
//!
//! Each agent run is one headless Claude Code session (its own model) that can only touch its own game's
//! folder through ROX-AI's tools: no shell, no other files, never the Roblox key.

use crate::autopilot::{run_claude_opts, ClaudeOpts};
use crate::config::Ctx;
use crate::store::Store;
use crate::util::now_iso;
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const TEAM_GUIDE: &str = include_str!("prompts/team.md");

// ------------------------------------------------------------------ the roster

pub struct AgentDef {
    pub id: &'static str,
    pub name: &'static str,
    pub role: &'static str,
    /// default model alias
    pub model: &'static str,
    /// Playbook chapters this agent reads
    pub chapters: &'static [&'static str],
    /// what the agent is responsible for (goes into its prompt)
    pub brief: &'static str,
    /// works in this version (others join in later updates)
    pub active: bool,
}

pub const AGENTS: [AgentDef; 15] = [
    AgentDef { id: "lead", name: "Lead", role: "Picks ideas and runs the team", model: "opus", chapters: &["ideas", "core-loops", "launch-liveops", "monetization"], active: true,
        brief: "You are the Lead (creative director + producer). You choose what to build, keep the game small enough to finish and polished enough to compete, split work into clear tasks with a 'done when', and judge whether the game is ready. You never write big chunks of code yourself; you delegate." },
    AgentDef { id: "designer", name: "Game Designer", role: "Turns ideas into a game plan", model: "opus", chapters: &["core-loops", "first-5-minutes", "progression-economy", "retention", "social"], active: true,
        brief: "You are the Game Designer. You own design.md: the hook, the core loop, the first 5 minutes, progression, retention, social features, the art direction, and a precise list of systems, UI screens and world areas with numbers. Concrete beats vague: name every item, value, timer and screen." },
    AgentDef { id: "economy", name: "Economy Designer", role: "Balances currencies, prices and rewards", model: "sonnet", chapters: &["progression-economy", "monetization"], active: true,
        brief: "You are the Economy Designer. You own all numbers: currencies, costs, rewards, upgrade curves, pacing, game passes and developer products (with Robux prices from the Playbook benchmarks). Put the numbers in src/shared/Config modules with unit tests that prove the pacing (time to first upgrade, time to max)." },
    AgentDef { id: "systems", name: "Systems Scripter", role: "Writes data, saving and server systems", model: "opus", chapters: &["technical", "core-loops"], active: true,
        brief: "You are the Systems Scripter. You own the server backbone: player data and saving (DataStores with session locking), remotes and validation, currencies, inventory, upgrades, purchases (MarketplaceService ProcessReceipt done right), anti-exploit. Rock-solid, server-authoritative, tested." },
    AgentDef { id: "gameplay", name: "Gameplay Scripter", role: "Writes the core loop and mechanics", model: "opus", chapters: &["core-loops", "technical", "first-5-minutes"], active: true,
        brief: "You are the Gameplay Scripter. You make the game feel good: the core loop mechanics, interactions, abilities, enemies/obstacles, rewards moments, juice (tweens, sounds, particles in code). Server decides outcomes, client makes them feel great." },
    AgentDef { id: "ui", name: "UI Builder", role: "Builds menus, HUD and shops", model: "sonnet", chapters: &["ui-ux", "first-5-minutes"], active: true,
        brief: "You are the UI Builder. You build every screen in code: HUD, currency display, shop, upgrades, inventory, settings, onboarding hints. Clean modern style, big tap targets, works on phone/tablet/PC (UIScale, UIAspectRatioConstraint, safe areas), smooth tweens, consistent colors and fonts." },
    AgentDef { id: "world", name: "World Builder", role: "Builds the map and spaces", model: "sonnet", chapters: &["first-5-minutes", "core-loops"], active: true,
        brief: "You are the World Builder. You build the map from Roblox parts and built-in materials (via default.project.json and/or a server build script that creates the world at startup): spawn, main areas, paths, landmarks, lighting and atmosphere. Readable layout, clear goals in sight, good performance (anchored parts, sensible part counts)." },
    AgentDef { id: "qa", name: "QA Tester", role: "Tests every build", model: "sonnet", chapters: &["technical", "first-5-minutes", "ui-ux"], active: true,
        brief: "You are the QA Tester. You write unit tests for the game's logic (tests/*.spec.luau) and remoteOnly specs that check the real server (scripts start without errors, remotes exist, data defaults are valid), run project_build and project_test, and file precise bug tasks (owner, steps, expected vs actual) for anything broken or missing from design.md." },
    AgentDef { id: "reviewer", name: "Code Reviewer", role: "Reviews code for bugs and exploits", model: "opus", chapters: &["technical"], active: true,
        brief: "You are the Code Reviewer. You read every script hunting for bugs, exploits (client trust, missing validation, remote spam), data loss risks, memory leaks, performance problems and messy code. Fix small things yourself (and keep the build green); file tasks for big ones." },
    AgentDef { id: "scout", name: "Market Scout", role: "Watches the Roblox charts", model: "sonnet", chapters: &["ideas"], active: false, brief: "" },
    AgentDef { id: "artist3d", name: "3D Artist", role: "Makes models", model: "sonnet", chapters: &[], active: false, brief: "" },
    AgentDef { id: "vfx", name: "VFX Artist", role: "Makes effects", model: "sonnet", chapters: &[], active: false, brief: "" },
    AgentDef { id: "thumbnail", name: "Thumbnail Artist", role: "Makes icons and thumbnails", model: "haiku", chapters: &["marketing"], active: false, brief: "" },
    AgentDef { id: "promo", name: "Promo Producer", role: "Makes promo videos", model: "haiku", chapters: &["marketing"], active: false, brief: "" },
    AgentDef { id: "liveops", name: "Live-ops Analyst", role: "Watches launched games", model: "sonnet", chapters: &["launch-liveops"], active: false, brief: "" },
];

pub fn agent(id: &str) -> Option<&'static AgentDef> {
    AGENTS.iter().find(|a| a.id == id)
}

/// How hard an agent thinks on a kind of work. Deep thinking where mistakes are expensive (the design, the plan,
/// fixing a broken build, reviews, the owner's feedback); normal for building from a clear spec.
pub fn effort_for(ctx: &Ctx, id: &str, kind: &str) -> String {
    if let Some(e) = ctx.settings.agent_effort.get(id).filter(|e| matches!(e.as_str(), "low" | "medium" | "high")) {
        return e.clone();
    }
    match kind {
        "design" | "plan" | "final" | "feedback" | "fix" | "ideate" | "review" => "high",
        _ => "medium",
    }
    .into()
}

pub fn model_for(ctx: &Ctx, id: &str) -> String {
    ctx.settings
        .agent_models
        .get(id)
        .filter(|m| !m.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| agent(id).map(|a| a.model.to_string()).unwrap_or_default())
}

pub fn model_label(m: &str) -> &'static str {
    let m = m.to_lowercase();
    if m.contains("opus") {
        "Opus"
    } else if m.contains("sonnet") {
        "Sonnet"
    } else if m.contains("haiku") {
        "Haiku"
    } else {
        "Default"
    }
}

// ------------------------------------------------------------------ the board

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GameInfo {
    pub slug: String,
    pub title: String,
    pub pitch: String,
    pub genre: String,
    pub inspired_by: String,
    pub twist: String,
    pub why_now: String,
    pub size: String,
    pub art_style: String,
    /// building · testing · review · ready · failed
    pub status: String,
    pub status_note: String,
    pub created: String,
    pub updated: String,
    pub ready_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub owner: String,
    /// design · plan · work · fix · qa · review · final
    pub kind: String,
    pub done_when: String,
    /// todo · doing · done · blocked
    pub status: String,
    pub deps: Vec<String>,
    pub notes: Vec<String>,
    pub attempts: u32,
    pub created_by: String,
    pub created: String,
    pub updated: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Entry {
    pub at: String,
    pub by: String,
    pub text: String,
    pub why: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Studio {
    pub game: GameInfo,
    pub tasks: Vec<Task>,
    pub decisions: Vec<Entry>,
    pub activity: Vec<Entry>,
    /// owner feedback (text, handled?)
    pub feedback: Vec<Entry>,
    pub agent_runs: u32,
    pub fix_rounds: u32,
    pub review_rounds: u32,
    pub last_build_ok: Option<bool>,
    /// the Lead's last verdict: "ready" (studio finalizes it) · "more_work" · ""
    pub verdict: String,
}

impl Studio {
    pub fn open(&self) -> impl Iterator<Item = &Task> {
        self.tasks.iter().filter(|t| t.status == "todo" || t.status == "doing")
    }
    fn next_id(&self) -> String {
        let n = self.tasks.iter().filter_map(|t| t.id.trim_start_matches('T').parse::<u32>().ok()).max().unwrap_or(0);
        format!("T{}", n + 1)
    }
    pub fn add_task(&mut self, title: &str, owner: &str, kind: &str, done_when: &str, deps: Vec<String>, by: &str) -> String {
        let id = self.next_id();
        self.tasks.push(Task {
            id: id.clone(),
            title: title.trim().to_string(),
            owner: owner.to_string(),
            kind: kind.to_string(),
            done_when: done_when.trim().to_string(),
            status: "todo".into(),
            deps,
            created_by: by.to_string(),
            created: now_iso(),
            updated: now_iso(),
            ..Default::default()
        });
        id
    }
    pub fn log(&mut self, by: &str, text: impl Into<String>) {
        self.activity.push(Entry { at: now_iso(), by: by.into(), text: text.into(), why: String::new() });
        let n = self.activity.len();
        if n > 300 {
            self.activity.drain(..n - 300);
        }
    }
    /// Tasks that can start now: todo, every dependency done, not tried too often.
    pub fn runnable(&self) -> Vec<&Task> {
        self.tasks
            .iter()
            .filter(|t| t.status == "todo" && t.attempts < 3)
            .filter(|t| t.deps.iter().all(|d| self.tasks.iter().any(|x| &x.id == d && x.status == "done")))
            .collect()
    }
    pub fn progress(&self) -> (usize, usize) {
        (self.tasks.iter().filter(|t| t.status == "done").count(), self.tasks.len())
    }
}

pub fn games_dir(ctx: &Ctx) -> PathBuf {
    crate::pipeline::games_dir(ctx)
}

fn board_store(dir: &Path) -> Store {
    Store::new(dir.join(".roxai"))
}

pub fn load(dir: &Path) -> Studio {
    board_store(dir).read("studio.json")
}

pub fn update<R>(dir: &Path, f: impl FnOnce(&mut Studio) -> R) -> Result<R> {
    board_store(dir).update("studio.json", |s: &mut Studio| {
        let r = f(s);
        s.game.updated = now_iso();
        r
    })
}

/// Every game the team has started, newest first.
pub fn all_games(ctx: &Ctx) -> Vec<(PathBuf, Studio)> {
    let mut v: Vec<(PathBuf, Studio)> = std::fs::read_dir(games_dir(ctx))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(".roxai").join("studio.json").is_file())
        .map(|p| {
            let s = load(&p);
            (p, s)
        })
        .collect();
    v.sort_by(|a, b| b.1.game.created.cmp(&a.1.game.created));
    v
}

/// The game being built right now (only one at a time).
pub fn current(ctx: &Ctx) -> Option<(PathBuf, Studio)> {
    all_games(ctx).into_iter().find(|(_, s)| !matches!(s.game.status.as_str(), "ready" | "failed" | "published"))
}

pub fn game_dir(ctx: &Ctx, slug: &str) -> Result<PathBuf> {
    let d = games_dir(ctx).join(crate::pipeline::slug(slug));
    if !d.join(".roxai").join("studio.json").is_file() {
        bail!("no game called {slug}");
    }
    Ok(d)
}

/// Start a new game from the Lead's pitch: project files from the template + the first two tasks.
pub fn create_game(ctx: &Ctx, info: GameInfo) -> Result<PathBuf> {
    if let Some((_, s)) = current(ctx) {
        bail!("the team is already building \"{}\" — games are built one at a time", s.game.title);
    }
    let size = info.size.to_lowercase();
    let cap = ctx.settings.max_game_size.to_lowercase();
    if size == "large" || (cap == "small" && size != "small") {
        bail!("size \"{}\" is over the cap: games must be {} for now", info.size, if cap == "small" { "small" } else { "small or medium" });
    }
    let mut slug = crate::pipeline::slug(&info.title);
    let base = slug.clone();
    let mut n = 2;
    while games_dir(ctx).join(&slug).exists() {
        slug = format!("{base}-{n}");
        n += 1;
    }
    let dir = games_dir(ctx).join(&slug);
    crate::pipeline::write_template(&dir, &info.title)?;
    update(&dir, |s| {
        s.game = GameInfo { slug: slug.clone(), status: "building".into(), created: now_iso(), size: if size.is_empty() { "small".into() } else { size }, ..info };
        let t1 = s.add_task(
            "Write the design doc (design.md)",
            "designer",
            "design",
            "design.md covers hook, core loop, first 5 minutes, progression with numbers, every system, UI screen and world area, monetization, art direction",
            vec![],
            "lead",
        );
        s.add_task("Plan the build", "lead", "plan", "every part of design.md is covered by a task with an owner and a 'done when'", vec![t1], "lead");
        s.log("lead", format!("Started a new game: {}", s.game.title));
    })?;
    crate::info!("team: new game {slug}");
    Ok(dir)
}

// ------------------------------------------------------------------ live status

/// What each agent is doing right now (shown on the Team page).
pub fn set_now(ctx: &Ctx, agent: &str, text: Option<(&str, &str)>) {
    let mut m = ctx.store.get("team_now").and_then(|v| v.as_object().cloned()).unwrap_or_default();
    match text {
        Some((game, t)) => {
            m.insert(agent.into(), json!({"text": t, "game": game, "since": now_iso()}));
        }
        None => {
            m.remove(agent);
        }
    }
    let _ = ctx.store.set("team_now", Value::Object(m));
}

pub fn now_map(ctx: &Ctx) -> serde_json::Map<String, Value> {
    if !crate::util::is_locked(&ctx.state_dir(), "studio") && !crate::util::is_locked(&ctx.state_dir(), "lead-chat") {
        return Default::default();
    }
    ctx.store.get("team_now").and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

// ------------------------------------------------------------------ running agents

fn team_workdir(ctx: &Ctx) -> Result<PathBuf> {
    let d = ctx.state_dir().join("team-workdir");
    std::fs::create_dir_all(&d)?;
    std::fs::write(d.join("CLAUDE.md"), "# ROX-AI studio\nYou are an agent in ROX-AI's game studio, running unattended. Use only the roxai tools. Never ask questions: decide, and log important decisions.\n")?;
    Ok(d)
}

fn prompt_for(ctx: &Ctx, a: &AgentDef, s: Option<&Studio>, mission: &str) -> String {
    let mut p = format!("{TEAM_GUIDE}\n\n# You\n{}\n\nPlaybook chapters for your job: {}. Read them before big decisions.\n", a.brief, a.chapters.join(", "));
    if !ctx.settings.avoid.trim().is_empty() {
        p.push_str(&format!("\nThe owner never wants games about: {}.\n", ctx.settings.avoid.trim()));
    }
    if let Some(s) = s {
        let g = &s.game;
        p.push_str(&format!(
            "\n# The game\n**{}** ({} · {} size · art style: {})\n{}\nInspired by: {} · Twist: {}\n",
            g.title, g.genre, g.size, g.art_style, g.pitch, g.inspired_by, g.twist
        ));
        let fb: Vec<&Entry> = s.feedback.iter().filter(|f| f.why != "handled").collect();
        if !fb.is_empty() {
            p.push_str("\nOwner feedback (top priority):\n");
            for f in fb {
                p.push_str(&format!("- {}\n", f.text));
            }
        }
    }
    p.push_str(&format!("\n# Your job now\n{mission}\n"));
    p
}

/// Run one agent session. Returns a one-line outcome for the log.
pub fn run_agent(ctx: &Ctx, dir: Option<&Path>, agent_id: &str, kind: &str, mission: &str, status_text: &str) -> Result<String> {
    let a = agent(agent_id).ok_or_else(|| anyhow!("unknown agent {agent_id}"))?;
    let s = dir.map(load);
    let slug = s.as_ref().map(|s| s.game.slug.clone()).unwrap_or_default();
    let prompt = prompt_for(ctx, a, s.as_ref(), mission);
    let tag = format!("team-{agent_id}-{}", crate::util::rand_hex(3));
    let opts = ClaudeOpts {
        model: model_for(ctx, agent_id),
        mcp_env: vec![("ROXAI_GAME".into(), slug.clone()), ("ROXAI_AGENT".into(), agent_id.into())],
        timeout: Duration::from_secs(45 * 60),
        tag: tag.clone(),
        max_turns: 150,
        workdir: Some(team_workdir(ctx)?),
        // only the Lead researches the market on the web; builders work purely through the project tools
        tools: if agent_id == "lead" { "mcp__roxai,WebSearch,WebFetch".into() } else { "mcp__roxai".into() },
        builtin: if agent_id == "lead" { "WebSearch,WebFetch".into() } else { String::new() },
        effort: effort_for(ctx, agent_id, kind),
    };
    set_now(ctx, agent_id, Some((&slug, status_text)));
    if let Some(d) = dir {
        let _ = update(d, |s| {
            s.agent_runs += 1;
            s.log(agent_id, status_text);
        });
    }
    let r = run_claude_opts(ctx, &prompt, &opts);
    set_now(ctx, agent_id, None);
    let _ = std::fs::remove_file(ctx.state_dir().join(format!("mcp-{tag}.json")));
    let r = r?;
    if r.auth_problem {
        let _ = ctx.store.set("blocked_reason", json!({"reason": "Claude sign-in needed (Settings → Run setup)", "at": now_iso()}));
        bail!("Claude needs you to sign in again");
    }
    if r.usage_limit {
        let until = (chrono::Utc::now() + chrono::Duration::minutes(60)).to_rfc3339();
        let _ = ctx.store.set("team_paused_until", json!(until));
        bail!("USAGE: Claude usage limit reached — the team takes a break and continues by itself");
    }
    if let Some(st) = &r.stopped {
        bail!("{} stopped: {st}", a.name);
    }
    Ok(r.result.lines().last().unwrap_or("").chars().take(300).collect())
}

fn mission(t: &Task, s: &Studio) -> String {
    let base = format!(
        "Your task is **{} — {}**.\nDone when: {}\n{}",
        t.id,
        t.title,
        if t.done_when.is_empty() { "the work is complete, tested and the build passes" } else { &t.done_when },
        if t.notes.is_empty() { String::new() } else { format!("Notes so far:\n- {}\n", t.notes.join("\n- ")) }
    );
    let how = match t.kind.as_str() {
        "design" => "Start with `board` and the Playbook chapters for your job, then write design.md with `project_write`. Make it specific enough that scripters can build it without asking anything: exact items, numbers, timers, screens, areas, and the first-5-minutes flow step by step. Keep it buildable at the agreed size. Then `task_update` done.",
        "plan" => "Read design.md, then split ALL of it into tasks with `task_add` (owner, a precise 'done when', and deps where order matters: shared config/data first, then systems, then gameplay/UI/world). Usually 8-20 tasks for a small game. Include an economy task for the numbers. Do not create QA, review or final tasks: the studio adds those. Log the key decisions, then `task_update` done.",
        "qa" => "Test the whole game against design.md: add unit tests for every piece of shared logic, add `remoteOnly` specs that check the server starts cleanly (no errors in output), remotes and data defaults exist. Run `project_build` and `project_test`. File a precise bug task (`task_add`, owner = whoever owns that area) for each problem. Then `task_update` done with a summary.",
        "review" => "Review every script (`project_files`, `project_outline`, then `project_read` the parts that matter: server code, remotes and data first). Fix small issues yourself with `project_edit` and keep `project_build` green. File tasks for bigger problems. Then `task_update` done with what you found.",
        "final" => "Decide if the game is ready for the owner. Check design.md against what was built (`project_files`, `project_read`), the last build/test results (`project_test` once), QA and review notes. If it meets the quality bar, call `game_verdict(\"ready\", summary)` where the summary is 2-4 sentences for the owner (what the game is, how to play). Otherwise add precise tasks for what's missing and call `game_verdict(\"more_work\", why)`. Be demanding but finish: this is round {round} of reviews; after round 3 accept the game unless something is broken.",
        "feedback" => "The owner gave feedback (see 'Owner feedback' above). Turn every point into precise tasks with `task_add` (right owner, clear 'done when'), update design.md if the design changes, log a `decision` for anything you choose not to do and why, then `task_update` done.",
        "fix" => "The build is failing. Run `project_build`, read the errors, fix the cause (not the test), and repeat until it passes. Then `task_update` done.",
        _ => "Read `board`, then only the parts of design.md your task needs (`project_outline design.md` gives the section line numbers; `project_read` with offset/limit). Do the work with the project tools, run `project_build` until it passes, then `task_update` done with what you built.",
    };
    format!("{base}\n{}", how.replace("{round}", &(s.review_rounds + 1).to_string()))
}

fn run_task(ctx: &Ctx, dir: &Path, task_id: &str) -> Result<()> {
    let s = load(dir);
    let t = s.tasks.iter().find(|t| t.id == task_id).cloned().ok_or_else(|| anyhow!("task {task_id} vanished"))?;
    update(dir, |s| {
        if let Some(x) = s.tasks.iter_mut().find(|x| x.id == t.id) {
            x.status = "doing".into();
            x.attempts += 1;
            x.updated = now_iso();
        }
    })?;
    let status = format!("{} · {}", t.id, t.title);
    let res = run_agent(ctx, Some(dir), &t.owner, &t.kind, &mission(&t, &s), &status);
    // a usage limit or sign-in problem is not the task's fault: it doesn't use up an attempt
    let not_its_fault = matches!(&res, Err(e) if { let m = format!("{e:#}"); m.contains("USAGE:") || m.contains("sign in") });
    update(dir, |s| {
        if not_its_fault {
            if let Some(x) = s.tasks.iter_mut().find(|x| x.id == t.id) {
                x.attempts = x.attempts.saturating_sub(1);
            }
        }
        let attempts_left = s.tasks.iter().find(|x| x.id == t.id).map(|x| x.attempts < 3).unwrap_or(false);
        if let Some(x) = s.tasks.iter_mut().find(|x| x.id == t.id) {
            if x.status == "doing" {
                // the agent ended without closing its task
                x.status = if attempts_left { "todo".into() } else { "blocked".into() };
                let why = match &res {
                    Ok(r) => format!("run ended without finishing{}", if r.is_empty() { String::new() } else { format!(": {r}") }),
                    Err(e) => format!("run failed: {e:#}"),
                };
                x.notes.push(why);
                x.updated = now_iso();
            }
        }
    })?;
    res.map(|_| ())
}

/// Local build after a batch of work: a failing build becomes a fix task.
fn check_build(ctx: &Ctx, dir: &Path) -> Result<bool> {
    let rep = run_pipeline(ctx, dir, false)?;
    let ok = rep.ok;
    update(dir, |s| {
        s.last_build_ok = Some(ok);
        if ok {
            s.fix_rounds = 0;
            return;
        }
        let failing = rep.steps.iter().find(|x| !x.ok).map(|x| format!("{}: {}", x.name, x.detail)).unwrap_or_default();
        if s.open().any(|t| t.kind == "fix") {
            return;
        }
        s.fix_rounds += 1;
        let short: String = failing.lines().take(3).collect::<Vec<_>>().join(" · ").chars().take(160).collect();
        let id = s.add_task(&format!("Fix the build: {short}"), "systems", "fix", "project_build passes", vec![], "studio");
        if let Some(t) = s.tasks.iter_mut().find(|t| t.id == id) {
            t.notes.push(failing);
        }
        s.log("studio", "The build broke — a fix task was added");
    })?;
    Ok(ok)
}

/// The build pipeline, waiting politely if another build is running.
pub fn run_pipeline(ctx: &Ctx, dir: &Path, remote: bool) -> Result<crate::pipeline::Report> {
    let noop = |_: &str, _: Option<f32>| {};
    for _ in 0..200 {
        match crate::pipeline::run(ctx, dir, remote, &noop) {
            Err(e) if e.to_string().contains("already running") => std::thread::sleep(Duration::from_secs(3)),
            r => return r,
        }
    }
    bail!("another build kept running for 10 minutes")
}

/// Pick tasks to run now: one, or several with different owners when parallel is on.
fn pick(ctx: &Ctx, s: &Studio) -> Vec<String> {
    let max = if ctx.settings.team_parallel { ctx.settings.team_max_parallel.clamp(1, 6) } else { 1 };
    let mut out: Vec<String> = vec![];
    let mut owners: Vec<&str> = vec![];
    let run = s.runnable();
    // fix tasks first, then the board order
    for t in run.iter().filter(|t| t.kind == "fix").chain(run.iter().filter(|t| t.kind != "fix")) {
        if out.len() >= max || out.contains(&t.id) {
            continue;
        }
        // design/plan/final/fix tasks run alone; everything else in parallel by owner
        let solo = matches!(t.kind.as_str(), "design" | "plan" | "final" | "fix" | "feedback");
        if solo && !out.is_empty() {
            continue;
        }
        if owners.contains(&t.owner.as_str()) {
            continue;
        }
        out.push(t.id.clone());
        owners.push(&t.owner);
        if solo {
            break;
        }
    }
    out
}

fn stop_requested(ctx: &Ctx) -> bool {
    ctx.stop_file().exists() || !crate::config::Settings::load(&ctx.home).team_enabled
}

/// Is the team taking a break (usage limit)?
pub fn paused_until(ctx: &Ctx) -> Option<String> {
    let u = ctx.store.get("team_paused_until")?.as_str()?.to_string();
    (crate::util::parse_dt(&u)? > chrono::Utc::now()).then_some(u)
}

/// The studio loop: keeps building until every requested game is done, the owner pauses the team, or
/// Claude needs a break. Safe to call any time (one studio at a time).
pub fn run(ctx: &Ctx) -> Result<Value> {
    let _lock = crate::util::single_instance(&ctx.state_dir(), "studio")?;
    let _awake = crate::util::KeepAwake::new(); // building runs unattended, often overnight
    crate::session::clear_stop(ctx);
    let mut log = vec![];
    loop {
        let ctx = &Ctx::load(); // pick up settings changes (models, parallel, pause)
        if stop_requested(ctx) {
            log.push("paused".to_string());
            break;
        }
        if let Some(u) = paused_until(ctx) {
            // wait out Claude's usage break here (the PC stays awake) instead of relying on a scheduled wake-up
            log.push(format!("took a break until {u}"));
            while paused_until(ctx).is_some() && !stop_requested(ctx) {
                std::thread::sleep(std::time::Duration::from_secs(30));
            }
            continue;
        }
        if crate::autopilot::blocked(ctx).is_some_and(|b| b.to_lowercase().contains("claude")) {
            log.push("waiting for Claude sign-in".into());
            break;
        }
        let Some((dir, s)) = current(ctx) else {
            let done = all_games(ctx).iter().filter(|(_, s)| s.game.status == "ready").count();
            if done >= ctx.settings.games_to_build.max(1) {
                log.push(format!("all {done} games are done"));
                break;
            }
            // the Lead picks the next game
            match ideate(ctx) {
                Ok(()) if current(ctx).is_some() => continue,
                Ok(()) => bail!("the Lead didn't pick a game"),
                Err(e) => {
                    log.push(format!("ideas: {e:#}"));
                    break;
                }
            }
        };
        if s.verdict == "ready" {
            let note = s.game.status_note.clone();
            update(&dir, |s| s.verdict.clear())?;
            finish_game(ctx, &dir, "ready", &note)?;
            continue;
        }
        if s.agent_runs >= ctx.settings.max_agent_runs_per_game {
            finish_game(ctx, &dir, "failed", "The team used its whole budget for this game without finishing it.")?;
            continue;
        }
        if s.fix_rounds >= 6 {
            finish_game(ctx, &dir, "failed", "The build kept breaking and the team couldn't fix it.")?;
            continue;
        }
        let batch = pick(ctx, &s);
        if batch.is_empty() {
            if s.open().next().is_some() {
                // tasks exist but none can start (stuck dependencies or blocked): let the Lead sort it out
                let stuck: Vec<String> = s.open().map(|t| t.id.clone()).collect();
                update(&dir, |s| {
                    for t in s.tasks.iter_mut().filter(|t| stuck.contains(&t.id)) {
                        t.status = "blocked".into();
                        t.notes.push("couldn't start: dependencies never finished".into());
                    }
                    s.log("studio", "Some tasks were stuck and were set aside for the Lead");
                })?;
            }
            next_phase(ctx, &dir)?;
            continue;
        }
        let results: Vec<(String, Result<()>)> = std::thread::scope(|sc| {
            let hs: Vec<_> = batch.iter().map(|id| (id.clone(), sc.spawn(|| run_task(ctx, &dir, id)))).collect();
            hs.into_iter().map(|(id, h)| (id, h.join().unwrap_or_else(|_| Err(anyhow!("agent crashed"))))).collect()
        });
        let mut usage_break = false;
        for (id, r) in results {
            if let Err(e) = r {
                let m = format!("{e:#}");
                crate::warn!("team task {id}: {m}");
                if m.contains("sign in") {
                    log.push(m);
                    return Ok(json!({"status": "paused", "log": log}));
                }
                usage_break |= m.contains("USAGE:");
            }
        }
        if usage_break {
            continue; // the top of the loop waits out the break, then building carries on
        }
        if stop_requested(ctx) {
            continue;
        }
        let code_changed = batch.iter().any(|id| load(&dir).tasks.iter().any(|t| &t.id == id && !matches!(t.kind.as_str(), "design" | "plan" | "feedback" | "final")));
        if code_changed {
            check_build(ctx, &dir)?;
        }
    }
    for a in AGENTS.iter() {
        set_now(ctx, a.id, None);
    }
    Ok(json!({"status": "stopped", "log": log}))
}

/// Nothing left to do on the board: move the game to its next phase.
fn next_phase(ctx: &Ctx, dir: &Path) -> Result<()> {
    let s = load(dir);
    let has = |kind: &str| s.tasks.iter().any(|t| t.kind == kind && t.status != "blocked");
    let last_final = s.tasks.iter().rposition(|t| t.kind == "final");
    let last_qa = s.tasks.iter().rposition(|t| t.kind == "qa");
    // QA + review are due when there is no QA since the last final review
    let qa_due = !has("qa") || last_final.is_some_and(|f| last_qa.is_none_or(|q| q < f));
    if s.last_build_ok != Some(true) && has("work") {
        if !check_build(ctx, dir)? {
            return Ok(());
        }
    }
    if qa_due {
        update(dir, |s| {
            let q = s.add_task("QA pass", "qa", "qa", "tests cover the game's logic and server start-up; every bug is filed as a task", vec![], "studio");
            s.add_task("Code review", "reviewer", "review", "every script reviewed; small issues fixed; big ones filed", vec![q], "studio");
            s.game.status = "testing".into();
            s.log("studio", "Building is done — QA and code review start");
        })?;
        return Ok(());
    }
    update(dir, |s| {
        s.review_rounds += 1;
        s.add_task("Final review", "lead", "final", "game_verdict called", vec![], "studio");
        s.game.status = "review".into();
        s.log("studio", "Ready for the Lead's final review");
    })?;
    Ok(())
}

/// The Lead comes up with the next game.
fn ideate(ctx: &Ctx) -> Result<()> {
    let done: Vec<String> = all_games(ctx).iter().map(|(_, s)| s.game.title.clone()).collect();
    let cap = if ctx.settings.max_game_size == "small" { "small" } else { "small or medium" };
    let mission = format!(
        "Pick the next game for the studio to build, then call `game_create` once.\n\
        1. Study the market with `charts` (several charts), `game_info`, the vault notes of learned games (`vault_search`, `vault_read`) and the Playbook (`playbook_read ideas`, `playbook_benchmarks`).\n\
        2. Come up with 3 candidate ideas: hook, core loop, inspired-by (mechanics only), the twist that makes it fresh, why now, size, money plan, risks. Score each 0-100 on demand, competition, timing, money potential, freshness and how well an AI team can build it well.\n\
        3. Choose the best one. Size must be {cap}: something a small team can make polished, not a sprawling game.\n\
        4. Choose the art style that fits (built from Roblox parts and materials: e.g. clean low-poly, bright toy-like, neon, cozy pastel).\n\
        5. `decision` with the three scores and why the winner won, then `game_create`.\n\
        Games already made (don't repeat them): {}.",
        if done.is_empty() { "none yet".to_string() } else { done.join(", ") }
    );
    run_agent(ctx, None, "lead", "ideate", &mission, "Picking the next game to build")?;
    Ok(())
}

/// Close a game: ready (into My Games, after a final test on Roblox's servers) or failed.
pub fn finish_game(ctx: &Ctx, dir: &Path, status: &str, note: &str) -> Result<()> {
    let mut status = status.to_string();
    let mut note = note.to_string();
    if status == "ready" && crate::cloud::connection(ctx).is_some() {
        let rep = run_pipeline(ctx, dir, true)?;
        if !rep.ok {
            let fail = rep.steps.iter().find(|s| !s.ok).map(|s| format!("{}: {}", s.name, s.detail)).unwrap_or_default();
            update(dir, |s| {
                let id = s.add_task("Fix what fails on Roblox's servers", "systems", "fix", "project_test passes", vec![], "studio");
                if let Some(t) = s.tasks.iter_mut().find(|t| t.id == id) {
                    t.notes.push(fail.clone());
                }
                s.game.status = "building".into();
                s.log("studio", "The final test on Roblox failed — back to building");
            })?;
            return Ok(());
        }
        note = format!("{note}\n\nTested on Roblox's servers (test place version {}).", rep.place_version.unwrap_or(0));
    }
    if status != "ready" && status != "failed" {
        status = "failed".into();
    }
    let title = update(dir, |s| {
        s.game.status = status.clone();
        s.game.status_note = note.trim().to_string();
        if status == "ready" {
            s.game.ready_at = now_iso();
        }
        for t in s.tasks.iter_mut().filter(|t| t.status == "todo" || t.status == "doing") {
            t.status = "blocked".into();
        }
        s.log("lead", if status == "ready" { "The game is ready for you to play" } else { "The game was stopped" });
        s.game.title.clone()
    })?;
    if status == "ready" {
        crate::autopilot::notify(ctx, "Game ready", &format!("{title} is ready to play in ROX-AI."));
    } else {
        crate::autopilot::notify(ctx, "ROX-AI team", &format!("{title}: {note}"));
    }
    Ok(())
}

/// Owner feedback on a game: it becomes the Lead's top priority and building resumes.
pub fn add_feedback(ctx: &Ctx, slug: &str, text: &str) -> Result<()> {
    let dir = game_dir(ctx, slug)?;
    update(&dir, |s| {
        s.feedback.push(Entry { at: now_iso(), by: "owner".into(), text: text.trim().to_string(), why: String::new() });
        s.add_task("Turn the owner's feedback into tasks", "lead", "feedback", "every point of the feedback is covered by a task", vec![], "owner");
        if matches!(s.game.status.as_str(), "ready" | "failed") {
            s.game.status = "building".into();
        }
        s.log("owner", format!("Feedback: {}", text.trim()));
    })?;
    Ok(())
}

// ------------------------------------------------------------------ chat with the Lead

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatMsg {
    pub from: String,
    pub text: String,
    pub at: String,
}

pub fn chat(ctx: &Ctx) -> Vec<ChatMsg> {
    ctx.store.get("lead_chat").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

pub fn chat_push(ctx: &Ctx, from: &str, text: &str) -> Result<()> {
    let mut c = chat(ctx);
    c.push(ChatMsg { from: from.into(), text: text.trim().to_string(), at: now_iso() });
    let n = c.len();
    if n > 200 {
        c.drain(..n - 200);
    }
    ctx.store.set("lead_chat", serde_json::to_value(c)?)
}

pub fn studio_summary(ctx: &Ctx) -> String {
    let mut out = String::new();
    let games = all_games(ctx);
    if games.is_empty() {
        out.push_str("No games yet.\n");
    }
    for (_, s) in games.iter().take(6) {
        let (d, t) = s.progress();
        out.push_str(&format!("- {} [{}] {} · {d}/{t} tasks done", s.game.title, s.game.slug, s.game.status));
        if let Some((work, pause, left)) = estimate(ctx, s) {
            out.push_str(&format!(" · about {} of work left ({left} steps)", human_minutes(work)));
            if pause > 0.0 {
                out.push_str(&format!(" + a {} Claude break", human_minutes(pause)));
            }
        }
        out.push('\n');
    }
    let st = &ctx.settings;
    out.push_str(&format!(
        "\nSettings: team {} · games to build {} · size cap {} · {} · avoid: {}\n",
        if st.team_enabled { "running" } else { "paused" },
        st.games_to_build,
        st.max_game_size,
        if st.team_parallel { "several agents at once" } else { "one agent at a time" },
        if st.avoid.trim().is_empty() { "nothing" } else { st.avoid.trim() }
    ));
    out
}

/// Answer the owner's latest chat message as the Lead.
pub fn chat_reply(ctx: &Ctx) -> Result<()> {
    let _lock = crate::util::single_instance(&ctx.state_dir(), "lead-chat")?;
    let msgs = chat(ctx);
    let history: String = msgs
        .iter()
        .rev()
        .take(16)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|m| format!("{}: {}\n", if m.from == "owner" { "Owner" } else { "Lead" }, m.text))
        .collect();
    let cur = current(ctx);
    let mission = format!(
        "The owner is chatting with you in the ROX-AI app. Reply as the Lead: warm, short (1-5 sentences), plain words, no jargon, no markdown headings.\n\
        You can act on what they ask: `board` for the current game, `task_add` to queue work, `decision` to log, `game_feedback` to file feedback on a game, \
        `team_settings` to change how many games to build, the size cap, one-at-a-time vs all agents at once, or to pause/resume the team. Say what you did.\n\
        Only the owner can press Publish; nothing goes public otherwise.\n\n\
        Studio right now:\n{}\n\nConversation (latest last):\n{history}\n\
        Your final message is sent to the owner as your reply.",
        studio_summary(ctx)
    );
    let r = run_agent(ctx, cur.as_ref().map(|(d, _)| d.as_path()), "lead", "chat", &mission, "Answering you");
    let text = match r {
        Ok(_) => {
            // run_agent returns the last line; read the full final message from the log
            last_result(ctx).unwrap_or_else(|| "Done.".into())
        }
        Err(e) => format!("I couldn't answer just now: {e:#}"),
    };
    chat_push(ctx, "lead", &text)
}

/// Full final message of the newest team-lead Claude run.
fn last_result(ctx: &Ctx) -> Option<String> {
    let mut logs: Vec<PathBuf> = std::fs::read_dir(ctx.logs_dir()).ok()?.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().contains("team-lead-")).collect();
    logs.sort();
    let raw = std::fs::read_to_string(logs.last()?).ok()?;
    let v: Value = serde_json::from_str(raw.trim().lines().last()?).ok()?;
    v["result"].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

// ------------------------------------------------------------------ for the app

pub fn stage_of(s: &Studio) -> (&'static str, String, f64) {
    let (d, t) = s.progress();
    let frac = if t == 0 { 0.0 } else { d as f64 / t as f64 };
    match s.game.status.as_str() {
        "ready" => ("ready", "Ready to play".into(), 1.0),
        "published" => ("published", "Published".into(), 1.0),
        "failed" => ("failed", s.game.status_note.clone(), frac),
        "testing" => ("testing", "Testing".into(), 0.85_f64.max(frac * 0.9)),
        "review" => ("testing", "Final review by the Lead".into(), 0.95),
        _ if s.tasks.iter().any(|t| t.kind == "plan" && t.status == "done") => ("building", format!("Building · {d} of {t} tasks done"), frac * 0.85),
        _ => ("idea", "Designing".into(), 0.05 + frac * 0.1),
    }
}

/// Rough time left for a game, from how long this team's finished tasks really took (breaks excluded).
/// Returns (minutes of work left, minutes of Claude break left, tasks left).
pub fn estimate(ctx: &Ctx, s: &Studio) -> Option<(f64, f64, usize)> {
    if matches!(s.game.status.as_str(), "ready" | "published" | "failed") || s.tasks.is_empty() {
        return None;
    }
    // pair each task's last start ("T6 · …") with its finish ("T6 done…") in the activity log
    let mut started: std::collections::HashMap<String, chrono::DateTime<chrono::Utc>> = Default::default();
    let mut took: Vec<f64> = vec![];
    for e in &s.activity {
        let Some(at) = crate::util::parse_dt(&e.at) else { continue };
        let Some((id, rest)) = e.text.split_once(' ') else { continue };
        if !id.starts_with('T') || !id[1..].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if rest.starts_with("· ") {
            started.insert(id.to_string(), at);
        } else if rest.starts_with("done") {
            if let Some(t0) = started.remove(id) {
                let m = (at - t0).num_seconds() as f64 / 60.0;
                if (0.5..=180.0).contains(&m) {
                    took.push(m);
                }
            }
        }
    }
    let per_task = if took.is_empty() { 15.0 } else { took.iter().sum::<f64>() / took.len() as f64 };
    let open = s.tasks.iter().filter(|t| t.status == "todo" || t.status == "doing").count();
    // phases still to come: QA + code review, the Lead's final review, the test on Roblox's servers
    let has = |k: &str| s.tasks.iter().any(|t| t.kind == k);
    let later = if !has("qa") { 3 } else if !has("final") { 1 } else { 0 };
    let left = open + later;
    let lanes = if ctx.settings.team_parallel { ctx.settings.team_max_parallel.max(1) as f64 } else { 1.0 };
    // fix rounds and retries add roughly a fifth on top
    let work = (left as f64 * per_task * 1.2 / lanes.min(left.max(1) as f64)).max(if left > 0 { per_task } else { 5.0 });
    let pause = paused_until(ctx)
        .and_then(|u| crate::util::parse_dt(&u))
        .map(|u| ((u - chrono::Utc::now()).num_seconds().max(0) as f64) / 60.0)
        .unwrap_or(0.0);
    Some((work.round(), pause.round(), left))
}

pub fn human_minutes(m: f64) -> String {
    let m = m.max(1.0).round() as i64;
    match m {
        0..=59 => format!("{m} min"),
        _ if m % 60 < 8 || m >= 600 => format!("{} h", (m as f64 / 60.0).round()),
        _ => format!("{} h {} min", m / 60, (m % 60) / 5 * 5),
    }
}

pub fn eta_json(ctx: &Ctx, s: &Studio) -> Value {
    match estimate(ctx, s) {
        Some((work, pause, left)) => {
            let at = chrono::Utc::now() + chrono::Duration::minutes((work + pause) as i64);
            {
            let paused = if !ctx.settings.team_enabled {
                Some("the team is paused")
            } else if crate::autopilot::blocked(ctx).is_some() {
                Some("waiting for you to sign in to Claude")
            } else {
                None
            };
            json!({"minutes": work, "break_minutes": pause, "tasks_left": left, "finish_at": at.to_rfc3339(), "paused": paused})
        }
        }
        None => Value::Null,
    }
}

pub fn my_games_json(ctx: &Ctx) -> Value {
    json!(all_games(ctx)
        .iter()
        .map(|(_, s)| {
            let (stage, text, p) = stage_of(s);
            json!({"id": s.game.slug, "title": s.game.title, "genre": s.game.genre, "stage": stage, "stage_text": text,
                "progress": (p * 100.0).round(), "updated": s.game.updated, "pitch": s.game.pitch, "summary": s.game.status_note,
                "eta": eta_json(ctx, s)})
        })
        .collect::<Vec<_>>())
}

pub fn game_json(ctx: &Ctx, slug: &str) -> Result<Value> {
    let dir = game_dir(ctx, slug)?;
    let s = load(&dir);
    let (stage, text, p) = stage_of(&s);
    let design = std::fs::read_to_string(dir.join("design.md")).unwrap_or_default();
    let last: Value = std::fs::read_to_string(dir.join(".roxai").join("last_build.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    Ok(json!({"game": s.game, "stage": stage, "stage_text": text, "progress": (p * 100.0).round(), "tasks": s.tasks,
        "decisions": s.decisions.iter().rev().take(30).collect::<Vec<_>>(), "activity": s.activity.iter().rev().take(40).collect::<Vec<_>>(),
        "feedback": s.feedback, "design": design, "last_build": last, "agent_runs": s.agent_runs, "eta": eta_json(ctx, &s)}))
}

/// The whole team for the Team and Home pages.
pub fn agents_json(ctx: &Ctx) -> Vec<Value> {
    let now = now_map(ctx);
    let running = crate::util::is_locked(&ctx.state_dir(), "studio");
    let paused = paused_until(ctx);
    AGENTS
        .iter()
        .map(|a| {
            let m = model_for(ctx, a.id);
            let (state, text) = if !a.active {
                ("soon", "Joins in a later update".to_string())
            } else if let Some(n) = now.get(a.id) {
                ("working", n["text"].as_str().unwrap_or("Working").to_string())
            } else if paused.is_some() {
                ("paused", "Taking a break (Claude usage limit)".into())
            } else if !ctx.settings.team_enabled {
                ("paused", "Team is paused".into())
            } else if running {
                ("idle", "Waiting for a task".into())
            } else {
                ("idle", "Ready".into())
            };
            json!({"id": a.id, "name": a.name, "role": a.role, "model": model_label(&m), "model_id": m, "default_model": a.model,
                "state": state, "text": text, "active": a.active})
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> (tempfile::TempDir, Ctx) {
        let d = tempfile::tempdir().unwrap();
        let c = Ctx::at(d.path());
        (d, c)
    }

    fn info(title: &str, size: &str) -> GameInfo {
        GameInfo { title: title.into(), pitch: "p".into(), size: size.into(), ..Default::default() }
    }

    #[test]
    fn create_game_starts_with_design_and_plan() {
        let (_d, c) = ctx();
        let dir = create_game(&c, info("Lava Tycoon!", "small")).unwrap();
        assert!(dir.join("default.project.json").is_file());
        let s = load(&dir);
        assert_eq!(s.game.slug, "lava-tycoon");
        assert_eq!(s.game.status, "building");
        assert_eq!(s.tasks.len(), 2);
        assert_eq!(s.runnable().len(), 1, "plan waits for design");
        assert_eq!(pick(&c, &s), vec!["T1".to_string()]);
        // one game at a time
        assert!(create_game(&c, info("Other", "small")).is_err());
        assert_eq!(current(&c).unwrap().1.game.slug, "lava-tycoon");
    }

    #[test]
    fn size_cap() {
        let (_d, mut c) = ctx();
        assert!(create_game(&c, info("Big", "large")).is_err());
        c.settings.max_game_size = "small".into();
        assert!(create_game(&c, info("Mid", "medium")).is_err());
    }

    #[test]
    fn parallel_picks_one_task_per_owner() {
        let (_d, mut c) = ctx();
        let dir = create_game(&c, info("G", "small")).unwrap();
        update(&dir, |s| {
            for t in s.tasks.iter_mut() {
                t.status = "done".into();
            }
            s.add_task("a", "systems", "work", "", vec![], "lead");
            s.add_task("b", "systems", "work", "", vec![], "lead");
            s.add_task("c", "ui", "work", "", vec![], "lead");
            s.add_task("d", "world", "work", "", vec!["T99".into()], "lead");
        })
        .unwrap();
        let s = load(&dir);
        assert_eq!(pick(&c, &s).len(), 1, "one at a time by default");
        c.settings.team_parallel = true;
        assert_eq!(pick(&c, &s), vec!["T3".to_string(), "T5".to_string()]);
        // a fix task always runs alone and first
        update(&dir, |s| {
            s.add_task("fix", "systems", "fix", "", vec![], "studio");
        })
        .unwrap();
        assert_eq!(pick(&c, &load(&dir)), vec!["T7".to_string()]);
    }

    #[test]
    fn phases_and_feedback() {
        let (_d, c) = ctx();
        let dir = create_game(&c, info("G", "small")).unwrap();
        update(&dir, |s| {
            for t in s.tasks.iter_mut() {
                t.status = "done".into();
            }
        })
        .unwrap();
        next_phase(&c, &dir).unwrap();
        let s = load(&dir);
        assert_eq!(s.game.status, "testing");
        assert!(s.tasks.iter().any(|t| t.kind == "qa") && s.tasks.iter().any(|t| t.kind == "review"));
        update(&dir, |s| {
            for t in s.tasks.iter_mut() {
                t.status = "done".into();
            }
        })
        .unwrap();
        next_phase(&c, &dir).unwrap();
        let s = load(&dir);
        assert_eq!(s.game.status, "review");
        assert_eq!(s.runnable()[0].kind, "final");
        finish_game(&c, &dir, "ready", "Fun game").unwrap();
        assert!(current(&c).is_none());
        assert_eq!(my_games_json(&c)[0]["stage"], "ready");
        add_feedback(&c, "g", "make it faster").unwrap();
        let s = load(&dir);
        assert_eq!(s.game.status, "building");
        assert!(prompt_for(&c, agent("lead").unwrap(), Some(&s), "x").contains("make it faster"));
    }

    #[test]
    fn models_per_agent() {
        let (_d, mut c) = ctx();
        assert_eq!(model_for(&c, "lead"), "opus");
        assert_eq!(model_for(&c, "ui"), "sonnet");
        c.settings.agent_models.insert("ui".into(), "haiku".into());
        assert_eq!(model_for(&c, "ui"), "haiku");
        assert_eq!(model_label("haiku"), "Haiku");
    }
}
