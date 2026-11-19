//! Obsidian vault builder.
//!
//! Every game note is a fixed *template* made of marked blocks:
//!   `<!-- roxai:auto:KEY -->` … `<!-- /roxai:auto:KEY -->`  rewritten by the daily scan (data)
//!   `<!-- roxai:ai:KEY -->`   … `<!-- /roxai:ai:KEY -->`    written by Claude after playtests / research
//!   everything under "## ✍️ My Notes" is yours and is never touched.
//! Blocks are replaced in place, so re-running anything is always safe (idempotent).

use crate::analysis::{self as A, Trend};
use crate::api::{GameDetails, RobloxApi};
use crate::store::{Playtest, Snap};
use crate::util::{atomic_write, today};
use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

pub const MY_NOTES: &str = "## ✍️ My Notes";

pub const DIR_CHARTS: &str = "01 Charts";
pub const DIR_GAMES: &str = "02 Games";
pub const DIR_PLAYTESTS: &str = "03 Playtests";
pub const DIR_GENRES: &str = "04 Genres";
pub const DIR_INSIGHTS: &str = "05 Insights";
pub const DIR_TEMPLATES: &str = "Templates";
pub const DIR_ATTACH: &str = "Attachments";
pub const DIR_PLAYBOOK: &str = "06 Playbook";
pub const ALL_DIRS: [&str; 8] = [
    DIR_CHARTS,
    DIR_GAMES,
    DIR_PLAYTESTS,
    DIR_GENRES,
    DIR_INSIGHTS,
    DIR_PLAYBOOK,
    DIR_TEMPLATES,
    DIR_ATTACH,
];

/// key, heading, what Claude should put there (also the empty placeholder hint)
pub const AI_SECTIONS: [(&str, &str, &str); 14] = [
    ("summary", "🎯 Summary & Why It's Trending", "Elevator pitch in 2-3 sentences, target audience, the *hook* that makes it spread, and why it is on the chart right now."),
    ("core_loop", "🔁 Core Gameplay Loop", "The loop as numbered steps (e.g. collect → sell → upgrade → unlock area → repeat), typical session length, what the player is optimizing."),
    ("ftue", "🚪 First 5 Minutes (Onboarding / FTUE)", "Exactly what happens from spawn: tutorial, arrows, first reward, first purchase prompt, how fast the first 'wow' moment arrives."),
    ("systems", "⚙️ Systems & Mechanics", "Every system observed: controls, tools, pets/units, combat, building, trading, rebirth/prestige, quests, zones, events. Bullet list with short notes."),
    ("progression", "📶 Progression & Economy", "Currencies, sources and sinks, upgrade curve, rebirth/prestige, zone gates, how long each tier takes, where the grind walls are."),
    ("ui_ux", "🖥️ UI / UX Layout", "HUD layout (what is on left/right/top/bottom), main buttons, shop placement, popups, readability on mobile, juice/feedback."),
    ("social", "👥 Social & Multiplayer", "Server size, co-op/competition, trading, leaderboards, friend invites, group/community rewards, chat usage."),
    ("retention", "🧲 Retention Hooks", "Daily rewards, streaks, timed chests, codes, limited events, offline earnings, update cadence, notifications, favorites/like prompts."),
    ("monetization", "💰 Monetization Analysis", "How and when it asks for Robux, best-value offers, pricing psychology, aggressiveness 1-10, which passes are likely best sellers and why."),
    ("art_audio", "🎨 Art, Audio & Game Feel", "Visual style, colors, camera, VFX/SFX, music, polish level, performance."),
    ("content_angles", "🎬 Content Angles", "Video hooks, 5 title ideas, thumbnail concepts, Shorts/TikTok moments, secrets/easter eggs, codes worth covering."),
    ("blueprint", "🛠️ Build Blueprint (Roblox Studio)", "How to build a game like this: MVP scope, ServerScriptService/ReplicatedStorage module list, DataStore schema, RemoteEvents, key Luau systems, asset list, effort estimate (days), step-by-step build order."),
    ("improve", "🚀 How To Beat It", "Weaknesses, player complaints, missing features, and concrete differentiation ideas for a competing game."),
    ("verdict", "🧾 Verdict & Scores", "Scores 1-10: Fun, Clarity, Retention, Monetization aggressiveness, Polish, Clone difficulty, Trend durability — each with one line of reasoning. Then a final verdict."),
];

pub fn ai_keys() -> Vec<&'static str> {
    AI_SECTIONS.iter().map(|s| s.0).collect()
}

/// One game row of a chart scan (also used by the dashboard, genre notes and MCP output).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChartRow {
    pub universe_id: u64,
    pub rank: u32,
    pub title: String,
    pub genre: String,
    pub subgenre: String,
    pub playing: u64,
    pub like_ratio: f64,
    pub note: Option<String>,
    pub passes: usize,
    pub last_playtest: String,
    pub rank_change_1d: Option<i64>,
    pub ccu_change_1d_pct: Option<f64>,
    pub visits_per_day: Option<u64>,
    pub momentum: u32,
    pub flags: Vec<String>,
    pub days_tracked: usize,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QueueItem {
    pub universe_id: u64,
    pub place_id: u64,
    pub title: String,
    pub rank: u32,
    pub reason: String,
    pub note: Option<String>,
}

// ------------------------------------------------------------ helpers ---

pub fn clean_display_name(name: &str) -> String {
    // strip chart noise like "[🎃 UPDATE]" or "(NEW!)"
    let re = Regex::new(r"[\[\(【][^\]\)】]{0,30}[\]\)】]").unwrap();
    let n = re.replace_all(name, " ");
    let kept: String = n
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || "-'&!.,:;?\"#%*@_/".contains(*c))
        .collect();
    let ws = Regex::new(r"\s+").unwrap();
    let out = ws
        .replace_all(&kept, " ")
        .trim()
        .trim_matches(|c| c == ' ' || c == '-' || c == '|' || c == '!')
        .to_string();
    if out.is_empty() {
        let t = name.trim();
        if t.is_empty() {
            "Untitled".into()
        } else {
            t.into()
        }
    } else {
        out
    }
}

