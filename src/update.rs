//! Self-update from GitHub Releases.
//!
//! `roxai update` (or the panel's Update button):
//!   1. asks GitHub for the latest release and compares versions
//!   2. downloads `ROX-AI-windows-x64.zip` and unpacks it to a temp folder
//!   3. starts that release's installer in update mode in its own window and exits —
//!      the installer replaces the program files (the running app can't overwrite itself),
//!      keeps your settings, vault and history, and reopens the panel when done.

use crate::config::Ctx;
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub const REPO: &str = "NawafLc/ROX-AI";
pub const ASSET: &str = "ROX-AI-windows-x64.zip";

#[derive(Debug, Clone, Serialize)]
pub struct Release {
    pub version: String,
    pub tag: String,
    pub url: String,
    pub published: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub current: String,
    pub latest: Option<Release>,
    pub update_available: bool,
}

/// Version of this build ("2.0.<CI build number>" for release builds).
pub fn current_version() -> &'static str {
    crate::VERSION
}

fn parse(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split(['.', '-', '+'])
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

/// true if `latest` is newer than `current` (numeric, component-wise).
pub fn is_newer(latest: &str, current: &str) -> bool {
    let (a, b) = (parse(latest), parse(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

fn agent() -> Result<ureq::Agent> {
    Ok(ureq::AgentBuilder::new()
        .tls_connector(Arc::new(native_tls::TlsConnector::new()?))
        .try_proxy_from_env(true)
        .timeout(Duration::from_secs(120))
        .user_agent("ROX-AI-updater")
        .build())
}

pub fn latest_release() -> Result<Release> {
    let r = agent()?
        .get(&format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .set("Accept", "application/vnd.github+json")
        .call();
    let v: serde_json::Value = match r {
        Ok(resp) => resp.into_json()?,
        Err(ureq::Error::Status(404, _)) => bail!(
            "No release found on GitHub (is the repository private, or no release published yet?)"
        ),
        Err(ureq::Error::Status(403, _)) => {
            bail!("GitHub rate limit reached — try again in an hour")
        }
        Err(e) => bail!("Could not reach GitHub: {e}"),
    };
    let tag = v["tag_name"].as_str().unwrap_or("").to_string();
    let url = v["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"] == ASSET))
        .and_then(|x| x["browser_download_url"].as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("https://github.com/{REPO}/releases/download/{tag}/{ASSET}"));
    Ok(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        tag,
        url,
        published: v["published_at"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(10)
            .collect(),
        notes: v["body"].as_str().unwrap_or("").chars().take(500).collect(),
    })
}

pub fn check() -> Result<Status> {
    let latest = latest_release()?;
    let cur = current_version().to_string();
    Ok(Status {
        update_available: is_newer(&latest.version, &cur),
        current: cur,
        latest: Some(latest),
    })
}

/// Download + unpack the release, start its installer in update mode. Returns a message.
pub fn run(ctx: &Ctx, force: bool) -> Result<String> {
    let st = check()?;
    let rel = st.latest.clone().unwrap();
    if !st.update_available && !force {
        return Ok(format!(
            "✅ You already have the latest version (v{}).",
            st.current
        ));
    }
    if !cfg!(windows) {
        bail!("Self-update installs the Windows build; on this OS rebuild from source (cargo build --release).");
    }
    if ctx.session_file().exists() {
        bail!("A playtest is running right now — update after it finishes (or press STOP first).");
    }
    crate::info!("updating v{} → v{}", st.current, rel.version);

    let dir = std::env::temp_dir().join(format!("roxai-update-{}", rel.version));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let zip = dir.join(ASSET);
    let resp = agent()?
        .get(&rel.url)
        .call()
        .map_err(|e| anyhow!("download failed: {e}"))?;
    let mut bytes = vec![];
    resp.into_reader()
        .take(200_000_000)
        .read_to_end(&mut bytes)?;
    if bytes.len() < 1_000_000 || &bytes[..2] != b"PK" {
        bail!(
            "downloaded file is not a valid release ZIP ({} bytes)",
            bytes.len()
        );
    }
    std::fs::write(&zip, &bytes)?;

    let out = dir.join("unpacked");
    let ps = format!(
        "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
        zip.display(),
        out.display()
    );
    let (code, msg) = crate::util::run_capture(
        "powershell",
        &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps],
        Duration::from_secs(180),
    )?;
    if code != 0 {
        bail!("could not unpack the update: {msg}");
    }
    let installer = find(&out, "install.ps1").context("install.ps1 missing from the release")?;

    // Launch the new installer in its own visible window, fully detached from us: it stops every running
    // ROX-AI process (including this one) before replacing the files, then reopens the panel.
    let mut c = std::process::Command::new("powershell");
    c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&installer)
        .arg("-Update");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0000_0010 | 0x0000_0200); // CREATE_NEW_CONSOLE | CREATE_NEW_PROCESS_GROUP
    }
    c.spawn().context("could not start the installer")?;
    let _ = ctx.store.set("last_notice", serde_json::json!({"title": "Updating ROX-AI", "text": format!("v{} → v{}", st.current, rel.version), "at": crate::util::now_iso()}));
    Ok(format!(
        "⬆️ Updating v{} → v{} — the installer window will finish and reopen ROX-AI.",
        st.current, rel.version
    ))
}

fn find(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find(&p, name) {
                return Some(f);
            }
        } else if p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name)) {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions() {
        assert!(is_newer("2.0.12", "2.0.4"));
        assert!(is_newer("v2.1.0", "2.0.99"));
        assert!(!is_newer("2.0.4", "2.0.4"));
        assert!(!is_newer("2.0.3", "2.0.4"));
        assert!(is_newer("2.0.5", "2.0.0"));
    }
}
