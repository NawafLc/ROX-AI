//! Settings and filesystem locations. Everything lives in one per-user folder:
//!   Windows: %LOCALAPPDATA%\ROX-AI      other: ~/.roxai      override: ROXAI_HOME

use crate::store::Store;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Folder that holds the exe (the portable app folder).
pub fn app_folder() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn writable(dir: &Path) -> bool {
    let _ = std::fs::create_dir_all(dir);
    let probe = dir.join(".write-test");
    let ok = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Portable by default: everything lives in a `data` folder next to the exe.
/// (Override with ROXAI_HOME; falls back to the user profile only if the app folder is read-only.)
pub fn default_home() -> PathBuf {
    if let Ok(h) = std::env::var("ROXAI_HOME") {
        if !h.trim().is_empty() {
            return PathBuf::from(h);
        }
    }
    let portable = app_folder().join("data");
    if writable(&portable) {
        return portable;
    }
    if cfg!(windows) {
        if let Ok(l) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(l).join("ROX-AI");
        }
    }
    home_dir().join(".roxai")
}

/// Old (installed) location used by versions before the portable app.
pub fn legacy_home() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("ROX-AI"))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    // vault
    pub vault_path: String,
    // charts
    pub sort_id: String,
    pub chart_limit: usize,
    pub extra_sorts: Vec<String>,
    pub country: String,
    // playtesting
    pub playtest_minutes: u32,
    pub playtests_per_run: usize,
    pub replay_after_days: f64,
    pub replay_if_updated: bool,
    pub playtest_interval_days: f64,
    pub idle_required_minutes: f64,
    pub screenshot_width: u32,
    pub jpeg_quality: u8,
    pub window_width: i32,
    pub window_height: i32,
    // Claude Code CLI (autopilot)
    pub claude_cli: String,
    pub claude_model: String,
    pub max_turns: u32,
    pub per_game_timeout_minutes: u64,
    // schedule (local time)
    pub scan_time: String,
    pub playtest_check_time: String,
    // safety
    pub kill_hotkey: String,
    pub allow_chat: bool,
    pub stop_on_human_input: bool,
    /// daily scan + weekly playtests in the background (Windows scheduled tasks); off = only when you click
    pub auto_run: bool,
    /// "passive" = only track chart data · "active" = Claude also learns each game (from YouTube videos)
    pub mode: String,
    /// AI playtesting on the real Roblox client (turned off for now; video learning replaces it)
    pub playtesting_enabled: bool,
    /// games Claude learns per run, how often learning runs, and when to re-learn a game
    pub learn_per_run: usize,
    pub learn_interval_days: f64,
    pub reanalyze_after_days: f64,
    pub videos_per_game: usize,
    /// genres / themes / mechanics to never build (comma separated, used by the idea pipeline)
    pub avoid: String,
    /// playbook chapters Claude rewrites per learning run, and how old a chapter must be to be refreshed
    pub playbook_chapters_per_run: usize,
    pub playbook_refresh_days: f64,
    // the AI team (game building)
    /// build games in the background
    pub team_enabled: bool,
    /// how many finished games the team should make (it builds them one at a time)
    pub games_to_build: usize,
    /// biggest game the Lead may pick: "small" or "medium" (large is off for now)
    pub max_game_size: String,
    /// false = one agent at a time · true = several agents at once
    pub team_parallel: bool,
    pub team_max_parallel: usize,
    /// per-agent model ("opus", "sonnet", "haiku"); missing = the agent's default
    pub agent_models: std::collections::BTreeMap<String, String>,
    /// per-agent thinking effort ("low", "medium", "high"); missing = by task (deep for design, planning, fixes, reviews)
    pub agent_effort: std::collections::BTreeMap<String, String>,
    /// safety cap: agent runs per game before the Lead must wrap up
    pub max_agent_runs_per_game: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            vault_path: String::new(), // empty = "ROX-AI Vault" next to the data folder (portable)
            sort_id: "top-trending".into(),
            chart_limit: 25,
            extra_sorts: vec!["top-playing-now".into(), "up-and-coming".into()],
            country: "all".into(),
            playtest_minutes: 12,
            playtests_per_run: 8,
            replay_after_days: 28.0,
            replay_if_updated: true,
            playtest_interval_days: 7.0,
            idle_required_minutes: 3.0,
            screenshot_width: 1280,
            jpeg_quality: 70,
            window_width: 1280,
            window_height: 720,
            claude_cli: String::new(),
            claude_model: String::new(),
            max_turns: 160,
            per_game_timeout_minutes: 35,
            scan_time: "10:17".into(),
            playtest_check_time: "14:23".into(),
            kill_hotkey: "ctrl+shift+q".into(),
            allow_chat: false,
            stop_on_human_input: true,
            auto_run: true,
            mode: "active".into(),
            playtesting_enabled: false,
            learn_per_run: 5,
            learn_interval_days: 1.0,
            reanalyze_after_days: 30.0,
            videos_per_game: 4,
            avoid: String::new(),
            playbook_chapters_per_run: 1,
            playbook_refresh_days: 7.0,
            team_enabled: false,
            games_to_build: 1,
            max_game_size: "medium".into(),
            team_parallel: false,
            team_max_parallel: 3,
            agent_models: Default::default(),
            agent_effort: Default::default(),
            max_agent_runs_per_game: 120,
        }
    }
}

