//! MCP server (Model Context Protocol, JSON-RPC 2.0 over stdio) — the tools Claude uses to study
//! charts, play games and write the vault. Registered in Claude Desktop and Claude Code by `roxai setup`.

use crate::analysis as A;
use crate::api::{HttpApi, RobloxApi};
use crate::config::Ctx;
use crate::session::Manager;
use crate::util::today;
use crate::vault::{ai_keys, Vault};
use anyhow::{anyhow, bail, Result};
use base64::Engine;
use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

const PROTOCOLS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

pub const INSTRUCTIONS: &str = "ROX-AI studies the Roblox charts and maintains an Obsidian vault with one detailed template note per game.
Typical flows:
- \"What's trending?\" → charts / game_info.
- \"Learn <game>\" → learn_playbook(universe_id): game_info → youtube_search → youtube_video (several) → vault_write_game_analysis with sources.
- \"Daily run\" → learn_queue, learn each game one at a time, then vault_write_insight with a market report.
Games are learned from public data and YouTube videos (no screen use).
- The Playbook (playbook_status / playbook_read / playbook_search) holds the rules for making successful Roblox games; the owner's rules in it always win.";

pub fn learn_text(ctx: &Ctx, uid: &str, name: &str, rank: &str) -> String {
    crate::LEARN_PLAYBOOK
        .replace("{universe_id}", uid)
        .replace("{name}", name)
        .replace("{rank}", rank)
        .replace("{videos}", &ctx.settings.videos_per_game.to_string())
}

