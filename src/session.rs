//! One AI playtest session, wrapped in fail-safes.
//!
//! Any one of these stops the session cleanly (independently of each other):
//!   * kill hotkey (default Ctrl+Shift+Q) and the panel's STOP button (state/STOP file)
//!   * touching your physical mouse/keyboard (human input monitor)
//!   * hard time budget + grace period, then Roblox is force-closed
//!   * Roblox crash / window gone / window hung > 30 s / workstation locked
//!   * every held key/button is released on any exit path
//!   * progress (timeline, screenshots) is written to the vault as it happens, so nothing is lost
//!   * a session left 'running' by a crashed process is marked 'interrupted' on next start

use crate::api::RobloxApi;
use crate::config::Ctx;
use crate::util::{now_iso, today};
use crate::vault::Vault;
use crate::win::{capture, capture::Shot, humanmon, input as IN, roblox as RB, window as W, Hwnd};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub fn request_stop(ctx: &Ctx, reason: &str) {
    let _ = std::fs::write(ctx.stop_file(), reason);
}
pub fn clear_stop(ctx: &Ctx) {
    let _ = std::fs::remove_file(ctx.stop_file());
}
pub fn stop_requested(ctx: &Ctx) -> Option<String> {
    std::fs::read_to_string(ctx.stop_file()).ok().map(|s| {
        if s.trim().is_empty() {
            "stop requested".into()
        } else {
            s
        }
    })
}

pub struct Session {
    pub universe_id: u64,
    pub place_id: u64,
    pub name: String,
    pub budget_s: f64,
    pub playtest_id: u64,
    pub note: PathBuf,
    pub game_note: Option<PathBuf>,
    pub started: Instant,
    pub started_ms: u64,
    pub started_iso: String,
    pub hwnd: Option<Hwnd>,
    pub ended: bool,
    pub actions: u32,
    pub shots_saved: u32,
    pub last_shot: Option<Arc<Shot>>,
    pub hung_since: Option<Instant>,
}

impl Session {
    pub fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }
    pub fn remaining(&self) -> f64 {
        self.budget_s - self.elapsed()
    }
    pub fn clock(&self) -> String {
        let e = self.elapsed() as u64;
        format!("{:02}:{:02}", e / 60, e % 60)
    }
    pub fn status_line(&self) -> String {
        let r = self.remaining();
        if r <= 0.0 {
            return format!("⛔ time budget used ({}). Only look/snap/note/playtest_finish allowed now — finish now.", self.clock());
        }
        let warn = if r < 120.0 {
            " ⚠️ under 2 minutes left: wrap up, take final screenshots and call playtest_finish."
        } else {
            ""
        };
        format!(
            "⏱️ {} elapsed, {}m{:02}s left · {} actions.{warn}",
            self.clock(),
            r as u64 / 60,
            r as u64 % 60,
            self.actions
        )
    }
}

pub struct Manager {
    pub ctx: Ctx,
    pub cur: Mutex<Option<Session>>,
    /// Set by the watchdog / aborts without needing the session lock (instant reaction).
    pub abort: Mutex<Option<String>>,
    wd_running: AtomicBool,
}

static MANAGER: OnceLock<Arc<Manager>> = OnceLock::new();

pub fn manager(ctx: &Ctx) -> Arc<Manager> {
    MANAGER
        .get_or_init(|| {
            Arc::new(Manager {
                ctx: ctx.clone(),
                cur: Mutex::new(None),
                abort: Mutex::new(None),
                wd_running: AtomicBool::new(false),
            })
        })
        .clone()
}

impl Manager {
    pub fn new(ctx: &Ctx) -> Manager {
        Manager {
            ctx: ctx.clone(),
            cur: Mutex::new(None),
            abort: Mutex::new(None),
            wd_running: AtomicBool::new(false),
        }
    }

    fn vault(&self) -> Vault {
        Vault::new(self.ctx.vault())
    }

    pub fn abort_reason(&self) -> Option<String> {
        self.abort.lock().unwrap().clone()
    }

    fn set_abort(&self, r: &str) {
        let mut a = self.abort.lock().unwrap();
        if a.is_none() {
            *a = Some(r.to_string());
        }
    }

    /// Raise if the session is over (used inside long actions; lock-free w.r.t. the session).
    pub fn check(&self) -> Result<()> {
        if let Some(r) = self.abort_reason() {
            bail!("Session ended: {r}. Call playtest_finish to save your report (it still works after an abort).");
        }
        Ok(())
    }

