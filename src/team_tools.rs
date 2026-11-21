//! MCP tools for the AI team. They exist only inside an agent session (ROX-AI starts the session with
//! ROXAI_AGENT set) and are scoped to one game folder: an agent can't read or write anything else.

use crate::config::Ctx;
use crate::team::{self, Entry, GameInfo};
use crate::util::now_iso;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const MAX_FILE: usize = 400 * 1024;

pub fn agent_id() -> Option<String> {
    std::env::var("ROXAI_AGENT").ok().filter(|s| !s.trim().is_empty())
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

pub fn defs() -> Vec<Value> {
    let t = |name: &str, desc: &str, schema: Value| json!({"name": name, "description": desc, "inputSchema": schema});
    let s = json!({"type": "string"});
    let mut v = vec![
        t("board", "The game you're building: info, open tasks in full, finished tasks in one line each, recent decisions and activity. Read it once at the start. full=true also shows finished tasks' notes.", obj(json!({"full": {"type": "boolean"}}), &[])),
        t("project_files", "List every file in the game project (path, lines).", obj(json!({}), &[])),
        t("project_outline", "A file's structure with line numbers (functions, types, tables, sections) without the code. Use it before reading a long file, then read only the lines you need.", obj(json!({"path": s}), &["path"])),
        t("project_read", "Read a project file (path relative to the game folder, e.g. src/shared/Config.luau). At most 400 lines per call: use offset/limit (lines) and project_outline for long files.",
          obj(json!({"path": s, "offset": {"type": "integer"}, "limit": {"type": "integer"}}), &["path"])),
        t("project_write", "Create or replace a whole project file.", obj(json!({"path": s, "content": s}), &["path", "content"])),
        t("project_edit", "Replace an exact piece of text in a project file (old must match exactly once unless replace_all).",
          obj(json!({"path": s, "old": s, "new": s, "replace_all": {"type": "boolean"}}), &["path", "old", "new"])),
        t("project_delete", "Delete a project file.", obj(json!({"path": s}), &["path"])),
        t("project_search", "Search project files for text (case-insensitive). Returns path:line: text (up to 60 hits). path = only files whose path contains this text.", obj(json!({"query": s, "path": s}), &["query"])),
        t("project_run", "Run a Luau snippet on this PC against the freshly built game and get what it prints (and its return value). require any ModuleScript, e.g. local C = require(game.ReplicatedStorage.Shared.Config). For quick checks, numbers and economy simulations; it changes no files. Never write failing tests just to print values.", obj(json!({"code": s}), &["code"])),
        t("project_build", "Build the place with Rojo, compile every script and run the unit tests (this PC). Run it after every change.", obj(json!({}), &[])),
        t("project_test", "Build, upload to the private test place and run all tests (including remoteOnly specs) on Roblox's servers, with the server output. Slower and rate limited: use it for real checks.", obj(json!({}), &[])),
        t("task_add", "Add a task to the board. owner: lead, designer, economy, systems, gameplay, ui, world, qa, reviewer.",
          obj(json!({"title": s, "owner": s, "done_when": s, "deps": {"type": "array", "items": s, "description": "task ids that must be done first"}}), &["title", "owner", "done_when"])),
        t("task_update", "Update a task: status todo / doing / done / blocked, with a note (what you did, or why it's blocked).",
          obj(json!({"id": s, "status": s, "note": s}), &["id", "status"])),
        t("decision", "Log an important decision with the reason (the owner reads these instead of being asked).", obj(json!({"text": s, "why": s}), &["text", "why"])),
    ];
    if agent_id().as_deref() == Some("lead") {
        v.push(t("game_create", "Start building a new game (Lead only, one game at a time). size: small or medium.",
            obj(json!({"title": s, "pitch": s, "genre": s, "inspired_by": s, "twist": s, "why_now": s, "size": s, "art_style": s}), &["title", "pitch", "genre", "size", "art_style"])));
        v.push(t("game_verdict", "Final review result (Lead only). verdict: ready (summary = 2-4 sentences for the owner: what it is, how to play) or more_work (add the tasks first; summary = why).",
            obj(json!({"verdict": s, "summary": s}), &["verdict", "summary"])));
        v.push(t("game_feedback", "File the owner's feedback on a game (it becomes top priority and building resumes).", obj(json!({"game": s, "text": s}), &["game", "text"])));
        v.push(t("team_settings", "Change how the team works (only when the owner asks): games_to_build, max_game_size (small/medium), team_parallel (several agents at once), team_enabled (false = pause).",
            obj(json!({"games_to_build": {"type": "integer"}, "max_game_size": s, "team_parallel": {"type": "boolean"}, "team_enabled": {"type": "boolean"}}), &[])));
    }
    v
}

pub fn names() -> Vec<String> {
    defs().iter().filter_map(|d| d["name"].as_str().map(String::from)).collect()
}

fn arg<'a>(a: &'a Value, k: &str) -> &'a str {
    a.get(k).and_then(|v| v.as_str()).unwrap_or("")
}

