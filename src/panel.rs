//! The ROX-AI app window.
//!
//! First start → a step-by-step **setup wizard** (welcome → location → apps → accounts → preferences → done).
//! Afterwards → a simple home screen: status card, two big buttons, one switch.
//! All slow work (installs, sign-ins, scans) runs on background threads with visible progress.

use crate::config::Ctx;
use crate::deps::{Action, Dep};
use eframe::egui::{self, Color32, RichText};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ACC: Color32 = Color32::from_rgb(0, 214, 168);
const RED: Color32 = Color32::from_rgb(240, 82, 96);
const AMBER: Color32 = Color32::from_rgb(255, 196, 82);
const CARD: Color32 = Color32::from_rgb(30, 34, 48);
const CARD2: Color32 = Color32::from_rgb(40, 45, 62);
const BG: Color32 = Color32::from_rgb(19, 21, 31);
const MUTED: Color32 = Color32::from_rgb(140, 148, 168);
const STEPS: [&str; 6] = [
    "Welcome",
    "Location",
    "Apps",
    "Accounts",
    "Preferences",
    "Done",
];

/// Run the app exe in the background (no window) — e.g. a scan or a playtest run.
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

#[derive(Default)]
struct Shared {
    deps: Option<Vec<Dep>>,
    /// item being installed / signed in: (id, progress text, fraction)
    job: Option<(String, String, Option<f32>)>,
    queue: Vec<(String, Action)>,
    toast: Option<(String, bool)>,
    update: Option<String>,
    moving: Option<String>,
}