    fn persist(&self, s: &Session) {
        let _ = crate::util::atomic_write(&self.ctx.session_file(), json!({
            "universe_id": s.universe_id, "name": s.name, "playtest_id": s.playtest_id, "note": s.note,
            "started": s.started_iso, "budget_s": s.budget_s, "elapsed": s.elapsed() as u64, "pid": std::process::id(),
            "ended": s.ended, "abort": self.abort_reason(),
        }).to_string().as_bytes());
    }

    /// A previous process died mid-session: mark it interrupted so the queue retries it later.
    pub fn recover_stale(&self) {
        let p = self.ctx.session_file();
        let Ok(txt) = std::fs::read_to_string(&p) else {
            return;
        };
        let Ok(d) = serde_json::from_str::<Value>(&txt) else {
            let _ = std::fs::remove_file(&p);
            return;
        };
        let pid = d["pid"].as_u64().unwrap_or(0) as u32;
        if d["ended"].as_bool().unwrap_or(false) {
            let _ = std::fs::remove_file(&p);
            return;
        }
        if pid != 0 && pid != std::process::id() && crate::util::pid_exists(pid) {
            return; // still running in another process
        }
        if pid == std::process::id() && self.cur.lock().unwrap().as_ref().is_some_and(|s| !s.ended)
        {
            return;
        }
        let v = self.vault();
        let note = PathBuf::from(d["note"].as_str().unwrap_or(""));
        if let Some(id) = d["playtest_id"].as_u64() {
            let _ = self.ctx.store.playtest_end(
                id,
                "interrupted",
                &v.rel(&note),
                "process stopped unexpectedly",
            );
        }
        if let Some(uid) = d["universe_id"].as_u64() {
            let _ = self.ctx.store.upsert_game(uid, |g| {
                g.playtest_fail_count += 1;
                g.last_fail = Some(now_iso());
            });
        }
        if note.exists() {
            let _ = v.set_fm_value(&note, "status", json!("interrupted"));
        }
        let _ = std::fs::remove_file(&p);
        crate::warn!("recovered stale session for {}", d["name"]);
    }

    pub fn start(
        self: &Arc<Self>,
        api: &dyn RobloxApi,
        uid: u64,
        minutes: Option<u32>,
    ) -> Result<String> {
        {
            let cur = self.cur.lock().unwrap();
            if let Some(s) = cur.as_ref().filter(|s| !s.ended) {
                bail!(
                    "A playtest of '{}' is already running — call playtest_finish first.",
                    s.name
                );
            }
        }
        self.recover_stale();
        clear_stop(&self.ctx);
        *self.abort.lock().unwrap() = None;
        let st = &self.ctx.settings;
        W::make_dpi_aware();
        if W::is_locked() {
            bail!("The PC is locked — playtests need an unlocked, logged-in desktop.");
        }
        if !RB::is_installed() {
            bail!("Roblox Player is not installed. Install it from roblox.com/download and log in with the alt account.");
        }
        let mut g = self.ctx.store.game(uid);
        if g.as_ref().is_none_or(|g| g.place_id == 0) {
            crate::scan::refresh_one(&self.ctx, api, uid)?;
            g = self.ctx.store.game(uid);
        }
        let g = g
            .filter(|g| g.place_id > 0)
            .ok_or_else(|| anyhow!("Unknown game {uid}"))?;
        let minutes = minutes.unwrap_or(st.playtest_minutes).clamp(3, 45);
        let v = self.vault();
        let game_note = g.note_path.as_ref().map(|n| v.abs(n));
        let note = v.playtest_note_path(&g.name);
        v.create_playtest_note(&note, game_note.as_deref(), &g.name, uid, minutes)?;
        let pid = self.ctx.store.playtest_start(uid)?;
        let s = Session {
            universe_id: uid,
            place_id: g.place_id,
            name: g.name.clone(),
            budget_s: minutes as f64 * 60.0,
            playtest_id: pid,
            note: note.clone(),
            game_note,
            started: Instant::now(),
            started_ms: humanmon::now_ms(),
            started_iso: now_iso(),
            hwnd: None,
            ended: false,
            actions: 0,
            shots_saved: 0,
            last_shot: None,
            hung_since: None,
        };
        self.persist(&s);
        *self.cur.lock().unwrap() = Some(s);

        let launched = (|| -> Result<Option<Hwnd>> {
            RB::kill_all();
            std::thread::sleep(Duration::from_millis(1500));
            RB::launch(g.place_id)?;
            RB::wait_for_window(
                Duration::from_secs(180),
                &|| match stop_requested(&self.ctx) {
                    Some(r) => bail!("Stopped: {r}"),
                    None => Ok(()),
                },
            )
        })();
        let hwnd = match launched {
            Ok(Some(h)) => h,
            Ok(None) => {
                self.fail("Roblox window never appeared (not installed, update stuck, or a login screen).");
                bail!("Roblox did not open within 3 minutes. Playtest cancelled.");
            }
            Err(e) => {
                self.fail(&format!("launch failed: {e}"));
                return Err(e);
            }
        };
        if !W::is_fullscreen(hwnd) {
            W::resize_client(hwnd, st.window_width, st.window_height);
        }
        W::focus(hwnd);
        if st.stop_on_human_input {
            humanmon::start();
        }
        {
            let mut cur = self.cur.lock().unwrap();
            if let Some(s) = cur.as_mut() {
                s.hwnd = Some(hwnd);
                // restart the clock after loading so the budget is real play time
                s.started = Instant::now();
                s.started_ms = humanmon::now_ms();
            }
        }
        self.start_watchdog();
        self.timeline(&format!(
            "Joined game (window ready). Budget {minutes} min."
        ))?;
        Ok(format!(
            "🎮 Playtest started: {} (place {}). Vault log: {}.",
            g.name,
            g.place_id,
            v.rel(&note)
        ))
    }

