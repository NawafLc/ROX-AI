//! The Playbook: what the AI team knows about making successful Roblox games.
//!
//! Eleven chapters. Each chapter note in `06 Playbook` has
//! - 📊 **Our numbers**: benchmarks ROX-AI computes itself from every tracked game (no AI),
//! - 🤖 **Rules**: written by Claude from our game notes, developer devlogs/postmortems and Roblox's docs — each
//!   with a confidence level (strong / emerging / opinion) and its evidence,
//! - 📚 **Sources** Claude read,
//! - ✍️ **My rules**: yours, never overwritten, and they always win over the rules above.
//!
//! Rules live in the store (structured, searchable) and are rendered into the notes.

use crate::config::Ctx;
use crate::util::{days_since, now_iso};
use crate::vault::{build_frontmatter, block, md_escape, split_frontmatter, table, Vault, DIR_GAMES, DIR_PLAYBOOK};
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;

pub const MY_RULES: &str = "## ✍️ My rules (always win)";
const STORE_KEY: &str = "playbook";

pub struct Chapter {
    pub id: &'static str,
    pub title: &'static str,
    pub emoji: &'static str,
    pub about: &'static str,
    /// what to look for in devlogs, docs and game notes
    pub focus: &'static str,
}

pub const CHAPTERS: [Chapter; 11] = [
    Chapter { id: "ideas", title: "Picking ideas", emoji: "💡", about: "Which game ideas win on Roblox right now, and which to avoid.",
        focus: "what makes an idea take off (hooks, trends, timing, 'brainrot'/meme waves, copy-and-improve), genre saturation vs opportunity, how big the first version should be, how developers validated ideas before building" },
    Chapter { id: "core-loops", title: "Core loops by genre", emoji: "🔁", about: "The loop that keeps players playing, for every genre.",
        focus: "the core loop of each genre (simulator, tycoon, obby, tower defense, RPG, horror, survival, shooter, roleplay, sports, party), loop length, reward pacing, what top games in each genre share" },
    Chapter { id: "first-5-minutes", title: "The first 5 minutes", emoji: "⏱️", about: "Spawn, tutorial and the first session: where most players are won or lost.",
        focus: "spawn location, time to first reward, tutorials vs learn-by-doing, first-session goals, early drop-off, onboarding mistakes devs fixed after launch, D1 retention lessons" },
    Chapter { id: "progression-economy", title: "Progression & economy", emoji: "📈", about: "Currencies, upgrades, rebirths and how fast players should progress.",
        focus: "currency design, upgrade curves, rebirth/prestige, offline progress, inflation, sinks and sources, pacing numbers that worked, economy bugs and fixes" },
    Chapter { id: "monetization", title: "Monetization & pricing", emoji: "💰", about: "Game passes, developer products and prices that sell without driving players away.",
        focus: "which game passes sell best, price points in Robux, developer product ladders, starter packs, limited offers, premium payouts, ethical monetization, what made revenue jump in devlogs" },
    Chapter { id: "retention", title: "Retention", emoji: "🧲", about: "What brings players back tomorrow, next week and next month.",
        focus: "daily rewards, streaks, quests, events, limited-time content, update cadence, notifications, favorites, D1/D7/D30 numbers devs shared and what moved them" },
    Chapter { id: "ui-ux", title: "UI & UX", emoji: "🖼️", about: "HUD, menus and buttons that work on phone, tablet, PC and console.",
        focus: "mobile-first layout, button size, HUD clutter, shop UX, popups, readability, controller support, UI patterns top games share, UI mistakes devs fixed" },
    Chapter { id: "social", title: "Social & multiplayer", emoji: "👥", about: "Playing with friends, trading, competition and the server as a community.",
        focus: "friend invites, co-op vs competitive, trading, leaderboards, server size, social spaces, clans, chat, how social features drove growth" },
    Chapter { id: "technical", title: "Technical", emoji: "🛠️", about: "Data saving, performance and anti-exploit: the things that must never break.",
        focus: "DataStore best practices (session locking, ProfileStore-style patterns, retries), performance on low-end phones, streaming, server authority, RemoteEvent validation, anti-exploit, common launch-breaking bugs" },
    Chapter { id: "launch-liveops", title: "Launch & live ops", emoji: "🚀", about: "Launching, the first weeks, and keeping a game alive with updates.",
        focus: "soft launch, testing with small groups, first-week checklist, update cadence of top games, events, what to fix first, reading analytics, when to keep going or move on" },
    Chapter { id: "marketing", title: "Marketing", emoji: "📣", about: "Thumbnails, titles, sponsored ads, YouTube/TikTok and getting the first players.",
        focus: "icons and thumbnails that get clicks, titles and descriptions, sponsored/ads budgets, TikTok and YouTube Shorts, creators/influencers, codes, how the first 1,000 players were found" },
];