struct App {
    ctx: Ctx,
    sh: Arc<Mutex<Shared>>,
    wizard: Option<usize>,
    move_to: Option<PathBuf>,
    desktop_shortcut: bool,
    start_shortcut: bool,
    activity: (String, String, Color32),
    playing: bool,
    last_tick: Instant,
    last_deps: Instant,
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
            move_to,
            desktop_shortcut: true,
            start_shortcut: true,
            activity: ("ROX-AI".into(), "Starting…".into(), ACC),
            playing: false,
            last_tick: Instant::now() - Duration::from_secs(10),
            last_deps: Instant::now(),
        };
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
            if let Ok(st) = crate::update::check() {
                if st.update_available {
                    sh.lock().unwrap().update = st.latest.map(|l| l.version);
                }
            }
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

    // ================================================================ wizard
    fn wizard_ui(&mut self, ui: &mut egui::Ui, step: usize) {
        ui.horizontal_wrapped(|ui| {
            for (i, name) in STEPS.iter().enumerate() {
                let (fill, fg) = if i == step {
                    (ACC, Color32::BLACK)
                } else if i < step {
                    (CARD2, ACC)
                } else {
                    (CARD, MUTED)
                };
                egui::Frame::new()
                    .fill(fill)
                    .corner_radius(20.0)
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("{}  {}", i + 1, name))
                                .small()
                                .color(fg),
                        );
                    });
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
                    ("Lets Claude play them", "Claude opens Roblox on an alt account and actually plays each game."),
                    ("Writes everything down", "One detailed Obsidian note per game: loop, systems, monetization, how to build it."),
                ] {
                    card(ui, CARD, |ui| {
                        ui.label(RichText::new(t).strong().size(15.0));
                        ui.label(RichText::new(d).color(MUTED));
                    });
                    ui.add_space(6.0);
                }
                ui.add_space(6.0);
                ui.label(RichText::new("Setup takes about 5 minutes. ROX-AI is portable — it keeps everything in its own folder.").color(MUTED));
                next_label = "Get started";
            }
            1 => self.step_location(ui),
            2 => {
                ui.label(RichText::new("Apps ROX-AI works with").size(22.0).strong());
                ui.label(
                    RichText::new(
                        "Green = already on your PC. Anything missing installs with one click.",
                    )
                    .color(MUTED),
                );
                ui.add_space(10.0);
                let apps = ["obsidian", "roblox", "claude_code", "claude_desktop"];
                self.dep_rows(ui, deps.as_deref(), &apps, job.as_ref());
                let missing: Vec<(String, Action)> = deps
                    .as_ref()
                    .map(|d| {
                        d.iter()
                            .filter(|x| apps.contains(&x.id) && !x.ok && x.id != "claude_desktop")
                            .filter_map(|x| x.action.map(|(a, _)| (x.id.to_string(), a)))
                            .collect()
                    })
                    .unwrap_or_default();
                ui.add_space(6.0);
                if !missing.is_empty()
                    && job.is_none()
                    && ui
                        .add(btn(
                            &format!("Install all missing ({})", missing.len()),
                            ACC,
                            230.0,
                            38.0,
                        ))
                        .clicked()
                {
                    self.enqueue(missing.clone());
                }
                next_enabled = job.is_none();
                if !missing.is_empty() {
                    next_label = "Skip for now";
                }
            }
            3 => {
                ui.label(RichText::new("Sign in").size(22.0).strong());
                ui.label(RichText::new("Two one-time sign-ins. Each opens your browser; this page turns green by itself.").color(MUTED));
                ui.add_space(10.0);
                self.dep_rows(
                    ui,
                    deps.as_deref(),
                    &["roblox_login", "claude_login"],
                    job.as_ref(),
                );
                ui.add_space(8.0);
                card(ui, CARD, |ui| {
                    ui.label(
                        RichText::new("Use an ALT Roblox account, never your main.")
                            .color(AMBER)
                            .strong(),
                    );
                    ui.label(RichText::new("Automated play is against Roblox's rules; an alt keeps your real account safe. After signing in on the website, press Play on any game once so the Roblox app remembers you.").color(MUTED).small());
                });
                next_enabled = job.is_none();
                if deps.as_ref().is_some_and(|d| {
                    d.iter()
                        .any(|x| (x.id == "roblox_login" || x.id == "claude_login") && !x.ok)
                }) {
                    next_label = "Skip for now";
                }
            }
            4 => {
                ui.label(RichText::new("Preferences").size(22.0).strong());
                ui.add_space(10.0);
                let s = &mut self.ctx.settings;
                card(ui, CARD, |ui| {
                    ui.checkbox(
                        &mut s.auto_run,
                        RichText::new("Run automatically").strong().size(15.0),
                    );
                    ui.label(RichText::new("Updates the charts every day, and once a week lets Claude play new games while your PC is idle.").color(MUTED).small());
                });
                ui.add_space(6.0);
                card(ui, CARD, |ui| {
                    ui.label(RichText::new("Games Claude plays each week").strong());
                    ui.add(egui::Slider::new(&mut s.playtests_per_run, 1..=25));
                    ui.label(
                        RichText::new("More games use more of your Claude plan.")
                            .color(MUTED)
                            .small(),
                    );
                });
                ui.add_space(6.0);
                card(ui, CARD, |ui| {
                    ui.label(RichText::new("Minutes per game").strong());
                    ui.add(egui::Slider::new(&mut s.playtest_minutes, 5..=30));
                });
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
                ui.label(
                    RichText::new(
                        "ROX-AI is reading today's charts and building your vault right now.",
                    )
                    .color(MUTED),
                );
                ui.add_space(14.0);
                if ui.add(btn("Open my vault", ACC, 220.0, 46.0)).clicked() {
                    let ctx = self.ctx.clone();
                    std::thread::spawn(move || crate::setup::open_vault(&ctx));
                }
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
                    if step > 0 && step < 5 && ui.add(btn("Back", CARD, 90.0, 42.0)).clicked() {
                        self.wizard = Some(step - 1);
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
            1 => {
                let shortcuts = (self.desktop_shortcut, self.start_shortcut);
                if let Some(dest) = self.move_to.clone() {
                    // copy to the new folder, restart from there, the new copy cleans up the old one
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
                self.wizard = Some(2);
            }
            4 => {
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
                self.wizard = Some(5);
            }
            5 => {
                self.wizard = None;
                self.tick();
            }
            s => self.wizard = Some(s + 1),
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
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 34.0), egui::Sense::hover());
                    ui.painter().circle_filled(
                        r.center(),
                        6.0,
                        if d.ok {
                            ACC
                        } else if running.is_some() {
                            AMBER
                        } else {
                            MUTED
                        },
                    );
                    ui.vertical(|ui| {
                        ui.label(RichText::new(d.name).strong().size(15.0));
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
                            ui.label(RichText::new("install Roblox first").color(MUTED).small());
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

    // ============================================================ home screen
    fn tick(&mut self) {
        self.last_tick = Instant::now();
        let st = self.ctx.state_dir();
        self.playing = false;
        if let Some(d) = std::fs::read_to_string(self.ctx.session_file())
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        {
            let el = d["elapsed"].as_u64().unwrap_or(0);
            let total = d["budget_s"].as_f64().unwrap_or(0.0) as u64;
            self.playing = true;
            self.activity = (
                format!(
                    "Playing {}",
                    crate::vault::clean_display_name(d["name"].as_str().unwrap_or("a game"))
                ),
                format!(
                    "{}:{:02} of {}:00 — hands off! Moving your mouse stops the AI.",
                    el / 60,
                    el % 60,
                    total / 60
                ),
                ACC,
            );
            return;
        }
        if crate::util::is_locked(&st, "autopilot") {
            self.activity = (
                "Playtests running".into(),
                "Claude is about to play the next game…".into(),
                ACC,
            );
            return;
        }
        if crate::util::is_locked(&st, "scan") {
            self.activity = (
                "Updating the charts…".into(),
                "Reading the top Roblox games (about a minute)".into(),
                ACC,
            );
            return;
        }
        let games = self.ctx.store.games().len();
        let played = self.ctx.store.count_playtests("completed");
        let missing = self.sh.lock().unwrap().deps.as_ref().map(|d| {
            d.iter()
                .filter(|x| !x.ok && x.id != "claude_desktop")
                .count()
        });
        if let Some(b) = self.ctx.store.get("blocked_reason") {
            self.activity = (
                "Needs you".into(),
                b["reason"].as_str().unwrap_or("").to_string(),
                AMBER,
            );
        } else if let Some(m) = missing.filter(|m| *m > 0) {
            self.activity = (
                format!("{m} setup step{} left", if m == 1 { "" } else { "s" }),
                "Click “Finish setup” to complete it.".into(),
                AMBER,
            );
        } else {
            let when = if self.ctx.settings.auto_run {
                "Charts update daily · Claude plays new games every week"
            } else {
                "Automatic mode is off — use the buttons below"
            };
            self.activity = (
                "All set".into(),
                format!("{games} games tracked · {played} played by Claude\n{when}"),
                ACC,
            );
        }
    }

    fn home_ui(&mut self, ui: &mut egui::Ui) {
        if self.last_tick.elapsed() > Duration::from_millis(1500) {
            self.tick();
        }
        let full = ui.available_width();
        let (title, sub, col) = self.activity.clone();
        card(ui, CARD, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 24.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 6.0, col);
                ui.label(RichText::new(title).size(21.0).strong().color(col));
            });
            ui.label(RichText::new(sub).color(MUTED));
            if self.playing {
                ui.add_space(8.0);
                if ui.add(btn("Stop", RED, 120.0, 34.0)).clicked() {
                    crate::session::request_stop(&self.ctx, "Stop button");
                    bg_cli(&["stop"]);
                    self.toast("Stopped. Roblox closed.", true);
                }
            }
        });
        ui.add_space(10.0);
        let deps = self.sh.lock().unwrap().deps.clone();
        let incomplete = deps
            .as_ref()
            .is_some_and(|d| d.iter().any(|x| !x.ok && x.id != "claude_desktop"));
        if incomplete && ui.add(btn("Finish setup", AMBER, 160.0, 34.0)).clicked() {
            let first_missing = deps
                .as_ref()
                .and_then(|d| d.iter().find(|x| !x.ok && x.id != "claude_desktop"))
                .map(|x| x.id)
                .unwrap_or("obsidian");
            self.wizard = Some(if first_missing.ends_with("_login") {
                3
            } else {
                2
            });
        }
        ui.add_space(14.0);
        let ready = deps.as_ref().is_some_and(|l| {
            l.iter()
                .filter(|d| d.id != "claude_desktop" && d.id != "obsidian")
                .all(|d| d.ok)
        });
        let bw = (full - 12.0) / 2.0;
        ui.horizontal(|ui| {
            if ui.add(btn("Open my vault", ACC, bw, 56.0)).clicked() {
                let ctx = self.ctx.clone();
                std::thread::spawn(move || crate::setup::open_vault(&ctx));
            }
            if ui.add_enabled(ready && !self.playing, btn("Play the top games now", CARD2, bw, 56.0)).on_disabled_hover_text("Finish the setup first (Roblox and Claude sign-ins).").clicked() {
                bg_cli(&["autopilot", "--force", "--idle-wait", "1"]);
                self.toast("Starting — Claude opens Roblox in a moment. Don't touch the mouse while it plays.", true);
            }
        });
        ui.add_space(12.0);
        let mut auto = self.ctx.settings.auto_run;
        if ui.checkbox(&mut auto, "Run automatically (update charts daily, play new games weekly when the PC is idle)").changed() {
            self.ctx.settings.auto_run = auto;
            let _ = self.ctx.settings.save(&self.ctx.home);
            let ctx = self.ctx.clone();
            std::thread::spawn(move || crate::setup::apply_auto_run(&ctx));
            self.tick();
        }
        if let Some((m, ok)) = self.sh.lock().unwrap().toast.clone() {
            ui.add_space(8.0);
            ui.label(RichText::new(m).color(if ok { ACC } else { RED }));
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                let upd = self.sh.lock().unwrap().update.clone();
                if let Some(v) = upd {
                    if ui
                        .add(btn(&format!("Update to v{v}"), AMBER, 0.0, 30.0))
                        .clicked()
                    {
                        self.toast("Downloading the update…", true);
                        let (ctx, sh) = (self.ctx.clone(), self.sh.clone());
                        std::thread::spawn(move || match crate::update::run(&ctx, false) {
                            Ok(_) => std::process::exit(0),
                            Err(e) => {
                                sh.lock().unwrap().toast =
                                    Some((format!("Update failed: {e}"), false))
                            }
                        });
                    }
                }
                self.more_menu(ui);
            });
        });
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
            if ui.button("Check for updates").clicked() {
                let sh = self.sh.clone();
                std::thread::spawn(move || {
                    let msg = match crate::update::check() {
                        Ok(s) if s.update_available => {
                            let v = s.latest.map(|l| l.version);
                            sh.lock().unwrap().update = v.clone();
                            (format!("Update available: v{} — click the yellow button.", v.unwrap_or_default()), true)
                        }
                        Ok(s) => (format!("You have the latest version (v{}).", s.current), true),
                        Err(e) => (format!("Couldn't check for updates: {e}"), false),
                    };
                    sh.lock().unwrap().toast = Some(msg);
                });
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
                > Duration::from_secs(if self.wizard.is_some() { 6 } else { 20 })
        {
            self.refresh_deps();
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("ROX-AI").size(24.0).strong().color(ACC));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("v{}", crate::VERSION))
                        .color(MUTED)
                        .small(),
                );
                if self.wizard.is_some() {
                    ui.label(RichText::new("Setup").color(MUTED));
                }
            });
        });
        ui.add_space(12.0);
        match self.wizard {
            Some(step) => self.wizard_ui(ui, step),
            None => self.home_ui(ui),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(Duration::from_millis(400));
        egui::Frame::new()
            .fill(BG)
            .inner_margin(22.0)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                self.body(ui)
            });
    }
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
            .with_inner_size([680.0, 700.0])
            .with_min_inner_size([600.0, 620.0])
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
            v.selection.bg_fill = ACC;
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