/// The game this session works on: the one it was started for, else the one being built now.
fn game(ctx: &Ctx) -> Result<PathBuf> {
    if let Ok(g) = std::env::var("ROXAI_GAME") {
        if !g.trim().is_empty() {
            return team::game_dir(ctx, &g);
        }
    }
    team::current(ctx).map(|(d, _)| d).ok_or_else(|| anyhow!("no game is being built yet (the Lead starts one with game_create)"))
}

/// A path inside the game folder. No absolute paths, no `..`, no hidden folders, nothing in build/.
pub fn safe_path(dir: &Path, rel: &str) -> Result<PathBuf> {
    let rel = rel.trim().replace('\\', "/");
    let rel = rel.trim_start_matches("./");
    if rel.is_empty() || rel.starts_with('/') || rel.contains(':') {
        bail!("use a path relative to the game folder, e.g. src/shared/Config.luau");
    }
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    if parts.iter().any(|p| *p == ".." || (p.starts_with('.') && *p != ".gitignore")) || parts.first() == Some(&"build") {
        bail!("that path is outside the game's source files");
    }
    Ok(parts.iter().fold(dir.to_path_buf(), |p, c| p.join(c)))
}

fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, usize)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut es: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    es.sort();
    for p in es {
        let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        if name.starts_with('.') && name != ".gitignore" {
            continue;
        }
        let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
        if p.is_dir() {
            if rel != "build" {
                walk(&p, root, out);
            }
        } else {
            let lines = std::fs::read_to_string(&p).map(|t| t.lines().count()).unwrap_or(0);
            out.push((rel, lines));
        }
    }
}