    fn fail(&self, reason: &str) {
        if let Some(note) = self.cur.lock().unwrap().as_ref().map(|s| s.note.clone()) {
            let _ = self
                .vault()
                .append_to_block(&note, "auto", "problems", &format!("- {reason}"));
        }
        self.set_abort(reason);
        self.end("failed", reason);
    }

    /// Idempotent shutdown: release input, close Roblox, update store and notes.
    pub fn end(
        &self,
        status: &str,
        summary: &str,
    ) -> Option<(PathBuf, Option<PathBuf>, u64, f64, u32)> {
        let mut cur = self.cur.lock().unwrap();
        let s = cur.as_mut()?;
        if s.ended {
            return Some((
                s.note.clone(),
                s.game_note.clone(),
                s.universe_id,
                s.elapsed(),
                s.shots_saved,
            ));
        }
        s.ended = true;
        IN::release_all();
        humanmon::stop();
        RB::kill_all();
        let v = self.vault();
        let _ = self
            .ctx
            .store
            .playtest_end(s.playtest_id, status, &v.rel(&s.note), summary);
        let uid = s.universe_id;
        if status == "completed" {
            let _ = self.ctx.store.upsert_game(uid, |g| {
                g.last_playtest = Some(now_iso());
                g.playtest_count += 1;
                g.playtest_fail_count = 0;
            });
        } else {
            let _ = self.ctx.store.upsert_game(uid, |g| {
                g.playtest_fail_count += 1;
                g.last_fail = Some(now_iso());
            });
        }
        let _ = v.set_fm_value(&s.note, "status", json!(status));
        let _ = v.set_fm_value(
            &s.note,
            "played_minutes",
            json!((s.elapsed() / 6.0).round() / 10.0),
        );
        if let Some(r) = self.abort_reason() {
            let _ = v.append_to_block(
                &s.note,
                "auto",
                "problems",
                &format!("- Session ended: {r}"),
            );
        }
        if let Some(gn) = s.game_note.as_ref().filter(|g| g.exists()) {
            let pts = self.ctx.store.playtests_for(uid, 10);
            let _ = v.set_block_in(
                gn,
                "auto",
                "playtests",
                &crate::vault::playtest_history_md(&pts),
            );
        }
        self.set_abort(&format!("session {status}"));
        self.persist(s);
        let _ = std::fs::remove_file(self.ctx.session_file());
        crate::info!("session {} ended: {status}", s.name);
        Some((
            s.note.clone(),
            s.game_note.clone(),
            uid,
            s.elapsed(),
            s.shots_saved,
        ))
    }

