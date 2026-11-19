//! `roxai` command line (also what the scheduled tasks, shortcuts and Claude call).

use crate::api::HttpApi;
use crate::config::Ctx;
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "roxai",
    version = crate::VERSION,
    about = "ROX-AI — Roblox chart analyst, AI playtester (MCP for Claude) and Obsidian vault builder"
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
    /// (internal) set by the setup wizard after moving the app: clean up the old folder
    #[arg(long, hide = true)]
    pub moved_from: Option<std::path::PathBuf>,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Refresh charts, trends and the vault (no Claude needed)
    Scan {
        #[arg(long)]
        if_needed: bool,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Weekly AI playtest run (only if due, unless --force)
    Autopilot {
        #[arg(long)]
        force: bool,
        /// called by Task Scheduler (short idle wait)
        #[arg(long)]
        scheduled: bool,
        #[arg(long)]
        max: Option<usize>,
        /// minutes to wait for the PC to be idle
        #[arg(long)]
        idle_wait: Option<u64>,
    },
    /// Playtest one game now (via Claude Code)
    Playtest { universe_id: u64 },
    /// Show the playtest queue
    Queue,
    /// Health check
    Doctor,
    /// Make sure everything is connected (runs automatically when the app opens)
    Setup,
    /// Remove the schedule and Claude connections from this PC (your folder stays)
    Remove,
    /// Clear an account blocker after you fixed the login
    Unblock,
    /// Stop any running playtest immediately
    Stop,
    /// Open the control panel (default when started without arguments)
    Panel,
    /// Run the MCP server on stdio (Claude Desktop / Claude Code start this)
    Mcp,
    /// Open the vault in Obsidian
    OpenVault,
    /// Delete the scheduled tasks
    RemoveTasks,
    /// Change a setting, e.g. `roxai set playtests_per_run 5`
    Set { key: String, value: String },
    /// Show settings
    Settings,
    /// Install the latest version from GitHub (keeps settings, vault and history)
    Update {
        /// only check, don't install
        #[arg(long)]
        check: bool,
        /// reinstall even if already up to date
        #[arg(long)]
        force: bool,
    },
    /// Search YouTube, or read one video (`--video <id or url>`)
    #[command(hide = true)]
    Youtube {
        query: Option<String>,
        #[arg(long)]
        video: Option<String>,
    },
    /// Learn one game now from YouTube videos (via Claude Code)
    Learn { universe_id: u64 },
    /// Write the Playbook now: one chapter, or the most outdated ones
    Playbook {
        /// chapter id, e.g. monetization
        #[arg(long)]
        chapter: Option<String>,
        /// write even if nothing is due
        #[arg(long)]
        force: bool,
        /// only refresh the notes and numbers (no Claude)
        #[arg(long)]
        render: bool,
    },
    /// Serve the app's interface + API on localhost (previews, and later the iPhone app)
    #[command(hide = true)]
    UiServer {
        #[arg(long, default_value_t = 7777)]
        port: u16,
        #[arg(long)]
        dist: Option<std::path::PathBuf>,
    },
    /// Write the app icon files (used when building)
    #[command(hide = true)]
    MakeIcons { dir: std::path::PathBuf },
    /// Windows self-test of input, capture, hooks and scheduler (used by CI)
    Selftest,
    /// Build and test games: Rojo + Lune on this PC, then the private test place on Roblox
    #[command(subcommand)]
    Pipeline(PipelineCmd),
    /// Connect ROX-AI to Roblox (Open Cloud API key + private test place)
    #[command(subcommand)]
    Roblox(RobloxCmd),
}

#[derive(Subcommand)]
pub enum PipelineCmd {
    /// Download Rojo and Lune into ROX-AI's tools folder
    Tools,
    /// Start a new game project from the template
    New { name: String },
    /// Build a project and run the local checks (add --remote to also test on Roblox)
    Build {
        dir: std::path::PathBuf,
        #[arg(long)]
        remote: bool,
    },
    /// Build + test the starter project end to end (uses the test place if connected)
    Selftest,
}

