//! `roxai` command line (also what the scheduled tasks, shortcuts and Claude call).

use crate::api::HttpApi;
use crate::config::Ctx;
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "roxai",
    version,
    about = "ROX-AI — Roblox chart analyst, AI playtester (MCP for Claude) and Obsidian vault builder"
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
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
    /// Vault, Claude registration, scheduled tasks, shortcuts
    Setup {
        #[arg(long)]
        open: bool,
    },
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
    /// Windows self-test of input, capture, hooks and scheduler (used by CI)
    Selftest,
}

fn print(v: impl std::fmt::Display) {
    use std::io::Write;
    let _ = writeln!(std::io::stdout(), "{v}");
}

pub fn run() -> i32 {
    let cli = Cli::parse();
    let mut ctx = Ctx::load();
    crate::logx::init(&ctx.logs_dir());
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
        Cmd::Setup { open } => {
            let api = HttpApi::new()?;
            print(pretty(&crate::setup::setup_all(ctx, &api, open)));
        }
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
        Cmd::Panel => crate::panel::run(ctx.clone())?,
        Cmd::Mcp => crate::mcp::serve(ctx.clone())?,
        Cmd::OpenVault => crate::setup::open_vault(ctx),
        Cmd::RemoveTasks => crate::setup::remove_tasks(),
        Cmd::Set { key, value } => {
            ctx.settings.set_from_str(&key, &value)?;
            ctx.settings.save(&ctx.home)?;
            let v = serde_json::to_value(&ctx.settings)?;
            print(format!("{key} = {}", v[&key]));
        }
        Cmd::Settings => print(pretty(&serde_json::to_value(&ctx.settings)?)),
        Cmd::Selftest => return Ok(crate::selftest::run(ctx)),
    }
    Ok(0)
}