/// sources: [{"title","url"}] or ["url", ...] → (title, url)
fn arg_sources(a: &Value) -> Vec<(String, String)> {
    a.get("sources")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| match x {
                    Value::String(u) => Some((u.clone(), u.clone())),
                    Value::Object(o) => {
                        let url = o
                            .get("url")
                            .and_then(|u| u.as_str())
                            .unwrap_or("")
                            .to_string();
                        let title = o
                            .get("title")
                            .and_then(|u| u.as_str())
                            .unwrap_or(&url)
                            .to_string();
                        (!url.is_empty()).then_some((title, url))
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

pub const PLAYTEST_TOOLS: [&str; 9] = [
    "playtest_start",
    "look",
    "act",
    "note",
    "snap",
    "playtest_finish",
    "playtest_abort",
    "session_status",
    "emergency_stop",
];

pub fn playbook_text(uid: &str, name: &str, rank: &str) -> String {
    crate::PLAYBOOK
        .replace("{universe_id}", uid)
        .replace("{name}", name)
        .replace("{rank}", rank)
}

/// Tool result: content blocks + error flag.
pub struct Out {
    pub content: Vec<Value>,
    pub is_error: bool,
}

impl Out {
    fn text(t: impl Into<String>) -> Out {
        Out {
            content: vec![json!({"type": "text", "text": t.into()})],
            is_error: false,
        }
    }
    fn json(v: &Value) -> Out {
        Out::text(serde_json::to_string_pretty(v).unwrap_or_default())
    }
    fn err(t: impl Into<String>) -> Out {
        Out {
            content: vec![json!({"type": "text", "text": t.into()})],
            is_error: true,
        }
    }
}

pub struct Server {
    pub ctx: Ctx,
    pub api: Arc<dyn RobloxApi>,
    pub mgr: Arc<Manager>,
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

pub fn tool_defs() -> Vec<Value> {
    let sec_keys = ai_keys().join(", ");
    let t = |name: &str, desc: &str, schema: Value| json!({"name": name, "description": desc, "inputSchema": schema});
    vec![
        t("youtube_search", "Search YouTube for videos (title, channel, length, views, age, url). Use it to find gameplay, guides and update videos about a Roblox game.",
          obj(json!({"query": {"type": "string"}, "limit": {"type": "integer", "default": 10}}), &["query"])),
        t("youtube_video", "Read one YouTube video without watching it: title, channel, description, chapters, timestamped transcript ([mm:ss]) and its thumbnail image.",
          obj(json!({"video_id": {"type": "string", "description": "video id or URL"}}), &["video_id"])),
        t("learn_queue", "Chart games that still need learning (never analyzed, stale, or updated since), best first.", obj(json!({"limit": {"type": "integer"}}), &[])),
        t("learn_playbook", "How to learn a game from data + YouTube videos and what to write. Read it before analyzing a game.", obj(json!({"universe_id": {"type": "integer"}}), &[])),
        t("playbook_status", "The ROX-AI Playbook (rules for making successful Roblox games): every chapter with rule counts, sources and when it was last written.", obj(json!({}), &[])),
        t("playbook_read", &format!("Read one Playbook chapter: the owner's rules (always follow them), the rules with confidence and evidence, and our own numbers. Chapters: {}.", crate::playbook::ids().join(", ")),
          obj(json!({"chapter": {"type": "string"}}), &["chapter"])),
        t("playbook_search", "Search the Playbook for rules about a topic (e.g. 'starter pack price', 'tutorial', 'DataStore'). Returns the best matching rules with chapter and confidence.",
          obj(json!({"query": {"type": "string"}, "chapter": {"type": "string"}, "max_results": {"type": "integer", "default": 15}}), &["query"])),
        t("playbook_benchmarks", "Our own numbers from every tracked game, overall and by genre: game passes, pass prices, developer products, server size, game age, update frequency, favorites per 1K visits.",
          obj(json!({"genre": {"type": "string"}}), &[])),
        t("playbook_guide", "How to write one Playbook chapter (sources to use, what makes a good rule). Read it before writing a chapter.", obj(json!({"chapter": {"type": "string"}}), &["chapter"])),
        t("playbook_write_rules", "Save rules to a Playbook chapter. replace=true writes the chapter's complete rule set; replace=false adds/updates rules. Each rule: {text, confidence: strong|emerging|opinion, why, evidence: [game titles or URLs]}.",
          obj(json!({"chapter": {"type": "string"}, "rules": {"type": "array", "items": {"type": "object", "properties": {"text": {"type": "string"}, "confidence": {"type": "string", "enum": ["strong", "emerging", "opinion"]}, "why": {"type": "string"}, "evidence": {"type": "array", "items": {"type": "string"}}}, "required": ["text", "confidence"]}}, "summary": {"type": "string"}, "replace": {"type": "boolean", "default": true}}), &["chapter", "rules"])),
        t("playbook_add_sources", "Record the devlogs, postmortems, docs and threads you used for a chapter (so later runs find new ones). Each: {title, url, kind: devlog|postmortem|docs|devforum|video|article, summary}.",
          obj(json!({"chapter": {"type": "string"}, "sources": {"type": "array", "items": {"type": "object"}}}), &["chapter", "sources"])),
        t("charts", "Live Roblox chart (default: the configured chart, usually 'top-trending'): rank, players, likes, genre and ROX-AI status for each game. Other sort_ids: top-playing-now, up-and-coming, fun-with-friends, top-revisited.",
          obj(json!({"sort_id": {"type": "string"}, "limit": {"type": "integer", "default": 25}}), &[])),
        t("list_chart_sorts", "List every chart Roblox currently shows (sortId + display name).", obj(json!({}), &[])),
        t("game_info", "Everything public about one game: description, creator, stats, game passes & developer products with prices, badges (award counts show how far players get), server sample, chart trend, monetization breakdown, and which vault sections are written. Also refreshes its vault note.",
          obj(json!({"universe_id": {"type": "integer"}}), &["universe_id"])),
        t("scan_charts", "Run the daily chart scan now (stats/trends for every chart game, chart note, genre notes, dashboard). ~1 minute.", obj(json!({"limit": {"type": "integer"}}), &[])),
        t("playtest_queue", "Games that should be playtested next (never played / stale / updated since last playtest), best first.", obj(json!({"limit": {"type": "integer"}}), &[])),
        t("playbook", "The full playtest playbook (how to play, safety rules, what to write). Read it before a playtest.", obj(json!({"universe_id": {"type": "integer"}}), &[])),
        t("vault_read_game", "Read the 🤖 analysis sections and ✍️ user notes stored for a game (extend rather than overwrite).", obj(json!({"universe_id": {"type": "integer"}}), &["universe_id"])),
        t("vault_write_game_analysis", &format!("Write/replace 🤖 sections of a game's template note without a playtest (e.g. from research). Section keys: {sec_keys}. Score keys (1-10): fun, clarity, retention, monetization_aggressiveness, polish, clone_difficulty, trend_durability."),
          obj(json!({"universe_id": {"type": "integer"}, "sections": {"type": "object", "additionalProperties": {"type": "string"}}, "scores": {"type": "object", "additionalProperties": {"type": "integer"}}, "one_liner": {"type": "string"}, "tags": {"type": "array", "items": {"type": "string"}}, "method": {"type": "string", "enum": ["videos", "data", "playtest"]}, "sources": {"type": "array", "items": {"type": "object", "properties": {"title": {"type": "string"}, "url": {"type": "string"}}}}}), &["universe_id", "sections"])),
        t("vault_write_insight", "Create a note in 05 Insights (weekly market report, comparisons, trends). Link games with [[02 Games/<Name>]].",
          obj(json!({"title": {"type": "string"}, "markdown": {"type": "string"}, "tags": {"type": "array", "items": {"type": "string"}}}), &["title", "markdown"])),
        t("vault_write_genre_patterns", "Write the 🤖 'Genre Patterns' section of a genre note.", obj(json!({"genre": {"type": "string"}, "markdown": {"type": "string"}}), &["genre", "markdown"])),
        t("vault_search", "Case-insensitive text/regex search across the vault. Returns matching lines with note paths.",
          obj(json!({"query": {"type": "string"}, "folder": {"type": "string"}, "max_results": {"type": "integer", "default": 30}}), &["query"])),
        t("vault_read_note", "Read any vault note by path relative to the vault (e.g. '00 Dashboard.md').", obj(json!({"path": {"type": "string"}, "max_chars": {"type": "integer", "default": 20000}}), &["path"])),
        t("vault_list", "List notes in a vault folder (default '02 Games').", obj(json!({"folder": {"type": "string"}}), &[])),
        t("playtest_start", "Close any running Roblox, join the game with the logged-in alt account and start a timed AI playtest (default ~12 min). Returns the first screenshot. Physical mouse/keyboard input or Ctrl+Shift+Q stops the AI instantly.",
          obj(json!({"universe_id": {"type": "integer"}, "minutes": {"type": "integer"}}), &["universe_id"])),
        t("look", "Screenshot of the game. grid=true overlays a labelled 100px grid to pick click coordinates.", obj(json!({"grid": {"type": "boolean", "default": false}}), &[])),
        t("act", "Perform up to 25 game actions in order, then (by default) return a screenshot. Types: walk{dir:'w'|'a'|'s'|'d' combos, seconds, sprint?, jump?} · jump{times} · key{key, hold?, times?} · keys{keys:[..], seconds} · click{x,y, button?, double?} · hold_click{x,y,seconds} · move_mouse{x,y} · camera{dx,dy} (~300 ≈ 90°) · scroll{clicks} (neg = zoom in) · type{text, enter?} · wait{seconds}. x/y are pixels in the latest screenshot.",
          obj(json!({"actions": {"type": "array", "items": {"type": "object"}}, "look_after": {"type": "boolean", "default": true}, "grid": {"type": "boolean", "default": false}}), &["actions"])),
        t("note", "Save a timestamped observation to the playtest log (crash-safe). Include numbers, prices, timers.", obj(json!({"text": {"type": "string"}}), &["text"])),
        t("snap", "Save the current screen into the vault playtest log with a caption.", obj(json!({"caption": {"type": "string"}}), &["caption"])),
        t("playtest_finish", &format!("End the playtest: closes Roblox, saves your report and writes the 🤖 sections of the game's template note (keys: {sec_keys}). Works even after an abort."),
          obj(json!({"report": {"type": "string"}, "sections": {"type": "object", "additionalProperties": {"type": "string"}}, "scores": {"type": "object", "additionalProperties": {"type": "integer"}}, "one_liner": {"type": "string"}, "tags": {"type": "array", "items": {"type": "string"}}}), &["report"])),
        t("playtest_abort", "Stop the playtest right away because it cannot continue (login/age-verification screen, ban, broken game). Closes Roblox.", obj(json!({"reason": {"type": "string"}}), &["reason"])),
        t("session_status", "Current playtest status (time left, actions, problems).", obj(json!({}), &[])),
        t("emergency_stop", "Immediately release all keys, close Roblox and end the session.", obj(json!({}), &[])),
        t("system_check", "Health check of the whole setup (Roblox, vault, schedule, Claude CLI, last runs, blockers).", obj(json!({}), &[])),
    ]
}

pub fn prompt_defs() -> Vec<Value> {
    vec![
        json!({"name": "playtest_game", "description": "Playtest one game and fill its vault template.", "arguments": [{"name": "universe_id", "description": "Roblox universe id", "required": true}]}),
        json!({"name": "learn_game", "description": "Learn one game from data + YouTube videos and fill its vault template.", "arguments": [{"name": "universe_id", "description": "Roblox universe id", "required": true}]}),
        json!({"name": "weekly_run", "description": "Playtest everything in the queue, then write the weekly market report.", "arguments": []}),
        json!({"name": "market_report", "description": "Write a market/trend report from the vault data without playing.", "arguments": []}),
    ]
}

fn arg_u64(a: &Value, k: &str) -> Option<u64> {
    a.get(k).and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_f64().map(|f| f as u64))
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    })
}
fn arg_str<'a>(a: &'a Value, k: &str) -> &'a str {
    a.get(k).and_then(|v| v.as_str()).unwrap_or("")
}
fn arg_bool(a: &Value, k: &str, d: bool) -> bool {
    a.get(k).and_then(|v| v.as_bool()).unwrap_or(d)
}
fn arg_tags(a: &Value) -> Option<Vec<String>> {
    a.get("tags").and_then(|v| v.as_array()).map(|v| {
        v.iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect()
    })
}
fn arg_map(a: &Value, k: &str) -> Option<Map<String, Value>> {
    match a.get(k) {
        Some(Value::Object(m)) => Some(m.clone()),
        Some(Value::String(s)) => serde_json::from_str::<Map<String, Value>>(s).ok(),
        _ => None,
    }
}

