//! The ROX-AI app window.
//!
//! First start → a step-by-step setup wizard. Afterwards three tabs:
//!   Home  — Passive / Active mode, what ROX-AI is doing, a couple of big buttons
//!   Games — dashboard of every tracked game and how it was analyzed (videos / data)
//!   Vault — read the Obsidian vault right inside the app
//! Slow work (installs, sign-ins, scans, learning) runs in the background with visible progress.

use crate::config::Ctx;
use crate::deps::{Action, Dep};
use crate::gamesview::GameRow;
use crate::vaultview::Block;
use eframe::egui::{self, Color32, RichText};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ACC: Color32 = Color32::from_rgb(0, 214, 168);
const RED: Color32 = Color32::from_rgb(240, 82, 96);
const AMBER: Color32 = Color32::from_rgb(255, 196, 82);
const BLUE: Color32 = Color32::from_rgb(110, 160, 255);
const CARD: Color32 = Color32::from_rgb(30, 34, 48);
const CARD2: Color32 = Color32::from_rgb(40, 45, 62);
const BG: Color32 = Color32::from_rgb(19, 21, 31);
const MUTED: Color32 = Color32::from_rgb(140, 148, 168);
const STEPS: [&str; 7] = [
    "Welcome",
    "Location",
    "Mode",
    "Apps",
    "Accounts",
    "Preferences",
    "Done",
];
const S_LOCATION: usize = 1;
const S_MODE: usize = 2;
const S_APPS: usize = 3;
const S_ACCOUNTS: usize = 4;
const S_PREFS: usize = 5;
const S_DONE: usize = 6;

/// Run the app exe in the background (no window) — e.g. a scan or a learning run.
pub fn bg_cli(args: &[&str]) {
    let mut c = crate::util::hidden_command(crate::util::bg_exe());
    c.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000 | 0x0000_0008); // no window + detached
    }
    let _ = c.spawn();
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Home,
    Games,
    Vault,
}

/// State of the always-visible update button (top-right corner).
#[derive(Clone, Default, PartialEq)]
enum Upd {
    #[default]
    Checking,
    UpToDate,
    Available(String),
    Installing,
    Failed(String),
}

#[derive(Default)]
struct Shared {
    deps: Option<Vec<Dep>>,
    /// item being installed / signed in: (id, progress text, fraction)
    job: Option<(String, String, Option<f32>)>,
    queue: Vec<(String, Action)>,
    toast: Option<(String, bool)>,
    upd: Upd,
    moving: Option<String>,
    games: Option<Vec<GameRow>>,
}

#[derive(Default)]
struct VaultState {
    current: Option<String>,
    history: Vec<String>,
    parsed: Option<(String, Vec<(String, String)>, Vec<Block>)>,
    tree: Vec<(&'static str, Vec<String>)>,
    tree_loaded: Option<Instant>,
    search: String,
}

struct App {
    ctx: Ctx,
    sh: Arc<Mutex<Shared>>,
    wizard: Option<usize>,
    tab: Tab,
    move_to: Option<PathBuf>,
    desktop_shortcut: bool,
    start_shortcut: bool,
    activity: (String, String, Color32),
    busy: bool,
    last_tick: Instant,
    last_deps: Instant,
    games_loaded: Option<Instant>,
    games_filter: usize,
    games_search: String,
    vault: VaultState,
}

fn btn(text: &str, fill: Color32, w: f32, h: f32) -> egui::Button<'static> {
    let fg = if fill == CARD || fill == CARD2 || fill == RED {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    egui::Button::new(
        RichText::new(text.to_string())
            .size(15.0)
            .strong()
            .color(fg),
    )
    .fill(fill)
    .min_size(egui::vec2(w, h))
    .corner_radius(8.0)
}

fn card<R>(ui: &mut egui::Ui, fill: Color32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let w = ui.available_width();
    egui::Frame::new()
        .fill(fill)
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(w - 28.0);
            add(ui)
        })
        .inner
}

fn dot(ui: &mut egui::Ui, color: Color32, h: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, h), egui::Sense::hover());
    ui.painter().circle_filled(r.center(), 6.0, color);
}

fn pill(ui: &mut egui::Ui, text: &str, fill: Color32, fg: Color32) {
    egui::Frame::new()
        .fill(fill)
        .corner_radius(20.0)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(fg));
        });
}

impl App {
    fn new(ctx: Ctx) -> App {
        let first = ctx.store.get("setup_complete").is_none();
        let here = crate::config::app_folder();
        let move_to =
            (first && crate::relocate::is_temporary(&here)).then(crate::relocate::suggested_folder);
        let mut a = App {
            ctx,
            sh: Arc::new(Mutex::new(Shared::default())),
            wizard: std::env::var("ROXAI_UI_STEP")
                .ok()
                .and_then(|v| v.parse().ok())
                .or(first.then_some(0)),
            tab: match std::env::var("ROXAI_UI_TAB").as_deref() {
                Ok("games") => Tab::Games,
                Ok("vault") => Tab::Vault,
                _ => Tab::Home,
            },
            move_to,
            desktop_shortcut: true,
            start_shortcut: true,
            activity: ("ROX-AI".into(), "Starting…".into(), ACC),
            busy: false,
            last_tick: Instant::now() - Duration::from_secs(10),
            last_deps: Instant::now(),
            games_loaded: None,
            games_filter: 0,
            games_search: String::new(),
            vault: VaultState::default(),
        };
        if let Ok(n) = std::env::var("ROXAI_UI_NOTE") {
            a.vault.current = Some(n);
        }
        a.startup();
        a
    }

