//! ROX-AI — Roblox chart analyst, AI playtester (MCP server for Claude) and Obsidian vault builder.

#[macro_use]
pub mod logx;

pub mod analysis;
pub mod api;
pub mod autopilot;
pub mod cli;
pub mod cloud;
pub mod config;
pub mod deps;
pub mod doctor;
pub mod gamesview;
pub mod mcp;
pub mod pipeline;
pub mod playbook;
pub mod relocate;
pub mod render;
pub mod scan;
pub mod selftest;
pub mod session;
pub mod setup;
pub mod store;
pub mod team;
pub mod team_tools;
pub mod ui_api;
pub mod update;
pub mod util;
pub mod vault;
pub mod win;
pub mod youtube;

pub use config::Ctx;

/// "2.0.<CI build number>" for release builds (set by CI via ROXAI_VERSION), else the crate version.
pub const VERSION: &str = match option_env!("ROXAI_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};
pub const PLAYBOOK: &str = include_str!("prompts/playtest.md");
pub const LEARN_PLAYBOOK: &str = include_str!("prompts/learn.md");
pub const PLAYBOOK_GUIDE: &str = include_str!("prompts/playbook.md");