/// Which chapters each agent reads (the rest it can still search).
pub const AGENT_CHAPTERS: [(&str, &[&str]); 7] = [
    ("lead", &["ideas", "core-loops", "launch-liveops", "monetization"]),
    ("designer", &["core-loops", "first-5-minutes", "progression-economy", "retention", "social"]),
    ("economy", &["progression-economy", "monetization"]),
    ("builder", &["technical", "core-loops"]),
    ("ui", &["ui-ux", "first-5-minutes"]),
    ("tester", &["technical", "first-5-minutes", "ui-ux"]),
    ("promo", &["marketing"]),
];

pub fn chapter(id_or_title: &str) -> Option<&'static Chapter> {
    let k = id_or_title.trim().to_lowercase();
    CHAPTERS.iter().find(|c| c.id == k || c.title.to_lowercase() == k || k.contains(c.id))
}

pub fn chapters_for(agent: &str) -> Vec<&'static str> {
    AGENT_CHAPTERS.iter().find(|(a, _)| *a == agent).map(|(_, c)| c.to_vec()).unwrap_or_default()
}

// ------------------------------------------------------------------ state

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Rule {
    pub id: String,
    pub text: String,
    /// strong · emerging · opinion
    pub confidence: String,
    pub why: String,
    /// game names (become [[links]]) or URLs
    pub evidence: Vec<String>,
    pub updated: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Source {
    pub title: String,
    pub url: String,
    /// devlog · postmortem · docs · devforum · video · article
    pub kind: String,
    pub summary: String,
    pub chapter: String,
    pub read_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ChapterState {
    pub rules: Vec<Rule>,
    pub summary: String,
    pub updated: Option<String>,
    pub runs: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct State {
    pub chapters: BTreeMap<String, ChapterState>,
    pub sources: Vec<Source>,
}

pub fn load(ctx: &Ctx) -> State {
    ctx.store.get(STORE_KEY).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

fn save(ctx: &Ctx, st: &State) -> Result<()> {
    ctx.store.set(STORE_KEY, serde_json::to_value(st)?)
}

fn norm_conf(c: &str) -> String {
    match c.trim().to_lowercase().as_str() {
        "strong" | "high" => "strong",
        "emerging" | "medium" | "moderate" => "emerging",
        _ => "opinion",
    }
    .to_string()
}

/// Replace (or extend) a chapter's rules. Returns how many rules the chapter now has.
pub fn write_rules(ctx: &Ctx, chapter_id: &str, rules: &[Value], summary: &str, replace: bool) -> Result<usize> {
    let ch = chapter(chapter_id).ok_or_else(|| anyhow!("unknown chapter '{chapter_id}' — use one of: {}", ids().join(", ")))?;
    let mut st = load(ctx);
    let cs = st.chapters.entry(ch.id.to_string()).or_default();
    if replace {
        cs.rules.clear();
    }
    let now = now_iso();
    for r in rules {
        let text = r["text"].as_str().or_else(|| r.as_str()).unwrap_or("").trim().to_string();
        if text.is_empty() {
            continue;
        }
        let evidence = r["evidence"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default();
        let rule = Rule {
            id: String::new(),
            text,
            confidence: norm_conf(r["confidence"].as_str().unwrap_or("opinion")),
            why: r["why"].as_str().unwrap_or("").trim().to_string(),
            evidence,
            updated: now.clone(),
        };
        if let Some(old) = cs.rules.iter_mut().find(|o| o.text.eq_ignore_ascii_case(&rule.text)) {
            *old = Rule { id: old.id.clone(), ..rule };
        } else {
            cs.rules.push(rule);
        }
    }
    for (i, r) in cs.rules.iter_mut().enumerate() {
        r.id = format!("{}-{}", ch.id, i + 1);
    }
    if !summary.trim().is_empty() {
        cs.summary = summary.trim().to_string();
    }
    cs.updated = Some(now);
    cs.runs += 1;
    let n = cs.rules.len();
    save(ctx, &st)?;
    render_all(ctx)?;
    Ok(n)
}

/// Remember sources that were read (so later runs find new ones). Returns how many were new.
pub fn add_sources(ctx: &Ctx, chapter_id: &str, sources: &[Value]) -> Result<usize> {
    let ch = chapter(chapter_id).map(|c| c.id).unwrap_or("general");
    let mut st = load(ctx);
    let mut n = 0;
    for s in sources {
        let url = s["url"].as_str().unwrap_or("").trim().to_string();
        if url.is_empty() {
            continue;
        }
        if let Some(old) = st.sources.iter_mut().find(|o| o.url == url) {
            if !s["summary"].as_str().unwrap_or("").is_empty() {
                old.summary = s["summary"].as_str().unwrap_or("").to_string();
            }
            continue;
        }
        st.sources.push(Source {
            title: s["title"].as_str().unwrap_or(&url).trim().to_string(),
            url,
            kind: s["kind"].as_str().unwrap_or("article").trim().to_lowercase(),
            summary: s["summary"].as_str().unwrap_or("").trim().to_string(),
            chapter: ch.to_string(),
            read_at: now_iso(),
        });
        n += 1;
    }
    save(ctx, &st)?;
    render_all(ctx)?;
    Ok(n)
}

pub fn ids() -> Vec<&'static str> {
    CHAPTERS.iter().map(|c| c.id).collect()
}

// ------------------------------------------------------------ benchmarks

/// Public facts about one tracked game, read from its note's frontmatter.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub name: String,
    pub genre: String,
    pub on_chart: bool,
    pub playing: f64,
    pub like: f64,
    pub visits: f64,
    pub favorites: f64,
    pub passes: f64,
    pub pass_median: f64,
    pub pass_max: f64,
    pub products: f64,
    pub buy_all: f64,
    pub server: f64,
    pub age_days: f64,
    pub since_update: f64,
}

pub fn facts(ctx: &Ctx) -> Vec<Facts> {
    let v = Vault::new(ctx.vault());
    let chart: std::collections::HashSet<u64> = ctx
        .store
        .latest_day(&ctx.settings.sort_id)
        .map(|d| ctx.store.chart_on(&d, &ctx.settings.sort_id).into_iter().map(|s| s.universe_id).collect())
        .unwrap_or_default();
    ctx.store
        .games()
        .into_values()
        .filter_map(|g| {
            let raw = split_frontmatter(&v.read(&v.abs(g.note_path.as_ref()?))).0;
            let n = |k: &str| raw.iter().find(|(rk, _)| rk == k).and_then(|(_, x)| x.trim().trim_matches('"').parse::<f64>().ok()).unwrap_or(f64::NAN);
            Some(Facts {
                name: crate::vault::clean_display_name(&g.name),
                genre: if g.genre.is_empty() { "Other".into() } else { g.genre.clone() },
                on_chart: chart.contains(&g.universe_id),
                playing: n("playing"),
                like: n("like_ratio"),
                visits: n("visits"),
                favorites: n("favorites"),
                passes: n("gamepass_count"),
                pass_median: n("gamepass_median"),
                pass_max: n("gamepass_max"),
                products: n("devproduct_count"),
                buy_all: n("buy_all_passes"),
                server: n("max_players"),
                age_days: n("game_age_days"),
                since_update: n("days_since_update"),
            })
        })
        .collect()
}

fn median(mut xs: Vec<f64>) -> Option<f64> {
    xs.retain(|x| x.is_finite());
    if xs.is_empty() {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let m = xs.len() / 2;
    Some(if xs.len() % 2 == 0 { (xs[m - 1] + xs[m]) / 2.0 } else { xs[m] })
}

fn med(fs: &[&Facts], f: impl Fn(&Facts) -> f64) -> Option<f64> {
    median(fs.iter().map(|x| f(x)).collect())
}

fn num(x: Option<f64>) -> String {
    match x {
        None => "–".into(),
        Some(v) if v >= 1e6 => format!("{:.1}M", v / 1e6),
        Some(v) if v >= 1e4 => format!("{:.0}K", v / 1e3),
        Some(v) if v >= 1e3 => format!("{:.1}K", v / 1e3),
        Some(v) if v.fract().abs() < 0.05 => format!("{v:.0}"),
        Some(v) => format!("{v:.1}"),
    }
}
fn robux(x: Option<f64>) -> String {
    x.map(|v| format!("R$ {v:.0}")).unwrap_or("–".into())
}

/// Genres with at least `min` games, biggest (by players) first.
fn by_genre<'a>(fs: &[&'a Facts], min: usize) -> Vec<(String, Vec<&'a Facts>)> {
    let mut m: BTreeMap<String, Vec<&'a Facts>> = BTreeMap::new();
    for &f in fs {
        m.entry(f.genre.clone()).or_default().push(f);
    }
    let mut v: Vec<(String, Vec<&Facts>)> = m.into_iter().filter(|(_, g)| g.len() >= min).collect();
    v.sort_by(|a, b| {
        let pa: f64 = a.1.iter().map(|f| f.playing.max(0.0)).filter(|x| x.is_finite()).sum();
        let pb: f64 = b.1.iter().map(|f| f.playing.max(0.0)).filter(|x| x.is_finite()).sum();
        pb.partial_cmp(&pa).unwrap()
    });
    v
}

/// Our own numbers for one chapter (markdown), or None when the chapter has no numeric benchmarks.
pub fn benchmarks_md(chapter_id: &str, fs: &[Facts]) -> Option<String> {
    if fs.is_empty() {
        return Some("_No games tracked yet — numbers appear after the first chart scan._".into());
    }
    let all: Vec<&Facts> = fs.iter().collect();
    let chart: Vec<&Facts> = fs.iter().filter(|f| f.on_chart).collect();
    let base = if chart.len() >= 5 { &chart } else { &all };
    let scope = format!("_{} games ({}), refreshed with every chart scan._", base.len(), if chart.len() >= 5 { "on today's chart" } else { "all tracked" });
    let genres = by_genre(base, 2);
    let total_players: f64 = base.iter().map(|f| f.playing).filter(|x| x.is_finite()).sum::<f64>().max(1.0);
    Some(match chapter_id {
        "ideas" => {
            let rows: Vec<Vec<String>> = genres
                .iter()
                .map(|(g, xs)| {
                    let p: f64 = xs.iter().map(|f| f.playing).filter(|x| x.is_finite()).sum();
                    vec![
                        md_escape(g),
                        xs.len().to_string(),
                        format!("{:.0}%", p / total_players * 100.0),
                        num(med(xs, |f| f.playing)),
                        num(med(xs, |f| f.like)),
                        num(med(xs, |f| f.age_days)),
                    ]
                })
                .collect();
            let young = all.iter().filter(|f| f.age_days.is_finite() && f.age_days <= 60.0).count();
            format!(
                "{scope}\n\n{}\n\n- **{young} of {}** tracked games are less than 60 days old — new games still break into the charts.\n- Median age of a chart game: **{} days**.",
                table(&["Genre", "Games", "Share of players", "Median playing", "Median like %", "Median age (days)"], &rows),
                all.len(),
                num(med(base, |f| f.age_days))
            )
        }
        "monetization" => {
            let mut rows = vec![vec![
                "**All**".to_string(),
                base.len().to_string(),
                num(med(base, |f| f.passes)),
                robux(med(base, |f| f.pass_median)),
                robux(med(base, |f| f.pass_max)),
                num(med(base, |f| f.products)),
                robux(med(base, |f| f.buy_all)),
            ]];
            for (g, xs) in &genres {
                rows.push(vec![
                    md_escape(g),
                    xs.len().to_string(),
                    num(med(xs, |f| f.passes)),
                    robux(med(xs, |f| f.pass_median)),
                    robux(med(xs, |f| f.pass_max)),
                    num(med(xs, |f| f.products)),
                    robux(med(xs, |f| f.buy_all)),
                ]);
            }
            format!(
                "{scope}\n\n{}",
                table(&["Genre", "Games", "Game passes", "Typical pass price", "Most expensive pass", "Developer products", "Cost of every pass"], &rows)
            )
        }
        "retention" => {
            let rows: Vec<Vec<String>> = genres
                .iter()
                .map(|(g, xs)| vec![md_escape(g), xs.len().to_string(), num(med(xs, |f| if f.visits > 0.0 { f.favorites / f.visits * 1000.0 } else { f64::NAN })), num(med(xs, |f| f.like))])
                .collect();
            format!(
                "{scope}\n\nFavorites per 1,000 visits is a rough sign of how many visitors want to come back.\n\n{}\n\nAll games: **{}** favorites per 1,000 visits.",
                table(&["Genre", "Games", "Favorites / 1K visits", "Like %"], &rows),
                num(med(base, |f| if f.visits > 0.0 { f.favorites / f.visits * 1000.0 } else { f64::NAN }))
            )
        }
        "social" | "technical" => {
            let rows: Vec<Vec<String>> = genres.iter().map(|(g, xs)| vec![md_escape(g), xs.len().to_string(), num(med(xs, |f| f.server))]).collect();
            format!("{scope}\n\nServer size (max players per server) by genre:\n\n{}\n\nAll games: **{}** players per server.", table(&["Genre", "Games", "Median server size"], &rows), num(med(base, |f| f.server)))
        }
        "launch-liveops" => {
            let within = |d: f64| base.iter().filter(|f| f.since_update.is_finite() && f.since_update <= d).count();
            format!(
                "{scope}\n\n- Median days since the last update: **{}**\n- Updated in the last 7 days: **{} of {}**\n- Updated in the last 30 days: **{} of {}**\n- Median game age: **{} days**",
                num(med(base, |f| f.since_update)),
                within(7.0),
                base.len(),
                within(30.0),
                base.len(),
                num(med(base, |f| f.age_days))
            )
        }
        _ => return None,
    })
}

/// All benchmarks as data (for agents): overall and per genre.
pub fn benchmarks_json(fs: &[Facts], genre: Option<&str>) -> Value {
    let pack = |xs: &[&Facts]| {
        json!({
            "games": xs.len(),
            "median_playing": med(xs, |f| f.playing), "median_like_pct": med(xs, |f| f.like),
            "median_game_passes": med(xs, |f| f.passes), "median_pass_price_robux": med(xs, |f| f.pass_median),
            "median_max_pass_price_robux": med(xs, |f| f.pass_max), "median_developer_products": med(xs, |f| f.products),
            "median_cost_of_all_passes_robux": med(xs, |f| f.buy_all), "median_server_size": med(xs, |f| f.server),
            "median_game_age_days": med(xs, |f| f.age_days), "median_days_since_update": med(xs, |f| f.since_update),
            "median_favorites_per_1k_visits": med(xs, |f| if f.visits > 0.0 { f.favorites / f.visits * 1000.0 } else { f64::NAN }),
        })
    };
    let all: Vec<&Facts> = fs.iter().collect();
    let mut out = json!({"all": pack(&all), "genres": {}});
    for (g, xs) in by_genre(&all, 1) {
        if genre.is_none_or(|q| g.eq_ignore_ascii_case(q)) {
            out["genres"][g] = pack(&xs);
        }
    }
    out
}

// ------------------------------------------------------------- rendering

fn file_name(c: &Chapter) -> String {
    let i = CHAPTERS.iter().position(|x| x.id == c.id).unwrap_or(0) + 1;
    format!("{i:02} {}.md", c.title.replace('&', "and"))
}

pub fn note_rel(c: &Chapter) -> String {
    format!("{DIR_PLAYBOOK}/{}", file_name(c))
}

fn evidence_md(e: &str, games: &std::collections::HashSet<String>) -> String {
    if e.starts_with("http://") || e.starts_with("https://") {
        let host = e.split('/').nth(2).unwrap_or("link").trim_start_matches("www.");
        format!("[{host}]({e})")
    } else {
        let name = e.trim_start_matches("[[").trim_end_matches("]]").trim_start_matches("02 Games/").to_string();
        let file = crate::vault::safe_filename(&name);
        if games.contains(&file.to_lowercase()) {
            format!("[[{DIR_GAMES}/{file}\\|{name}]]")
        } else {
            name
        }
    }
}

fn rules_md(cs: Option<&ChapterState>, games: &std::collections::HashSet<String>) -> String {
    let Some(cs) = cs.filter(|c| !c.rules.is_empty()) else {
        return "## 🤖 Rules\n> [!todo] Not written yet\n> Claude writes this chapter from your game notes, developer devlogs and Roblox's docs during a learning run.".into();
    };
    let mut out = String::from("## 🤖 Rules\n");
    if !cs.summary.is_empty() {
        out.push_str(&format!("> [!summary] In short\n> {}\n\n", cs.summary.replace('\n', "\n> ")));
    }
    for (conf, label, hint) in [("strong", "Strong", "seen again and again, in data and from developers"), ("emerging", "Emerging", "good signs, not proven yet"), ("opinion", "Opinion", "advice from developers, unproven")] {
        let rs: Vec<&Rule> = cs.rules.iter().filter(|r| r.confidence == conf).collect();
        if rs.is_empty() {
            continue;
        }
        out.push_str(&format!("\n### {label}\n_{hint}_\n\n"));
        for r in rs {
            out.push_str(&format!("- **{}**", r.text.trim_end_matches('.')));
            if !r.why.is_empty() {
                out.push_str(&format!(" — {}", r.why));
            }
            if !r.evidence.is_empty() {
                let ev: Vec<String> = r.evidence.iter().map(|e| evidence_md(e, games)).collect();
                out.push_str(&format!(" <sub>Evidence: {}</sub>", ev.join(", ")));
            }
            out.push('\n');
        }
    }
    out
}

fn sources_md(st: &State, id: &str) -> String {
    let src: Vec<&Source> = st.sources.iter().filter(|s| s.chapter == id).collect();
    if src.is_empty() {
        return "## 📚 Sources\n_None yet._".into();
    }
    let rows: Vec<Vec<String>> = src
        .iter()
        .rev()
        .take(40)
        .map(|s| vec![format!("[{}]({})", md_escape(&s.title).replace(['[', ']'], ""), s.url), s.kind.clone(), md_escape(&s.summary.chars().take(140).collect::<String>()), s.read_at.chars().take(10).collect()])
        .collect();
    format!("## 📚 Sources\n{}", table(&["Source", "Kind", "What it taught", "Read"], &rows))
}

/// Your own rules for a chapter (text under "My rules"), if any.
pub fn my_rules(text: &str) -> String {
    text.split_once(MY_RULES).map(|(_, b)| b.trim().trim_start_matches("- ").trim().to_string()).filter(|s| !s.is_empty() && s != "-").unwrap_or_default()
}

/// Write every chapter note and the playbook index (keeps your "My rules").
pub fn render_all(ctx: &Ctx) -> Result<()> {
    let v = Vault::new(ctx.vault());
    v.dir(DIR_PLAYBOOK);
    let st = load(ctx);
    let fs = facts(ctx);
    let games: std::collections::HashSet<String> = std::fs::read_dir(v.root.join(DIR_GAMES))
        .map(|rd| rd.flatten().filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().to_lowercase())).collect())
        .unwrap_or_default();
    let mut index_rows = vec![];
    for c in &CHAPTERS {
        let p = v.abs(&note_rel(c));
        let old = v.read(&p);
        let mine = old.split_once(MY_RULES).map(|(_, b)| b.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "- ".into());
        let cs = st.chapters.get(c.id);
        let count = |k: &str| cs.map(|c| c.rules.iter().filter(|r| r.confidence == k).count()).unwrap_or(0);
        let fm = build_frontmatter(
            &[
                ("type", json!("playbook-chapter")),
                ("chapter", json!(c.id)),
                ("rules", json!(cs.map(|c| c.rules.len()).unwrap_or(0))),
                ("strong", json!(count("strong"))),
                ("emerging", json!(count("emerging"))),
                ("opinion", json!(count("opinion"))),
                ("updated", json!(cs.and_then(|c| c.updated.clone()).map(|u| u.chars().take(10).collect::<String>()).unwrap_or_default())),
                ("tags", json!(["playbook"])),
            ],
            &vec![],
        );
        let mut body = format!(
            "{fm}# {} {}\n\n> [!info] {}\n> 📊 numbers come from the games ROX-AI tracks · 🤖 rules are written by Claude from game notes, developer devlogs and Roblox's docs · ✍️ **My rules** are yours and always win.\n\n",
            c.emoji, c.title, c.about
        );
        if let Some(b) = benchmarks_md(c.id, &fs) {
            body.push_str(&block("auto", "benchmarks", &format!("## 📊 Our numbers\n{b}")));
            body.push_str("\n\n");
        }
        body.push_str(&block("ai", "rules", &rules_md(cs, &games)));
        body.push_str("\n\n");
        body.push_str(&block("ai", "sources", &sources_md(&st, c.id)));
        body.push_str(&format!("\n\n{MY_RULES}\n\n{mine}\n"));
        if body != old {
            v.write(&p, &body)?;
        }
        index_rows.push(vec![
            format!("[[{DIR_PLAYBOOK}/{}\\|{} {}]]", file_name(c).trim_end_matches(".md"), c.emoji, c.title),
            cs.map(|c| c.rules.len()).unwrap_or(0).to_string(),
            format!("{} / {} / {}", count("strong"), count("emerging"), count("opinion")),
            cs.and_then(|c| c.updated.clone()).map(|u| u.chars().take(10).collect()).unwrap_or("not yet".into()),
        ]);
    }
    let total: usize = st.chapters.values().map(|c| c.rules.len()).sum();
    let index = format!(
        "{}# 📘 The ROX-AI Playbook\n\nWhat your AI team knows about making successful Roblox games: **{total} rules** from {} sources, plus our own numbers from {} tracked games. Each agent reads the chapters for its job.\n\n{}\n\n**Confidence:** strong = seen again and again · emerging = promising · opinion = developer advice, unproven.\nWrite your own rules under **✍️ My rules** in any chapter — they always win.\n",
        build_frontmatter(&[("type", json!("playbook")), ("rules", json!(total))], &vec![]),
        st.sources.len(),
        fs.len(),
        table(&["Chapter", "Rules", "Strong / emerging / opinion", "Updated"], &index_rows)
    );
    let ip = v.abs(&format!("{DIR_PLAYBOOK}/00 Playbook.md"));
    if v.read(&ip) != index {
        v.write(&ip, &index)?;
    }
    Ok(())
}