    /// Silent setup + first data, all in the background.
    fn startup(&mut self) {
        let (ctx, sh) = (self.ctx.clone(), self.sh.clone());
        let first = self.wizard.is_some();
        std::thread::spawn(move || {
            let deps = crate::deps::status(&ctx);
            sh.lock().unwrap().deps = Some(deps);
            let api = crate::api::HttpApi::new().ok();
            let _ =
                crate::setup::ensure(&ctx, api.as_ref().map(|a| a as &dyn crate::api::RobloxApi));
            let scanned = !ctx
                .store
                .chart_on(&crate::util::today(), &ctx.settings.sort_id)
                .is_empty();
            if !first && !scanned && !crate::util::is_locked(&ctx.state_dir(), "scan") {
                bg_cli(&["scan", "--if-needed"]);
            }
            check_updates(&sh);
        });
    }

    fn refresh_deps(&mut self) {
        self.last_deps = Instant::now();
        let (ctx, sh) = (self.ctx.clone(), self.sh.clone());
        std::thread::spawn(move || {
            let d = crate::deps::status(&ctx);
            sh.lock().unwrap().deps = Some(d);
        });
    }

    fn toast(&self, msg: impl Into<String>, ok: bool) {
        self.sh.lock().unwrap().toast = Some((msg.into(), ok));
    }

    fn set_mode(&mut self, mode: &str) {
        if self.ctx.settings.mode == mode {
            return;
        }
        self.ctx.settings.mode = mode.into();
        let _ = self.ctx.settings.save(&self.ctx.home);
        let ctx = self.ctx.clone();
        let sh = self.sh.clone();
        std::thread::spawn(move || {
            crate::setup::apply_auto_run(&ctx);
            let d = crate::deps::status(&ctx);
            sh.lock().unwrap().deps = Some(d);
        });
        self.tick();
    }

    /// Queue installs / sign-ins; one worker runs them in order with live progress.
    fn enqueue(&mut self, items: Vec<(String, Action)>) {
        let start_worker = {
            let mut g = self.sh.lock().unwrap();
            for it in items {
                if !g.queue.contains(&it) && g.job.as_ref().map(|j| &j.0) != Some(&it.0) {
                    g.queue.push(it);
                }
            }
            g.job.is_none()
        };
        if !start_worker {
            return;
        }
        let (ctx, sh) = (self.ctx.clone(), self.sh.clone());
        std::thread::spawn(move || loop {
            let (id, act) = {
                let mut g = sh.lock().unwrap();
                if g.queue.is_empty() {
                    g.job = None;
                    break;
                }
                let (id, act) = g.queue.remove(0);
                g.job = Some((id.clone(), "Starting…".into(), None));
                (id, act)
            };
            let (sh2, id2) = (sh.clone(), id.clone());
            let report = move |text: &str, frac: Option<f32>| {
                sh2.lock().unwrap().job = Some((id2.clone(), text.to_string(), frac));
            };
            let r = crate::deps::act(&ctx, &id, act, &report);
            let deps = crate::deps::status(&ctx);
            let mut g = sh.lock().unwrap();
            g.deps = Some(deps);
            g.toast = Some(match r {
                Ok(m) => (m, true),
                Err(e) => (format!("{e}"), false),
            });
        });
    }

    fn missing_required(&self) -> Option<usize> {
        self.sh
            .lock()
            .unwrap()
            .deps
            .as_ref()
            .map(|d| d.iter().filter(|x| x.required && !x.ok).count())
    }

