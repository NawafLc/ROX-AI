//! Daily chart scan — pure data, no Claude needed, safe to run any number of times per day.

use crate::analysis::{self as A};
use crate::api::{ChartEntry, RobloxApi};
use crate::config::Ctx;
use crate::store::Snap;
use crate::util::{days_since, single_instance};
use crate::vault::{clean_display_name, ChartRow, QueueItem, Vault};
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

pub fn sort_name(sid: &str) -> String {
    match sid {
        "top-trending" => "Top Trending".into(),
        "top-playing-now" => "Top Playing Now".into(),
        "up-and-coming" => "Up-and-Coming".into(),
        "fun-with-friends" => "Fun with Friends".into(),
        "top-revisited" => "Top Revisited".into(),
        s => s
            .split('-')
            .map(|w| {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[derive(Debug, Serialize)]
pub struct ScanResult {
    pub day: String,
    pub chart: String,
    pub games: usize,
    pub new: usize,
    pub dropped: usize,
    pub errors: usize,
    pub chart_note: String,
    pub queue: Vec<String>,
    #[serde(skip)]
    pub rows: Vec<ChartRow>,
}

pub fn ensure_icon(
    v: &Vault,
    api: &dyn RobloxApi,
    uid: u64,
    url: &str,
    refresh: bool,
) -> Option<String> {
    let f = v
        .dir(crate::vault::DIR_ATTACH)
        .join(uid.to_string())
        .join("icon.png");
    if !url.is_empty() && (!f.exists() || refresh) {
        if let Some(data) = api.download(url) {
            let _ = std::fs::create_dir_all(f.parent().unwrap());
            let _ = std::fs::write(&f, data);
        }
    }
    f.exists().then(|| v.rel(&f))
}

/// Which games Claude should play next, best first.
/// Which chart games Claude should learn (from videos) next, best rank first.
pub fn learn_queue(ctx: &Ctx, limit: Option<usize>) -> Vec<QueueItem> {
    let s = &ctx.settings;
    let Some(day) = ctx.store.latest_day(&s.sort_id) else {
        return vec![];
    };
    let games = ctx.store.games();
    let mut out = vec![];
    for snap in ctx.store.chart_on(&day, &s.sort_id) {
        let Some(g) = games.get(&snap.universe_id) else {
            continue;
        };
        let age = g.last_analysis.as_deref().and_then(days_since);
        let reason = match age {
            None => Some("not learned yet".to_string()),
            Some(a) if a >= s.reanalyze_after_days => {
                Some(format!("last learned {} days ago", a as i64))
            }
            Some(a) if a >= 7.0 && days_since(&g.roblox_updated).is_some_and(|u| u < a) => {
                Some("game updated since".into())
            }
            _ => None,
        };
        if let Some(reason) = reason {
            out.push(QueueItem {
                universe_id: g.universe_id,
                place_id: g.place_id,
                title: clean_display_name(&g.name),
                rank: snap.rank,
                reason,
                note: g.note_path.clone(),
            });
        }
    }
    out.truncate(limit.unwrap_or(s.learn_per_run));
    out
}

pub fn playtest_queue(ctx: &Ctx, limit: Option<usize>, day: Option<&str>) -> Vec<QueueItem> {
    let s = &ctx.settings;
    let Some(day) = day
        .map(|d| d.to_string())
        .or_else(|| ctx.store.latest_day(&s.sort_id))
    else {
        return vec![];
    };
    let games = ctx.store.games();
    let mut out = vec![];
    for snap in ctx.store.chart_on(&day, &s.sort_id) {
        let Some(g) = games.get(&snap.universe_id) else {
            continue;
        };
        let age = g.last_playtest.as_deref().and_then(days_since);
        if g.playtest_fail_count >= 3 {
            let since_fail = g.last_fail.as_deref().and_then(days_since).unwrap_or(0.0);
            if since_fail < 14.0 {
                continue; // keeps failing (crash, ban wall…) — retry in 2 weeks
            }
        }
        let reason = match (&g.last_playtest, age) {
            (None, _) => Some("never playtested".to_string()),
            (Some(_), Some(a)) if a >= s.replay_after_days => {
                Some(format!("last playtest {} days ago", a as i64))
            }
            (Some(_), Some(a))
                if s.replay_if_updated
                    && a >= 7.0
                    && days_since(&g.roblox_updated).is_some_and(|u| u < a) =>
            {
                Some("game updated since last playtest".into())
            }
            _ => None,
        };
        if let Some(reason) = reason {
            out.push(QueueItem {
                universe_id: g.universe_id,
                place_id: g.place_id,
                title: clean_display_name(&g.name),
                rank: snap.rank,
                reason,
                note: g.note_path.clone(),
            });
        }
    }
    out.truncate(limit.unwrap_or(s.playtests_per_run));
    out
}

pub fn recent_playtests(ctx: &Ctx, n: usize) -> Vec<(String, Option<String>, String, String)> {
    let games = ctx.store.games();
    ctx.store
        .recent_playtests(n)
        .into_iter()
        .map(|p| {
            let title = games
                .get(&p.universe_id)
                .map(|g| clean_display_name(&g.name))
                .unwrap_or(p.universe_id.to_string());
            (
                p.started.chars().take(10).collect(),
                (!p.note_path.is_empty()).then(|| p.note_path.clone()),
                title,
                p.status,
            )
        })
        .collect()
}

pub fn run_scan(
    ctx: &Ctx,
    api: &dyn RobloxApi,
    limit: Option<usize>,
    day: Option<String>,
) -> Result<ScanResult> {
    let _lock = single_instance(&ctx.state_dir(), "scan")?;
    let run = ctx.store.run_start("scan")?;
    match scan_inner(
        ctx,
        api,
        limit.unwrap_or(ctx.settings.chart_limit),
        day.unwrap_or_else(crate::util::today),
    ) {
        Ok(r) => {
            let mut detail = serde_json::to_value(&r).unwrap_or_default();
            detail.as_object_mut().map(|o| o.remove("queue"));
            ctx.store.run_end(run, "ok", &detail.to_string())?;
            Ok(r)
        }
        Err(e) => {
            crate::error!("scan failed: {e:#}");
            ctx.store.run_end(run, "error", &format!("{e:#}"))?;
            Err(e)
        }
    }
}

fn scan_inner(ctx: &Ctx, api: &dyn RobloxApi, limit: usize, day: String) -> Result<ScanResult> {
    let s = &ctx.settings;
    let v = Vault::new(ctx.vault());
    v.init(Some(api))?;
    let main = api.chart(&s.sort_id, limit, &s.country)?;
    if main.is_empty() {
        bail!("Roblox returned an empty chart (API change or outage) — will retry next run");
    }
    crate::info!("chart {}: {} games", s.sort_id, main.len());

    // snapshots for today's chart first, so per-game trends include today
    let infos = api.games_info(&main.iter().map(|e| e.universe_id).collect::<Vec<_>>());
    let snaps: Vec<Snap> = main
        .iter()
        .map(|e| {
            let gi = infos.get(&e.universe_id);
            let g = |k: &str| {
                gi.and_then(|x| x.get(k))
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0)
            };
            Snap {
                day: day.clone(),
                rank: e.rank,
                universe_id: e.universe_id,
                playing: if g("playing") > 0 {
                    g("playing")
                } else {
                    e.playing
                },
                visits: g("visits"),
                favorites: g("favoritedCount"),
                up_votes: e.up_votes,
                down_votes: e.down_votes,
            }
        })
        .collect();
    ctx.store.add_snapshots(&s.sort_id, &day, snaps)?;

    // a few games in parallel: Roblox APIs are slow per call but tolerate light concurrency
    let queue = Mutex::new(main.clone());
    let rows = Mutex::new(Vec::<ChartRow>::new());
    std::thread::scope(|sc| {
        for _ in 0..4 {
            sc.spawn(|| loop {
                let Some(e) = queue.lock().unwrap().pop() else {
                    break;
                };
                let row = match process_game(ctx, api, &v, &e, &day) {
                    Ok(r) => r,
                    Err(err) => {
                        crate::error!("game {} failed: {err:#}", e.universe_id);
                        ChartRow {
                            universe_id: e.universe_id,
                            rank: e.rank,
                            title: clean_display_name(&e.name),
                            genre: e.genre.clone(),
                            playing: e.playing,
                            error: Some(err.to_string()),
                            ..Default::default()
                        }
                    }
                };
                rows.lock().unwrap().push(row);
            });
        }
    });
    let mut rows = rows.into_inner().unwrap();
    rows.sort_by_key(|r| r.rank);

    // extra charts — snapshots for trend history + short tables
    let mut extra: Vec<(String, Vec<ChartRow>)> = vec![];
    for sid in &s.extra_sorts {
        match api.chart(sid, limit, &s.country) {
            Ok(ch) => {
                let info = api.games_info(&ch.iter().map(|c| c.universe_id).collect::<Vec<_>>());
                let games = ctx.store.games();
                let mut snaps = vec![];
                let mut srows = vec![];
                for c in &ch {
                    let gi = info.get(&c.universe_id);
                    let g = |k: &str| {
                        gi.and_then(|x| x.get(k))
                            .and_then(|x| x.as_u64())
                            .unwrap_or(0)
                    };
                    snaps.push(Snap {
                        day: day.clone(),
                        rank: c.rank,
                        universe_id: c.universe_id,
                        playing: c.playing,
                        visits: g("visits"),
                        favorites: g("favoritedCount"),
                        up_votes: c.up_votes,
                        down_votes: c.down_votes,
                    });
                    srows.push(ChartRow {
                        universe_id: c.universe_id,
                        rank: c.rank,
                        title: clean_display_name(&c.name),
                        genre: c.genre.clone(),
                        playing: c.playing,
                        note: games.get(&c.universe_id).and_then(|g| g.note_path.clone()),
                        ..Default::default()
                    });
                }
                ctx.store.add_snapshots(sid, &day, snaps)?;
                extra.push((sort_name(sid), srows));
            }
            Err(e) => crate::warn!("extra sort {sid} failed: {e:#}"),
        }
    }

    let today_ids: HashSet<u64> = rows.iter().map(|r| r.universe_id).collect();
    let new_ids: HashSet<u64> = rows
        .iter()
        .filter(|r| r.days_tracked <= 1)
        .map(|r| r.universe_id)
        .collect();
    let mut dropped = vec![];
    if let Some(prev) = ctx.store.previous_day(&day, &s.sort_id) {
        let games = ctx.store.games();
        for sn in ctx.store.chart_on(&prev, &s.sort_id) {
            if !today_ids.contains(&sn.universe_id) {
                dropped.push((
                    games
                        .get(&sn.universe_id)
                        .map(|g| clean_display_name(&g.name))
                        .unwrap_or(sn.universe_id.to_string()),
                    sn.rank,
                ));
            }
        }
    }
    let sname = sort_name(&s.sort_id);
    let chart_path = v.chart_note(&day, &sname, &rows, &new_ids, &dropped, &extra)?;
    v.genre_notes(&rows)?;
    let queue = playtest_queue(ctx, None, Some(&day));
    v.dashboard(
        &day,
        &sname,
        &rows,
        &new_ids,
        &dropped,
        &queue,
        &recent_playtests(ctx, 8),
        &crate::doctor::short_status(ctx),
    )?;
    if let Err(e) = crate::playbook::render_all(ctx) {
        crate::warn!("playbook refresh failed: {e:#}");
    }
    Ok(ScanResult {
        day: day.clone(),
        chart: sname,
        games: rows.len(),
        new: new_ids.len(),
        dropped: dropped.len(),
        errors: rows.iter().filter(|r| r.error.is_some()).count(),
        chart_note: chart_path.to_string_lossy().into_owned(),
        queue: queue.iter().map(|q| q.title.clone()).collect(),
        rows,
    })
}

fn process_game(
    ctx: &Ctx,
    api: &dyn RobloxApi,
    v: &Vault,
    e: &ChartEntry,
    day: &str,
) -> Result<ChartRow> {
    let s = &ctx.settings;
    let mut d = api.game_details(e.universe_id, true);
    if d.name.is_empty() {
        d.name = e.name.clone();
        d.place_id = if d.place_id > 0 {
            d.place_id
        } else {
            e.place_id
        };
        d.playing = if d.playing > 0 { d.playing } else { e.playing };
        d.up_votes = if d.up_votes > 0 {
            d.up_votes
        } else {
            e.up_votes
        };
        d.down_votes = if d.down_votes > 0 {
            d.down_votes
        } else {
            e.down_votes
        };
        d.genre = if d.genre.is_empty() {
            e.genre.clone()
        } else {
            d.genre.clone()
        };
    }
    let hist = ctx.store.history(e.universe_id, &s.sort_id, 60);
    let tr = A::trend(&hist);
    let row = ctx.store.game(e.universe_id);
    let refresh = row
        .as_ref()
        .is_some_and(|g| days_since(&g.last_seen).is_some_and(|x| x > 7.0));
    let icon = ensure_icon(v, api, e.universe_id, &d.icon_url, refresh);
    let pts = ctx.store.playtests_for(e.universe_id, 10);
    let path = v.update_game_note(
        &mut d,
        &tr,
        Some(e.rank),
        &s.sort_id,
        &hist,
        icon.as_deref(),
        row.as_ref().and_then(|g| g.note_path.as_deref()),
        Some(&pts),
    )?;
    let note_rel = v.rel(&path);
    let best = [
        Some(tr.best_rank).filter(|x| *x > 0),
        row.as_ref().and_then(|g| g.best_rank),
        Some(e.rank),
    ]
    .into_iter()
    .flatten()
    .min();
    let g = ctx.store.upsert_game(e.universe_id, |g| {
        g.place_id = d.place_id;
        g.name = d.name.clone();
        g.creator = d.creator_name.clone();
        g.genre = d.genre.clone();
        g.subgenre = d.subgenre.clone();
        g.last_seen = day.to_string();
        g.best_rank = best;
        g.roblox_created = d.created.clone();
        g.roblox_updated = d.updated.clone();
        g.note_path = Some(note_rel.clone());
    })?;
    Ok(ChartRow {
        universe_id: e.universe_id,
        rank: e.rank,
        title: clean_display_name(&d.name),
        genre: d.genre.clone(),
        subgenre: d.subgenre.clone(),
        playing: d.playing,
        like_ratio: d.like_ratio(),
        note: Some(note_rel),
        passes: d.game_passes.len(),
        last_playtest: g
            .last_playtest
            .map(|x| x.chars().take(10).collect())
            .unwrap_or_default(),
        rank_change_1d: tr.rank_change_1d,
        ccu_change_1d_pct: tr.ccu_change_1d_pct,
        visits_per_day: tr.visits_per_day,
        momentum: tr.momentum,
        flags: tr.flags.clone(),
        days_tracked: tr.days_tracked,
        error: None,
    })
}

/// Refresh a single game note on demand (used by MCP tools).
pub fn refresh_one(ctx: &Ctx, api: &dyn RobloxApi, uid: u64) -> Result<PathBuf> {
    let s = &ctx.settings;
    let v = Vault::new(ctx.vault());
    v.init(None)?;
    let mut d = api.game_details(uid, true);
    if d.name.is_empty() {
        bail!("Roblox has no experience with universe id {uid}");
    }
    let hist = ctx.store.history(uid, &s.sort_id, 60);
    let tr = A::trend(&hist);
    let today = crate::util::today();
    let rank = hist.last().filter(|h| h.day == today).map(|h| h.rank);
    let row = ctx.store.game(uid);
    let icon = ensure_icon(&v, api, uid, &d.icon_url, false);
    let pts = ctx.store.playtests_for(uid, 10);
    let path = v.update_game_note(
        &mut d,
        &tr,
        rank,
        &s.sort_id,
        &hist,
        icon.as_deref(),
        row.as_ref().and_then(|g| g.note_path.as_deref()),
        Some(&pts),
    )?;
    let rel = v.rel(&path);
    ctx.store.upsert_game(uid, |g| {
        g.place_id = d.place_id;
        g.name = d.name.clone();
        g.creator = d.creator_name.clone();
        g.genre = d.genre.clone();
        g.subgenre = d.subgenre.clone();
        g.roblox_created = d.created.clone();
        g.roblox_updated = d.updated.clone();
        g.note_path = Some(rel);
    })?;
    Ok(path)
}
