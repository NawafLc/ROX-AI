//! The ROX-AI app window. Deliberately simple:
//!   one status card · a short setup checklist (only what's missing) · two big buttons · one switch.
//! Everything else happens automatically; slow work runs in the background so the window never freezes.

use crate::config::Ctx;
use crate::deps::{Action, Dep};
use eframe::egui::{self, Color32, RichText};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ACC: Color32 = Color32::from_rgb(0, 214, 168);
const RED: Color32 = Color32::from_rgb(240, 82, 96);
const AMBER: Color32 = Color32::from_rgb(255, 196, 82);
const CARD: Color32 = Color32::from_rgb(30, 34, 48);
const BG: Color32 = Color32::from_rgb(19, 21, 31);
const MUTED: Color32 = Color32::from_rgb(140, 148, 168);

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
    busy: Option<String>,
    toast: Option<(String, bool)>,
    update: Option<String>,
}

struct App {
    ctx: Ctx,
    sh: Arc<Mutex<Shared>>,
    activity: (String, String, Color32),
    playing: bool,
    last_tick: Instant,
    last_deps: Instant,
}

impl App {
    fn new(ctx: Ctx) -> App {
        let mut a = App {
            ctx,
            sh: Arc::new(Mutex::new(Shared::default())),
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
        std::thread::spawn(move || {
            let api = crate::api::HttpApi::new().ok();
            let _ =
                crate::setup::ensure(&ctx, api.as_ref().map(|a| a as &dyn crate::api::RobloxApi));
            let scanned_today = !ctx
                .store
                .chart_on(&crate::util::today(), &ctx.settings.sort_id)
                .is_empty();
            if !scanned_today && !crate::util::is_locked(&ctx.state_dir(), "scan") {
                bg_cli(&["scan", "--if-needed"]);
            }
            let deps = crate::deps::status(&ctx);
            sh.lock().unwrap().deps = Some(deps);
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

    fn run_dep(&mut self, d: &Dep, action: Action) {
        let (ctx, sh, id, name) = (self.ctx.clone(), self.sh.clone(), d.id, d.name);
        let verb = if action == Action::Install {
            "Installing"
        } else {
            "Signing in to"
        };
        sh.lock().unwrap().busy = Some(format!("{verb} {name}… (this can take a few minutes)"));
        std::thread::spawn(move || {
            let r = crate::deps::act(&ctx, id, action);
            let deps = crate::deps::status(&ctx);
            let mut g = sh.lock().unwrap();
            g.busy = None;
            g.deps = Some(deps);
            g.toast = Some(match r {
                Ok(m) => (m, true),
                Err(e) => (format!("{name}: {e}"), false),
            });
        });
    }

    /// What is ROX-AI doing right now? (cheap file checks, every ~1.5 s)
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
                format!(
                    "Almost ready — {m} step{} left",
                    if m == 1 { "" } else { "s" }
                ),
                "Finish the setup below. You only do this once.".into(),
                AMBER,
            );
        } else if missing.is_none() {
            self.activity = ("ROX-AI".into(), "Getting things ready…".into(), ACC);
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

    fn body(&mut self, ui: &mut egui::Ui) {
        if self.last_tick.elapsed() > Duration::from_millis(1500) {
            self.tick();
        }
        if self.last_deps.elapsed() > Duration::from_secs(20) {
            self.refresh_deps();
        }
        let full = ui.available_width();

        ui.horizontal(|ui| {
            ui.label(RichText::new("ROX-AI").size(24.0).strong().color(ACC));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("v{}", crate::VERSION))
                        .color(MUTED)
                        .small(),
                );
            });
        });
        ui.add_space(10.0);