/// A chapter as agents read it: your rules first, then the rules, then the numbers.
pub fn read_chapter(ctx: &Ctx, id: &str) -> Result<String> {
    let c = chapter(id).ok_or_else(|| anyhow!("unknown chapter '{id}' — chapters: {}", ids().join(", ")))?;
    let v = Vault::new(ctx.vault());
    let text = v.read(&v.abs(&note_rel(c)));
    let st = load(ctx);
    let cs = st.chapters.get(c.id);
    let mut out = format!("# {} {}\n{}\n\n", c.emoji, c.title, c.about);
    let mine = my_rules(&text);
    if !mine.is_empty() {
        out.push_str(&format!("## The owner's rules (ALWAYS follow these, they override everything below)\n{mine}\n\n"));
    }
    match cs.filter(|c| !c.rules.is_empty()) {
        Some(cs) => {
            if !cs.summary.is_empty() {
                out.push_str(&format!("Summary: {}\n\n", cs.summary));
            }
            out.push_str("## Rules\n");
            for r in &cs.rules {
                out.push_str(&format!("- [{}] ({}) {}{}{}\n", r.id, r.confidence, r.text, if r.why.is_empty() { String::new() } else { format!(" — {}", r.why) }, if r.evidence.is_empty() { String::new() } else { format!(" [evidence: {}]", r.evidence.join(", ")) }));
            }
        }
        None => out.push_str("## Rules\n(none written yet)\n"),
    }
    if let Some(b) = benchmarks_md(c.id, &facts(ctx)) {
        out.push_str(&format!("\n## Our numbers\n{b}\n"));
    }
    Ok(out)
}

