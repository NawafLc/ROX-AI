//! Data for the in-app Games dashboard: every tracked game with chart stats and how it was analyzed.

use crate::config::Ctx;
use crate::vault::{split_frontmatter, Vault};

#[derive(Clone, Debug, Default)]
pub struct GameRow {
    pub uid: u64,
    pub rank: Option<u32>,
    pub title: String,
    pub genre: String,
    pub playing: u64,
    pub like_ratio: f64,
    pub momentum: u32,
    /// "videos", "data", "playtest" or "" (not analyzed yet — only chart data)
    pub source: String,
    pub last_analysis: String,
    pub fun: Option<u32>,
    pub one_liner: String,
    pub note: Option<String>,
}

impl GameRow {
    pub fn source_label(&self) -> &'static str {
        match self.source.as_str() {
            "videos" => "Videos",
            "playtest" => "Playtest",
            "data" => "Data + AI",
            _ => "Data only",
        }
    }
    pub fn learned(&self) -> bool {
        !self.source.is_empty()
    }
}

fn fm_num(raw: &[(String, String)], k: &str) -> Option<f64> {
    raw.iter()
        .find(|(rk, _)| rk == k)
        .and_then(|(_, v)| v.trim_matches('"').parse().ok())
}
fn fm_str(raw: &[(String, String)], k: &str) -> String {
    raw.iter()
        .find(|(rk, _)| rk == k)
        .map(|(_, v)| v.trim().trim_matches('"').to_string())
        .unwrap_or_default()
}

pub fn rows(ctx: &Ctx) -> Vec<GameRow> {
    let v = Vault::new(ctx.vault());
    let ranks: std::collections::HashMap<u64, u32> = ctx
        .store
        .latest_day(&ctx.settings.sort_id)
        .map(|d| {
            ctx.store
                .chart_on(&d, &ctx.settings.sort_id)
                .into_iter()
                .map(|s| (s.universe_id, s.rank))
                .collect()
        })
        .unwrap_or_default();
    let mut out: Vec<GameRow> = ctx
        .store
        .games()
        .into_values()
        .map(|g| {
            let raw = g
                .note_path
                .as_ref()
                .map(|n| split_frontmatter(&v.read(&v.abs(n))).0)
                .unwrap_or_default();
            GameRow {
                uid: g.universe_id,
                rank: ranks.get(&g.universe_id).copied(),
                title: crate::vault::clean_display_name(&g.name),
                genre: g.genre.clone(),
                playing: fm_num(&raw, "playing").unwrap_or(0.0) as u64,
                like_ratio: fm_num(&raw, "like_ratio").unwrap_or(0.0),
                momentum: fm_num(&raw, "momentum").unwrap_or(50.0) as u32,
                source: g
                    .analysis_source
                    .clone()
                    .or_else(|| {
                        let s = fm_str(&raw, "analysis_source");
                        (!s.is_empty()).then_some(s)
                    })
                    .unwrap_or_default(),
                last_analysis: g
                    .last_analysis
                    .clone()
                    .map(|x| x.chars().take(10).collect())
                    .unwrap_or_default(),
                fun: fm_num(&raw, "score_fun").map(|x| x as u32),
                one_liner: fm_str(&raw, "one_liner"),
                note: g.note_path.clone(),
            }
        })
        .collect();
    out.sort_by(|a, b| {
        a.rank
            .unwrap_or(9999)
            .cmp(&b.rank.unwrap_or(9999))
            .then(b.playing.cmp(&a.playing))
    });
    out
}

/// Notes for the vault browser, grouped by folder (newest first for dated folders).
pub fn vault_tree(ctx: &Ctx) -> Vec<(&'static str, Vec<String>)> {
    let v = Vault::new(ctx.vault());
    let list = |dir: &str, newest_first: bool| -> Vec<String> {
        let mut f: Vec<String> = std::fs::read_dir(v.root.join(dir))
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "md"))
                    .map(|p| v.rel(&p))
                    .collect()
            })
            .unwrap_or_default();
        f.sort();
        if newest_first {
            f.reverse();
        }
        f
    };
    vec![
        ("Dashboard", vec!["00 Dashboard.md".to_string()]),
        ("Games", list(crate::vault::DIR_GAMES, false)),
        ("Insights", list(crate::vault::DIR_INSIGHTS, true)),
        ("Charts", list(crate::vault::DIR_CHARTS, true)),
        ("Genres", list(crate::vault::DIR_GENRES, false)),
        ("Playtests", list(crate::vault::DIR_PLAYTESTS, true)),
    ]
}

/// Resolve a [[wikilink]] target (with or without folder / .md) to a note path relative to the vault.
pub fn resolve(ctx: &Ctx, target: &str) -> Option<String> {
    let v = Vault::new(ctx.vault());
    let t = target.split('#').next().unwrap_or(target).trim();
    let with_md = if t.ends_with(".md") {
        t.to_string()
    } else {
        format!("{t}.md")
    };
    if v.abs(&with_md).exists() {
        return Some(with_md);
    }
    let name = with_md
        .rsplit('/')
        .next()
        .unwrap_or(&with_md)
        .to_lowercase();
    vault_tree(ctx).into_iter().flat_map(|(_, f)| f).find(|f| {
        f.rsplit('/')
            .next()
            .is_some_and(|n| n.to_lowercase() == name)
    })
}