        // ---- status card
        let (title, sub, col) = self.activity.clone();
        egui::Frame::new()
            .fill(CARD)
            .corner_radius(12.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.set_width(full - 32.0);
                ui.horizontal(|ui| {
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(14.0, 22.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 6.0, col);
                    ui.label(RichText::new(title).size(20.0).strong().color(col));
                });
                ui.add_space(2.0);
                ui.label(RichText::new(sub).color(MUTED));
                if self.playing {
                    ui.add_space(8.0);
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Stop")
                                    .size(16.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(RED)
                            .min_size(egui::vec2(120.0, 34.0)),
                        )
                        .clicked()
                    {
                        crate::session::request_stop(&self.ctx, "Stop button");
                        bg_cli(&["stop"]);
                        self.toast("Stopped. Roblox closed.", true);
                    }
                }
            });
        ui.add_space(12.0);

        // ---- setup checklist: only what's missing
        let (deps, busy) = {
            let g = self.sh.lock().unwrap();
            (g.deps.clone(), g.busy.clone())
        };
        match &deps {
            None => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label(RichText::new("Checking what's already on this PC…").color(MUTED));
                });
            }
            Some(list) => {
                let missing: Vec<Dep> = list.iter().filter(|d| !d.ok).cloned().collect();
                let have: Vec<&str> = list.iter().filter(|d| d.ok).map(|d| d.name).collect();
                for d in &missing {
                    egui::Frame::new()
                        .fill(CARD)
                        .corner_radius(10.0)
                        .inner_margin(egui::Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.set_width(full - 28.0);
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(d.name).strong().size(15.0));
                                    ui.label(RichText::new(d.why).color(MUTED).small());
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if d.action.is_none() {
                                            ui.label(
                                                RichText::new("after the step above")
                                                    .color(MUTED)
                                                    .small(),
                                            );
                                        }
                                        if let Some((act, label)) = d.action {
                                            let b = egui::Button::new(
                                                RichText::new(label).strong().color(Color32::BLACK),
                                            )
                                            .fill(ACC)
                                            .min_size(egui::vec2(96.0, 30.0));
                                            if ui.add_enabled(busy.is_none(), b).clicked() {
                                                self.run_dep(d, act);
                                            }
                                        }
                                    },
                                );
                            });
                        });
                    ui.add_space(6.0);
                }
                if let Some(b) = &busy {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label(RichText::new(b).color(ACC));
                    });
                }
                if !have.is_empty() {
                    ui.label(
                        RichText::new(format!("Already on this PC:  {}", have.join(" · ")))
                            .color(MUTED)
                            .small(),
                    );
                }
            }
        }
        ui.add_space(14.0);

        // ---- the two main buttons
        let ready = deps.as_ref().is_some_and(|l| {
            l.iter()
                .filter(|d| d.id != "claude_desktop" && d.id != "obsidian")
                .all(|d| d.ok)
        });
        let bw = (full - 12.0) / 2.0;
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new("Open my vault").size(16.0).strong().color(Color32::BLACK)).fill(ACC).min_size(egui::vec2(bw, 52.0))).clicked() {
                let ctx = self.ctx.clone();
                std::thread::spawn(move || crate::setup::open_vault(&ctx));
            }
            let play = egui::Button::new(RichText::new("Play the top games now").size(16.0).strong()).fill(CARD).min_size(egui::vec2(bw, 52.0));
            if ui.add_enabled(ready && !self.playing, play).on_disabled_hover_text("Finish the setup first (Roblox and Claude sign-ins).").clicked() {
                bg_cli(&["autopilot", "--force", "--idle-wait", "1"]);
                self.toast("Starting — Claude opens Roblox in a moment. Don't touch the mouse while it plays.", true);
            }
        });
        ui.add_space(12.0);

        // ---- the one switch
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

        // ---- footer
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                let upd = self.sh.lock().unwrap().update.clone();
                if let Some(v) = upd {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(format!("Update to v{v}"))
                                    .strong()
                                    .color(Color32::BLACK),
                            )
                            .fill(AMBER),
                        )
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
            if ui.button("Check for updates").clicked() {
                let sh = self.sh.clone();
                std::thread::spawn(move || {
                    let msg = match crate::update::check() {
                        Ok(s) if s.update_available => {
                            let v = s.latest.map(|l| l.version);
                            sh.lock().unwrap().update = v.clone();
                            (
                                format!(
                                    "Update available: v{} — click the yellow button.",
                                    v.unwrap_or_default()
                                ),
                                true,
                            )
                        }
                        Ok(s) => (
                            format!("You have the latest version (v{}).", s.current),
                            true,
                        ),
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
            if ui.button("Settings file").clicked() {
                crate::util::open_path(
                    &crate::config::Settings::path(&self.ctx.home).to_string_lossy(),
                );
                ui.close();
            }
            if self.ctx.store.get("blocked_reason").is_some()
                && ui.button("I fixed the login").clicked()
            {
                let _ = self
                    .ctx
                    .store
                    .set("blocked_reason", serde_json::Value::Null);
                self.refresh_deps();
                ui.close();
            }
            ui.separator();
            if ui
                .button("Remove from this PC")
                .on_hover_text(
                    "Turns off the schedule and disconnects Claude. Then just delete the folder.",
                )
                .clicked()
            {
                let (ctx, sh) = (self.ctx.clone(), self.sh.clone());
                std::thread::spawn(move || {
                    let m = crate::setup::remove_from_pc(&ctx);
                    sh.lock().unwrap().toast = Some((m, true));
                });
                self.ctx.settings.auto_run = false;
                ui.close();
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(Duration::from_millis(500));
        egui::Frame::new()
            .fill(BG)
            .inner_margin(20.0)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                self.body(ui)
            });
    }
}

/// Use Windows' own UI font (Segoe UI) so the app looks native; symbols fall back to Segoe UI Symbol.
fn install_fonts(ctx: &egui::Context) {
    let dir =
        std::path::PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into()))
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
            .with_inner_size([620.0, 640.0])
            .with_min_inner_size([560.0, 560.0])
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
            cc.egui_ctx.set_visuals(v);
            cc.egui_ctx.style_mut_of(egui::Theme::Dark, |s| {
                s.spacing.button_padding = egui::vec2(12.0, 6.0);
                s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            });
            Ok(Box::new(App::new(ctx)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("window failed: {e}"))
}
