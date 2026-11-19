//! The apps ROX-AI works with: detects what you already have and offers one-click install / sign-in
//! for what's missing. Everything runs hidden (no console windows); installs use winget when present,
//! otherwise the official download is opened.

use crate::config::Ctx;
use crate::util::run_capture;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
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
    /// needed for the chosen mode (optional items never block "ready")
    pub required: bool,
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
    let active = ctx.is_active();
    let playtest = ctx.settings.playtesting_enabled;
    let blocked = ctx
        .store
        .get("blocked_reason")
        .map(|b| b["reason"].as_str().unwrap_or("").to_lowercase())
        .unwrap_or_default();
    let obs = crate::setup::obsidian_exe().is_some();
    let mut v = vec![];
    if active {
        let cc = crate::autopilot::find_claude(ctx).is_some();
        let cc_login = cc && claude_signed_in(ctx) && !blocked.contains("claude");
        v.push(Dep {
            id: "claude_code",
            name: "Claude Code",
            why: "Lets Claude learn the games for you",
            ok: cc,
            required: true,
            action: (!cc).then_some((Action::Install, "Install")),
        });
        v.push(Dep {
            id: "claude_login",
            name: "Claude account",
            why: "Uses your Claude subscription",
            ok: cc_login,
            required: true,
            action: (cc && !cc_login).then_some((Action::SignIn, "Sign in")),
        });
        let yt = crate::youtube::ytdlp_path(&ctx.tools_dir()).is_some();
        v.push(Dep {
            id: "video_reader",
            name: "Video reader",
            why: "Recommended — reads YouTube captions more reliably (kept inside ROX-AI's folder)",
            ok: yt,
            required: false,
            action: (!yt).then_some((Action::Install, "Install")),
        });
    }
    if playtest {
        let rb = crate::win::roblox::is_installed();
        let rb_login =
            rb && roblox_signed_in() && !(blocked.contains("login") && !blocked.contains("claude"));
        v.push(Dep {
            id: "roblox",
            name: "Roblox",
            why: "The AI plays the games in it",
            ok: rb,
            required: true,
            action: (!rb).then_some((Action::Install, "Install")),
        });
        v.push(Dep {
            id: "roblox_login",
            name: "Roblox alt account",
            why: "Sign in once — use an ALT account, never your main",
            ok: rb_login,
            required: true,
            action: (rb && !rb_login).then_some((Action::SignIn, "Sign in")),
        });
    }
    let bt = crate::pipeline::tools_ready(ctx);
    v.push(Dep {
        id: "build_tools",
        name: "Game building tools",
        why: "Rojo and Lune build and check your games without opening Studio (kept inside ROX-AI's folder)",
        ok: bt,
        required: false,
        action: (!bt).then_some((Action::Install, "Install")),
    });
    v.push(Dep {
        id: "obsidian",
        name: "Obsidian",
        why: "Optional — your vault also opens inside ROX-AI",
        ok: obs,
        required: false,
        action: (!obs).then_some((Action::Install, "Install")),
    });
    let cd = claude_desktop_installed();
    v.push(Dep {
        id: "claude_desktop",
        name: "Claude Desktop",
        why: "Optional — chat with ROX-AI yourself",
        ok: cd,
        required: false,
        action: (!cd).then_some((Action::Install, "Install")),
    });
    v
}

/// Progress callback: (what's happening, optional 0..1 progress).
pub type Report<'a> = &'a dyn Fn(&str, Option<f32>);

fn have_winget() -> bool {
    cfg!(windows)
        && run_capture("winget", &["--version"], Duration::from_secs(20)).is_ok_and(|(c, _)| c == 0)
}

fn winget(id: &str, name: &str, report: Report) -> Result<()> {
    report(
        &format!("Installing {name} with Windows Package Manager…"),
        None,
    );
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
            "--disable-interactivity",
        ],
        Duration::from_secs(20 * 60),
    )?;
    if code != 0 && !out.to_lowercase().contains("already installed") {
        bail!("winget exit {code}");
    }
    Ok(())
}

pub(crate) fn agent() -> Result<ureq::Agent> {
    Ok(ureq::AgentBuilder::new()
        .tls_connector(std::sync::Arc::new(native_tls::TlsConnector::new()?))
        .try_proxy_from_env(true)
        .timeout_connect(Duration::from_secs(30))
        .user_agent("ROX-AI-setup")
        .build())
}

/// Download with a progress bar into the temp folder.
pub(crate) fn download(url: &str, file: &str, name: &str, report: Report) -> Result<PathBuf> {
    use std::io::{Read, Write};
    report(&format!("Downloading {name}…"), Some(0.0));
    let resp = agent()?
        .get(url)
        .call()
        .map_err(|e| anyhow::anyhow!("download failed: {e}"))?;
    let total: Option<u64> = resp.header("content-length").and_then(|v| v.parse().ok());
    let path = std::env::temp_dir().join(file);
    let mut out = std::fs::File::create(&path)?;
    let mut rd = resp.into_reader();
    let mut buf = vec![0u8; 256 * 1024];
    let mut got: u64 = 0;
    loop {
        let n = rd.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        got += n as u64;
        let mb = got as f64 / 1_048_576.0;
        match total {
            Some(t) if t > 0 => report(
                &format!(
                    "Downloading {name}… {mb:.0} / {:.0} MB",
                    t as f64 / 1_048_576.0
                ),
                Some(got as f32 / t as f32),
            ),
            _ => report(&format!("Downloading {name}… {mb:.0} MB"), None),
        }
    }
    Ok(path)
}