pub fn safe_filename(name: &str) -> String {
    let n = clean_display_name(name);
    let bad = Regex::new(r#"[<>:"/\\|?*#^\[\]]"#).unwrap();
    let n = bad.replace_all(&n, "");
    let ws = Regex::new(r"\s+").unwrap();
    let n = ws
        .replace_all(&n, " ")
        .trim()
        .trim_matches('.')
        .trim()
        .to_string();
    let n: String = n.chars().take(80).collect();
    if n.is_empty() || !n.chars().any(|c| c.is_alphanumeric()) {
        "Untitled".into()
    } else {
        n
    }
}

pub fn md_escape(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ").trim().to_string()
}

pub fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut out = vec![
        format!("| {} |", headers.join(" | ")),
        format!(
            "|{}|",
            headers.iter().map(|_| "---").collect::<Vec<_>>().join("|")
        ),
    ];
    for r in rows {
        let cells: Vec<String> = r
            .iter()
            .map(|c| {
                if c.starts_with("[[") {
                    c.replace('|', "\\|")
                } else {
                    md_escape(c)
                }
            })
            .collect();
        out.push(format!("| {} |", cells.join(" | ")));
    }
    if rows.is_empty() {
        out.push(format!(
            "| {} |",
            headers.iter().map(|_| "–").collect::<Vec<_>>().join(" | ")
        ));
    }
    out.join("\n")
}

pub fn link(target: &str, alias: Option<&str>) -> String {
    let t = target.strip_suffix(".md").unwrap_or(target);
    match alias {
        Some(a) => format!("[[{t}|{a}]]"),
        None => format!("[[{t}]]"),
    }
}

pub fn glink(note: &Option<String>, title: &str) -> String {
    match note {
        Some(n) => link(n, Some(title)),
        None => title.to_string(),
    }
}

// ---------------------------------------------------------- frontmatter ---

pub type RawFm = Vec<(String, String)>;

pub fn split_frontmatter(text: &str) -> (RawFm, String) {
    let re = Regex::new(r"(?s)\A---\n(.*?)\n---\n?").unwrap();
    let Some(m) = re.captures(text) else {
        return (vec![], text.to_string());
    };
    let mut raw: RawFm = vec![];
    let key_re = Regex::new(r"^([A-Za-z0-9_\-]+):(.*)$").unwrap();
    for line in m.get(1).unwrap().as_str().lines() {
        if let (Some(c), false) = (key_re.captures(line), line.starts_with(' ')) {
            raw.push((c[1].to_string(), c[2].trim().to_string()));
        } else if let Some(last) = raw.last_mut() {
            last.1.push('\n');
            last.1.push_str(line);
        }
    }
    (raw, text[m.get(0).unwrap().end()..].to_string())
}

pub fn yaml_value(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => serde_json::to_string(&s.replace('\n', " ")).unwrap(),
        Value::Array(a) if a.is_empty() => "[]".into(),
        Value::Array(a) => a
            .iter()
            .map(|x| format!("\n  - {}", yaml_value(x).trim()))
            .collect(),
        Value::Object(_) => serde_json::to_string(v).unwrap(),
    }
}

pub fn render_fm(raw: &RawFm) -> String {
    let mut s = String::from("---\n");
    for (k, v) in raw {
        if v.is_empty() {
            s.push_str(&format!("{k}:\n"));
        } else if v.starts_with('\n') {
            s.push_str(&format!("{k}:{v}\n"));
        } else {
            s.push_str(&format!("{k}: {v}\n"));
        }
    }
    s.push_str("---\n");
    s
}

pub fn build_frontmatter(values: &[(&str, Value)], keep: &RawFm) -> String {
    let mut raw: RawFm = values
        .iter()
        .map(|(k, v)| (k.to_string(), yaml_value(v)))
        .collect();
    for (k, v) in keep {
        if !raw.iter().any(|(rk, _)| rk == k) {
            raw.push((k.clone(), v.clone()));
        }
    }
    render_fm(&raw)
}

fn fm_get<'a>(raw: &'a RawFm, k: &str) -> Option<&'a str> {
    raw.iter().find(|(rk, _)| rk == k).map(|(_, v)| v.as_str())
}

fn fm_set(raw: &mut RawFm, k: &str, v: String) {
    if let Some(e) = raw.iter_mut().find(|(rk, _)| rk == k) {
        e.1 = v;
    } else {
        raw.push((k.to_string(), v));
    }
}

// ---------------------------------------------------------------- blocks ---

pub fn block(kind: &str, key: &str, content: &str) -> String {
    format!(
        "<!-- roxai:{kind}:{key} -->\n{}\n<!-- /roxai:{kind}:{key} -->",
        content.trim()
    )
}

fn block_span(text: &str, kind: &str, key: &str) -> Option<(usize, usize, usize, usize)> {
    let open = format!("<!-- roxai:{kind}:{key} -->");
    let close = format!("<!-- /roxai:{kind}:{key} -->");
    let s = text.find(&open)?;
    let e = s + text[s..].find(&close)?;
    Some((s, s + open.len(), e, e + close.len()))
}

pub fn get_block(text: &str, kind: &str, key: &str) -> Option<String> {
    let (_, a, b, _) = block_span(text, kind, key)?;
    Some(text[a..b].trim_matches('\n').to_string())
}

pub fn set_block(text: &str, kind: &str, key: &str, content: &str) -> String {
    let new = block(kind, key, content);
    if let Some((s, _, _, e)) = block_span(text, kind, key) {
        return format!("{}{}{}", &text[..s], new, &text[e..]);
    }
    if let Some(i) = text.find(MY_NOTES) {
        return format!("{}{}\n\n{}", &text[..i], new, &text[i..]);
    }
    format!("{}\n\n{}\n", text.trim_end(), new)
}

// --------------------------------------------------------------- vault ---

#[derive(Clone, Debug)]
pub struct Vault {
    pub root: PathBuf,
}

const README: &str = "# 🤖 ROX-AI Vault

This vault is built and updated automatically by **ROX-AI**: it follows the Roblox charts every day, has Claude
playtest the top games once a week, and turns each game into a detailed **template / blueprint**.

| Folder | What's inside |
|---|---|
| [[00 Dashboard]] | Today's chart, risers, new entries, playtest queue, everything at a glance |
| `01 Charts` | One note per day per chart, with rank changes and movers |
| `02 Games` | **One template note per game**: stats, trends, monetization data + Claude's full analysis & Studio blueprint |
| `03 Playtests` | Logs of every AI playtest session with screenshots |
| `04 Genres` | Genre overviews: which genres are winning, patterns |
| `05 Insights` | Weekly market reports and cross-game insights written by Claude |
| `06 Playbook` | **The playbook**: rules for making successful Roblox games, backed by evidence and our own numbers |

**Legend in game notes:** 📊 = data refreshed daily · 🤖 = written by Claude · ✍️ = yours (never overwritten).

Tip: the **Dataview** community plugin is pre-installed (click *Trust author and enable plugins* once) for live tables.
";

impl Vault {
    pub fn new(root: impl Into<PathBuf>) -> Vault {
        Vault { root: root.into() }
    }

