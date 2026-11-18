//! ROX-AI — Roblox chart analyst, AI playtester (MCP server for Claude) and Obsidian vault builder.

#[macro_use]
pub mod logx;

pub mod analysis;
pub mod api;
pub mod autopilot;
pub mod cli;
pub mod config;
pub mod doctor;
pub mod mcp;
pub mod panel;
pub mod scan;
pub mod selftest;
pub mod session;
pub mod setup;
pub mod store;
pub mod util;
pub mod vault;
pub mod win;

pub use config::Ctx;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PLAYBOOK: &str = include_str!("prompts/playtest.md");