fn run_installer(path: &Path, args: &[&str], name: &str, report: Report) -> Result<()> {
    report(&format!("Installing {name}…"), None);
    let st = std::process::Command::new(path)
        .args(args)
        .status()
        .with_context(|| format!("couldn't start the {name} installer"))?;
    if !st.success() {
        bail!("{name} installer exited with {st}");
    }
    Ok(())
}

fn obsidian_download_url() -> Result<String> {
    let v: serde_json::Value = agent()?
        .get("https://api.github.com/repos/obsidianmd/obsidian-releases/releases/latest")
        .call()?
        .into_json()?;
    v["assets"]
        .as_array()
        .and_then(|a| {
            a.iter().find(|x| {
                x["name"].as_str().is_some_and(|n| {
                    n.starts_with("Obsidian-") && n.ends_with(".exe") && !n.contains("arm")
                })
            })
        })
        .and_then(|x| x["browser_download_url"].as_str())
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("no Obsidian installer in the latest release"))
}

fn wait_until(check: impl Fn() -> bool, secs: u64) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    check()
}

/// Perform an install / sign-in. Blocking (run it on a background thread). Returns a short message.
pub fn act(ctx: &Ctx, id: &str, action: Action, report: Report) -> Result<String> {
    if !cfg!(windows) && action == Action::Install && id != "video_reader" && id != "build_tools" {
        bail!("installing apps is only supported on Windows");
    }
    let wg = action == Action::Install && have_winget();
    match (id, action) {
        ("obsidian", Action::Install) => {
            if !(wg
                && winget("Obsidian.Obsidian", "Obsidian", report).is_ok()
                && crate::setup::obsidian_exe().is_some())
            {
                let url = obsidian_download_url()?;
                let f = download(&url, "Obsidian-setup.exe", "Obsidian", report)?;
                run_installer(&f, &["/S"], "Obsidian", report)?;
            }
            if !wait_until(|| crate::setup::obsidian_exe().is_some(), 60) {
                bail!("Obsidian didn't finish installing");
            }
            let _ = crate::setup::register_obsidian_vault(ctx);
            Ok("Obsidian is installed.".into())
        }
        ("roblox", Action::Install) => {
            if !(wg
                && winget("Roblox.Roblox", "Roblox", report).is_ok()
                && crate::win::roblox::is_installed())
            {
                let f = download(
                    "https://www.roblox.com/download/client?os=win",
                    "RobloxPlayerInstaller.exe",
                    "Roblox",
                    report,
                )?;
                report("Installing Roblox (its own window may appear)…", None);
                let _ = std::process::Command::new(&f).spawn();
            }
            if !wait_until(crate::win::roblox::is_installed, 600) {
                bail!("Roblox didn't finish installing");
            }
            crate::win::roblox::kill_all(); // the installer opens Roblox afterwards; close it
            Ok("Roblox is installed.".into())
        }
        ("claude_desktop", Action::Install) => {
            let ok = (wg
                && winget("Anthropic.Claude", "Claude Desktop", report).is_ok()
                && claude_desktop_installed())
                || download(
                    "https://claude.ai/api/desktop/win32/x64/exe/latest/redirect",
                    "Claude-Setup.exe",
                    "Claude Desktop",
                    report,
                )
                .and_then(|f| run_installer(&f, &[], "Claude Desktop", report))
                .is_ok_and(|_| wait_until(claude_desktop_installed, 120));
            if !ok {
                crate::util::open_path("https://claude.ai/download");
                bail!("couldn't install automatically — the download page is open");
            }
            let _ = crate::setup::register_claude_desktop(ctx);
            Ok("Claude Desktop is installed — open it once and sign in.".into())
        }
        ("build_tools", Action::Install) => {
            crate::pipeline::install_tools(ctx, report)?;
            Ok("Game building tools installed (inside ROX-AI's folder).".into())
        }
        ("video_reader", Action::Install) => {
            crate::youtube::install_ytdlp(&ctx.tools_dir(), report)?;
            Ok("Video reader installed (inside ROX-AI's folder).".into())
        }
        ("claude_code", Action::Install) => {
            report("Installing Claude Code…", None);
            let (code, out) = run_capture(
                "powershell",
                &[
                    "-NoProfile",
                    "-NonInteractive",
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
                let tail: String = out
                    .chars()
                    .rev()
                    .take(200)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                bail!("Claude Code install did not finish ({code}): {tail}");
            }
            let _ = crate::setup::register_claude_code(ctx);
            Ok("Claude Code is installed. Next: sign in.".into())
        }
        ("roblox_login", Action::SignIn) => {
            report("Waiting for you to sign in to Roblox…", None);
            // sign in on the website, then any game's Play button hands the login to the Roblox app
            crate::util::open_path("https://www.roblox.com/login");
            if wait_until(roblox_signed_in, 600) {
                crate::win::roblox::kill_all();
                Ok("Signed in to Roblox.".into())
            } else {
                Ok("After signing in on the website, press Play on any game once so the Roblox app remembers you.".into())
            }
        }
        ("claude_login", Action::SignIn) => {
            let Some(c) = crate::autopilot::find_claude(ctx) else {
                bail!("Install Claude Code first")
            };
            report("Finish signing in to Claude in your browser…", None);
            let mut child = crate::util::hidden_command(&c)
                .args(["auth", "login"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            let ok = wait_until(|| claude_signed_in(ctx), 300);
            let _ = child.kill();
            if !ok {
                bail!("sign-in wasn't completed in the browser — try again");
            }
            if ctx.store.get("blocked_reason").is_some_and(|b| {
                b["reason"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains("claude")
            }) {
                let _ = ctx.store.set("blocked_reason", serde_json::Value::Null);
            }
            Ok("Signed in to Claude.".into())
        }
        _ => bail!("nothing to do for {id}"),
    }
}