#[derive(Subcommand)]
pub enum RobloxCmd {
    /// Save the API key (read from standard input, never from the command line) and the test place
    Connect {
        #[arg(long)]
        place: String,
    },
    /// Show the connection
    Status,
    /// Forget the API key
    Disconnect,
}

fn json_chapters(ctx: &Ctx) -> serde_json::Value {
    serde_json::Value::Array(crate::playbook::status(ctx))
}

fn print(v: impl std::fmt::Display) {
    use std::io::Write;
    let _ = writeln!(std::io::stdout(), "{v}");
}

pub fn run() -> i32 {
    let cli = Cli::parse();
    let mut ctx = Ctx::load();
    crate::logx::init(&ctx.logs_dir());
    if let Some(old) = cli.moved_from.clone() {
        std::thread::spawn(move || crate::relocate::finish_move(&old));
    }
    match dispatch(&mut ctx, cli.cmd.unwrap_or(Cmd::Panel)) {
        Ok(code) => code,
        Err(e) => {
            crate::error!("{e:#}");
            print(format!("error: {e:#}"));
            1
        }
    }
}

fn dispatch(ctx: &mut Ctx, cmd: Cmd) -> Result<i32> {
    let pretty = |v: &serde_json::Value| serde_json::to_string_pretty(v).unwrap_or_default();
    match cmd {
        Cmd::Scan { if_needed, limit } => {
            if if_needed
                && !ctx
                    .store
                    .chart_on(&crate::util::today(), &ctx.settings.sort_id)
                    .is_empty()
            {
                print("already scanned today");
                return Ok(0);
            }
            let api = HttpApi::new()?;
            let r = crate::scan::run_scan(ctx, &api, limit, None)?;
            print(pretty(&serde_json::to_value(&r)?));
        }
        Cmd::Autopilot {
            force,
            scheduled,
            max,
            idle_wait,
        } => {
            let api = HttpApi::new()?;
            let wait =
                Duration::from_secs(idle_wait.unwrap_or(if scheduled { 50 } else { 180 }) * 60);
            print(pretty(&crate::autopilot::run(
                ctx, &api, force, max, None, wait,
            )));
        }
        Cmd::Playtest { universe_id } => {
            let api = HttpApi::new()?;
            print(pretty(&crate::autopilot::run(
                ctx,
                &api,
                true,
                None,
                Some(universe_id),
                Duration::from_secs(60),
            )));
        }
        Cmd::Queue => print(pretty(&serde_json::to_value(crate::scan::playtest_queue(
            ctx,
            Some(100),
            None,
        ))?)),
        Cmd::Doctor => print(crate::doctor::report_text(ctx, true)),
        Cmd::Setup => {
            let api = HttpApi::new()?;
            print(pretty(&crate::setup::ensure(ctx, Some(&api))));
        }
        Cmd::Remove => print(crate::setup::remove_from_pc(ctx)),
        Cmd::Unblock => {
            ctx.store.set("blocked_reason", serde_json::Value::Null)?;
            print("Blocker cleared — playtests will resume at the next check.");
        }
        Cmd::Stop => {
            crate::session::request_stop(ctx, "roxai stop");
            crate::win::input::release_all();
            crate::win::roblox::kill_all();
            print("Stop requested; Roblox closed.");
        }
        Cmd::Panel => {
            let app = crate::util::bg_exe();
            if app == std::env::current_exe().unwrap_or_default() {
                print("Start the app with ROX-AI.exe");
            } else {
                std::process::Command::new(app).spawn()?;
            }
        }
        Cmd::UiServer { port, dist } => {
            let dist = dist.unwrap_or_else(|| crate::config::app_folder().join("ui"));
            crate::ui_api::serve(port, dist)?
        }
        Cmd::MakeIcons { dir } => {
            std::fs::create_dir_all(&dir)?;
            for (name, size) in [("32x32.png", 32u32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)] {
                crate::setup::icon_image(size).save(dir.join(name))?;
            }
            let ctx2 = crate::config::Ctx::at(std::env::temp_dir().join("roxai-icon"));
            let _ = std::fs::remove_file(ctx2.home.join("roxai.ico"));
            std::fs::copy(crate::setup::make_icon(&ctx2)?, dir.join("icon.ico"))?;
            print("icons written");
        }
        Cmd::Mcp => crate::mcp::serve(ctx.clone())?,
        Cmd::OpenVault => print(crate::setup::open_vault(ctx)),
        Cmd::RemoveTasks => crate::setup::remove_tasks(),
        Cmd::Set { key, value } => {
            ctx.settings.set_from_str(&key, &value)?;
            ctx.settings.save(&ctx.home)?;
            let v = serde_json::to_value(&ctx.settings)?;
            print(format!("{key} = {}", v[&key]));
        }
        Cmd::Settings => print(pretty(&serde_json::to_value(&ctx.settings)?)),
        Cmd::Update { check, force } => {
            if check {
                print(pretty(&serde_json::to_value(crate::update::check()?)?));
            } else {
                let msg = crate::update::run(ctx, force)?;
                crate::autopilot::notify(ctx, "ROX-AI update", &msg);
                print(msg);
            }
        }
        Cmd::Youtube { query, video } => match video {
            Some(v) => print(pretty(&serde_json::to_value(crate::youtube::video(
                &v,
                &ctx.tools_dir(),
            )?)?)),
            None => print(pretty(&serde_json::to_value(crate::youtube::search(
                &query.unwrap_or_default(),
                8,
            )?)?)),
        },
        Cmd::Learn { universe_id } => {
            let api = HttpApi::new()?;
            print(pretty(&crate::autopilot::run(
                ctx,
                &api,
                true,
                None,
                Some(universe_id),
                Duration::from_secs(60),
            )));
        }
        Cmd::Playbook { chapter, force, render } => {
            if render {
                crate::playbook::render_all(ctx)?;
                print(pretty(&json_chapters(ctx)));
            } else {
                print(pretty(&crate::playbook::run(ctx, chapter.as_deref(), force)?));
            }
        }
        Cmd::Selftest => return Ok(crate::selftest::run(ctx)),
        Cmd::Pipeline(p) => {
            let report = |t: &str, _f: Option<f32>| eprintln!("... {t}");
            let rep = match p {
                PipelineCmd::Tools => {
                    crate::pipeline::install_tools(ctx, &report)?;
                    print(pretty(&crate::pipeline::tools_status(ctx)));
                    return Ok(0);
                }
                PipelineCmd::New { name } => {
                    print(crate::pipeline::new_project(ctx, &name)?.display());
                    return Ok(0);
                }
                PipelineCmd::Build { dir, remote } => crate::pipeline::run(ctx, &dir, remote, &report)?,
                PipelineCmd::Selftest => crate::pipeline::selftest(ctx, &report)?,
            };
            for s in &rep.steps {
                print(format!("{} {:<24} {}", if s.ok { "OK  " } else { "FAIL" }, s.name, s.detail.replace('\n', "\n      ")));
            }
            return Ok(if rep.ok { 0 } else { 1 });
        }
        Cmd::Roblox(r) => match r {
            RobloxCmd::Connect { place } => {
                let mut key = String::new();
                std::io::stdin().read_line(&mut key)?;
                let c = crate::cloud::connect(ctx, &key, &place)?;
                print(format!("Connected to \"{}\" (place {}, experience {}).", c.place_name, c.place_id, c.universe_id));
            }
            RobloxCmd::Status => match crate::cloud::connection(ctx) {
                Some(c) => print(pretty(&serde_json::to_value(c)?)),
                None => print("Not connected."),
            },
            RobloxCmd::Disconnect => {
                crate::cloud::disconnect(ctx)?;
                print("Roblox key removed from this PC.");
            }
        },
    }
    Ok(0)
}