/// Search rules, your rules and benchmarks. Returns (chapter, confidence, text).
pub fn search(ctx: &Ctx, query: &str, only: Option<&str>, max: usize) -> Vec<Value> {
    let words: Vec<String> = query.to_lowercase().split_whitespace().filter(|w| w.len() > 2).map(String::from).collect();
    let score = |t: &str| -> usize {
        let l = t.to_lowercase();
        if l.contains(&query.to_lowercase()) {
            return 100;
        }
        words.iter().filter(|w| l.contains(w.as_str())).count()
    };
    let st = load(ctx);
    let v = Vault::new(ctx.vault());
    let mut hits: Vec<(usize, Value)> = vec![];
    for c in CHAPTERS.iter().filter(|c| only.is_none_or(|o| chapter(o).is_some_and(|x| x.id == c.id))) {
        for line in my_rules(&v.read(&v.abs(&note_rel(c)))).lines() {
            let s = score(line);
            if s > 0 {
                hits.push((s + 50, json!({"chapter": c.id, "confidence": "owner (always wins)", "text": line.trim_start_matches("- ")})));
            }
        }
        if let Some(cs) = st.chapters.get(c.id) {
            for r in &cs.rules {
                let s = score(&format!("{} {} {}", r.text, r.why, r.evidence.join(" ")));
                if s > 0 {
                    let boost = match r.confidence.as_str() { "strong" => 3, "emerging" => 2, _ => 1 };
                    hits.push((s * 10 + boost, json!({"chapter": c.id, "id": r.id, "confidence": r.confidence, "text": r.text, "why": r.why, "evidence": r.evidence})));
                }
            }
        }
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0));
    hits.into_iter().take(max).map(|h| h.1).collect()
}

