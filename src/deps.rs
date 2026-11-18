//! The apps ROX-AI works with: detects what you already have and offers one-click install / sign-in
//! for what's missing. Everything runs hidden (no console windows); installs use winget when present,
//! otherwise the official download is opened.

use crate::config::Ctx;
use crate::util::run_capture;
use anyhow::{bail, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Install,
    SignIn,
}

#[derive(Clone, Debug)]
pub struct Dep {
    pub id: &'static str,
    pub name: &'static str,
    pub why: &'static str,
    pub ok: bool,
    pub action: Option<(Action, &'static str)>,
}

fn env_path(k: &str) -> Option<PathBuf> {
    std::env::var_os(k).map(PathBuf::from)
}

pub fn claude_desktop_installed() -> bool {
    if !cfg!(windows) {
        return false;
    }
    let la = env_path("LOCALAPPDATA").unwrap_or_default();
    let direct = [
        la.join("AnthropicClaude").join("claude.exe"),
        la.join("Programs").join("Claude").join("Claude.exe"),
        la.join("AnthropicClaude").join("Claude.exe"),
    ];
    if direct.iter().any(|p| p.exists()) {
        return true;
    }
    std::fs::read_dir(la.join("Packages"))
        .map(|rd| {
            rd.flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("Claude_"))
        })
        .unwrap_or(false)
}

/// The Roblox app keeps its login in this file once you've signed in inside the app.
pub fn roblox_signed_in() -> bool {
    let Some(la) = env_path("LOCALAPPDATA") else {
        return false;
    };
    let f = la
        .join("Roblox")
        .join("LocalStorage")
        .join("RobloxCookies.dat");
    std::fs::metadata(f).map(|m| m.len() > 200).unwrap_or(false)
}

pub fn claude_signed_in(ctx: &Ctx) -> bool {
    let Some(c) = crate::autopilot::find_claude(ctx) else {
        return false;
    };
    run_capture(
        &c.to_string_lossy(),
        &["auth", "status"],
        Duration::from_secs(30),
    )
    .is_ok_and(|(code, _)| code == 0)
}

pub fn status(ctx: &Ctx) -> Vec<Dep> {
    let obs = crate::setup::obsidian_exe().is_some();
    let rb = crate::win::roblox::is_installed();
    let blocked = ctx
        .store
        .get("blocked_reason")
        .map(|b| b["reason"].as_str().unwrap_or("").to_lowercase())
        .unwrap_or_default();
    let rb_login =
        rb && roblox_signed_in() && !(blocked.contains("login") && !blocked.contains("claude"));
    let cd = claude_desktop_installed();
    let cc = crate::autopilot::find_claude(ctx).is_some();
    let cc_login = cc && claude_signed_in(ctx) && !blocked.contains("claude");
    vec![
        Dep {
            id: "obsidian",
            name: "Obsidian",
            why: "Shows your vault of game notes",
            ok: obs,
            action: (!obs).then_some((Action::Install, "Install")),
        },
        Dep {
            id: "roblox",
            name: "Roblox",
            why: "The AI plays the games in it",
            ok: rb,
            action: (!rb).then_some((Action::Install, "Install")),
        },
        Dep {
            id: "roblox_login",
            name: "Roblox alt account",
            why: "Sign in once — use an ALT account, never your main",
            ok: rb_login,
            action: (rb && !rb_login).then_some((Action::SignIn, "Sign in")),
        },
        Dep {
            id: "claude_code",
            name: "Claude Code",
            why: "Lets Claude play the games automatically",
            ok: cc,
            action: (!cc).then_some((Action::Install, "Install")),
        },
        Dep {
            id: "claude_login",
            name: "Claude account",
            why: "Uses your Claude subscription",
            ok: cc_login,
            action: (cc && !cc_login).then_some((Action::SignIn, "Sign in")),
        },
        Dep {
            id: "claude_desktop",
            name: "Claude Desktop",
            why: "Optional — chat with ROX-AI yourself",
            ok: cd,
            action: (!cd).then_some((Action::Install, "Install")),
        },
    ]
}

