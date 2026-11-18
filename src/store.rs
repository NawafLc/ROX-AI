//! History store: games, chart snapshots (for trends), playtests, runs, key/values.
//!
//! Plain JSON files guarded by an OS file lock, so the scan, the MCP server (spawned by Claude) and the
//! autopilot can all write safely at the same time. Writes are atomic (temp + rename); a corrupt file is
//! backed up and starts fresh instead of crashing anything.

use crate::util::{atomic_write, now_iso, today};
use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Game {
    pub universe_id: u64,
    pub place_id: u64,
    pub name: String,
    pub creator: String,
    pub genre: String,
    pub subgenre: String,
    pub first_seen: String,
    pub last_seen: String,
    pub best_rank: Option<u32>,
    pub roblox_created: String,
    pub roblox_updated: String,
    pub note_path: Option<String>,
    pub last_playtest: Option<String>,
    pub playtest_count: u32,
    pub playtest_fail_count: u32,
    pub last_fail: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Snap {
    pub day: String,
    pub rank: u32,
    pub universe_id: u64,
    pub playing: u64,
    pub visits: u64,
    pub favorites: u64,
    pub up_votes: u64,
    pub down_votes: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Playtest {
    pub id: u64,
    pub universe_id: u64,
    pub started: String,
    pub finished: Option<String>,
    pub status: String,
    pub note_path: String,
    pub summary: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Run {
    pub id: u64,
    pub kind: String,
    pub started: String,
    pub finished: Option<String>,
    pub status: String,
    pub detail: String,
}

type Games = BTreeMap<u64, Game>;
type Snaps = BTreeMap<String, Vec<Snap>>; // day -> chart rows

#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
}

struct Guard(#[allow(dead_code)] File);

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Store {
        let dir = dir.into();
        let _ = std::fs::create_dir_all(&dir);
        Store { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn lock(&self) -> Result<Guard> {
        let f = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(self.dir.join(".lock"))?;
        f.lock()?;
        Ok(Guard(f))
    }

    fn read<T: DeserializeOwned + Default>(&self, file: &str) -> T {
        let p = self.dir.join(file);
        match std::fs::read(&p) {
            Ok(b) if !b.is_empty() => match serde_json::from_slice(&b) {
                Ok(v) => v,
                Err(e) => {
                    crate::warn!(
                        "store file {file} is corrupt ({e}); backing it up and starting fresh"
                    );
                    let _ = std::fs::copy(
                        &p,
                        self.dir
                            .join(format!("{file}.corrupt-{}", crate::util::rand_hex(6))),
                    );
                    T::default()
                }
            },
            _ => T::default(),
        }
    }

    fn write<T: Serialize>(&self, file: &str, v: &T) -> Result<()> {
        atomic_write(&self.dir.join(file), &serde_json::to_vec(v)?)
    }

    /// Read-modify-write under the cross-process lock.
    fn update<T: DeserializeOwned + Serialize + Default, R>(
        &self,
        file: &str,
        f: impl FnOnce(&mut T) -> R,
    ) -> Result<R> {
        let _g = self.lock()?;
        let mut v: T = self.read(file);
        let r = f(&mut v);
        self.write(file, &v)?;
        Ok(r)
    }

    fn snap_file(sort_id: &str) -> String {
        let clean: String = sort_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("snapshots-{clean}.json")
    }

    // ------------------------------------------------------------ games
    pub fn game(&self, uid: u64) -> Option<Game> {
        self.read::<Games>("games.json").remove(&uid)
    }

    pub fn games(&self) -> Games {
        self.read("games.json")
    }

    /// Create (first_seen = today) or modify a game.
    pub fn upsert_game(&self, uid: u64, f: impl FnOnce(&mut Game)) -> Result<Game> {
        self.update::<Games, _>("games.json", |m| {
            let g = m.entry(uid).or_insert_with(|| Game {
                universe_id: uid,
                first_seen: today(),
                ..Default::default()
            });
            f(g);
            g.clone()
        })
    }

    // -------------------------------------------------------- snapshots
    /// Store one chart for one day (replaces that day's rows for the same games).
    pub fn add_snapshots(&self, sort_id: &str, day: &str, rows: Vec<Snap>) -> Result<()> {
        self.update::<Snaps, _>(&Self::snap_file(sort_id), |m| {
            let e = m.entry(day.to_string()).or_default();
            for r in rows {
                e.retain(|x| x.universe_id != r.universe_id);
                e.push(Snap {
                    day: day.to_string(),
                    ..r
                });
            }
            e.sort_by_key(|s| s.rank);
        })
    }

    /// Oldest -> newest, at most `days` entries.
    pub fn history(&self, uid: u64, sort_id: &str, days: usize) -> Vec<Snap> {
        let m: Snaps = self.read(&Self::snap_file(sort_id));
        let mut out: Vec<Snap> = m
            .values()
            .flat_map(|v| v.iter().filter(|s| s.universe_id == uid).cloned())
            .collect();
        if out.len() > days {
            out = out.split_off(out.len() - days);
        }
        out
    }

    pub fn chart_on(&self, day: &str, sort_id: &str) -> Vec<Snap> {
        let m: Snaps = self.read(&Self::snap_file(sort_id));
        m.get(day).cloned().unwrap_or_default()
    }

    pub fn previous_day(&self, day: &str, sort_id: &str) -> Option<String> {
        let m: Snaps = self.read(&Self::snap_file(sort_id));
        m.keys().rfind(|d| d.as_str() < day).cloned()
    }

    pub fn latest_day(&self, sort_id: &str) -> Option<String> {
        let m: Snaps = self.read(&Self::snap_file(sort_id));
        m.keys().next_back().cloned()
    }

    // ------------------------------------------------------------- runs
    pub fn run_start(&self, kind: &str) -> Result<u64> {
        self.update::<Vec<Run>, _>("runs.json", |v| {
            let id = v.last().map(|r| r.id + 1).unwrap_or(1);
            v.push(Run {
                id,
                kind: kind.into(),
                started: now_iso(),
                status: "running".into(),
                ..Default::default()
            });
            if v.len() > 1000 {
                v.drain(..v.len() - 1000);
            }
            id
        })
    }

    pub fn run_end(&self, id: u64, status: &str, detail: &str) -> Result<()> {
        self.update::<Vec<Run>, _>("runs.json", |v| {
            if let Some(r) = v.iter_mut().find(|r| r.id == id) {
                r.finished = Some(now_iso());
                r.status = status.into();
                r.detail = detail.chars().take(4000).collect();
            }
        })
    }

    pub fn last_run(&self, kind: &str, status: Option<&str>) -> Option<Run> {
        let v: Vec<Run> = self.read("runs.json");
        v.into_iter()
            .rev()
            .find(|r| r.kind == kind && status.is_none_or(|s| r.status == s))
    }

    // -------------------------------------------------------- playtests
    pub fn playtest_start(&self, uid: u64) -> Result<u64> {
        self.update::<Vec<Playtest>, _>("playtests.json", |v| {
            let id = v.last().map(|r| r.id + 1).unwrap_or(1);
            v.push(Playtest {
                id,
                universe_id: uid,
                started: now_iso(),
                status: "running".into(),
                ..Default::default()
            });
            id
        })
    }

    pub fn playtest_end(&self, id: u64, status: &str, note: &str, summary: &str) -> Result<()> {
        self.update::<Vec<Playtest>, _>("playtests.json", |v| {
            if let Some(p) = v.iter_mut().find(|p| p.id == id) {
                p.finished = Some(now_iso());
                p.status = status.into();
                p.note_path = note.into();
                p.summary = summary.chars().take(2000).collect();
            }
        })
    }

    pub fn playtest(&self, id: u64) -> Option<Playtest> {
        self.read::<Vec<Playtest>>("playtests.json")
            .into_iter()
            .find(|p| p.id == id)
    }

    pub fn playtests_for(&self, uid: u64, n: usize) -> Vec<Playtest> {
        let v: Vec<Playtest> = self.read("playtests.json");
        v.into_iter()
            .rev()
            .filter(|p| p.universe_id == uid)
            .take(n)
            .collect()
    }

    pub fn recent_playtests(&self, n: usize) -> Vec<Playtest> {
        let v: Vec<Playtest> = self.read("playtests.json");
        v.into_iter().rev().take(n).collect()
    }

    pub fn count_playtests(&self, status: &str) -> usize {
        self.read::<Vec<Playtest>>("playtests.json")
            .iter()
            .filter(|p| p.status == status)
            .count()
    }

    // --------------------------------------------------------------- kv
    pub fn get(&self, k: &str) -> Option<Value> {
        let m: BTreeMap<String, Value> = self.read("kv.json");
        m.get(k).cloned().filter(|v| !v.is_null())
    }

    pub fn set(&self, k: &str, v: Value) -> Result<()> {
        self.update::<BTreeMap<String, Value>, _>("kv.json", |m| {
            if v.is_null() {
                m.remove(k);
            } else {
                m.insert(k.to_string(), v);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::new(d.path());
        s.upsert_game(5, |g| g.name = "A".into()).unwrap();
        assert_eq!(s.game(5).unwrap().name, "A");
        assert_eq!(s.game(5).unwrap().first_seen, today());
        s.add_snapshots(
            "top-trending",
            "2026-01-01",
            vec![Snap {
                rank: 2,
                universe_id: 5,
                playing: 10,
                ..Default::default()
            }],
        )
        .unwrap();
        s.add_snapshots(
            "top-trending",
            "2026-01-02",
            vec![Snap {
                rank: 1,
                universe_id: 5,
                playing: 20,
                ..Default::default()
            }],
        )
        .unwrap();
        assert_eq!(s.history(5, "top-trending", 60).len(), 2);
        assert_eq!(
            s.previous_day("2026-01-02", "top-trending").as_deref(),
            Some("2026-01-01")
        );
        assert_eq!(s.latest_day("top-trending").as_deref(), Some("2026-01-02"));
        let r = s.run_start("scan").unwrap();
        s.run_end(r, "ok", "x").unwrap();
        assert_eq!(s.last_run("scan", Some("ok")).unwrap().id, r);
        s.set("k", serde_json::json!({"a": 1})).unwrap();
        assert_eq!(s.get("k").unwrap()["a"], 1);
        s.set("k", Value::Null).unwrap();
        assert!(s.get("k").is_none());
    }

    #[test]
    fn corrupt_file_recovers() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("games.json"), b"{not json").unwrap();
        let s = Store::new(d.path());
        assert!(s.games().is_empty());
        s.upsert_game(1, |_| {}).unwrap();
        assert!(s.game(1).is_some());
    }

    #[test]
    fn concurrent_writers_do_not_lose_updates() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::new(d.path());
        let hs: Vec<_> = (0..8)
            .map(|i| {
                let s = s.clone();
                std::thread::spawn(move || {
                    for j in 0..10 {
                        s.upsert_game(i * 100 + j, |_| {}).unwrap();
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        assert_eq!(s.games().len(), 80);
    }
}