/// Overview of every chapter (for the app and for Claude).
pub fn status(ctx: &Ctx) -> Vec<Value> {
    let st = load(ctx);
    CHAPTERS
        .iter()
        .map(|c| {
            let cs = st.chapters.get(c.id);
            let count = |k: &str| cs.map(|c| c.rules.iter().filter(|r| r.confidence == k).count()).unwrap_or(0);
            json!({
                "id": c.id, "title": c.title, "emoji": c.emoji, "about": c.about, "note": note_rel(c),
                "rules": cs.map(|c| c.rules.len()).unwrap_or(0), "strong": count("strong"), "emerging": count("emerging"), "opinion": count("opinion"),
                "sources": st.sources.iter().filter(|s| s.chapter == c.id).count(),
                "updated": cs.and_then(|c| c.updated.clone()), "summary": cs.map(|c| c.summary.clone()).unwrap_or_default(),
            })
        })
        .collect()
}

// ------------------------------------------------------------- learning

/// Chapters that need (re)writing, most urgent first: never written, then oldest.
pub fn due(ctx: &Ctx) -> Vec<&'static Chapter> {
    let st = load(ctx);
    let mut d: Vec<(f64, usize, &Chapter)> = CHAPTERS
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let age = st.chapters.get(c.id).and_then(|s| s.updated.as_deref()).and_then(days_since);
            match age {
                None => Some((f64::MAX, i, c)),
                Some(a) if a >= ctx.settings.playbook_refresh_days => Some((a, i, c)),
                _ => None,
            }
        })
        .collect();
    d.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)));
    d.into_iter().map(|x| x.2).collect()
}