impl Server {
    pub fn new(ctx: Ctx, api: Arc<dyn RobloxApi>) -> Server {
        let mgr = Arc::new(Manager::new(&ctx));
        Server { ctx, api, mgr }
    }

    fn vault(&self) -> Vault {
        Vault::new(self.ctx.vault())
    }

    fn note_for(&self, uid: u64) -> Result<std::path::PathBuf> {
        if uid == 0 {
            bail!("universe_id must be a real game's universe id (find it with charts, vault_search or vault_list)");
        }
        let v = self.vault();
        if let Some(n) = self.ctx.store.game(uid).and_then(|g| g.note_path) {
            let p = v.abs(&n);
            if p.exists() {
                return Ok(p);
            }
        }
        if let Some(p) = v.find_game_note(uid) {
            return Ok(p);
        }
        crate::scan::refresh_one(&self.ctx, self.api.as_ref(), uid)
    }

    fn shot_out(&self, text: &str, grid: bool) -> Out {
        match self.mgr.look(grid) {
            Ok(shot) => {
                let dark = if shot.brightness < 12.0 {
                    " (screen almost black — loading or a fade)"
                } else {
                    ""
                };
                Out {
                    content: vec![
                        json!({"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(&shot.jpeg), "mimeType": "image/jpeg"}),
                        json!({"type": "text", "text": format!("{text}\nScreenshot {}x{}px{dark}. {}", shot.width, shot.height, self.mgr.status_line())}),
                    ],
                    is_error: false,
                }
            }
            Err(e) => {
                let mut o = Out::text(format!("{text}\n⚠️ no screenshot: {e}"));
                o.is_error = !self.mgr.has_session();
                o
            }
        }
    }

    pub fn call(&self, name: &str, a: &Value) -> Out {
        if let Some(r) = crate::team_tools::call(&self.ctx, name, a) {
            return match r {
                Ok(t) => Out::text(t),
                Err(e) => Out::err(format!("{e:#}")),
            };
        }
        match self.call_inner(name, a) {
            Ok(o) => o,
            Err(e) => {
                crate::error!("tool {name} failed: {e:#}");
                Out::err(format!("ERROR in {name}: {e:#}"))
            }
        }
    }

    fn call_inner(&self, name: &str, a: &Value) -> Result<Out> {
        let ctx = &self.ctx;
        let s = &ctx.settings;
        let v = self.vault();
        Ok(match name {
            "charts" => {
                let sid = if arg_str(a, "sort_id").is_empty() {
                    s.sort_id.clone()
                } else {
                    arg_str(a, "sort_id").to_string()
                };
                let limit = arg_u64(a, "limit").unwrap_or(25).min(100) as usize;
                let games = ctx.store.games();
                let rows: Vec<Value> = self.api.chart(&sid, limit, &s.country)?.into_iter().map(|e| {
                    let g = games.get(&e.universe_id);
                    json!({"rank": e.rank, "universe_id": e.universe_id, "name": e.name, "genre": e.genre, "playing": e.playing,
                           "like_ratio": A::like_ratio(e.up_votes, e.down_votes), "in_vault": g.is_some_and(|g| g.note_path.is_some()),
                           "last_playtest": g.and_then(|g| g.last_playtest.clone()).map(|x| x.chars().take(10).collect::<String>()).unwrap_or_default()})
                }).collect();
                Out::json(&json!({"chart": sid, "games": rows}))
            }
            "list_chart_sorts" => Out::json(&Value::Array(self.api.sorts(&s.country)?)),
            "game_info" => {
                let uid =
                    arg_u64(a, "universe_id").ok_or_else(|| anyhow!("universe_id required"))?;
                let mut d = self.api.game_details(uid, true);
                if d.name.is_empty() {
                    return Ok(Out::err(format!(
                        "No Roblox experience found for universe id {uid}."
                    )));
                }
                let path = crate::scan::refresh_one(ctx, self.api.as_ref(), uid).ok();
                let hist = ctx.store.history(uid, &s.sort_id, 60);
                let written: Vec<String> = path
                    .as_ref()
                    .map(|p| {
                        v.read_note_sections(p)
                            .into_iter()
                            .filter(|(k, val)| {
                                ai_keys().contains(&k.as_str()) && !val.contains("Not analyzed yet")
                            })
                            .map(|(k, _)| k)
                            .collect()
                    })
                    .unwrap_or_default();
                let mon = A::monetization(&mut d.game_passes, &mut d.dev_products);
                let mut badges = d.badges.clone();
                badges.sort_by(|x, y| y.awarded.cmp(&x.awarded));
                badges.truncate(25);
                Out::json(&json!({
                    "universe_id": d.universe_id, "place_id": d.place_id, "name": d.name, "url": d.url(),
                    "creator": format!("{} ({}{})", d.creator_name, d.creator_type, if d.creator_verified { ", verified" } else { "" }),
                    "genre": format!("{} / {}", d.genre, d.subgenre), "playing": d.playing, "visits": d.visits, "favorites": d.favorites,
                    "likes": d.up_votes, "dislikes": d.down_votes, "like_ratio": d.like_ratio(), "max_players": d.max_players,
                    "created": d.created, "updated": d.updated, "description": d.description.chars().take(3000).collect::<String>(),
                    "monetization_summary": mon, "game_passes": d.game_passes, "dev_products": d.dev_products.iter().take(60).collect::<Vec<_>>(),
                    "badges_top": badges, "badge_total": d.badge_total, "server_sample": d.server_sample,
                    "trend": if hist.is_empty() { Value::Null } else { serde_json::to_value(A::trend(&hist))? },
                    "vault_note": path.as_ref().map(|p| v.rel(p)),
                    "ai_sections_written": written,
                    "ai_sections_missing": ai_keys().into_iter().filter(|k| !written.iter().any(|w| w == k)).collect::<Vec<_>>(),
                }))
            }
            "scan_charts" => {
                let r = crate::scan::run_scan(
                    ctx,
                    self.api.as_ref(),
                    arg_u64(a, "limit").map(|x| x as usize).filter(|x| *x > 0),
                    None,
                )?;
                Out::json(&serde_json::to_value(&r)?)
            }
            "playtest_queue" => Out::json(&serde_json::to_value(crate::scan::playtest_queue(
                ctx,
                arg_u64(a, "limit").map(|x| x as usize).filter(|x| *x > 0),
                None,
            ))?),
            "playbook" => match arg_u64(a, "universe_id") {
                Some(uid) => {
                    let g = ctx.store.game(uid);
                    Out::text(playbook_text(
                        &uid.to_string(),
                        &g.as_ref()
                            .map(|g| g.name.clone())
                            .unwrap_or(uid.to_string()),
                        &g.and_then(|g| g.best_rank)
                            .map(|r| r.to_string())
                            .unwrap_or("?".into()),
                    ))
                }
                None => Out::text(playbook_text("<id>", "<game>", "?")),
            },
            "vault_read_game" => {
                let uid =
                    arg_u64(a, "universe_id").ok_or_else(|| anyhow!("universe_id required"))?;
                let p = self.note_for(uid)?;
                Out::json(&json!({"note": v.rel(&p), "sections": v.read_note_sections(&p)}))
            }
            "vault_write_game_analysis" => {
                let uid =
                    arg_u64(a, "universe_id").ok_or_else(|| anyhow!("universe_id required"))?;
                let secs = arg_map(a, "sections").unwrap_or_default();
                let p = self.note_for(uid)?;
                let sources = arg_sources(a);
                let method = match arg_str(a, "method") {
                    "" if !sources.is_empty() => "videos",
                    "" => "data",
                    m => m,
                }
                .to_string();
                let w = v.write_ai_sections(
                    &p,
                    &secs,
                    arg_map(a, "scores").as_ref(),
                    Some(arg_str(a, "one_liner")),
                    arg_tags(a).as_deref(),
                    &format!("analyzed ({method})"),
                )?;
                v.set_sources(&p, &method, &sources)?;
                ctx.store.upsert_game(uid, |g| {
                    g.analysis_source = Some(method.clone());
                    g.last_analysis = Some(crate::util::now_iso());
                    g.videos_used = sources.iter().map(|(_, u)| u.clone()).collect();
                })?;
                let saved = secs
                    .keys()
                    .filter(|k| ai_keys().contains(&k.as_str()))
                    .count();
                Out::text(format!(
                    "Saved {saved} section(s) to {}.{}",
                    v.rel(&p),
                    if w.is_empty() {
                        String::new()
                    } else {
                        format!(" Warnings: {}", w.join("; "))
                    }
                ))
            }
            "vault_write_insight" => {
                let p = v.write_insight(
                    arg_str(a, "title"),
                    arg_str(a, "markdown"),
                    &arg_tags(a).unwrap_or_default(),
                )?;
                Out::text(format!("Saved {}", v.rel(&p)))
            }
            "vault_write_genre_patterns" => {
                let p = v.set_genre_patterns(arg_str(a, "genre"), arg_str(a, "markdown"))?;
                Out::text(format!("Saved {}", v.rel(&p)))
            }
            "vault_search" => {
                let q = arg_str(a, "query");
                let rx = regex::RegexBuilder::new(q)
                    .case_insensitive(true)
                    .build()
                    .unwrap_or_else(|_| {
                        regex::RegexBuilder::new(&regex::escape(q))
                            .case_insensitive(true)
                            .build()
                            .unwrap()
                    });
                let max = arg_u64(a, "max_results").unwrap_or(30) as usize;
                let root = if arg_str(a, "folder").is_empty() {
                    v.root.clone()
                } else {
                    v.root.join(arg_str(a, "folder"))
                };
                let mut hits = vec![];
                let mut files = vec![];
                walk_md(&root, &mut files);
                files.sort();
                'outer: for f in files {
                    for (i, line) in v.read(&f).lines().enumerate() {
                        if rx.is_match(line) {
                            hits.push(format!(
                                "{}:{}: {}",
                                v.rel(&f),
                                i + 1,
                                line.trim().chars().take(200).collect::<String>()
                            ));
                            if hits.len() >= max {
                                break 'outer;
                            }
                        }
                    }
                }
                Out::text(if hits.is_empty() {
                    "No matches.".into()
                } else {
                    hits.join("\n")
                })
            }
            "vault_read_note" => {
                let rel = arg_str(a, "path");
                if rel.contains("..") {
                    return Ok(Out::err("Path must be inside the vault."));
                }
                let p = v.abs(rel);
                if !p.exists() {
                    let mut files = vec![];
                    walk_md(&v.root, &mut files);
                    let sim: Vec<String> = files
                        .iter()
                        .filter(|f| {
                            f.file_name().is_some_and(|n| {
                                n.to_string_lossy()
                                    .to_lowercase()
                                    .contains(&rel.to_lowercase())
                            })
                        })
                        .take(10)
                        .map(|f| v.rel(f))
                        .collect();
                    return Ok(Out::err(format!("Not found. Similar: {sim:?}")));
                }
                Out::text(
                    v.read(&p)
                        .chars()
                        .take(arg_u64(a, "max_chars").unwrap_or(20000) as usize)
                        .collect::<String>(),
                )
            }
            "vault_list" => {
                let folder = if arg_str(a, "folder").is_empty() {
                    "02 Games"
                } else {
                    arg_str(a, "folder")
                };
                let mut names: Vec<String> = std::fs::read_dir(v.root.join(folder))
                    .map(|rd| {
                        rd.flatten()
                            .map(|e| e.path())
                            .filter(|p| p.extension().is_some_and(|e| e == "md"))
                            .map(|p| v.rel(&p))
                            .collect()
                    })
                    .unwrap_or_default();
                names.sort();
                Out::text(if names.is_empty() {
                    "Folder empty or not found.".into()
                } else {
                    names.join("\n")
                })
            }
            "youtube_search" => {
                let q = arg_str(a, "query");
                let n = arg_u64(a, "limit").unwrap_or(10) as usize;
                Out::json(&serde_json::to_value(crate::youtube::search(q, n)?)?)
            }
            "youtube_video" => {
                let id = arg_str(a, "video_id");
                let info = crate::youtube::video(id, &ctx.tools_dir())?;
                let mut text = format!(
                    "# {}\nChannel: {} · {} min · {} views · {}\n\n## Description\n{}\n",
                    info.title, info.channel, info.length_seconds / 60, info.views, info.url,
                    info.description.chars().take(2500).collect::<String>()
                );
                if !info.chapters.is_empty() {
                    text.push_str(&format!("\n## Chapters\n{}\n", info.chapters.join("\n")));
                }
                match &info.transcript {
                    Some(t) => text.push_str(&format!("\n## Transcript ({})\n{t}\n", info.transcript_source)),
                    None => text.push_str(&format!("\n## Transcript\n_not available: {}_ — use the title, description and chapters, and pick another video if needed.\n", info.note)),
                }
                let mut content = vec![json!({"type": "text", "text": text})];
                if let Some(img) = crate::youtube::thumbnail(&info.id) {
                    content.insert(0, json!({"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(img), "mimeType": "image/jpeg"}));
                }
                Out { content, is_error: false }
            }
            "playbook_status" => Out::json(&json!({"chapters": crate::playbook::status(ctx), "due": crate::playbook::due(ctx).iter().map(|c| c.id).collect::<Vec<_>>()})),
            "playbook_read" => Out::text(crate::playbook::read_chapter(ctx, arg_str(a, "chapter"))?),
            "playbook_search" => {
                let ch = arg_str(a, "chapter");
                let hits = crate::playbook::search(ctx, arg_str(a, "query"), (!ch.is_empty()).then_some(ch), arg_u64(a, "max_results").unwrap_or(15) as usize);
                if hits.is_empty() {
                    Out::text("No matching rules. Try other words, or read a chapter with playbook_read.")
                } else {
                    Out::json(&Value::Array(hits))
                }
            }
            "playbook_benchmarks" => {
                let g = arg_str(a, "genre");
                Out::json(&crate::playbook::benchmarks_json(&crate::playbook::facts(ctx), (!g.is_empty()).then_some(g)))
            }
            "playbook_guide" => match crate::playbook::chapter(arg_str(a, "chapter")) {
                Some(c) => Out::text(crate::playbook::guide(ctx, c)),
                None => Out::err(format!("Unknown chapter. Chapters: {}", crate::playbook::ids().join(", "))),
            },
            "playbook_write_rules" => {
                let rules = a.get("rules").and_then(|r| r.as_array()).cloned().unwrap_or_default();
                if rules.is_empty() {
                    return Ok(Out::err("rules is empty"));
                }
                let n = crate::playbook::write_rules(ctx, arg_str(a, "chapter"), &rules, arg_str(a, "summary"), arg_bool(a, "replace", true))?;
                Out::text(format!("Saved. The chapter now has {n} rules."))
            }
            "playbook_add_sources" => {
                let src = a.get("sources").and_then(|r| r.as_array()).cloned().unwrap_or_default();
                let n = crate::playbook::add_sources(ctx, arg_str(a, "chapter"), &src)?;
                Out::text(format!("Recorded {n} new source(s)."))
            }
            "learn_queue" => Out::json(&serde_json::to_value(crate::scan::learn_queue(
                ctx,
                arg_u64(a, "limit").map(|x| x as usize).filter(|x| *x > 0),
            ))?),
            "learn_playbook" => match arg_u64(a, "universe_id") {
                Some(uid) => {
                    let g = ctx.store.game(uid);
                    Out::text(learn_text(ctx, &uid.to_string(), &g.as_ref().map(|g| g.name.clone()).unwrap_or(uid.to_string()), &g.and_then(|g| g.best_rank).map(|r| r.to_string()).unwrap_or("?".into())))
                }
                None => Out::text(learn_text(ctx, "<id>", "<game>", "?")),
            },
            "playtest_start" if !ctx.settings.playtesting_enabled => {
                Out::err("Playtesting is turned off in ROX-AI. Learn the game from videos instead: learn_playbook(universe_id).")
            }
            "playtest_start" => {
                let uid =
                    arg_u64(a, "universe_id").ok_or_else(|| anyhow!("universe_id required"))?;
                let msg = self.mgr.start(
                    self.api.as_ref(),
                    uid,
                    arg_u64(a, "minutes").map(|m| m as u32).filter(|m| *m > 0),
                )?;
                // let the game load a bit
                for _ in 0..12 {
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                    if self.mgr.look(false).is_ok_and(|s| s.brightness > 15.0) {
                        break;
                    }
                }
                self.shot_out(&format!("{msg}\nRead playbook() if you haven't. Use act/look/note/snap, then playtest_finish. Hands off: physical input stops the AI."), false)
            }
            "look" => self.shot_out("👀", arg_bool(a, "grid", false)),
            "act" => {
                let actions = a
                    .get("actions")
                    .and_then(|x| x.as_array())
                    .cloned()
                    .unwrap_or_default();
                match self.mgr.act(&actions) {
                    Ok(done) => {
                        let txt = format!("✅ {}", done.join("; "));
                        if arg_bool(a, "look_after", true) {
                            self.shot_out(&txt, arg_bool(a, "grid", false))
                        } else {
                            Out::text(format!("{txt}\n{}", self.mgr.status_line()))
                        }
                    }
                    Err(e) => self.shot_out(&format!("⚠️ {e}"), false),
                }
            }
            "note" => {
                self.mgr.timeline(arg_str(a, "text"))?;
                Out::text(format!("noted. {}", self.mgr.status_line()))
            }
            "snap" => Out::text(format!(
                "📸 saved {}",
                self.mgr.snap(arg_str(a, "caption"))?
            )),
            "playtest_finish" => self.finish(a)?,
            "playtest_abort" => {
                let reason = arg_str(a, "reason");
                if !self.mgr.has_session() {
                    return Ok(Out::text("No session running."));
                }
                *self.mgr.abort.lock().unwrap() = Some(format!("blocked: {reason}"));
                self.mgr.end("blocked", reason);
                let account = regex::Regex::new(r"(?i)login|log in|sign in|verif|captcha|ban")
                    .unwrap()
                    .is_match(reason);
                if account {
                    ctx.store.set(
                        "blocked_reason",
                        json!({"reason": reason, "at": crate::util::now_iso()}),
                    )?;
                }
                Out::text(format!(
                    "Session stopped: {reason}. {}",
                    if account {
                        "⚠️ This looks account-related: the autopilot pauses playtests until the user fixes the Roblox login."
                    } else {
                        "You can call playtest_finish to save partial findings."
                    }
                ))
            }
            "session_status" => match self.mgr.status_json() {
                Some(j) => Out::json(&j),
                None => Out::text("No playtest running."),
            },
            "emergency_stop" => {
                crate::win::input::release_all();
                if self.mgr.has_session() {
                    *self.mgr.abort.lock().unwrap() = Some("emergency_stop".into());
                    self.mgr.end("stopped", "emergency stop");
                }
                crate::win::roblox::kill_all();
                Out::text("Stopped. All inputs released and Roblox closed.")
            }
            "system_check" => Out::text(crate::doctor::report_text(ctx, true)),
            other => Out::err(format!("Unknown tool '{other}'")),
        })
    }

    fn finish(&self, a: &Value) -> Result<Out> {
        let v = self.vault();
        if !self.mgr.has_session() {
            return Ok(Out::text("No playtest session to finish."));
        }
        let early = self.mgr.abort_reason();
        let (budget, elapsed0) = {
            let cur = self.mgr.cur.lock().unwrap();
            let s = cur.as_ref().unwrap();
            (s.budget_s, s.elapsed())
        };
        let status = if early.is_none() || elapsed0 > budget * 0.5 {
            "completed"
        } else {
            "partial"
        };
        let report = arg_str(a, "report");
        let one = arg_str(a, "one_liner");
        let summary: String = if one.is_empty() {
            report.chars().take(200).collect()
        } else {
            one.to_string()
        };
        let (note, game_note, uid, elapsed, shots) = self
            .mgr
            .end(status, &summary)
            .ok_or_else(|| anyhow!("no session"))?;
        v.set_block_in(
            &note,
            "ai",
            "report",
            &format!("## 🤖 Final Report\n{}", report.trim()),
        )?;
        let secs = arg_map(a, "sections").unwrap_or_default();
        let mut warns = vec![];
        let target = match &game_note {
            Some(g) if g.exists() => Some(g.clone()),
            _ => self.note_for(uid).ok(),
        };
        if let Some(t) = &target {
            if !secs.is_empty() || arg_map(a, "scores").is_some() {
                warns = v.write_ai_sections(
                    t,
                    &secs,
                    arg_map(a, "scores").as_ref(),
                    Some(one),
                    arg_tags(a).as_deref(),
                    "playtested",
                )?;
            }
            if status == "completed" {
                v.set_fm_value(t, "last_playtest", json!(today()))?;
            }
        }
        let missing: Vec<&str> = ai_keys()
            .into_iter()
            .filter(|k| !secs.contains_key(*k))
            .collect();
        let mut out = format!(
            "✅ Playtest saved ({status}). Log: {} · Game note: {} · {shots} screenshots · {} min.",
            v.rel(&note),
            target.as_ref().map(|t| v.rel(t)).unwrap_or("-".into()),
            (elapsed / 60.0) as u64
        );
        if let Some(e) = early {
            if !e.starts_with("session ") {
                out.push_str(&format!("\nSession had ended early: {e}."));
            }
        }
        if !missing.is_empty() {
            out.push_str(&format!("\nSections not provided (kept as before): {}. Use vault_write_game_analysis to add them.", missing.join(", ")));
        }
        if !warns.is_empty() {
            out.push_str(&format!("\nWarnings: {}", warns.join("; ")));
        }
        self.mgr.clear();
        Ok(Out::text(out))
    }

    pub fn get_prompt(&self, name: &str, args: &Value) -> Result<Value> {
        let text = match name {
            "playtest_game" => {
                let uid = arg_u64(args, "universe_id").ok_or_else(|| anyhow!("universe_id required"))?;
                let g = self.ctx.store.game(uid);
                playbook_text(&uid.to_string(), &g.as_ref().map(|g| g.name.clone()).unwrap_or(uid.to_string()), &g.and_then(|g| g.best_rank).map(|r| r.to_string()).unwrap_or("?".into()))
                    + "\n\nStart now with game_info, then vault_read_game, then playtest_start."
            }
            "learn_game" => {
                let uid = arg_u64(args, "universe_id").ok_or_else(|| anyhow!("universe_id required"))?;
                let g = self.ctx.store.game(uid);
                learn_text(&self.ctx, &uid.to_string(), &g.as_ref().map(|g| g.name.clone()).unwrap_or(uid.to_string()), &g.and_then(|g| g.best_rank).map(|r| r.to_string()).unwrap_or("?".into()))
                    + "\n\nStart now with game_info."
            }
            "weekly_run" => "Run the ROX-AI weekly cycle:\n1. scan_charts()\n2. playtest_queue() and playtest every listed game one after another, following playbook() exactly (always playtest_finish each).\n3. Then write a weekly market report with vault_write_insight (title 'Weekly Market Report'): genre shifts, rising mechanics, monetization patterns, best opportunities to build, with [[links]] to game notes; and update vault_write_genre_patterns for the top 3 genres.".into(),
            "market_report" => "Read '00 Dashboard.md' and the latest notes in '01 Charts' (vault_read_note / vault_list), use game_info for the top movers, then write a detailed report with vault_write_insight: what's trending and why, genre mix changes, monetization patterns, 5 concrete game ideas with reasoning, and content ideas.".into(),
            other => return Err(anyhow!("unknown prompt {other}")),
        };
        Ok(
            json!({"description": name, "messages": [{"role": "user", "content": {"type": "text", "text": text}}]}),
        )
    }

    /// Handle one JSON-RPC message; returns the response (None for notifications).
    pub fn handle(&self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        id.as_ref()?;
        let id = id.unwrap();
        let ok = |r: Value| Some(json!({"jsonrpc": "2.0", "id": id, "result": r}));
        let err = |code: i64, m: String| {
            Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": m}}))
        };
        match method {
            "initialize" => {
                let req = params
                    .get("protocolVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or(PROTOCOLS[1]);
                let ver = if PROTOCOLS.contains(&req) {
                    req
                } else {
                    PROTOCOLS[1]
                };
                ok(
                    json!({"protocolVersion": ver, "capabilities": {"tools": {"listChanged": false}, "prompts": {"listChanged": false}},
                          "serverInfo": {"name": "roxai", "version": crate::VERSION}, "instructions": INSTRUCTIONS}),
                )
            }
            "ping" => ok(json!({})),
            "tools/list" => {
                let pt = self.ctx.settings.playtesting_enabled;
                let mut tools: Vec<Value> = tool_defs()
                    .into_iter()
                    .filter(|t| pt || !PLAYTEST_TOOLS.contains(&t["name"].as_str().unwrap_or("")))
                    .collect();
                if crate::team_tools::agent_id().is_some() {
                    tools.extend(crate::team_tools::defs());
                }
                ok(json!({"tools": tools}))
            }
            "prompts/list" => ok(json!({"prompts": prompt_defs()})),
            "resources/list" => ok(json!({"resources": []})),
            "resources/templates/list" => ok(json!({"resourceTemplates": []})),
            "prompts/get" => match self.get_prompt(
                params.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                params.get("arguments").unwrap_or(&json!({})),
            ) {
                Ok(r) => ok(r),
                Err(e) => err(-32602, e.to_string()),
            },
            "tools/call" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let o = self.call(name, &args);
                ok(json!({"content": o.content, "isError": o.is_error}))
            }
            _ => err(-32601, format!("Method not found: {method}")),
        }
    }
}

fn walk_md(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            {
                continue;
            }
            if p.is_dir() {
                walk_md(&p, out);
            } else if p.extension().is_some_and(|x| x == "md") {
                out.push(p);
            }
        }
    }
}

/// Run the stdio server until stdin closes.
pub fn serve(ctx: Ctx) -> Result<()> {
    crate::win::window::make_dpi_aware();
    let api: Arc<dyn RobloxApi> = Arc::new(HttpApi::new()?);
    let server = Arc::new(Server::new(ctx.clone(), api.clone()));
    server.mgr.recover_stale();
    let _ = Vault::new(ctx.vault()).init(None);
    crate::info!("MCP server started (v{})", crate::VERSION);
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let stdin = std::io::stdin();
    let mut workers = vec![];
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_msg(
                    &out,
                    &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("Parse error: {e}")}}),
                );
                continue;
            }
        };
        let slow = msg.get("method").and_then(|m| m.as_str()) == Some("tools/call");
        if slow {
            // tool calls run on their own thread so pings & other requests keep flowing
            let (srv, out) = (server.clone(), out.clone());
            workers.push(std::thread::spawn(move || {
                if let Some(r) = srv.handle(&msg) {
                    write_msg(&out, &r);
                }
            }));
            workers.retain(|w| !w.is_finished());
        } else if let Some(r) = server.handle(&msg) {
            write_msg(&out, &r);
        }
    }
    for w in workers {
        let _ = w.join();
    }
    // stdin closed: make sure nothing is left running
    if server.mgr.has_session() {
        *server.mgr.abort.lock().unwrap() = Some("Claude disconnected".into());
        server.mgr.end("interrupted", "Claude disconnected");
    }
    Ok(())
}

fn write_msg(out: &Arc<Mutex<std::io::Stdout>>, v: &Value) {
    let mut o = out.lock().unwrap();
    let _ = writeln!(o, "{}", serde_json::to_string(v).unwrap_or_default());
    let _ = o.flush();
}