fn winget(id: &str) -> Result<()> {
    let (code, out) = run_capture(
        "winget",
        &[
            "install",
            "--id",
            id,
            "-e",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
        ],
        Duration::from_secs(20 * 60),
    )?;
    if code != 0 && !out.to_lowercase().contains("already installed") {
        bail!("winget exit {code}");
    }
    Ok(())
}

/// Perform an install / sign-in. Blocking (run it on a background thread). Returns a short message.
pub fn act(ctx: &Ctx, id: &str, action: Action) -> Result<String> {
    match (id, action) {
        ("obsidian", Action::Install) => match winget("Obsidian.Obsidian") {
            Ok(()) => Ok("Obsidian installed.".into()),
            Err(_) => {
                crate::util::open_path("https://obsidian.md/download");
                Ok("Opened the Obsidian download page — run the installer, then come back.".into())
            }
        },
        ("roblox", Action::Install) => match winget("Roblox.Roblox") {
            Ok(()) => Ok("Roblox installed.".into()),
            Err(_) => {
                crate::util::open_path("https://www.roblox.com/download");
                Ok("Opened the Roblox download page — run it, then come back.".into())
            }
        },
        ("roblox_login", Action::SignIn) => {
            // Sign in on the website, then open any game once: the Roblox app takes over the login.
            crate::util::open_path("https://www.roblox.com/login");
            Ok("Sign in with your ALT account in the browser, then press Play on any game once and close it.".into())
        }
        ("claude_desktop", Action::Install) => match winget("Anthropic.Claude") {
            Ok(()) => Ok("Claude Desktop installed — open it once and sign in.".into()),
            Err(_) => {
                crate::util::open_path("https://claude.ai/download");
                Ok("Opened the Claude Desktop download page.".into())
            }
        },
        ("claude_code", Action::Install) => {
            #[cfg(windows)]
            {
                let (code, out) = run_capture(
                    "powershell",
                    &[
                        "-NoProfile",
                        "-ExecutionPolicy",
                        "Bypass",
                        "-WindowStyle",
                        "Hidden",
                        "-Command",
                        "irm https://claude.ai/install.ps1 | iex",
                    ],
                    Duration::from_secs(15 * 60),
                )?;
                if crate::autopilot::find_claude(ctx).is_none() {
                    bail!(
                        "Claude Code install did not finish ({code}): {}",
                        out.chars()
                            .rev()
                            .take(200)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect::<String>()
                    );
                }
                let _ = crate::setup::register_claude_code(ctx);
                Ok("Claude Code installed. Next: sign in.".into())
            }
            #[cfg(not(windows))]
            {
                bail!("install Claude Code from https://claude.ai/download")
            }
        }
        ("claude_login", Action::SignIn) => {
            let Some(c) = crate::autopilot::find_claude(ctx) else {
                bail!("Install Claude Code first")
            };
            // opens your browser; we wait in the background until the login completes
            let mut child = crate::util::hidden_command(&c)
                .args(["auth", "login"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(300) {
                std::thread::sleep(Duration::from_secs(3));
                if claude_signed_in(ctx) {
                    let _ = child.kill();
                    if ctx.store.get("blocked_reason").is_some_and(|b| {
                        b["reason"]
                            .as_str()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains("claude")
                    }) {
                        let _ = ctx.store.set("blocked_reason", serde_json::Value::Null);
                    }
                    return Ok("Signed in to Claude.".into());
                }
                if let Ok(Some(_)) = child.try_wait() {
                    if claude_signed_in(ctx) {
                        return Ok("Signed in to Claude.".into());
                    }
                }
            }
            let _ = child.kill();
            bail!("Sign-in wasn't completed in the browser (try again)")
        }
        _ => bail!("nothing to do for {id}"),
    }
}