impl Settings {
    pub fn path(home: &Path) -> PathBuf {
        home.join("settings.json")
    }

    /// Load settings; a corrupt file never bricks the app (it is backed up and defaults are used).
    pub fn load(home: &Path) -> Settings {
        let p = Self::path(home);
        let s = match std::fs::read_to_string(&p) {
            Ok(txt) => match serde_json::from_str::<Settings>(&txt) {
                Ok(s) => s,
                Err(_) => {
                    let _ = std::fs::rename(&p, home.join("settings.corrupt.json"));
                    Settings::default()
                }
            },
            Err(_) => Settings::default(),
        };
        let _ = s.save(home);
        s
    }

    pub fn save(&self, home: &Path) -> anyhow::Result<()> {
        crate::util::atomic_write(
            &Self::path(home),
            serde_json::to_string_pretty(self)?.as_bytes(),
        )
    }

    /// Set a field from text (used by `roxai set key value`).
    pub fn set_from_str(&mut self, key: &str, value: &str) -> anyhow::Result<()> {
        let mut v = serde_json::to_value(&*self)?;
        let obj = v.as_object_mut().unwrap();
        let cur = obj
            .get(key)
            .ok_or_else(|| anyhow::anyhow!("unknown setting '{key}'"))?;
        let new = match cur {
            serde_json::Value::Bool(_) => serde_json::Value::Bool(matches!(
                value.to_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )),
            serde_json::Value::Number(n) if n.is_u64() || n.is_i64() => {
                serde_json::json!(value.parse::<i64>()?)
            }
            serde_json::Value::Number(_) => serde_json::json!(value.parse::<f64>()?),
            serde_json::Value::Array(_) => serde_json::json!(value
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()),
            _ => serde_json::Value::String(value.to_string()),
        };
        obj.insert(key.to_string(), new);
        *self = serde_json::from_value(v)?;
        Ok(())
    }
}

/// Everything a component needs: where the app lives, its settings, and the data store.
#[derive(Clone)]
pub struct Ctx {
    pub home: PathBuf,
    pub settings: Settings,
    pub store: Store,
}

impl Ctx {
    pub fn load() -> Ctx {
        Ctx::at(default_home())
    }

    pub fn at(home: impl Into<PathBuf>) -> Ctx {
        let home = home.into();
        let _ = std::fs::create_dir_all(&home);
        let settings = Settings::load(&home);
        let store = Store::new(home.join("data"));
        Ctx {
            home,
            settings,
            store,
        }
    }

    pub fn reload(&mut self) {
        self.settings = Settings::load(&self.home);
    }

    /// The Obsidian vault: settings.vault_path if set, else "ROX-AI Vault" beside the data folder.
    pub fn is_active(&self) -> bool {
        self.settings.mode != "passive"
    }

    /// ROX-AI's own helper tools (e.g. the video reader) — inside the portable data folder.
    pub fn tools_dir(&self) -> PathBuf {
        self.home.join("tools")
    }

    pub fn vault(&self) -> PathBuf {
        if self.settings.vault_path.trim().is_empty() {
            self.home
                .parent()
                .unwrap_or(&self.home)
                .join("ROX-AI Vault")
        } else {
            PathBuf::from(&self.settings.vault_path)
        }
    }

    pub fn logs_dir(&self) -> PathBuf {
        let p = self.home.join("logs");
        let _ = std::fs::create_dir_all(&p);
        p
    }

    pub fn state_dir(&self) -> PathBuf {
        let p = self.home.join("state");
        let _ = std::fs::create_dir_all(&p);
        p
    }

    pub fn stop_file(&self) -> PathBuf {
        self.state_dir().join("STOP")
    }

    pub fn session_file(&self) -> PathBuf {
        self.state_dir().join("session.json")
    }
}