/// The instructions Claude follows to write one chapter.
pub fn guide(ctx: &Ctx, c: &Chapter) -> String {
    let st = load(ctx);
    let read: Vec<String> = st.sources.iter().filter(|s| s.chapter == c.id).map(|s| s.url.clone()).collect();
    crate::PLAYBOOK_GUIDE
        .replace("{chapter}", c.id)
        .replace("{title}", c.title)
        .replace("{about}", c.about)
        .replace("{focus}", c.focus)
        .replace("{read}", &if read.is_empty() { "none yet".into() } else { read.join("\n") })
        .replace("{chapters}", &ids().join(", "))
}

/// Rewrite up to `max` due chapters with Claude. The caller holds the learning lock.
pub fn learn_due(ctx: &Ctx, max: usize) -> Vec<Value> {
    due(ctx).into_iter().take(max).map(|c| learn_chapter(ctx, c)).collect()
}

pub fn learn_chapter(ctx: &Ctx, c: &Chapter) -> Value {
    if ctx.stop_file().exists() {
        return json!({"chapter": c.id, "status": "stopped by user"});
    }
    let _ = ctx.store.set("playbook_now", json!({"chapter": c.title, "at": now_iso()}));
    let before = load(ctx).chapters.get(c.id).and_then(|s| s.updated.clone());
    let prompt = format!(
        "Write the '{}' chapter of the ROX-AI Playbook. First call playbook_guide(chapter=\"{}\") and follow it exactly: read the chapter and our game notes, find and read developer devlogs/postmortems and Roblox docs, then save with playbook_write_rules and playbook_add_sources. Work autonomously; nobody will answer questions.",
        c.title, c.id
    );
    let r = crate::autopilot::run_claude(ctx, &prompt, Duration::from_secs(30 * 60), &format!("playbook-{}", c.id));
    let _ = ctx.store.set("playbook_now", Value::Null);
    let after = load(ctx).chapters.get(c.id).and_then(|s| s.updated.clone());
    let status = if after.is_some() && after != before { "written" } else { "not finished" };
    crate::info!("playbook {} → {status}", c.id);
    match r {
        Ok(r) => json!({"chapter": c.id, "status": status, "claude": {"returncode": r.returncode, "stopped": r.stopped, "turns": r.turns, "log": r.log, "usage_limit": r.usage_limit, "auth_problem": r.auth_problem}}),
        Err(e) => json!({"chapter": c.id, "status": "error", "error": e.to_string()}),
    }
}