    // ================================================================ wizard
    fn wizard_ui(&mut self, ui: &mut egui::Ui, step: usize) {
        let active = self.ctx.is_active();
        ui.horizontal_wrapped(|ui| {
            for (i, name) in STEPS.iter().enumerate() {
                if i == S_ACCOUNTS && !active {
                    continue;
                }
                let (fill, fg) = if i == step {
                    (ACC, Color32::BLACK)
                } else if i < step {
                    (CARD2, ACC)
                } else {
                    (CARD, MUTED)
                };
                pill(ui, name, fill, fg);
            }
        });
        ui.add_space(18.0);
        let (deps, job) = {
            let g = self.sh.lock().unwrap();
            (g.deps.clone(), g.job.clone())
        };
        let mut next_enabled = true;
        let mut next_label = "Next";
        match step {
            0 => {
                ui.label(RichText::new("Welcome to ROX-AI").size(26.0).strong());
                ui.add_space(8.0);
                for (t, d) in [
                    ("Follows the Roblox charts", "Every day it reads the top trending games and tracks how they grow."),
                    ("Learns each game from YouTube", "Claude reads gameplay and guide videos about every new chart game — it never touches your screen."),
                    ("Writes everything down", "One detailed note per game: loop, systems, monetization, how to build it. Read it right here in the app."),
                ] {
                    card(ui, CARD, |ui| {
                        ui.label(RichText::new(t).strong().size(15.0));
                        ui.label(RichText::new(d).color(MUTED));
                    });
                    ui.add_space(6.0);
                }
                ui.add_space(6.0);
                ui.label(RichText::new("Setup takes a few minutes. ROX-AI is portable — it keeps everything in its own folder.").color(MUTED));
                next_label = "Get started";
            }
            S_LOCATION => self.step_location(ui),
            S_MODE => {
                ui.label(RichText::new("How should ROX-AI work?").size(22.0).strong());
                ui.label(RichText::new("You can switch any time on the Home tab.").color(MUTED));
                ui.add_space(10.0);
                self.mode_cards(ui, true);
            }
            S_APPS => {
                ui.label(RichText::new("Apps ROX-AI works with").size(22.0).strong());
                ui.label(
                    RichText::new(
                        "Green = already on your PC. Anything missing installs with one click.",
                    )
                    .color(MUTED),
                );
                ui.add_space(10.0);
                let ids: Vec<&str> = deps
                    .as_ref()
                    .map(|d| {
                        d.iter()
                            .filter(|x| !x.id.ends_with("_login"))
                            .map(|x| x.id)
                            .collect()
                    })
                    .unwrap_or_default();
                self.dep_rows(ui, deps.as_deref(), &ids, job.as_ref());
                let missing: Vec<(String, Action)> = deps
                    .as_ref()
                    .map(|d| {
                        d.iter()
                            .filter(|x| {
                                ids.contains(&x.id)
                                    && !x.ok
                                    && (x.required || x.id == "video_reader")
                            })
                            .filter_map(|x| x.action.map(|(a, _)| (x.id.to_string(), a)))
                            .collect()
                    })
                    .unwrap_or_default();
                ui.add_space(6.0);
                if !missing.is_empty()
                    && job.is_none()
                    && ui
                        .add(btn(
                            &format!("Install all recommended ({})", missing.len()),
                            ACC,
                            260.0,
                            38.0,
                        ))
                        .clicked()
                {
                    self.enqueue(missing.clone());
                }
                next_enabled = job.is_none();
                if deps
                    .as_ref()
                    .is_some_and(|d| d.iter().any(|x| ids.contains(&x.id) && x.required && !x.ok))
                {
                    next_label = "Skip for now";
                }
            }
            S_ACCOUNTS => {
                ui.label(RichText::new("Sign in").size(22.0).strong());
                ui.label(
                    RichText::new(
                        "One-time sign-in. It opens your browser; this page turns green by itself.",
                    )
                    .color(MUTED),
                );
                ui.add_space(10.0);
                let ids: Vec<&str> = deps
                    .as_ref()
                    .map(|d| {
                        d.iter()
                            .filter(|x| x.id.ends_with("_login"))
                            .map(|x| x.id)
                            .collect()
                    })
                    .unwrap_or_default();
                self.dep_rows(ui, deps.as_deref(), &ids, job.as_ref());
                next_enabled = job.is_none();
                if deps
                    .as_ref()
                    .is_some_and(|d| d.iter().any(|x| x.id.ends_with("_login") && !x.ok))
                {
                    next_label = "Skip for now";
                }
            }
            S_PREFS => {
                ui.label(RichText::new("Preferences").size(22.0).strong());
                ui.add_space(10.0);
                let active = self.ctx.is_active();
                let s = &mut self.ctx.settings;
                card(ui, CARD, |ui| {
                    ui.checkbox(
                        &mut s.auto_run,
                        RichText::new("Run automatically").strong().size(15.0),
                    );
                    let what = if active {
                        "Updates the charts every day and lets Claude learn the new games in the background."
                    } else {
                        "Updates the charts every day in the background."
                    };
                    ui.label(RichText::new(what).color(MUTED).small());
                });
                if active {
                    ui.add_space(6.0);
                    card(ui, CARD, |ui| {
                        ui.label(RichText::new("New games Claude learns per day").strong());
                        ui.add(egui::Slider::new(&mut s.learn_per_run, 1..=25));
                        ui.label(
                            RichText::new("More games use more of your Claude plan.")
                                .color(MUTED)
                                .small(),
                        );
                    });
                    ui.add_space(6.0);
                    card(ui, CARD, |ui| {
                        ui.label(RichText::new("Videos per game").strong());
                        ui.add(egui::Slider::new(&mut s.videos_per_game, 2..=8));
                    });
                }
                next_label = "Finish setup";
            }
            _ => {
                ui.label(
                    RichText::new("You're all set!")
                        .size(26.0)
                        .strong()
                        .color(ACC),
                );
                ui.add_space(8.0);
                ui.label(RichText::new("ROX-AI is reading today's charts now. Your games appear on the Games tab within a minute.").color(MUTED));
                next_label = "Go to ROX-AI";
            }
        }

        if let Some((m, ok)) = self.sh.lock().unwrap().toast.clone() {
            ui.add_space(8.0);
            ui.label(RichText::new(m).color(if ok { ACC } else { RED }));
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let moving = self.sh.lock().unwrap().moving.clone();
                    if ui
                        .add_enabled(
                            next_enabled && moving.is_none(),
                            btn(next_label, ACC, 150.0, 42.0),
                        )
                        .clicked()
                    {
                        self.wizard_next(step);
                    }
                    if step > 0 && step < S_DONE && ui.add(btn("Back", CARD, 90.0, 42.0)).clicked()
                    {
                        let back = if step == S_PREFS && !self.ctx.is_active() {
                            S_APPS
                        } else {
                            step - 1
                        };
                        self.wizard = Some(back);
                        self.sh.lock().unwrap().toast = None;
                    }
                    if let Some(m) = moving {
                        ui.add(egui::Spinner::new());
                        ui.label(RichText::new(m).color(ACC));
                    }
                });
            });
        });
    }

    fn wizard_next(&mut self, step: usize) {
        self.sh.lock().unwrap().toast = None;
        match step {
            S_LOCATION => {
                let shortcuts = (self.desktop_shortcut, self.start_shortcut);
                if let Some(dest) = self.move_to.clone() {
                    // copy to the new folder, restart from there; the new copy cleans up the old one
                    let sh = self.sh.clone();
                    sh.lock().unwrap().moving = Some("Moving ROX-AI…".into());
                    std::thread::spawn(move || match crate::relocate::move_app(&dest) {
                        Ok(exe) => {
                            let _ =
                                crate::relocate::create_shortcuts(&exe, shortcuts.0, shortcuts.1);
                            let old = crate::config::app_folder();
                            match std::process::Command::new(&exe)
                                .arg("--moved-from")
                                .arg(&old)
                                .current_dir(&dest)
                                .spawn()
                            {
                                Ok(_) => std::process::exit(0),
                                Err(e) => {
                                    let mut g = sh.lock().unwrap();
                                    g.moving = None;
                                    g.toast = Some((
                                        format!("Couldn't start the moved copy: {e}"),
                                        false,
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            let mut g = sh.lock().unwrap();
                            g.moving = None;
                            g.toast = Some((format!("Couldn't move: {e}"), false));
                        }
                    });
                    return;
                }
                if shortcuts.0 || shortcuts.1 {
                    let exe = crate::util::bg_exe();
                    std::thread::spawn(move || {
                        let _ = crate::relocate::create_shortcuts(&exe, shortcuts.0, shortcuts.1);
                    });
                }
                self.wizard = Some(S_MODE);
            }
            S_APPS => {
                self.wizard = Some(if self.ctx.is_active() {
                    S_ACCOUNTS
                } else {
                    S_PREFS
                })
            }
            S_PREFS => {
                let _ = self.ctx.settings.save(&self.ctx.home);
                let _ = self
                    .ctx
                    .store
                    .set("setup_complete", serde_json::json!(crate::util::now_iso()));
                let ctx = self.ctx.clone();
                std::thread::spawn(move || {
                    crate::setup::apply_auto_run(&ctx);
                });
                if !crate::util::is_locked(&self.ctx.state_dir(), "scan") {
                    bg_cli(&["scan", "--if-needed"]);
                }
                self.wizard = Some(S_DONE);
            }
            S_DONE => {
                self.wizard = None;
                self.tab = Tab::Home;
                self.tick();
            }
            s => self.wizard = Some(s + 1),
        }
    }

    /// The Passive / Active choice (used in the wizard and on Home).
    fn mode_cards(&mut self, ui: &mut egui::Ui, big: bool) {
        let active = self.ctx.is_active();
        let w = (ui.available_width() - 10.0) / 2.0;
        let h = if big { 150.0 } else { 104.0 };
        let mut choose = None;
        ui.horizontal(|ui| {
            for (mode, title, lines) in [
                (
                    "passive",
                    "Passive",
                    [
                        "Just the data",
                        "Daily chart tracking, trends, store prices.",
                        "No Claude needed.",
                    ],
                ),
                (
                    "active",
                    "Active",
                    [
                        "Learn every game",
                        "Claude learns each new chart game from YouTube videos.",
                        "Uses your Claude plan — never your screen.",
                    ],
                ),
            ] {
                let selected = (mode == "active") == active;
                let fill = if selected { CARD2 } else { CARD };
                let stroke = if selected {
                    egui::Stroke::new(2.0, ACC)
                } else {
                    egui::Stroke::new(1.0, CARD2)
                };
                let resp = egui::Frame::new()
                    .fill(fill)
                    .stroke(stroke)
                    .corner_radius(12.0)
                    .inner_margin(14.0)
                    .show(ui, |ui| {
                        ui.set_width(w - 32.0);
                        ui.set_min_height(h - 28.0);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                dot(ui, if selected { ACC } else { MUTED }, 22.0);
                                ui.label(
                                    RichText::new(title).size(19.0).strong().color(if selected {
                                        ACC
                                    } else {
                                        Color32::WHITE
                                    }),
                                );
                            });
                            ui.label(RichText::new(lines[0]).strong());
                            if big {
                                ui.label(RichText::new(lines[1]).color(MUTED).small());
                            }
                            ui.label(RichText::new(lines[2]).color(MUTED).small());
                        });
                    })
                    .response
                    .interact(egui::Sense::click());
                if resp.clicked() {
                    choose = Some(mode);
                }
                resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            }
        });
        if let Some(m) = choose {
            self.set_mode(m);
        }
    }

    fn step_location(&mut self, ui: &mut egui::Ui) {
        let here = crate::config::app_folder();
        ui.label(
            RichText::new("Where should ROX-AI live?")
                .size(22.0)
                .strong(),
        );
        ui.label(RichText::new("ROX-AI keeps its settings, history and your vault in one folder. Nothing is installed into Windows.").color(MUTED));
        ui.add_space(10.0);
        let temp = crate::relocate::is_temporary(&here);
        let mut keep = self.move_to.is_none();
        card(ui, if keep { CARD2 } else { CARD }, |ui| {
            if ui
                .radio(keep, RichText::new("Keep it where it is").strong())
                .clicked()
            {
                keep = true;
            }
            ui.label(RichText::new(here.to_string_lossy()).color(MUTED).small());
            if temp {
                ui.label(RichText::new("This looks like Downloads or a temporary folder — moving it is recommended.").color(AMBER).small());
            }
        });
        ui.add_space(6.0);
        let target = self
            .move_to
            .clone()
            .unwrap_or_else(crate::relocate::suggested_folder);
        card(ui, if keep { CARD } else { CARD2 }, |ui| {
            if ui
                .radio(!keep, RichText::new("Move it to this folder").strong())
                .clicked()
            {
                keep = false;
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(target.to_string_lossy()).color(MUTED).small());
                #[cfg(windows)]
                if ui.small_button("Change…").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .set_title("Choose a folder for ROX-AI")
                        .pick_folder()
                    {
                        let p = if p
                            .file_name()
                            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("ROX-AI"))
                        {
                            p
                        } else {
                            p.join("ROX-AI")
                        };
                        self.move_to = Some(p);
                        keep = false;
                    }
                }
            });
        });
        if keep {
            self.move_to = None;
        } else if self.move_to.is_none() {
            self.move_to = Some(target);
        }
        ui.add_space(10.0);
        ui.checkbox(&mut self.desktop_shortcut, "Add a Desktop shortcut");
        ui.checkbox(&mut self.start_shortcut, "Add to the Start menu");
    }

    /// Rows for the given dependency ids, with Install / Sign in buttons and live progress.
    fn dep_rows(
        &mut self,
        ui: &mut egui::Ui,
        deps: Option<&[Dep]>,
        ids: &[&str],
        job: Option<&(String, String, Option<f32>)>,
    ) {
        let Some(list) = deps else {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label(RichText::new("Checking what's already on this PC…").color(MUTED));
            });
            return;
        };
        let queued: Vec<String> = self
            .sh
            .lock()
            .unwrap()
            .queue
            .iter()
            .map(|q| q.0.clone())
            .collect();
        for id in ids {
            let Some(d) = list.iter().find(|x| x.id == *id).cloned() else {
                continue;
            };
            let running = job.filter(|j| j.0 == d.id);
            card(ui, CARD, |ui| {
                ui.horizontal(|ui| {
                    dot(
                        ui,
                        if d.ok {
                            ACC
                        } else if running.is_some() {
                            AMBER
                        } else {
                            MUTED
                        },
                        34.0,
                    );
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(d.name).strong().size(15.0));
                            if !d.required && !d.ok {
                                pill(ui, "optional", CARD2, MUTED);
                            }
                        });
                        let sub = if d.ok {
                            "Ready".to_string()
                        } else {
                            d.why.to_string()
                        };
                        ui.label(
                            RichText::new(sub)
                                .color(if d.ok { ACC } else { MUTED })
                                .small(),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if d.ok {
                            ui.label(RichText::new("Ready").color(ACC).strong());
                        } else if running.is_some() {
                            ui.add(egui::Spinner::new());
                        } else if queued.contains(&d.id.to_string()) {
                            ui.label(RichText::new("waiting…").color(MUTED).small());
                        } else if let Some((act, label)) = d.action {
                            if ui
                                .add_enabled(job.is_none(), btn(label, ACC, 100.0, 32.0))
                                .clicked()
                            {
                                self.enqueue(vec![(d.id.to_string(), act)]);
                            }
                        } else {
                            ui.label(RichText::new("install the app first").color(MUTED).small());
                        }
                    });
                });
                if let Some((_, text, frac)) = running {
                    ui.add_space(4.0);
                    let bar = match frac {
                        Some(f) => egui::ProgressBar::new(*f),
                        None => egui::ProgressBar::new(0.0).animate(true),
                    };
                    ui.add(bar.text(text.clone()).fill(ACC));
                }
            });
            ui.add_space(6.0);
        }
    }

    // ============================================================ Home tab
    fn tick(&mut self) {
        self.last_tick = Instant::now();
        let st = self.ctx.state_dir();
        self.busy = false;
        if let Some(d) = std::fs::read_to_string(self.ctx.session_file())
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        {
            let el = d["elapsed"].as_u64().unwrap_or(0);
            self.busy = true;
            self.activity = (
                format!(
                    "Playing {}",
                    crate::vault::clean_display_name(d["name"].as_str().unwrap_or("a game"))
                ),
                format!(
                    "{}:{:02} — moving your mouse stops the AI.",
                    el / 60,
                    el % 60
                ),
                ACC,
            );
            return;
        }
        if crate::util::is_locked(&st, "autopilot") {
            self.busy = true;
            let now = self.ctx.store.get("learning_now");
            let game = now
                .as_ref()
                .and_then(|n| n["game"].as_str().map(String::from));
            self.activity = match game {
                Some(g) => (format!("Learning {}", crate::vault::clean_display_name(&g)), "Claude is reading YouTube videos about it — this takes a few minutes per game.".into(), BLUE),
                None => ("Learning new games…".into(), "Picking the games to learn".into(), BLUE),
            };
            return;
        }
        if crate::util::is_locked(&st, "scan") {
            self.busy = true;
            self.activity = (
                "Updating the charts…".into(),
                "Reading the top Roblox games (about a minute)".into(),
                ACC,
            );
            return;
        }
        let games = self.ctx.store.games();
        let learned = games
            .values()
            .filter(|g| g.analysis_source.is_some())
            .count();
        if let Some(b) = self.ctx.store.get("blocked_reason") {
            self.activity = (
                "Needs you".into(),
                b["reason"].as_str().unwrap_or("").to_string(),
                AMBER,
            );
        } else if let Some(m) = self.missing_required().filter(|m| *m > 0) {
            self.activity = (
                format!("{m} setup step{} left", if m == 1 { "" } else { "s" }),
                "Click “Finish setup” to complete it.".into(),
                AMBER,
            );
        } else {
            let auto = if !self.ctx.settings.auto_run {
                "Automatic mode is off — use the buttons below."
            } else if self.ctx.is_active() {
                "Charts update daily · new games are learned automatically."
            } else {
                "Charts update daily."
            };
            self.activity = (
                "All set".into(),
                format!("{} games tracked · {learned} learned\n{auto}", games.len()),
                ACC,
            );
        }
    }

    fn home_ui(&mut self, ui: &mut egui::Ui) {
        self.mode_cards(ui, false);
        ui.add_space(12.0);
        let (title, sub, col) = self.activity.clone();
        card(ui, CARD, |ui| {
            ui.horizontal(|ui| {
                dot(ui, col, 24.0);
                ui.label(RichText::new(title).size(20.0).strong().color(col));
            });
            ui.label(RichText::new(sub).color(MUTED));
            if self.busy && crate::util::is_locked(&self.ctx.state_dir(), "autopilot") {
                ui.add_space(6.0);
                if ui.add(btn("Stop", RED, 100.0, 30.0)).clicked() {
                    crate::session::request_stop(&self.ctx, "Stop button");
                    bg_cli(&["stop"]);
                    self.toast("Stopping after the current step…", true);
                }
            }
        });
        ui.add_space(10.0);
        let missing = self.missing_required().unwrap_or(0);
        if missing > 0 && ui.add(btn("Finish setup", AMBER, 160.0, 34.0)).clicked() {
            let first = self
                .sh
                .lock()
                .unwrap()
                .deps
                .as_ref()
                .and_then(|d| {
                    d.iter()
                        .find(|x| x.required && !x.ok)
                        .map(|x| x.id.to_string())
                })
                .unwrap_or_default();
            self.wizard = Some(if first.ends_with("_login") {
                S_ACCOUNTS
            } else {
                S_APPS
            });
        }
        ui.add_space(10.0);
        let w = (ui.available_width() - 12.0) / 2.0;
        ui.horizontal(|ui| {
            if ui.add(btn("Browse my games", ACC, w, 54.0)).clicked() {
                self.tab = Tab::Games;
            }
            if self.ctx.is_active() {
                let ready = missing == 0;
                if ui.add_enabled(ready && !self.busy, btn("Learn new games now", CARD2, w, 54.0)).on_disabled_hover_text(if ready { "Already working…" } else { "Finish the setup first (Claude sign-in)." }).clicked() {
                    bg_cli(&["autopilot", "--force"]);
                    self.toast("Learning started — Claude is reading YouTube videos. Keep using your PC normally.", true);
                    self.last_tick = Instant::now() - Duration::from_secs(10);
                }
            } else if ui.add_enabled(!self.busy, btn("Update charts now", CARD2, w, 54.0)).clicked() {
                bg_cli(&["scan"]);
                self.toast("Updating the charts…", true);
            }
        });
        ui.add_space(12.0);
        let mut auto = self.ctx.settings.auto_run;
        if ui
            .checkbox(&mut auto, "Run automatically in the background")
            .changed()
        {
            self.ctx.settings.auto_run = auto;
            let _ = self.ctx.settings.save(&self.ctx.home);
            let ctx = self.ctx.clone();
            std::thread::spawn(move || crate::setup::apply_auto_run(&ctx));
            self.tick();
        }
    }

    // =========================================================== Games tab
    fn games_ui(&mut self, ui: &mut egui::Ui) {
        if self
            .games_loaded
            .is_none_or(|t| t.elapsed() > Duration::from_secs(15))
        {
            self.games_loaded = Some(Instant::now());
            let (ctx, sh) = (self.ctx.clone(), self.sh.clone());
            std::thread::spawn(move || {
                let r = crate::gamesview::rows(&ctx);
                sh.lock().unwrap().games = Some(r);
            });
        }
        let Some(all) = self.sh.lock().unwrap().games.clone() else {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label("Loading games…");
            });
            return;
        };
        let learned = all.iter().filter(|g| g.learned()).count();
        ui.horizontal(|ui| {
            for (i, label) in [
                format!("All {}", all.len()),
                format!("Learned {learned}"),
                format!("Data only {}", all.len() - learned),
                "On the chart".to_string(),
            ]
            .iter()
            .enumerate()
            {
                if ui
                    .selectable_label(self.games_filter == i, RichText::new(label).strong())
                    .clicked()
                {
                    self.games_filter = i;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.games_search)
                        .hint_text("Search games…")
                        .desired_width(180.0),
                );
            });
        });
        ui.add_space(8.0);
        let q = self.games_search.to_lowercase();
        let rows: Vec<&GameRow> = all
            .iter()
            .filter(|g| match self.games_filter {
                1 => g.learned(),
                2 => !g.learned(),
                3 => g.rank.is_some(),
                _ => true,
            })
            .filter(|g| {
                q.is_empty()
                    || g.title.to_lowercase().contains(&q)
                    || g.genre.to_lowercase().contains(&q)
            })
            .collect();
        if rows.is_empty() {
            ui.label(
                RichText::new("No games yet — the first chart scan fills this in about a minute.")
                    .color(MUTED),
            );
            return;
        }
        let mut open: Option<String> = None;
        let mut learn: Option<u64> = None;
        let active = self.ctx.is_active();
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                egui::Grid::new("games")
                    .striped(true)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        for h in [
                            "#",
                            "Game",
                            "Genre",
                            "Playing",
                            "Like %",
                            "Momentum",
                            "Learned from",
                            "Fun",
                        ] {
                            ui.label(RichText::new(h).small().strong().color(MUTED));
                        }
                        ui.end_row();
                        for g in rows {
                            ui.label(g.rank.map(|r| format!("#{r}")).unwrap_or("–".into()));
                            ui.vertical(|ui| {
                                ui.set_min_width(260.0);
                                if ui
                                    .link(RichText::new(&g.title).strong().color(Color32::WHITE))
                                    .on_hover_text("Open this game's note")
                                    .clicked()
                                {
                                    open = g.note.clone();
                                }
                                if !g.one_liner.is_empty() {
                                    let mut ol = crate::vaultview::clean(&g.one_liner);
                                    if ol.chars().count() > 48 {
                                        ol = ol.chars().take(46).collect::<String>() + "…";
                                    }
                                    ui.label(RichText::new(ol).small().color(MUTED));
                                }
                            });
                            ui.label(RichText::new(&g.genre).color(MUTED));
                            ui.label(crate::analysis::n(g.playing));
                            ui.label(format!("{:.0}%", g.like_ratio));
                            ui.label(g.momentum.to_string());
                            ui.horizontal(|ui| {
                                let (fill, fg) = match g.source.as_str() {
                                    "videos" => (Color32::from_rgb(20, 70, 60), ACC),
                                    "playtest" => (Color32::from_rgb(30, 50, 90), BLUE),
                                    "data" => (CARD2, Color32::WHITE),
                                    _ => (CARD, MUTED),
                                };
                                pill(ui, g.source_label(), fill, fg);
                                if !g.learned()
                                    && active
                                    && ui
                                        .small_button("Learn")
                                        .on_hover_text(
                                            "Let Claude learn this game from YouTube now",
                                        )
                                        .clicked()
                                {
                                    learn = Some(g.uid);
                                }
                            });
                            ui.label(g.fun.map(|f| format!("{f}/10")).unwrap_or("–".into()));
                            ui.end_row();
                        }
                    });
            });
        if let Some(n) = open {
            self.open_note(&n);
        }
        if let Some(uid) = learn {
            if self.missing_required().unwrap_or(1) > 0 {
                self.toast("Finish the setup first (Claude sign-in).", false);
            } else {
                bg_cli(&["learn", &uid.to_string()]);
                self.toast(
                    "Learning started — it takes a few minutes. Keep using your PC normally.",
                    true,
                );
            }
        }
    }

    // =========================================================== Vault tab
    fn open_note(&mut self, rel: &str) {
        if let Some(cur) = self.vault.current.take() {
            if cur != rel {
                self.vault.history.push(cur);
            }
        }
        self.vault.current = Some(rel.to_string());
        self.vault.parsed = None;
        self.tab = Tab::Vault;
    }

    fn vault_ui(&mut self, ui: &mut egui::Ui) {
        if self
            .vault
            .tree_loaded
            .is_none_or(|t| t.elapsed() > Duration::from_secs(20))
        {
            self.vault.tree = crate::gamesview::vault_tree(&self.ctx);
            self.vault.tree_loaded = Some(Instant::now());
        }
        if self.vault.current.is_none() {
            self.vault.current = Some("00 Dashboard.md".into());
        }
        let cur = self.vault.current.clone().unwrap();
        if self.vault.parsed.as_ref().is_none_or(|p| p.0 != cur) {
            let text = std::fs::read_to_string(self.ctx.vault().join(&cur))
                .unwrap_or_else(|_| "# Not found\nThis note doesn't exist yet.".into());
            let (fm, blocks) = crate::vaultview::parse(&text);
            self.vault.parsed = Some((cur.clone(), fm, blocks));
        }
        let mut nav: Option<String> = None;
        let total_h = ui.available_height();
        ui.horizontal_top(|ui| {
            // left: list of notes
            ui.vertical(|ui| {
                ui.set_width(210.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.vault.search)
                        .hint_text("Find a note…")
                        .desired_width(200.0),
                );
                ui.add_space(4.0);
                let q = self.vault.search.to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt("vault-tree")
                    .max_height(total_h - 40.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (folder, files) in &self.vault.tree {
                            let shown: Vec<&String> = files
                                .iter()
                                .filter(|f| q.is_empty() || f.to_lowercase().contains(&q))
                                .collect();
                            if shown.is_empty() {
                                continue;
                            }
                            if *folder == "Dashboard" {
                                if ui
                                    .selectable_label(
                                        cur == "00 Dashboard.md",
                                        RichText::new("Dashboard").strong(),
                                    )
                                    .clicked()
                                {
                                    nav = Some("00 Dashboard.md".into());
                                }
                                continue;
                            }
                            egui::CollapsingHeader::new(
                                RichText::new(format!("{folder} ({})", shown.len())).strong(),
                            )
                            .default_open(*folder == "Games" || !q.is_empty())
                            .show(ui, |ui| {
                                for f in shown.iter().take(200) {
                                    let name =
                                        f.rsplit('/').next().unwrap_or(f).trim_end_matches(".md");
                                    if ui.selectable_label(**f == cur, name).clicked() {
                                        nav = Some((*f).clone());
                                    }
                                }
                            });
                        }
                    });
            });
            ui.separator();
            // right: the note
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.vault.history.is_empty(), egui::Button::new("← Back"))
                        .clicked()
                    {
                        if let Some(prev) = self.vault.history.pop() {
                            self.vault.current = Some(prev);
                            self.vault.parsed = None;
                        }
                    }
                    ui.label(
                        RichText::new(cur.trim_end_matches(".md"))
                            .color(MUTED)
                            .small(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Open in Obsidian").clicked() {
                            let (ctx, n) = (self.ctx.clone(), cur.clone());
                            std::thread::spawn(move || crate::setup::open_note(&ctx, &n));
                        }
                    });
                });
                ui.separator();
                if let Some((_, fm, blocks)) = &self.vault.parsed {
                    egui::ScrollArea::vertical()
                        .id_salt(("note", cur.clone()))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if let Some(t) = crate::vaultview::render(ui, blocks, fm) {
                                nav = Some(t);
                            }
                        });
                }
            });
        });
        if let Some(t) = nav {
            match crate::gamesview::resolve(&self.ctx, &t) {
                Some(rel) => {
                    if self.vault.current.as_deref() != Some(rel.as_str()) {
                        self.open_note(&rel);
                    }
                }
                None => self.toast(format!("Note not found: {t}"), false),
            }
        }
    }

    // ============================================================ chrome
    /// Always visible, top-right: "Check for updates" / "Up to date" / "Update to vX".
    fn update_button(&mut self, ui: &mut egui::Ui) {
        let state = self.sh.lock().unwrap().upd.clone();
        match state {
            Upd::Checking => {
                ui.add_enabled(false, egui::Button::new(RichText::new("Checking for updates…").small()));
            }
            Upd::Installing => {
                ui.add(egui::Spinner::new());
                ui.label(RichText::new("Updating…").color(AMBER).small());
            }
            Upd::Available(v) => {
                if ui.add(btn(&format!("Update to v{v}"), AMBER, 0.0, 28.0)).on_hover_text("Downloads the new version and restarts ROX-AI. Your settings, vault and history are kept.").clicked() {
                    self.sh.lock().unwrap().upd = Upd::Installing;
                    let (ctx, sh) = (self.ctx.clone(), self.sh.clone());
                    std::thread::spawn(move || match crate::update::run(&ctx, false) {
                        Ok(_) => std::process::exit(0),
                        Err(e) => {
                            let mut g = sh.lock().unwrap();
                            g.upd = Upd::Failed(e.to_string());
                            g.toast = Some((format!("Update failed: {e}"), false));
                        }
                    });
                }
            }
            Upd::UpToDate => {
                if ui.add(egui::Button::new(RichText::new("✔ Up to date").small().color(ACC))).on_hover_text("Click to check again").clicked() {
                    self.sh.lock().unwrap().upd = Upd::Checking;
                    let sh = self.sh.clone();
                    std::thread::spawn(move || check_updates(&sh));
                }
            }
            Upd::Failed(e) => {
                if ui.add(egui::Button::new(RichText::new("Check for updates").small())).on_hover_text(format!("Last check failed: {e}")).clicked() {
                    self.sh.lock().unwrap().upd = Upd::Checking;
                    let sh = self.sh.clone();
                    std::thread::spawn(move || check_updates(&sh));
                }
            }
        }
    }

    fn more_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button(RichText::new("More").color(MUTED), |ui| {
            if ui.button("Update charts now").clicked() {
                bg_cli(&["scan"]);
                ui.close();
            }
            if ui.button("Run setup again").clicked() {
                self.wizard = Some(0);
                ui.close();
            }
            if ui.button("Open the vault in Obsidian").clicked() {
                let ctx = self.ctx.clone();
                std::thread::spawn(move || crate::setup::open_vault(&ctx));
                ui.close();
            }
            if ui.button("Open the ROX-AI folder").clicked() {
                crate::util::open_path(&crate::config::app_folder().to_string_lossy());
                ui.close();
            }
            if ui.button("Logs").clicked() {
                crate::util::open_path(&self.ctx.logs_dir().to_string_lossy());
                ui.close();
            }
            if self.ctx.store.get("blocked_reason").is_some() && ui.button("I fixed the login").clicked() {
                let _ = self.ctx.store.set("blocked_reason", serde_json::Value::Null);
                self.refresh_deps();
                ui.close();
            }
            ui.separator();
            if ui.button("Remove from this PC").on_hover_text("Turns off the schedule, removes shortcuts and disconnects Claude. Then just delete the folder.").clicked() {
                let (ctx, sh) = (self.ctx.clone(), self.sh.clone());
                std::thread::spawn(move || {
                    crate::relocate::remove_shortcuts();
                    let m = crate::setup::remove_from_pc(&ctx);
                    sh.lock().unwrap().toast = Some((m, true));
                });
                self.ctx.settings.auto_run = false;
                ui.close();
            }
        });
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        let idle = self.sh.lock().unwrap().job.is_none();
        if idle
            && self.last_deps.elapsed()
                > Duration::from_secs(if self.wizard.is_some() { 6 } else { 30 })
        {
            self.refresh_deps();
        }
        if self.last_tick.elapsed() > Duration::from_millis(1500) {
            self.tick();
        }
        // header: logo · tabs · update button · version
        ui.horizontal(|ui| {
            ui.label(RichText::new("ROX-AI").size(24.0).strong().color(ACC));
            ui.add_space(14.0);
            if self.wizard.is_none() {
                for (t, label) in [
                    (Tab::Home, "Home"),
                    (Tab::Games, "Games"),
                    (Tab::Vault, "Vault"),
                ] {
                    let sel = self.tab == t;
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(label).size(15.0).strong().color(if sel {
                                    Color32::BLACK
                                } else {
                                    Color32::WHITE
                                }),
                            )
                            .fill(if sel { ACC } else { CARD })
                            .corner_radius(8.0)
                            .min_size(egui::vec2(78.0, 30.0)),
                        )
                        .clicked()
                    {
                        self.tab = t;
                    }
                }
            } else {
                ui.label(RichText::new("Setup").color(MUTED));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("v{}", crate::VERSION))
                        .color(MUTED)
                        .small(),
                );
                self.update_button(ui);
                if self.wizard.is_none() {
                    self.more_menu(ui);
                }
            });
        });
        ui.add_space(12.0);
        match self.wizard {
            Some(step) => self.wizard_ui(ui, step),
            None => {
                match self.tab {
                    Tab::Home => self.home_ui(ui),
                    Tab::Games => self.games_ui(ui),
                    Tab::Vault => self.vault_ui(ui),
                }
                if let Some((m, ok)) = self.sh.lock().unwrap().toast.clone() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(m).color(if ok { ACC } else { RED }));
                }
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(Duration::from_millis(400));
        egui::Frame::new()
            .fill(BG)
            .inner_margin(20.0)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                self.body(ui)
            });
    }
}