    // ---------------------------------------------------------- watchdog
    fn start_watchdog(self: &Arc<Self>) {
        if self.wd_running.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            me.watchdog();
            me.wd_running.store(false, Ordering::SeqCst);
        });
    }

    fn watchdog(&self) {
        let st = self.ctx.settings.clone();
        let grace = 5.0 * 60.0;
        let mut last_persist = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(200));
            let (ended, remaining, started_ms) = match self.cur.lock().unwrap().as_ref() {
                Some(s) => (s.ended, s.remaining(), s.started_ms),
                None => return,
            };
            if ended {
                return;
            }
            let mut reason: Option<String> = None;
            if W::hotkey_down(&st.kill_hotkey) {
                reason = Some(format!("kill hotkey {} pressed", st.kill_hotkey));
            } else if let Some(r) = stop_requested(&self.ctx) {
                reason = Some(format!("stop requested ({r})"));
            } else if st.stop_on_human_input && humanmon::human_since(started_ms + 2000) {
                reason = Some("you used your mouse/keyboard — AI stepped back".into());
            } else if remaining < -grace {
                reason = Some("time budget + grace period exceeded (Claude never finished)".into());
            } else if !RB::is_running() {
                reason = Some("Roblox closed or crashed".into());
            } else if W::is_locked() {
                reason = Some("PC got locked".into());
            } else if let Some(h) = RB::find_window() {
                let mut cur = self.cur.lock().unwrap();
                if let Some(s) = cur.as_mut() {
                    if W::is_hung(h) {
                        let since = *s.hung_since.get_or_insert_with(Instant::now);
                        if since.elapsed() > Duration::from_secs(30) {
                            reason = Some("Roblox window frozen for 30 s".into());
                        }
                    } else {
                        s.hung_since = None;
                    }
                }
            }
            if let Some(r) = reason {
                crate::warn!("watchdog abort: {r}");
                self.set_abort(&r);
                IN::release_all();
                let status = if r.contains("budget") {
                    "timeout"
                } else if r.contains("hotkey") || r.contains("stop") || r.contains("mouse") {
                    "stopped"
                } else {
                    "crashed"
                };
                self.end(status, &r);
                return;
            }
            if last_persist.elapsed() > Duration::from_secs(5) {
                if let Some(s) = self.cur.lock().unwrap().as_ref() {
                    self.persist(s);
                }
                last_persist = Instant::now();
            }
        }
    }

    // ----------------------------------------------------------- actions
    pub fn status_line(&self) -> String {
        self.cur
            .lock()
            .unwrap()
            .as_ref()
            .filter(|s| !s.ended)
            .map(|s| s.status_line())
            .unwrap_or_default()
    }

    pub fn look(&self, grid: bool) -> Result<Arc<Shot>> {
        let (hwnd, ended) = {
            let cur = self.cur.lock().unwrap();
            let s = cur.as_ref().ok_or_else(|| {
                anyhow!("No playtest running. Call playtest_start(universe_id) first.")
            })?;
            (s.hwnd, s.ended)
        };
        if ended {
            bail!("Session ended: {}", self.abort_reason().unwrap_or_default());
        }
        let h = hwnd.ok_or_else(|| anyhow!("No Roblox window."))?;
        if W::is_minimized(h) {
            W::focus(h);
        }
        let r = W::client_rect(h)
            .filter(|r| r.width >= 100)
            .ok_or_else(|| anyhow!("Roblox window has no visible area."))?;
        let shot = Arc::new(capture::grab(
            r,
            self.ctx.settings.screenshot_width,
            self.ctx.settings.jpeg_quality,
            grid,
        )?);
        if let Some(s) = self.cur.lock().unwrap().as_mut() {
            s.last_shot = Some(shot.clone());
        }
        Ok(shot)
    }

    fn to_screen(shot: &Shot, x: f64, y: f64) -> Result<(i32, i32)> {
        if !(0.0..=shot.width as f64).contains(&x) || !(0.0..=shot.height as f64).contains(&y) {
            bail!(
                "({x},{y}) is outside the screenshot ({}x{}).",
                shot.width,
                shot.height
            );
        }
        Ok((
            (shot.rect.left as f64 + x * shot.scale) as i32,
            (shot.rect.top as f64 + y * shot.scale) as i32,
        ))
    }

    pub fn act(&self, actions: &[Value]) -> Result<Vec<String>> {
        let (hwnd, shot) = {
            let cur = self.cur.lock().unwrap();
            let s = cur.as_ref().ok_or_else(|| {
                anyhow!("No playtest running. Call playtest_start(universe_id) first.")
            })?;
            if s.ended {
                bail!(
                    "Session ended: {}. Call playtest_finish to save your report.",
                    self.abort_reason().unwrap_or_default()
                );
            }
            if s.remaining() <= 0.0 {
                bail!("Time budget used up. Take a final look/snap if needed and call playtest_finish now.");
            }
            (s.hwnd, s.last_shot.clone())
        };
        self.check()?;
        let h = hwnd
            .or_else(RB::find_window)
            .ok_or_else(|| anyhow!("Roblox window not found."))?;
        if W::foreground() != h && !W::focus(h) {
            bail!("Could not focus the Roblox window (another window is on top).");
        }
        let res = self.run_actions(actions, shot.as_deref());
        IN::release_all();
        let done = res?;
        if let Some(s) = self.cur.lock().unwrap().as_mut() {
            s.actions += done.len() as u32;
        }
        Ok(done)
    }

    fn run_actions(&self, actions: &[Value], shot: Option<&Shot>) -> Result<Vec<String>> {
        let chk = || self.check();
        let f = |a: &Value, k: &str, d: f64| {
            a.get(k)
                .and_then(|x| {
                    x.as_f64()
                        .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
                })
                .unwrap_or(d)
        };
        let need_shot = || {
            shot.ok_or_else(|| anyhow!("Call look() first so coordinates refer to a screenshot."))
        };
        let mut done = vec![];
        for (i, a) in actions.iter().take(25).enumerate() {
            self.check()?;
            let t = a
                .get("type")
                .or_else(|| a.get("do"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_lowercase();
            match t.as_str() {
                "key" => {
                    let k = a
                        .get("key")
                        .and_then(|x| x.as_str())
                        .ok_or_else(|| anyhow!("key action needs 'key'"))?;
                    if k.trim() == "/" && !self.ctx.settings.allow_chat {
                        bail!("Opening public chat is disabled (allow_chat=false).");
                    }
                    for _ in 0..(f(a, "times", 1.0) as u32).clamp(1, 20) {
                        IN::tap(k, f(a, "hold", 0.06))?;
                        std::thread::sleep(Duration::from_millis(80));
                    }
                    done.push(format!("key {k}"));
                }
                "keys" => {
                    let keys: Vec<String> = a
                        .get("keys")
                        .and_then(|x| x.as_array())
                        .map(|v| {
                            v.iter()
                                .filter_map(|k| k.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    if keys.iter().any(|k| k.trim() == "/") && !self.ctx.settings.allow_chat {
                        bail!("Opening public chat is disabled.");
                    }
                    IN::hold_keys(&keys, f(a, "seconds", 0.3), &chk)?;
                    done.push(format!("hold {}", keys.join("+")));
                }
                "walk" => {
                    let dir = a
                        .get("dir")
                        .or_else(|| a.get("direction"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("w")
                        .to_lowercase();
                    let mut keys: Vec<String> = dir
                        .chars()
                        .filter(|c| "wasd".contains(*c))
                        .map(|c| c.to_string())
                        .collect();
                    if keys.is_empty() {
                        keys.push("w".into());
                    }
                    if a.get("sprint").and_then(|x| x.as_bool()).unwrap_or(false) {
                        keys.push("shift".into());
                    }
                    if a.get("jump").and_then(|x| x.as_bool()).unwrap_or(false) {
                        keys.push("space".into());
                    }
                    let secs = f(a, "seconds", 1.0);
                    IN::hold_keys(&keys, secs, &chk)?;
                    done.push(format!("walk {dir} {secs}s"));
                }
                "jump" => {
                    for _ in 0..(f(a, "times", 1.0) as u32).clamp(1, 10) {
                        IN::tap("space", 0.1)?;
                        std::thread::sleep(Duration::from_millis(350));
                    }
                    done.push("jump".into());
                }
                "click" | "hold_click" | "press_hold" => {
                    let (x, y) = Self::to_screen(need_shot()?, f(a, "x", -1.0), f(a, "y", -1.0))?;
                    let btn = a.get("button").and_then(|x| x.as_str()).unwrap_or("left");
                    if t == "click" {
                        IN::click_at(
                            x,
                            y,
                            btn,
                            a.get("double").and_then(|x| x.as_bool()).unwrap_or(false),
                            f(a, "hold", 0.05),
                        )?;
                    } else {
                        IN::click_at(x, y, btn, false, f(a, "seconds", 1.0).min(15.0))?;
                    }
                    done.push(format!("{t} {},{}", f(a, "x", 0.0), f(a, "y", 0.0)));
                }
                "move_mouse" => {
                    let (x, y) = Self::to_screen(need_shot()?, f(a, "x", -1.0), f(a, "y", -1.0))?;
                    IN::move_abs(x, y)?;
                    done.push("move mouse".into());
                }
                "camera" => {
                    IN::drag_camera(f(a, "dx", 0.0) as i32, f(a, "dy", 0.0) as i32)?;
                    done.push(format!("camera {},{}", f(a, "dx", 0.0), f(a, "dy", 0.0)));
                }
                "scroll" => {
                    IN::wheel(f(a, "clicks", -3.0) as i32)?;
                    done.push(format!("scroll {}", f(a, "clicks", -3.0)));
                }
                "type" => {
                    let text: String = a
                        .get("text")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .chars()
                        .take(200)
                        .collect();
                    IN::type_text(&text)?;
                    if a.get("enter").and_then(|x| x.as_bool()).unwrap_or(false) {
                        IN::tap("enter", 0.06)?;
                    }
                    done.push(format!("type {} chars", text.chars().count()));
                }
                "wait" => {
                    let end = Instant::now()
                        + Duration::from_secs_f64(f(a, "seconds", 1.0).clamp(0.0, 30.0));
                    while Instant::now() < end {
                        self.check()?;
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    done.push(format!("wait {}s", f(a, "seconds", 1.0)));
                }
                other => bail!("action #{}: unknown type '{other}'", i + 1),
            }
            std::thread::sleep(Duration::from_secs_f64(f(a, "pause", 0.12).clamp(0.0, 5.0)));
        }
        Ok(done)
    }

    // ------------------------------------------------------------- notes
    pub fn timeline(&self, text: &str) -> Result<()> {
        let (note, clock) = {
            let cur = self.cur.lock().unwrap();
            let s = cur
                .as_ref()
                .ok_or_else(|| anyhow!("No playtest running."))?;
            (s.note.clone(), s.clock())
        };
        self.vault().append_to_block(
            &note,
            "auto",
            "timeline",
            &format!("- `{clock}` {}", text.trim()),
        )
    }

    pub fn snap(&self, caption: &str) -> Result<String> {
        let shot = match self.look(false) {
            Ok(s) => s,
            Err(_) => self
                .cur
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|s| s.last_shot.clone())
                .ok_or_else(|| anyhow!("No screenshot available."))?,
        };
        let (note, uid, clock, n) = {
            let mut cur = self.cur.lock().unwrap();
            let s = cur
                .as_mut()
                .ok_or_else(|| anyhow!("No playtest running."))?;
            s.shots_saved += 1;
            (s.note.clone(), s.universe_id, s.clock(), s.shots_saved)
        };
        let v = self.vault();
        let dir = v.dir(crate::vault::DIR_ATTACH).join(uid.to_string());
        std::fs::create_dir_all(&dir)?;
        let stem = note
            .file_stem()
            .map(|x| x.to_string_lossy().replace(' ', "_"))
            .unwrap_or_default();
        let file = dir.join(format!("{stem}_{n:02}.jpg"));
        let img = if shot.full.width() > 1600 {
            image::imageops::resize(
                &shot.full,
                1600,
                (shot.full.height() as f64 * 1600.0 / shot.full.width() as f64) as u32,
                image::imageops::FilterType::Triangle,
            )
        } else {
            shot.full.clone()
        };
        std::fs::write(&file, capture::jpeg(&img, 82))?;
        let rel = v.rel(&file);
        v.append_to_block(
            &note,
            "auto",
            "shots",
            &format!("![[{rel}|640]]\n*{clock} — {}*\n", caption.trim()),
        )?;
        Ok(rel)
    }

    pub fn has_session(&self) -> bool {
        self.cur.lock().unwrap().is_some()
    }

    pub fn clear(&self) {
        *self.cur.lock().unwrap() = None;
    }

    pub fn status_json(&self) -> Option<Value> {
        let cur = self.cur.lock().unwrap();
        let s = cur.as_ref()?;
        Some(
            json!({"game": s.name, "elapsed_s": s.elapsed() as u64, "remaining_s": s.remaining().max(0.0) as u64, "actions": s.actions,
            "screens_saved": s.shots_saved, "ended": s.ended, "abort_reason": self.abort_reason(), "log": self.vault().rel(&s.note), "date": today()}),
        )
    }
}