/// `roxai playbook`: rewrite one chapter (or the most urgent ones) now.
pub fn run(ctx: &Ctx, chapter_id: Option<&str>, force: bool) -> Result<Value> {
    let _lock = crate::util::single_instance(&ctx.state_dir(), "autopilot").map_err(|_| anyhow!("ROX-AI is already learning — try again when it's done"))?;
    crate::session::clear_stop(ctx);
    let (ok, info) = crate::autopilot::claude_ready(ctx);
    if !ok {
        bail!("{info}");
    }
    let run_id = ctx.store.run_start("playbook").unwrap_or(0);
    let res = match chapter_id {
        Some(id) => {
            let c = chapter(id).ok_or_else(|| anyhow!("unknown chapter '{id}' — chapters: {}", ids().join(", ")))?;
            vec![learn_chapter(ctx, c)]
        }
        None if force => learn_due(ctx, ctx.settings.playbook_chapters_per_run.max(1)).into_iter().chain(if due(ctx).is_empty() { vec![learn_chapter(ctx, oldest(ctx))] } else { vec![] }).collect(),
        None => learn_due(ctx, ctx.settings.playbook_chapters_per_run.max(1)),
    };
    let v = json!({"chapters": res});
    let _ = ctx.store.run_end(run_id, "ok", &v.to_string());
    Ok(v)
}

