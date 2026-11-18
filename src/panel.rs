//! ROX-AI control panel (desktop app, egui).
//!
//! Everything runs automatically; this window is for looking and for the big STOP button.
//! Buttons launch detached background processes, so closing the panel never interrupts a run.

use crate::config::Ctx;
use eframe::egui;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ACC: egui::Color32 = egui::Color32::from_rgb(0, 224, 176);
const RED: egui::Color32 = egui::Color32::from_rgb(255, 77, 94);
const CARD: egui::Color32 = egui::Color32::from_rgb(27, 31, 46);
const BG: egui::Color32 = egui::Color32::from_rgb(18, 20, 31);
const MUTED: egui::Color32 = egui::Color32::from_rgb(138, 147, 168);

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
struct Info {
    health: String,
    queue: String,
}

#[derive(Default, Clone)]
struct UpdateState {
    checking: bool,
    latest: Option<String>,
    available: bool,
    error: Option<String>,
}

struct PanelApp {
    ctx: Ctx,
    info: Arc<Mutex<Info>>,
    update: Arc<Mutex<UpdateState>>,
    msg: (String, egui::Color32),
    session: String,
    last_tick: Instant,
    last_refresh: Instant,
}

impl PanelApp {
    fn refresh(&mut self) {
        self.last_refresh = Instant::now();
        let (ctx, info) = (self.ctx.clone(), self.info.clone());
        std::thread::spawn(move || {
            let h = crate::doctor::report_text(&ctx, false);
            let q = crate::scan::playtest_queue(&ctx, None, None);
            let qt = if q.is_empty() {
                "Nothing queued.".to_string()
            } else {
                q.iter()
                    .enumerate()
                    .map(|(i, x)| format!("{}. {}  (#{}, {})", i + 1, x.title, x.rank, x.reason))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            *info.lock().unwrap() = Info {
                health: h,
                queue: qt,
            };
        });
    }

    fn tick(&mut self) {
        self.last_tick = Instant::now();
        let p = self.ctx.session_file();
        self.session = match std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        {
            Some(d) => {
                let el = d["elapsed"].as_u64().unwrap_or(0);
                format!("🎮 PLAYTESTING: {}   ⏱ {}:{:02} / {} min\nHands off the mouse & keyboard — touching them (or {}) stops the AI.",
                    d["name"].as_str().unwrap_or("?"), el / 60, el % 60, d["budget_s"].as_f64().unwrap_or(0.0) as u64 / 60, self.ctx.settings.kill_hotkey.to_uppercase())
            }
            None => {
                let mut t = "💤 Idle — everything runs automatically (daily scan, weekly playtests when your PC is idle).".to_string();
                if let Some(n) = self.ctx.store.get("last_notice") {
                    t.push_str(&format!(
                        "\nLast: {} — {}",
                        n["title"].as_str().unwrap_or(""),
                        n["text"].as_str().unwrap_or("")
                    ));
                }
                t
            }
        };
    }

    /// Ask GitHub (in the background) whether a newer release exists.
    fn check_update(&mut self) {
        let st = self.update.clone();
        st.lock().unwrap().checking = true;
        std::thread::spawn(move || {
            let r = crate::update::check();
            let mut g = st.lock().unwrap();
            g.checking = false;
            match r {
                Ok(s) => {
                    g.latest = s.latest.map(|l| l.version);
                    g.available = s.update_available;
                    g.error = None;
                }
                Err(e) => g.error = Some(e.to_string()),
            }
        });
    }

    fn click_update(&mut self) {
        let st = self.update.lock().unwrap().clone();
        if st.available {
            self.fire(&["update"], "⬆️ Downloading the update… an installer window will open, finish, and reopen ROX-AI.");
        } else {
            self.check_update();
            self.msg = (
                match (&st.error, &st.latest) {
                    (Some(e), _) => format!("Update check failed: {e}"),
                    (None, Some(_)) => format!(
                        "✅ You have the latest version (v{}). Re-checking GitHub…",
                        crate::VERSION
                    ),
                    _ => "Checking GitHub for updates…".into(),
                },
                ACC,
            );
        }
    }

    fn fire(&mut self, args: &[&str], msg: &str) {
        bg_cli(args);
        self.msg = (msg.to_string(), ACC);
        self.last_refresh = Instant::now() - Duration::from_secs(26);
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        if self.last_tick.elapsed() > Duration::from_millis(1500) {
            self.tick();
        }
        if self.last_refresh.elapsed() > Duration::from_secs(30) {
            self.refresh();
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("ROX-AI").size(26.0).strong().color(ACC));
            ui.label(
                egui::RichText::new("Roblox chart analyst · AI playtester · Obsidian vault")
                    .color(MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("🔄 Refresh").clicked() {
                    self.refresh();
                    self.tick();
                    self.check_update();
                }
                ui.label(egui::RichText::new(format!("v{}", crate::VERSION)).color(MUTED));
            });
        });
        ui.add_space(6.0);
        egui::Frame::new()
            .fill(CARD)
            .corner_radius(8.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let col = if self.session.starts_with("🎮") {
                    ACC
                } else {
                    egui::Color32::WHITE
                };
                ui.label(egui::RichText::new(&self.session).size(14.0).color(col));
            });
        ui.add_space(8.0);
        let hot = self.ctx.settings.kill_hotkey.to_uppercase();
        let w = (ui.available_width() - 16.0) / 3.0;
        let big =
            |t: &str, fill: egui::Color32| {
                egui::Button::new(egui::RichText::new(t).size(14.0).strong().color(
                    if fill == CARD {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::BLACK
                    },
                ))
                .fill(fill)
                .min_size(egui::vec2(w, 38.0))
            };
        ui.horizontal(|ui| {
            if ui.add(big("📖 Open Vault", ACC)).clicked() {
                self.fire(&["open-vault"], "Opening the vault…");
            }
            if ui.add(big("📊 Scan charts now", CARD)).clicked() {
                self.fire(&["scan"], "Chart scan started (about a minute).");
            }
            if ui.add(big("🕹️ Run playtests now", CARD)).clicked() {
                self.fire(
                    &["autopilot", "--force"],
                    "Playtest run started — hands off the PC.",
                );
            }
        });
        ui.horizontal(|ui| {
            if ui.add(big(&format!("⛔ STOP ({hot})"), RED)).clicked() {
                crate::session::request_stop(&self.ctx, "panel STOP button");
                bg_cli(&["stop"]);
                self.msg = ("⛔ Stop sent — Roblox closed, AI halted.".into(), RED);
            }
            if ui.add(big("✅ I fixed the login", CARD)).clicked() {
                self.fire(
                    &["unblock"],
                    "Blocker cleared — playtests resume at the next check.",
                );
            }
            if ui.add(big("🔧 Repair setup", CARD)).clicked() {
                self.fire(&["setup"], "Re-running setup…");
            }
        });
        ui.horizontal(|ui| {
            if ui.add(big("📁 Logs", CARD)).clicked() {
                crate::util::open_path(&self.ctx.logs_dir().to_string_lossy());
            }
            if ui.add(big("⚙️ Settings", CARD)).clicked() {
                crate::util::open_path(
                    &crate::config::Settings::path(&self.ctx.home).to_string_lossy(),
                );
            }
            let us = self.update.lock().unwrap().clone();
            let (label, fill) = if us.available {
                (
                    format!("⬆️ Update to v{}", us.latest.clone().unwrap_or_default()),
                    ACC,
                )
            } else if us.checking {
                ("⏳ Checking for updates…".to_string(), CARD)
            } else {
                ("⬆️ Check for updates".to_string(), CARD)
            };
            if ui.add(big(&label, fill)).clicked() {
                self.click_update();
            }
        });
        if !self.msg.0.is_empty() {
            ui.label(egui::RichText::new(&self.msg.0).color(self.msg.1));
        }
        ui.add_space(8.0);
        let info = self.info.lock().unwrap();
        ui.label(egui::RichText::new("System health").strong().color(MUTED));
        egui::Frame::new()
            .fill(CARD)
            .corner_radius(8.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(if info.health.is_empty() {
                                "checking…"
                            } else {
                                &info.health
                            })
                            .monospace(),
                        );
                    });
            });
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("Next playtest queue")
                .strong()
                .color(MUTED),
        );
        egui::Frame::new()
            .fill(CARD)
            .corner_radius(8.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(if info.queue.is_empty() {
                    "…"
                } else {
                    &info.queue
                });
            });
    }
}

impl eframe::App for PanelApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(Duration::from_millis(500));
        egui::Frame::new()
            .fill(BG)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                self.draw(ui)
            });
    }
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
            .with_inner_size([780.0, 660.0])
            .with_min_inner_size([640.0, 520.0])
            .with_icon(icon),
        ..Default::default()
    };
    let mut app = PanelApp {
        ctx,
        info: Arc::new(Mutex::new(Info::default())),
        update: Arc::new(Mutex::new(UpdateState::default())),
        msg: (String::new(), ACC),
        session: String::new(),
        last_tick: Instant::now(),
        last_refresh: Instant::now(),
    };
    app.tick();
    app.refresh();
    app.check_update();
    eframe::run_native(
        "ROX-AI",
        opts,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("panel failed: {e}"))
}