    pub fn dir(&self, name: &str) -> PathBuf {
        let d = self.root.join(name);
        let _ = std::fs::create_dir_all(&d);
        d
    }

    pub fn rel(&self, p: &Path) -> String {
        p.strip_prefix(&self.root)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/")
    }

    pub fn abs(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub fn write(&self, p: &Path, text: &str) -> Result<()> {
        atomic_write(p, text.as_bytes())
    }

    pub fn read(&self, p: &Path) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }

    pub fn init(&self, api: Option<&dyn RobloxApi>) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        for d in ALL_DIRS {
            self.dir(d);
        }
        if !self.root.join("README.md").exists() {
            self.write(&self.root.join("README.md"), README)?;
        }
        let tpl: String = AI_SECTIONS
            .iter()
            .map(|(_, h, g)| format!("- **{h}** — {g}\n"))
            .collect();
        self.write(&self.dir(DIR_TEMPLATES).join("Game Template.md"), &format!("# 🎮 Game Template\n\nEvery note in `02 Games` follows this structure.\n\n{tpl}\nData blocks (📊) are filled from the Roblox APIs: snapshot, chart trend, description, game passes, developer products, badges, playtest history.\n"))?;
        self.write(&self.dir(DIR_TEMPLATES).join("Playtest Template.md"), "# 🕹️ Playtest Template\n\n- Timeline of observations (auto-saved during play)\n- Screenshots\n- Final report\n- Problems / blockers\n")?;
        let obs = self.root.join(".obsidian");
        std::fs::create_dir_all(&obs)?;
        if !obs.join("app.json").exists() {
            self.write(&obs.join("app.json"), &serde_json::to_string_pretty(&json!({"attachmentFolderPath": DIR_ATTACH, "newFileLocation": "folder", "newFileFolderPath": DIR_INSIGHTS, "readableLineLength": false}))?)?;
        }
        if !obs.join("community-plugins.json").exists() {
            self.write(&obs.join("community-plugins.json"), "[\"dataview\"]")?;
        }
        if let Some(api) = api {
            self.install_dataview(&obs, api);
        }
        if !self.root.join("00 Dashboard.md").exists() {
            self.write(
                &self.root.join("00 Dashboard.md"),
                "# 🏠 ROX-AI Dashboard\n\n_Waiting for the first chart scan…_\n",
            )?;
        }
        Ok(())
    }

    fn install_dataview(&self, obs: &Path, api: &dyn RobloxApi) {
        let d = obs.join("plugins").join("dataview");
        if d.join("main.js").exists() {
            return;
        }
        let base = "https://github.com/blacksmithgu/obsidian-dataview/releases/latest/download/";
        let main = api.download(&format!("{base}main.js"));
        let manifest = api.download(&format!("{base}manifest.json"));
        if let (Some(m), Some(mf)) = (main, manifest) {
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::write(d.join("main.js"), m);
            let _ = std::fs::write(d.join("manifest.json"), mf);
            if let Some(css) = api.download(&format!("{base}styles.css")) {
                let _ = std::fs::write(d.join("styles.css"), css);
            }
            crate::info!("Dataview plugin installed into vault");
        } else {
            crate::warn!("Could not pre-install Dataview (vault still works with static tables)");
        }
    }

    // -------------------------------------------------------- game notes
    pub fn game_note_path(&self, uid: u64, name: &str, existing: Option<&str>) -> PathBuf {
        if let Some(e) = existing {
            let p = self.abs(e);
            if p.exists() {
                return p;
            }
        }
        let base = safe_filename(name);
        let p = self.dir(DIR_GAMES).join(format!("{base}.md"));
        if p.exists() {
            let (raw, _) = split_frontmatter(&self.read(&p));
            if fm_get(&raw, "universe_id").map(|v| v.trim_matches('"')) != Some(&uid.to_string()) {
                return self.dir(DIR_GAMES).join(format!("{base} ({uid}).md"));
            }
        }
        p
    }

    pub fn find_game_note(&self, uid: u64) -> Option<PathBuf> {
        let needle = format!("\nuniverse_id: {uid}\n");
        std::fs::read_dir(self.dir(DIR_GAMES))
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.extension().is_some_and(|e| e == "md") && {
                    let t = self.read(p);
                    t.chars().take(800).collect::<String>().contains(&needle)
                }
            })
    }

    pub fn new_game_body(title: &str, icon_rel: Option<&str>) -> String {
        let mut parts = vec![format!("# {title}\n")];
        if let Some(i) = icon_rel {
            parts.push(format!("![[{i}|180]]\n"));
        }
        parts.push("> [!info] ROX-AI game template\n> 📊 sections refresh daily from Roblox · 🤖 sections are written by Claude (learned from YouTube videos and data) · ✍️ **My Notes** is yours and never overwritten.\n".into());
        parts.push(block("auto", "snapshot", "## 📊 Snapshot\n_pending_") + "\n");
        parts.push(block("auto", "trend", "## 📈 Chart Trend\n_pending_") + "\n");
        for (key, heading, hint) in AI_SECTIONS {
            parts.push(
                block(
                    "ai",
                    key,
                    &format!("## {heading}\n> [!todo] Not analyzed yet\n> {hint}"),
                ) + "\n",
            );
            if key == "monetization" {
                parts
                    .push(block("auto", "monetization_data", "## 📊 Store Data\n_pending_") + "\n");
            }
        }
        parts.push(
            block(
                "auto",
                "description",
                "## 📊 Official Description\n_pending_",
            ) + "\n",
        );
        parts.push(block("auto", "badges", "## 📊 Badges\n_pending_") + "\n");
        parts.push(
            block(
                "auto",
                "playtests",
                "## 📊 Playtest History\n_No playtests yet._",
            ) + "\n",
        );
        parts.push(format!("{MY_NOTES}\n\n- \n"));
        parts.join("\n")
    }

    /// Create or refresh the data parts of a game note. Never touches 🤖 or ✍️ sections.
    #[allow(clippy::too_many_arguments)]
    pub fn update_game_note(
        &self,
        d: &mut GameDetails,
        tr: &Trend,
        chart_rank: Option<u32>,
        sort_id: &str,
        history: &[Snap],
        icon_rel: Option<&str>,
        existing: Option<&str>,
        playtests: Option<&[Playtest]>,
    ) -> Result<PathBuf> {
        let path = self.game_note_path(d.universe_id, &d.name, existing);
        let title = clean_display_name(&d.name);
        let (raw_fm, mut body) = if path.exists() {
            split_frontmatter(&self.read(&path))
        } else {
            (vec![], Self::new_game_body(&title, icon_rel))
        };
        let mon = A::monetization(&mut d.game_passes, &mut d.dev_products);
        let lr = d.like_ratio();
        let age = crate::util::days_since(&d.created).map(|x| x.floor() as i64);
        let upd = crate::util::days_since(&d.updated).map(|x| x.floor() as i64);
        let opt = |v: Option<u64>| v.map(|x| json!(x)).unwrap_or(json!(""));

        let fm: Vec<(&str, Value)> = vec![
            ("type", json!("roblox-game")),
            ("universe_id", json!(d.universe_id)),
            ("place_id", json!(d.place_id)),
            ("name", json!(d.name)),
            ("url", json!(d.url())),
            ("creator", json!(d.creator_name)),
            ("creator_type", json!(d.creator_type)),
            ("genre", json!(d.genre)),
            ("subgenre", json!(d.subgenre)),
            ("chart", json!(sort_id)),
            (
                "chart_rank",
                chart_rank.map(|r| json!(r)).unwrap_or(json!("")),
            ),
            (
                "best_rank",
                if tr.best_rank > 0 {
                    json!(tr.best_rank)
                } else {
                    chart_rank.map(|r| json!(r)).unwrap_or(json!(""))
                },
            ),
            ("days_on_chart", json!(tr.days_tracked)),
            (
                "momentum",
                json!(if tr.days_tracked > 0 { tr.momentum } else { 50 }),
            ),
            ("playing", json!(d.playing)),
            ("visits", json!(d.visits)),
            ("favorites", json!(d.favorites)),
            ("likes", json!(d.up_votes)),
            ("dislikes", json!(d.down_votes)),
            ("like_ratio", json!(lr)),
            ("max_players", json!(d.max_players)),
            ("game_age_days", age.map(|x| json!(x)).unwrap_or(json!(""))),
            (
                "days_since_update",
                upd.map(|x| json!(x)).unwrap_or(json!("")),
            ),
            (
                "roblox_created",
                json!(d.created.chars().take(10).collect::<String>()),
            ),
            (
                "roblox_updated",
                json!(d.updated.chars().take(10).collect::<String>()),
            ),
            ("gamepass_count", json!(mon.passes.count)),
            ("gamepass_median", opt(mon.passes.median)),
            ("gamepass_max", opt(mon.passes.max)),
            ("devproduct_count", json!(mon.products.count)),
            ("buy_all_passes", json!(mon.buy_all_passes_robux)),
            ("flags", json!(tr.flags)),
            ("last_scan", json!(today())),
        ];
        let mut keep: RawFm = raw_fm
            .into_iter()
            .filter(|(k, _)| !fm.iter().any(|(fk, _)| fk == k))
            .collect();
        if fm_get(&keep, "tags").is_none() {
            let g = Regex::new(r"\W+")
                .unwrap()
                .replace_all(
                    &(if d.genre.is_empty() {
                        "unknown".into()
                    } else {
                        d.genre.to_lowercase()
                    }),
                    "-",
                )
                .to_string();
            keep.push(("tags".into(), yaml_value(&json!(["roblox", "game", g]))));
        }
        if fm_get(&keep, "status").is_none() {
            keep.push(("status".into(), "\"tracked\"".into()));
        }

        // snapshot
        let ss = d
            .server_sample
            .as_ref()
            .map(|s| {
                format!(
                    "{} · {} fps · {} ms · {}% full",
                    s.servers_sampled, s.avg_fps, s.avg_ping, s.full_pct
                )
            })
            .unwrap_or("–".into());
        let mut snap = vec![
            "## 📊 Snapshot".to_string(),
            format!(
                "[▶ Play on Roblox]({}) · Creator: **{}**{} ({})",
                d.url(),
                md_escape(&d.creator_name),
                if d.creator_verified { " ✅" } else { "" },
                d.creator_type
            ),
            String::new(),
            table(
                &["Metric", "Value", "Metric", "Value"],
                &[
                    vec![
                        "Chart rank".into(),
                        chart_rank
                            .map(|r| format!("#{r}"))
                            .unwrap_or("off chart".into()),
                        "Playing now".into(),
                        A::n(d.playing),
                    ],
                    vec![
                        "Best rank".into(),
                        if tr.best_rank > 0 {
                            format!("#{}", tr.best_rank)
                        } else {
                            "–".into()
                        },
                        "Visits".into(),
                        A::n(d.visits),
                    ],
                    vec![
                        "Momentum".into(),
                        format!("{}/100", if tr.days_tracked > 0 { tr.momentum } else { 50 }),
                        "Favorites".into(),
                        A::n(d.favorites),
                    ],
                    vec![
                        "Likes".into(),
                        format!("{lr}% ({}👍 / {}👎)", A::n(d.up_votes), A::n(d.down_votes)),
                        "Server size".into(),
                        d.max_players.to_string(),
                    ],
                    vec![
                        "Genre".into(),
                        format!("{} › {}", d.genre, d.subgenre)
                            .trim_end_matches(" › ")
                            .to_string(),
                        "Avatar".into(),
                        d.avatar_type.clone(),
                    ],
                    vec![
                        "Released".into(),
                        age.map(|a| {
                            format!(
                                "{} ({a}d ago)",
                                d.created.chars().take(10).collect::<String>()
                            )
                        })
                        .unwrap_or("–".into()),
                        "Last update".into(),
                        upd.map(|a| {
                            format!(
                                "{} ({a}d ago)",
                                d.updated.chars().take(10).collect::<String>()
                            )
                        })
                        .unwrap_or("–".into()),
                    ],
                    vec![
                        "Private servers".into(),
                        if d.vip_servers { "yes" } else { "no" }.into(),
                        "Servers sampled".into(),
                        ss,
                    ],
                ],
            ),
        ];
        if !tr.flags.is_empty() {
            snap.push(format!(
                "\n{}",
                tr.flags
                    .iter()
                    .map(|f| format!("`{f}`"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        body = set_block(&body, "auto", "snapshot", &snap.join("\n"));

        // trend
        let hist_rows: Vec<Vec<String>> = history
            .iter()
            .rev()
            .take(30)
            .map(|h| {
                vec![
                    h.day.clone(),
                    h.rank.to_string(),
                    A::n(h.playing),
                    A::n(h.visits),
                    A::like_ratio(h.up_votes, h.down_votes).to_string(),
                ]
            })
            .collect();
        let trt = [
            "## 📈 Chart Trend".to_string(),
            table(
                &[
                    "Δ rank 1d",
                    "Δ rank 7d",
                    "CCU Δ 1d",
                    "CCU Δ 7d",
                    "Visits/day",
                    "Favs/day",
                    "Days tracked",
                ],
                &[vec![
                    A::fmt_delta(tr.rank_change_1d),
                    A::fmt_delta(tr.rank_change_7d),
                    A::fmt_pct(tr.ccu_change_1d_pct),
                    A::fmt_pct(tr.ccu_change_7d_pct),
                    A::fmt_num(tr.visits_per_day.map(|x| x as f64)),
                    A::fmt_num(tr.favorites_per_day.map(|x| x as f64)),
                    tr.days_tracked.to_string(),
                ]],
            ),
            String::new(),
            "<details><summary>Daily history</summary>\n".into(),
            table(&["Day", "Rank", "Playing", "Visits", "Like %"], &hist_rows),
            "\n</details>".into(),
        ];
        body = set_block(&body, "auto", "trend", &trt.join("\n"));

        // monetization data
        let mut passes = d.game_passes.clone();
        passes.sort_by_key(|x| x.price.unwrap_or(0));
        let mut prods = d.dev_products.clone();
        prods.sort_by_key(|x| x.price.unwrap_or(0));
        let o = |v: Option<u64>| v.map(|x| x.to_string()).unwrap_or("–".into());
        let md = [
            "## 📊 Store Data".to_string(),
            format!("**{} game passes** (median R${}, max R${}, buy-all R${}) · **{} developer products** (median R${}, max R${})", mon.passes.count, o(mon.passes.median), o(mon.passes.max), mon.buy_all_passes_robux, mon.products.count, o(mon.products.median), o(mon.products.max)),
            String::new(),
            format!("Pass mix: {}", if mon.pass_categories.is_empty() { "–".into() } else { mon.pass_categories.iter().map(|(k, v)| format!("{k} ×{v}")).collect::<Vec<_>>().join(", ") }),
            String::new(),
            "### Game passes".into(),
            table(&["Pass", "R$", "Type", "Description"], &passes.iter().map(|g| vec![g.name.clone(), g.price.map(|p| p.to_string()).unwrap_or("off-sale".into()), g.category.clone(), g.description.chars().take(140).collect()]).collect::<Vec<_>>()),
            String::new(),
            "<details><summary>Developer products</summary>\n".into(),
            table(&["Product", "R$", "Type", "Description"], &prods.iter().map(|g| vec![g.name.clone(), o(g.price), g.category.clone(), g.description.chars().take(120).collect()]).collect::<Vec<_>>()),
            "\n</details>".into(),
        ];
        body = set_block(&body, "auto", "monetization_data", &md.join("\n"));

        // description
        let desc = if d.description.trim().is_empty() {
            "_No description._".to_string()
        } else {
            d.description.trim().to_string()
        };
        body = set_block(
            &body,
            "auto",
            "description",
            &format!(
                "## 📊 Official Description\n{}",
                desc.lines()
                    .map(|l| format!("> {l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        );

        // badges
        let btxt = if d.badges.is_empty() {
            "## 📊 Badges\n_No public badges._".to_string()
        } else {
            let mut bl = d.badges.clone();
            bl.sort_by(|a, b| b.awarded.cmp(&a.awarded));
            format!(
                "## 📊 Badges\n{} badges (sorted by times awarded — shows how far players actually get):\n\n{}",
                d.badge_total,
                table(&["Badge", "Awarded", "Win rate", "Description"], &bl.iter().take(40).map(|b| vec![b.name.clone(), A::n(b.awarded), format!("{:.2}%", b.win_rate * 100.0), b.description.chars().take(100).collect()]).collect::<Vec<_>>())
            )
        };
        body = set_block(&body, "auto", "badges", &btxt);

        if let Some(pts) = playtests {
            body = set_block(&body, "auto", "playtests", &playtest_history_md(pts));
        }
        self.write(&path, &(build_frontmatter(&fm, &keep) + &body))?;
        Ok(path)
    }

    /// Write Claude's analysis into the 🤖 blocks. Unknown keys are ignored (returned as warnings).
    pub fn write_ai_sections(
        &self,
        path: &Path,
        sections: &Map<String, Value>,
        scores: Option<&Map<String, Value>>,
        one_liner: Option<&str>,
        tags: Option<&[String]>,
        status: &str,
    ) -> Result<Vec<String>> {
        let text = self.read(path);
        let (mut raw, mut body) = split_frontmatter(&text);
        let mut warnings = vec![];
        for (key, content) in sections {
            let Some((_, heading, _)) = AI_SECTIONS.iter().find(|s| s.0 == key) else {
                warnings.push(format!(
                    "unknown section '{key}' ignored (valid: {})",
                    ai_keys().join(", ")
                ));
                continue;
            };
            let mut c = content
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| content.to_string())
                .trim()
                .to_string();
            if !c.starts_with("## ") {
                c = format!("## {heading}\n{c}");
            }
            body = set_block(
                &body,
                "ai",
                key,
                &format!("{c}\n\n<sub>🤖 updated {}</sub>", today()),
            );
        }
        if let Some(scores) = scores {
            let clean = Regex::new(r"[^a-z0-9]+").unwrap();
            for (k, v) in scores {
                let num = v
                    .as_f64()
                    .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()));
                match num {
                    Some(n) => {
                        let key = format!(
                            "score_{}",
                            clean.replace_all(&k.to_lowercase(), "_").trim_matches('_')
                        );
                        fm_set(
                            &mut raw,
                            &key,
                            (n.round().clamp(0.0, 10.0) as i64).to_string(),
                        );
                    }
                    None => warnings.push(format!("score {k}={v} is not a number")),
                }
            }
        }
        if let Some(o) = one_liner.filter(|s| !s.trim().is_empty()) {
            fm_set(&mut raw, "one_liner", yaml_value(&json!(o)));
        }
        if let Some(tags) = tags {
            let item = Regex::new(r"-\s*(.+)").unwrap();
            let mut merged: Vec<String> = fm_get(&raw, "tags")
                .map(|t| {
                    item.captures_iter(t)
                        .map(|c| c[1].trim().trim_matches('"').to_string())
                        .collect()
                })
                .unwrap_or_default();
            let clean = Regex::new(r"[^\w\-/]+").unwrap();
            for t in tags {
                let t = clean
                    .replace_all(&t.to_lowercase(), "-")
                    .trim_matches('-')
                    .to_string();
                if !t.is_empty() && !merged.contains(&t) {
                    merged.push(t);
                }
            }
            fm_set(&mut raw, "tags", yaml_value(&json!(merged)));
        }
        fm_set(&mut raw, "status", yaml_value(&json!(status)));
        fm_set(&mut raw, "last_analysis", yaml_value(&json!(today())));
        self.write(path, &(render_fm(&raw) + &body))?;
        Ok(warnings)
    }

    pub fn read_note_sections(&self, path: &Path) -> BTreeMap<String, String> {
        let text = self.read(path);
        let mut out = BTreeMap::new();
        for k in ai_keys() {
            if let Some(b) = get_block(&text, "ai", k) {
                out.insert(k.to_string(), b);
            }
        }
        out.insert(
            "my_notes".into(),
            text.split_once(MY_NOTES)
                .map(|(_, b)| b.trim().to_string())
                .unwrap_or_default(),
        );
        out
    }

    /// "📺 Sources" block + `analysis_source` frontmatter: where Claude's analysis came from.
    pub fn set_sources(
        &self,
        path: &Path,
        method: &str,
        sources: &[(String, String)],
    ) -> Result<()> {
        let label = match method {
            "videos" => "Learned from YouTube videos",
            "playtest" => "Learned by playtesting",
            _ => "Learned from public data",
        };
        let list = if sources.is_empty() {
            "_No external sources._".to_string()
        } else {
            sources
                .iter()
                .map(|(t, u)| format!("- [{}]({u})", md_escape(t)))
                .collect::<Vec<_>>()
                .join("\n")
        };
        self.set_block_in(
            path,
            "auto",
            "sources",
            &format!("## 📺 Sources\n**{label}** · {}\n\n{list}", today()),
        )?;
        self.set_fm_value(path, "analysis_source", json!(method))
    }

    pub fn set_fm_value(&self, path: &Path, key: &str, v: Value) -> Result<()> {
        let (mut raw, body) = split_frontmatter(&self.read(path));
        fm_set(&mut raw, key, yaml_value(&v));
        self.write(path, &(render_fm(&raw) + &body))
    }

    pub fn append_to_block(&self, path: &Path, kind: &str, key: &str, line: &str) -> Result<()> {
        let text = self.read(path);
        let cur = get_block(&text, kind, key)
            .unwrap_or_default()
            .replace("_None._", "");
        let new = set_block(&text, kind, key, &format!("{}\n{line}", cur.trim_end()));
        self.write(path, &new)
    }

    pub fn set_block_in(&self, path: &Path, kind: &str, key: &str, content: &str) -> Result<()> {
        let text = self.read(path);
        self.write(path, &set_block(&text, kind, key, content))
    }

    // -------------------------------------------------------- chart notes
    pub fn chart_note(
        &self,
        day: &str,
        sort_name: &str,
        rows: &[ChartRow],
        new_ids: &HashSet<u64>,
        dropped: &[(String, u32)],
        extra: &[(String, Vec<ChartRow>)],
    ) -> Result<PathBuf> {
        let path = self.dir(DIR_CHARTS).join(format!("{day} {sort_name}.md"));
        let mut risers: Vec<&ChartRow> = rows
            .iter()
            .filter(|r| r.rank_change_1d.unwrap_or(0) > 0)
            .collect();
        risers.sort_by_key(|r| -r.rank_change_1d.unwrap_or(0));
        let mut lines = vec![
            build_frontmatter(
                &[
                    ("type", json!("chart-snapshot")),
                    ("day", json!(day)),
                    ("chart", json!(sort_name)),
                    ("games", json!(rows.len())),
                ],
                &vec![],
            ),
            format!("# 📈 {sort_name} — {day}\n"),
            table(
                &[
                    "#",
                    "Δ",
                    "Game",
                    "Genre",
                    "Playing",
                    "CCU Δ1d",
                    "Like %",
                    "Visits/day",
                    "Flags",
                ],
                &rows
                    .iter()
                    .map(|r| {
                        vec![
                            r.rank.to_string(),
                            if new_ids.contains(&r.universe_id) {
                                "🆕".into()
                            } else {
                                A::fmt_delta(r.rank_change_1d)
                            },
                            glink(&r.note, &r.title),
                            r.genre.clone(),
                            A::n(r.playing),
                            A::fmt_pct(r.ccu_change_1d_pct),
                            format!("{}", r.like_ratio),
                            A::fmt_num(r.visits_per_day.map(|x| x as f64)),
                            r.flags.join(" "),
                        ]
                    })
                    .collect::<Vec<_>>(),
            ),
            String::new(),
            "## 🆕 New entries".into(),
            or_none(
                rows.iter()
                    .filter(|r| new_ids.contains(&r.universe_id))
                    .map(|r| {
                        format!(
                            "- {} at **#{}** ({})",
                            glink(&r.note, &r.title),
                            r.rank,
                            r.genre
                        )
                    })
                    .collect(),
                "_None today._",
            ),
            String::new(),
            "## 🚀 Biggest climbers".into(),
            or_none(
                risers
                    .iter()
                    .take(5)
                    .map(|r| {
                        format!(
                            "- {} {} → #{}",
                            glink(&r.note, &r.title),
                            A::fmt_delta(r.rank_change_1d),
                            r.rank
                        )
                    })
                    .collect(),
                "_None._",
            ),
            String::new(),
            "## 📉 Dropped off".into(),
            or_none(
                dropped
                    .iter()
                    .map(|(t, r)| format!("- {t} (was #{r})"))
                    .collect(),
                "_None._",
            ),
        ];
        for (sname, srows) in extra {
            lines.push(String::new());
            lines.push(format!("## Also: {sname}"));
            lines.push(table(
                &["#", "Game", "Genre", "Playing"],
                &srows
                    .iter()
                    .map(|r| {
                        vec![
                            r.rank.to_string(),
                            glink(&r.note, &r.title),
                            r.genre.clone(),
                            A::n(r.playing),
                        ]
                    })
                    .collect::<Vec<_>>(),
            ));
        }
        self.write(&path, &(lines.join("\n") + "\n"))?;
        Ok(path)
    }

    pub fn genre_notes(&self, rows: &[ChartRow]) -> Result<()> {
        let mut by: BTreeMap<String, Vec<&ChartRow>> = BTreeMap::new();
        for r in rows.iter().filter(|r| r.note.is_some()) {
            by.entry(if r.genre.is_empty() {
                "Unknown".into()
            } else {
                r.genre.clone()
            })
            .or_default()
            .push(r);
        }
        for (genre, mut gs) in by {
            gs.sort_by_key(|r| r.rank);
            let path = self
                .dir(DIR_GENRES)
                .join(format!("{}.md", safe_filename(&genre)));
            let text = if path.exists() {
                self.read(&path)
            } else {
                format!("{}# 🧩 {genre}\n\n{}\n\n{MY_NOTES}\n\n- \n", build_frontmatter(&[("type", json!("genre")), ("genre", json!(genre))], &vec![]), block("ai", "patterns", "## 🤖 Genre Patterns\n> [!todo] Claude writes cross-game patterns here during weekly runs."))
            };
            let avg_like = (gs.iter().map(|g| g.like_ratio).sum::<f64>() / gs.len() as f64 * 10.0)
                .round()
                / 10.0;
            let content = format!(
                "## 📊 Games on the chart now ({})\nTotal playing: **{}** · avg like ratio **{avg_like}%**\n\n{}",
                gs.len(),
                A::n(gs.iter().map(|g| g.playing).sum()),
                table(&["#", "Game", "Sub-genre", "Playing", "Like %", "Passes", "Momentum"], &gs.iter().map(|g| vec![g.rank.to_string(), glink(&g.note, &g.title), g.subgenre.clone(), A::n(g.playing), g.like_ratio.to_string(), g.passes.to_string(), g.momentum.to_string()]).collect::<Vec<_>>())
            );
            self.write(&path, &set_block(&text, "auto", "current", &content))?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn dashboard(
        &self,
        day: &str,
        sort_name: &str,
        rows: &[ChartRow],
        new_ids: &HashSet<u64>,
        dropped: &[(String, u32)],
        queue: &[QueueItem],
        recent: &[(String, Option<String>, String, String)],
        status_lines: &[String],
    ) -> Result<PathBuf> {
        let path = self.root.join("00 Dashboard.md");
        let rows: Vec<&ChartRow> = rows.iter().filter(|r| r.note.is_some()).collect();
        let mut genres: BTreeMap<String, usize> = BTreeMap::new();
        for r in &rows {
            *genres
                .entry(if r.genre.is_empty() {
                    "Unknown".into()
                } else {
                    r.genre.clone()
                })
                .or_default() += 1;
        }
        let mut gv: Vec<(String, usize)> = genres.into_iter().collect();
        gv.sort_by(|a, b| b.1.cmp(&a.1));
        let mut hot = rows.clone();
        hot.sort_by_key(|r| std::cmp::Reverse(r.momentum));
        let text = [
            "# 🏠 ROX-AI Dashboard\n".to_string(),
            format!("_Last scan: **{day}** · chart: **{sort_name}** · {} games_\n", rows.len()),
            format!("> [!abstract] System status\n{}\n", status_lines.iter().map(|s| format!("> {s}")).collect::<Vec<_>>().join("\n")),
            format!("## 🏆 Today's {sort_name}"),
            table(&["#", "Δ", "Game", "Genre", "Playing", "Like %", "Momentum", "Playtested"], &rows.iter().map(|r| vec![
                r.rank.to_string(), if new_ids.contains(&r.universe_id) { "🆕".into() } else { A::fmt_delta(r.rank_change_1d) },
                glink(&r.note, &r.title), r.genre.clone(), A::n(r.playing), r.like_ratio.to_string(), r.momentum.to_string(),
                if r.last_playtest.is_empty() { "—".into() } else { r.last_playtest.clone() },
            ]).collect::<Vec<_>>()),
            String::new(),
            "## 🔥 Hottest momentum".into(),
            or_none(hot.iter().take(5).map(|r| format!("- {} — momentum **{}**, CCU {}", glink(&r.note, &r.title), r.momentum, A::fmt_pct(r.ccu_change_1d_pct))).collect(), "_Needs 2+ days of data._"),
            String::new(),
            "## 🆕 New today".into(),
            or_none(rows.iter().filter(|r| new_ids.contains(&r.universe_id)).map(|r| format!("- {} (#{})", glink(&r.note, &r.title), r.rank)).collect(), "_None._"),
            String::new(),
            "## 📉 Fell off the chart".into(),
            or_none(dropped.iter().map(|(t, _)| format!("- {t}")).collect(), "_None._"),
            String::new(),
            "## 🧩 Genre mix".into(),
            table(&["Genre", "Games"], &gv.iter().map(|(g, n)| vec![g.clone(), n.to_string()]).collect::<Vec<_>>()),
            String::new(),
            "## 🕹️ Next playtest queue".into(),
            or_none(queue.iter().enumerate().map(|(i, q)| format!("{}. {} — {}", i + 1, glink(&q.note, &q.title), q.reason)).collect(), "_Queue empty — everything is up to date._"),
            String::new(),
            "## 📝 Recent playtests".into(),
            or_none(recent.iter().map(|(date, note, title, status)| format!("- {date} · {} · {status}", glink(note, title))).collect(), "_None yet._"),
            String::new(),
            "## 🔎 Live queries (Dataview)".into(),
            "```dataview\nTABLE chart_rank AS \"#\", playing AS \"Playing\", like_ratio AS \"Like %\", momentum, score_fun AS \"Fun\", score_retention AS \"Retention\", last_analysis AS \"Analyzed\"\nFROM \"02 Games\"\nWHERE chart_rank\nSORT chart_rank ASC\n```".into(),
            String::new(),
            "Best clone targets (high fun, low clone difficulty):".into(),
            "```dataview\nTABLE score_fun AS Fun, score_clone_difficulty AS \"Clone diff.\", genre, one_liner\nFROM \"02 Games\"\nWHERE score_fun\nSORT (score_fun - score_clone_difficulty) DESC\nLIMIT 15\n```".into(),
            String::new(),
            "Monetization leaders:".into(),
            "```dataview\nTABLE gamepass_count AS Passes, gamepass_median AS \"Median R$\", buy_all_passes AS \"Buy-all R$\", devproduct_count AS Products, score_monetization_aggressiveness AS Aggro\nFROM \"02 Games\"\nSORT buy_all_passes DESC\nLIMIT 15\n```".into(),
            String::new(),
            "Latest insights:".into(),
            "```dataview\nLIST FROM \"05 Insights\" SORT file.ctime DESC LIMIT 10\n```".into(),
        ];
        self.write(&path, &(text.join("\n") + "\n"))?;
        Ok(path)
    }

    // ---------------------------------------------------------- playtests
    pub fn playtest_note_path(&self, name: &str) -> PathBuf {
        let base = format!("{} {}", today(), safe_filename(name));
        let dir = self.dir(DIR_PLAYTESTS);
        let mut p = dir.join(format!("{base}.md"));
        let mut i = 2;
        while p.exists() {
            p = dir.join(format!("{base} #{i}.md"));
            i += 1;
        }
        p
    }

    pub fn create_playtest_note(
        &self,
        path: &Path,
        game_note: Option<&Path>,
        name: &str,
        uid: u64,
        minutes: u32,
    ) -> Result<()> {
        let fm = build_frontmatter(
            &[
                ("type", json!("playtest")),
                ("universe_id", json!(uid)),
                ("game", json!(name)),
                (
                    "date",
                    json!(chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()),
                ),
                ("budget_minutes", json!(minutes)),
                ("status", json!("running")),
                ("tags", json!(["playtest"])),
            ],
            &vec![],
        );
        let body = [
            format!("# 🕹️ Playtest — {}", clean_display_name(name)),
            format!(
                "Game note: {}\n",
                game_note
                    .map(|g| link(&self.rel(g), None))
                    .unwrap_or("–".into())
            ),
            block("auto", "timeline", "## ⏱️ Timeline\n"),
            String::new(),
            block("auto", "shots", "## 📸 Screenshots\n"),
            String::new(),
            block("ai", "report", "## 🤖 Final Report\n_pending_"),
            String::new(),
            block("auto", "problems", "## ⚠️ Problems\n_None._"),
            String::new(),
            MY_NOTES.into(),
            String::new(),
            "- ".into(),
        ];
        self.write(path, &(fm + &body.join("\n") + "\n"))
    }

    pub fn write_insight(&self, title: &str, content: &str, tags: &[String]) -> Result<PathBuf> {
        let path = self
            .dir(DIR_INSIGHTS)
            .join(format!("{} {}.md", today(), safe_filename(title)));
        let mut t = vec![json!("insight")];
        t.extend(tags.iter().map(|x| json!(x)));
        let fm = build_frontmatter(
            &[
                ("type", json!("insight")),
                ("date", json!(today())),
                ("tags", Value::Array(t)),
            ],
            &vec![],
        );
        let body = content.trim();
        let body = if body.starts_with("# ") {
            body.to_string()
        } else {
            format!("# {title}\n\n{body}")
        };
        self.write(&path, &(fm + &body + "\n"))?;
        Ok(path)
    }

    pub fn set_genre_patterns(&self, genre: &str, content: &str) -> Result<PathBuf> {
        let path = self
            .dir(DIR_GENRES)
            .join(format!("{}.md", safe_filename(genre)));
        let text = if path.exists() {
            self.read(&path)
        } else {
            format!(
                "{}# 🧩 {genre}\n\n{MY_NOTES}\n\n- \n",
                build_frontmatter(
                    &[("type", json!("genre")), ("genre", json!(genre))],
                    &vec![]
                )
            )
        };
        let c = if content.trim().starts_with("## ") {
            content.trim().to_string()
        } else {
            format!("## 🤖 Genre Patterns\n{}", content.trim())
        };
        self.write(
            &path,
            &set_block(
                &text,
                "ai",
                "patterns",
                &format!("{c}\n\n<sub>🤖 updated {}</sub>", today()),
            ),
        )?;
        Ok(path)
    }
}

pub fn playtest_history_md(pts: &[Playtest]) -> String {
    if pts.is_empty() {
        return "## 📊 Playtest History\n_No playtests yet._".into();
    }
    format!(
        "## 📊 Playtest History\n{}",
        table(
            &["Date", "Status", "Log", "Summary"],
            &pts.iter()
                .map(|p| vec![
                    p.started.chars().take(10).collect(),
                    p.status.clone(),
                    if p.note_path.is_empty() {
                        "–".into()
                    } else {
                        link(&p.note_path, Some("log"))
                    },
                    p.summary.chars().take(160).collect()
                ])
                .collect::<Vec<_>>()
        )
    )
}

fn or_none(lines: Vec<String>, none: &str) -> String {
    if lines.is_empty() {
        none.to_string()
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(
            clean_display_name("[🎃 UPDATE] Pet Sim Mega"),
            "Pet Sim Mega"
        );
        assert_eq!(safe_filename("Brainrot | Battles"), "Brainrot Battles");
        assert_eq!(safe_filename("What?? #1 [x]"), "What 1");
        assert_eq!(safe_filename("🔥🔥"), "Untitled");
        assert_eq!(clean_display_name("Do Not Stop."), "Do Not Stop.");
    }

    #[test]
    fn blocks_idempotent() {
        let t = "# T\n\n## ✍️ My Notes\nmine";
        let t1 = set_block(t, "auto", "a", "one");
        let t2 = set_block(&t1, "auto", "a", "two $1 \\1");
        assert_eq!(t2.matches("<!-- roxai:auto:a -->").count(), 1);
        assert!(t2.contains("two $1 \\1") && !t2.contains("one"));
        assert!(t2.ends_with("mine"));
        assert_eq!(get_block(&t2, "auto", "a").unwrap(), "two $1 \\1");
    }

    #[test]
    fn frontmatter_roundtrip() {
        let fm = build_frontmatter(
            &[
                ("a", json!(1)),
                ("b", json!("x: y")),
                ("c", json!(["p", "q"])),
                ("d", json!(true)),
                ("e", Value::Null),
            ],
            &vec![],
        );
        let (raw, body) = split_frontmatter(&(fm.clone() + "body"));
        assert_eq!(body, "body");
        assert_eq!(fm_get(&raw, "a"), Some("1"));
        assert_eq!(fm_get(&raw, "b"), Some("\"x: y\""));
        assert!(fm_get(&raw, "c").unwrap().contains("- \"p\""));
        assert_eq!(render_fm(&raw), fm);
    }

    #[test]
    fn ai_sections_preserve_everything() {
        let d = tempfile::tempdir().unwrap();
        let v = Vault::new(d.path());
        v.init(None).unwrap();
        let p = v.dir(DIR_GAMES).join("G.md");
        let body = Vault::new_game_body("G", None) + "my private idea\n";
        v.write(
            &p,
            &(build_frontmatter(
                &[("universe_id", json!(5)), ("tags", json!(["roblox"]))],
                &vec![],
            ) + &body),
        )
        .unwrap();
        let secs =
            json!({"summary": "Great game", "blueprint": "## custom heading\nsteps", "nope": "x"});
        let scores = json!({"fun": 9, "Clone Difficulty": "4", "bad": "x"});
        let w = v
            .write_ai_sections(
                &p,
                secs.as_object().unwrap(),
                scores.as_object(),
                Some("one line"),
                Some(&["Pets".to_string()]),
                "analyzed",
            )
            .unwrap();
        let t = v.read(&p);
        assert!(
            t.contains("Great game")
                && t.contains("## custom heading")
                && t.contains("my private idea")
        );
        assert!(t.contains("score_fun: 9") && t.contains("score_clone_difficulty: 4"));
        assert!(w.iter().any(|x| x.contains("nope")) && w.iter().any(|x| x.contains("bad")));
        assert!(t.contains("\"pets\"") && t.contains("\"roblox\""));
        assert_eq!(v.find_game_note(5).unwrap(), p);
        let s = v.read_note_sections(&p);
        assert!(s["summary"].contains("Great game") && s["my_notes"].contains("my private idea"));
    }
}