/// Ask GitHub whether a newer release exists and update the button.
fn check_updates(sh: &Arc<Mutex<Shared>>) {
    let st = match crate::update::check() {
        Ok(s) if s.update_available => {
            Upd::Available(s.latest.map(|l| l.version).unwrap_or_default())
        }
        Ok(_) => Upd::UpToDate,
        Err(e) => Upd::Failed(e.to_string()),
    };
    sh.lock().unwrap().upd = st;
}

/// Use Windows' own UI font (Segoe UI) so the app looks native; symbols fall back to Segoe UI Symbol.
fn install_fonts(ctx: &egui::Context) {
    let dir = PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into()))
        .join("Fonts");
    let mut fonts = egui::FontDefinitions::default();
    let mut added = vec![];
    for (name, file) in [("segoe", "segoeui.ttf"), ("segoe_sym", "seguisym.ttf")] {
        if let Ok(bytes) = std::fs::read(dir.join(file)) {
            fonts
                .font_data
                .insert(name.into(), Arc::new(egui::FontData::from_owned(bytes)));
            added.push(name);
        }
    }
    if added.is_empty() {
        return;
    }
    let prop = fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default();
    for (i, n) in added.iter().enumerate() {
        prop.insert(i, n.to_string());
    }
    ctx.set_fonts(fonts);
}

pub fn run(ctx: Ctx) -> anyhow::Result<()> {
    let ico = crate::setup::icon_image(64);
    let icon = egui::IconData {
        rgba: ico.into_raw(),
        width: 64,
        height: 64,
    };
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ROX-AI")
            .with_inner_size([980.0, 720.0])
            .with_min_inner_size([760.0, 600.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        "ROX-AI",
        opts,
        Box::new(|cc| {
            install_fonts(&cc.egui_ctx);
            let mut v = egui::Visuals::dark();
            v.panel_fill = BG;
            v.window_fill = BG;
            v.selection.bg_fill = Color32::from_rgb(0, 120, 95);
            v.hyperlink_color = ACC;
            cc.egui_ctx.set_visuals(v);
            cc.egui_ctx.style_mut_of(egui::Theme::Dark, |s| {
                s.spacing.button_padding = egui::vec2(12.0, 6.0);
                s.spacing.item_spacing = egui::vec2(8.0, 6.0);
                s.spacing.slider_width = 280.0;
            });
            Ok(Box::new(App::new(ctx)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("window failed: {e}"))
}