fn build_text(rep: &crate::pipeline::Report) -> String {
    let mut out = format!("{}\n", if rep.ok { "BUILD OK" } else { "BUILD FAILED" });
    for s in &rep.steps {
        out.push_str(&format!("{} {}: {}\n", if s.ok { "✓" } else { "✗" }, s.name, s.detail));
    }
    if !rep.remote_logs.is_empty() {
        let tail: Vec<&String> = rep.remote_logs.iter().rev().take(60).collect::<Vec<_>>().into_iter().rev().collect();
        out.push_str("\nServer output on Roblox:\n");
        for l in tail {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

fn board_text(dir: &Path, full: bool) -> String {
    let s = team::load(dir);
    let me = agent_id().unwrap_or_default();
    let g = &s.game;
    let (d, t) = s.progress();
    let mut out = format!(
        "# {} ({})\nStatus: {} · {d}/{t} tasks done · size {} · art: {}\nPitch: {}\nGenre: {} · Inspired by: {} · Twist: {}\n\n## Tasks\n",
        g.title, g.slug, g.status, g.size, g.art_style, g.pitch, g.genre, g.inspired_by, g.twist
    );
    for x in &s.tasks {
        if x.status == "done" && !full {
            // finished work in one line: the details live in the code and the decisions
            out.push_str(&format!("- {} [done] {} → {}\n", x.id, x.title, x.owner));
            continue;
        }
        let note_len = if full || x.owner == me { 600 } else { 240 };
        out.push_str(&format!(
            "- {} [{}] {} → {} · done when: {}{}{}\n",
            x.id,
            x.status,
            x.title,
            x.owner,
            if full || x.owner == me { x.done_when.clone() } else { cut(&x.done_when, 160) },
            if x.deps.is_empty() { String::new() } else { format!(" · after {}", x.deps.join(", ")) },
            x.notes.last().map(|n| format!("\n    last note: {}", cut(n, note_len))).unwrap_or_default()
        ));
    }
    if !s.feedback.is_empty() {
        out.push_str("\n## Owner feedback\n");
        for f in &s.feedback {
            out.push_str(&format!("- {}{}\n", f.text, if f.why == "handled" { " (handled)" } else { "" }));
        }
    }
    out.push_str("\n## Recent decisions\n");
    for x in s.decisions.iter().rev().take(if full { 30 } else { 8 }) {
        out.push_str(&format!("- [{}] {} — {}\n", x.by, cut(&x.text, 300), cut(&x.why, 200)));
    }
    out.push_str("\n## Recent activity\n");
    for x in s.activity.iter().rev().filter(|x| !x.text.starts_with("files:")).take(8) {
        out.push_str(&format!("- [{}] {}\n", x.by, cut(&x.text, 160)));
    }
    if !dir.join("design.md").is_file() {
        out.push_str("\n(design.md doesn't exist yet)\n");
    }
    out
}

/// Run a team tool. None = not a team tool.
pub fn call(ctx: &Ctx, name: &str, a: &Value) -> Option<Result<String>> {
    let me = agent_id()?;
    if !names().iter().any(|n| n == name) {
        return None;
    }
    Some(call_inner(ctx, &me, name, a))
}

fn call_inner(ctx: &Ctx, me: &str, name: &str, a: &Value) -> Result<String> {
    Ok(match name {
        "game_create" => {
            let info = GameInfo {
                title: arg(a, "title").trim().to_string(),
                pitch: arg(a, "pitch").into(),
                genre: arg(a, "genre").into(),
                inspired_by: arg(a, "inspired_by").into(),
                twist: arg(a, "twist").into(),
                why_now: arg(a, "why_now").into(),
                size: arg(a, "size").to_lowercase(),
                art_style: arg(a, "art_style").into(),
                ..Default::default()
            };
            if info.title.is_empty() {
                bail!("title is required");
            }
            let dir = team::create_game(ctx, info)?;
            format!("Created. The Game Designer writes design.md next, then you plan the build.\n\n{}", board_text(&dir, false))
        }
        "team_settings" => {
            let mut st = crate::config::Settings::load(&ctx.home);
            if let Some(n) = a.get("games_to_build").and_then(|v| v.as_u64()) {
                st.games_to_build = n.clamp(1, 50) as usize;
            }
            match arg(a, "max_game_size") {
                "" => {}
                "small" | "medium" => st.max_game_size = arg(a, "max_game_size").into(),
                _ => bail!("max_game_size must be small or medium (large is off for now)"),
            }
            if let Some(b) = a.get("team_parallel").and_then(|v| v.as_bool()) {
                st.team_parallel = b;
            }
            if let Some(b) = a.get("team_enabled").and_then(|v| v.as_bool()) {
                st.team_enabled = b;
                if b {
                    crate::ui_api::bg(&["team", "run"]);
                }
            }
            st.save(&ctx.home)?;
            format!("Saved: games to build {}, size cap {}, {}, team {}.", st.games_to_build, st.max_game_size, if st.team_parallel { "several agents at once" } else { "one agent at a time" }, if st.team_enabled { "running" } else { "paused" })
        }
        "game_feedback" => {
            let g = if arg(a, "game").is_empty() { team::current(ctx).map(|(_, s)| s.game.slug).unwrap_or_default() } else { arg(a, "game").to_string() };
            team::add_feedback(ctx, &g, arg(a, "text"))?;
            if crate::config::Settings::load(&ctx.home).team_enabled {
                crate::ui_api::bg(&["team", "run"]);
            }
            "Filed. It's the team's top priority now.".into()
        }
        _ => {
            let dir = game(ctx)?;
            game_tool(ctx, me, &dir, name, a)?
        }
    })
}

fn game_tool(ctx: &Ctx, me: &str, dir: &Path, name: &str, a: &Value) -> Result<String> {
    Ok(match name {
        "board" => board_text(dir, a.get("full").and_then(|v| v.as_bool()).unwrap_or(false)),
        "project_outline" => {
            let p = safe_path(dir, arg(a, "path"))?;
            let text = std::fs::read_to_string(&p).map_err(|_| anyhow!("no file {}", arg(a, "path")))?;
            outline(&text)
        }
        "project_files" => {
            let mut v = vec![];
            walk(dir, dir, &mut v);
            v.iter().map(|(p, n)| format!("{p} ({n} lines)")).collect::<Vec<_>>().join("\n")
        }
        "project_read" => {
            let p = safe_path(dir, arg(a, "path"))?;
            let text = std::fs::read_to_string(&p).map_err(|_| anyhow!("no file {}", arg(a, "path")))?;
            let off = a.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let lim = a.get("limit").and_then(|v| v.as_u64()).unwrap_or(400).clamp(1, 400) as usize;
            let lines: Vec<&str> = text.lines().collect();
            let end = (off + lim).min(lines.len());
            let body = lines.get(off.min(lines.len())..end).unwrap_or(&[]).join("\n");
            if end < lines.len() {
                format!("(lines {}-{} of {}; read on with offset={end}, or use project_outline)\n{body}", off + 1, end, lines.len())
            } else if off > 0 {
                format!("(lines {}-{} of {})\n{body}", off + 1, end, lines.len())
            } else {
                body
            }
        }
        "project_write" => {
            let p = safe_path(dir, arg(a, "path"))?;
            let content = arg(a, "content");
            if content.len() > MAX_FILE {
                bail!("file too big: split it into smaller modules");
            }
            std::fs::create_dir_all(p.parent().unwrap())?;
            crate::util::atomic_write(&p, content.as_bytes())?;
            touched(dir, me, &format!("wrote {}", arg(a, "path")));
            format!("Wrote {} ({} lines).", arg(a, "path"), content.lines().count())
        }
        "project_edit" => {
            let p = safe_path(dir, arg(a, "path"))?;
            let text = std::fs::read_to_string(&p).map_err(|_| anyhow!("no file {}", arg(a, "path")))?;
            let (old, new) = (arg(a, "old"), arg(a, "new"));
            if old.is_empty() {
                bail!("old is empty");
            }
            let n = text.matches(old).count();
            let all = a.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);
            if n == 0 {
                bail!("old text not found in {} — read the file again and copy it exactly", arg(a, "path"));
            }
            if n > 1 && !all {
                bail!("old text appears {n} times — include more surrounding lines, or set replace_all");
            }
            let out = if all { text.replace(old, new) } else { text.replacen(old, new, 1) };
            crate::util::atomic_write(&p, out.as_bytes())?;
            touched(dir, me, &format!("edited {}", arg(a, "path")));
            format!("Edited {} ({} replacement{}).", arg(a, "path"), if all { n } else { 1 }, if all && n > 1 { "s" } else { "" })
        }
        "project_delete" => {
            let p = safe_path(dir, arg(a, "path"))?;
            std::fs::remove_file(&p).map_err(|_| anyhow!("no file {}", arg(a, "path")))?;
            touched(dir, me, &format!("deleted {}", arg(a, "path")));
            format!("Deleted {}.", arg(a, "path"))
        }
        "project_search" => {
            let q = arg(a, "query").to_lowercase();
            if q.is_empty() {
                bail!("query is empty");
            }
            let only = arg(a, "path").to_lowercase().replace('\\', "/");
            let mut files = vec![];
            walk(dir, dir, &mut files);
            let mut hits = vec![];
            let mut more = false;
            'f: for (rel, _) in files.into_iter().filter(|(r, _)| only.is_empty() || r.to_lowercase().contains(&only)) {
                let Ok(text) = std::fs::read_to_string(dir.join(&rel)) else { continue };
                for (i, l) in text.lines().enumerate() {
                    if l.to_lowercase().contains(&q) {
                        if hits.len() >= 60 {
                            more = true;
                            break 'f;
                        }
                        hits.push(format!("{rel}:{}: {}", i + 1, cut(l.trim(), 200)));
                    }
                }
            }
            if hits.is_empty() {
                "No matches.".into()
            } else if more {
                format!("{}\n(more matches: narrow the query or pass path)", hits.join("\n"))
            } else {
                hits.join("\n")
            }
        }
        "project_run" => {
            let code = arg(a, "code");
            if code.trim().is_empty() {
                bail!("code is empty");
            }
            // wait for a build that's running (another agent's) instead of failing
            let t0 = std::time::Instant::now();
            loop {
                match crate::pipeline::run_snippet(ctx, dir, code) {
                    Err(e) if e.to_string().contains("already running") && t0.elapsed().as_secs() < 600 => std::thread::sleep(std::time::Duration::from_secs(3)),
                    r => break r?,
                }
            }
        }
        "project_build" => {
            let rep = team::run_pipeline(ctx, dir, false)?;
            let _ = team::update(dir, |s| s.last_build_ok = Some(rep.ok));
            build_text(&rep)
        }
        "project_test" => {
            if crate::cloud::connection(ctx).is_none() {
                let rep = team::run_pipeline(ctx, dir, false)?;
                return Ok(format!("(Roblox isn't connected, so only the local checks ran.)\n{}", build_text(&rep)));
            }
            let rep = team::run_pipeline(ctx, dir, true)?;
            let _ = team::update(dir, |s| s.last_build_ok = Some(rep.ok));
            build_text(&rep)
        }
        "task_add" => {
            let owner = arg(a, "owner").to_lowercase();
            if !team::agent(&owner).is_some_and(|x| x.active) {
                bail!("owner must be one of: lead, designer, economy, systems, gameplay, ui, world, qa, reviewer");
            }
            let deps: Vec<String> = a.get("deps").and_then(|v| v.as_array()).map(|v| v.iter().filter_map(|x| x.as_str().map(|s| s.to_uppercase())).collect()).unwrap_or_default();
            let kind = "work";
            let id = team::update(dir, |s| {
                let id = s.add_task(arg(a, "title"), &owner, kind, arg(a, "done_when"), deps, me);
                if matches!(s.game.status.as_str(), "testing" | "review") && me != "qa" && me != "reviewer" {
                    // new work after testing started: build again, then QA again
                    s.game.status = "building".into();
                }
                s.log(me, format!("added {id}: {}", arg(a, "title")));
                id
            })?;
            format!("Added {id}.")
        }
        "task_update" => {
            let id = arg(a, "id").to_uppercase();
            let status = arg(a, "status").to_lowercase();
            if !matches!(status.as_str(), "todo" | "doing" | "done" | "blocked") {
                bail!("status must be todo, doing, done or blocked");
            }
            let note = arg(a, "note").trim().to_string();
            team::update(dir, |s| -> Result<()> {
                let t = s.tasks.iter_mut().find(|t| t.id == id).ok_or_else(|| anyhow!("no task {id}"))?;
                if t.kind == "final" && status == "done" && s.verdict.is_empty() {
                    bail!("call game_verdict first");
                }
                t.status = status.clone();
                if !note.is_empty() {
                    t.notes.push(note.clone());
                }
                t.updated = now_iso();
                let (title, kind) = (t.title.clone(), t.kind.clone());
                if kind == "feedback" && status == "done" {
                    for f in s.feedback.iter_mut() {
                        f.why = "handled".into();
                    }
                }
                s.log(me, format!("{id} {status}: {title}"));
                Ok(())
            })??;
            format!("{id} is {status}.")
        }
        "decision" => {
            team::update(dir, |s| s.decisions.push(Entry { at: now_iso(), by: me.into(), text: arg(a, "text").trim().into(), why: arg(a, "why").trim().into() }))?;
            "Logged.".into()
        }
        "game_verdict" => {
            if me != "lead" {
                bail!("only the Lead gives the verdict");
            }
            let v = arg(a, "verdict").to_lowercase();
            let summary = arg(a, "summary").trim().to_string();
            match v.as_str() {
                "ready" => {
                    team::update(dir, |s| {
                        s.verdict = "ready".into();
                        s.game.status_note = summary.clone();
                        for t in s.tasks.iter_mut().filter(|t| t.kind == "final" && t.status != "done") {
                            t.status = "done".into();
                            t.notes.push(format!("verdict: ready — {summary}"));
                        }
                        s.log("lead", "Verdict: ready");
                    })?;
                    "Marked ready. ROX-AI runs the final test on Roblox and hands it to the owner.".into()
                }
                "more_work" => {
                    team::update(dir, |s| {
                        s.verdict.clear();
                        for t in s.tasks.iter_mut().filter(|t| t.kind == "final" && t.status != "done") {
                            t.status = "done".into();
                            t.notes.push(format!("verdict: more work — {summary}"));
                        }
                        s.game.status = "building".into();
                        s.decisions.push(Entry { at: now_iso(), by: "lead".into(), text: "More work needed before it's ready".into(), why: summary.clone() });
                        s.log("lead", "Verdict: more work");
                    })?;
                    "Back to building. Make sure the tasks for the missing work are on the board.".into()
                }
                _ => bail!("verdict must be ready or more_work"),
            }
        }
        _ => bail!("unknown tool {name}"),
    })
}

fn touched(dir: &Path, me: &str, what: &str) {
    let _ = team::update(dir, |s| {
        s.last_build_ok = None;
        // keep the activity feed readable: merge a run of file changes by the same agent
        if let Some(last) = s.activity.last_mut() {
            if last.by == me && last.text.starts_with("files: ") && last.text.len() < 400 {
                last.text.push_str(&format!(", {what}"));
                return;
            }
        }
        s.log(me, format!("files: {what}"));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_paths() {
        let d = Path::new("C:/g");
        assert!(safe_path(d, "src/shared/Config.luau").is_ok());
        assert!(safe_path(d, "./design.md").is_ok());
        for bad in ["../x", "src/../../x", "/etc/passwd", "C:/Windows/x", ".roxai/studio.json", "build/place.rbxl", "src/.secret", ""] {
            assert!(safe_path(d, bad).is_err(), "{bad} should be refused");
        }
    }
}

/// Shorten text to n characters (with an ellipsis).
fn cut(t: &str, n: usize) -> String {
    if t.chars().count() > n {
        format!("{}…", t.chars().take(n).collect::<String>())
    } else {
        t.to_string()
    }
}

/// The skeleton of a Luau (or Markdown/JSON) file: declarations and headings with their line numbers.
pub fn outline(text: &str) -> String {
    let n = text.lines().count();
    let mut out = vec![format!("({n} lines)")];
    for (i, l) in text.lines().enumerate() {
        let t = l.trim_start();
        let indent = l.len() - t.len();
        let keep = t.starts_with('#')
            || t.starts_with("function ")
            || t.starts_with("local function ")
            || t.starts_with("export type ")
            || t.starts_with("type ")
            || (indent == 0
                && !t.starts_with("--")
                && !t.starts_with(['-', '*', '|', '>'])
                && (t.starts_with("local ") || t.starts_with("return ") || t.contains(" = ")))
            || (indent <= 4 && t.contains(" = function"));
        if keep {
            out.push(format!("{:>5}: {}", i + 1, cut(t.trim_end(), 140)));
        }
        if out.len() > 300 {
            out.push("… (outline cut at 300 entries)".into());
            break;
        }
    }
    out.join("\n")
}