fn oldest(ctx: &Ctx) -> &'static Chapter {
    let st = load(ctx);
    CHAPTERS
        .iter()
        .min_by(|a, b| {
            let ua = st.chapters.get(a.id).and_then(|s| s.updated.clone()).unwrap_or_default();
            let ub = st.chapters.get(b.id).and_then(|s| s.updated.clone()).unwrap_or_default();
            ua.cmp(&ub)
        })
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        let d = std::env::temp_dir().join(format!("roxai-pb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut c = Ctx::at(d.clone());
        c.settings.vault_path = d.join("vault").to_string_lossy().into_owned();
        c
    }

    #[test]
    fn rules_render_search_and_keep_my_rules() {
        let c = ctx();
        render_all(&c).unwrap();
        let v = Vault::new(c.vault());
        let p = v.abs(&note_rel(chapter("monetization").unwrap()));
        assert!(v.read(&p).contains("Not written yet"));
        // the owner writes a rule; it must survive every re-render
        let t = v.read(&p).replace(&format!("{MY_RULES}\n\n- "), &format!("{MY_RULES}\n\n- Never sell pay-to-win weapons"));
        v.write(&p, &t).unwrap();
        let n = write_rules(
            &c,
            "monetization",
            &[
                json!({"text": "Offer a 2x money pass around 199 Robux", "confidence": "strong", "why": "Most top simulators sell one", "evidence": ["Throw Food For Animals", "https://devforum.roblox.com/t/1"]}),
                json!({"text": "Starter pack under 100 Robux", "confidence": "medium"}),
            ],
            "Cheap, clear passes sell.",
            true,
        )
        .unwrap();
        assert_eq!(n, 2);
        let text = v.read(&p);
        assert!(text.contains("Offer a 2x money pass"));
        assert!(text.contains("### Strong") && text.contains("### Emerging"));
        assert!(text.contains("Never sell pay-to-win weapons"), "my rules lost: {text}");
        assert!(text.contains("[devforum.roblox.com](https://devforum.roblox.com/t/1)"));
        // agents see the owner's rule first
        let r = read_chapter(&c, "monetization").unwrap();
        assert!(r.find("Never sell pay-to-win").unwrap() < r.find("2x money pass").unwrap());
        // search finds both kinds
        let hits = search(&c, "pass robux", None, 10);
        assert!(hits.iter().any(|h| h["id"] == "monetization-1"));
        let hits = search(&c, "pay-to-win", None, 10);
        assert!(hits[0]["confidence"].as_str().unwrap().starts_with("owner"));
        // the chapter is no longer due, the others are
        assert!(!due(&c).iter().any(|ch| ch.id == "monetization"));
        assert_eq!(due(&c).len(), CHAPTERS.len() - 1);
        // sources dedupe
        assert_eq!(add_sources(&c, "monetization", &[json!({"title": "A devlog", "url": "https://youtu.be/x", "kind": "devlog"})]).unwrap(), 1);
        assert_eq!(add_sources(&c, "monetization", &[json!({"title": "A devlog", "url": "https://youtu.be/x"})]).unwrap(), 0);
        assert!(v.read(&p).contains("A devlog"));
        assert!(v.read(&v.abs(&format!("{DIR_PLAYBOOK}/00 Playbook.md"))).contains("2 rules"));
    }

    #[test]
    fn benchmarks_from_facts() {
        let f = |g: &str, p: f64, price: f64| Facts { name: g.into(), genre: g.into(), on_chart: true, playing: p, like: 90.0, passes: 6.0, pass_median: price, pass_max: price * 2.0, products: 10.0, buy_all: price * 6.0, server: 12.0, age_days: 30.0, since_update: 3.0, visits: 1e6, favorites: 5e3, ..Default::default() };
        let fs = vec![f("Simulation", 1000.0, 199.0), f("Simulation", 3000.0, 299.0), f("Action", 500.0, 99.0), f("Action", 700.0, 149.0), f("RPG", 100.0, 49.0)];
        let m = benchmarks_md("monetization", &fs).unwrap();
        assert!(m.contains("Simulation") && m.contains("R$ 249"), "{m}");
        let i = benchmarks_md("ideas", &fs).unwrap();
        assert!(i.contains("Share of players"));
        assert!(benchmarks_md("marketing", &fs).is_none());
        let j = benchmarks_json(&fs, Some("action"));
        assert_eq!(j["genres"]["Action"]["games"], 2);
        assert!(j["genres"].get("Simulation").is_none());
    }
}
