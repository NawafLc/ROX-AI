//! Self-update from GitHub Releases.
//!
//! Update button / `roxai update`: asks GitHub for the latest release, downloads the ZIP, swaps the exes
//! inside the portable folder and restarts. Settings, history and vault are kept.

use crate::config::Ctx;
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::io::Read;
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

/// Download the release and swap the program files in place (portable, no installer, no console).
/// Windows lets a running exe be *renamed*, so: current → *.old.exe, new → current name, relaunch.
/// The .old files are deleted on the next start. Settings, history and vault are never touched.
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
        bail!("Updates install the Windows build; on this OS rebuild from source (cargo build --release).");
    }
    if ctx.session_file().exists() {
        bail!("A playtest is running right now — update after it finishes (or press Stop first).");
    }
    crate::info!("updating v{} → v{}", st.current, rel.version);
    let resp = agent()?
        .get(&rel.url)
        .call()
        .map_err(|e| anyhow!("download failed: {e}"))?;
    let mut bytes = vec![];
    resp.into_reader()
        .take(200_000_000)
        .read_to_end(&mut bytes)?;
    if bytes.len() < 500_000 || &bytes[..2] != b"PK" {
        bail!(
            "downloaded file is not a valid release ({} bytes)",
            bytes.len()
        );
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("bad release ZIP")?;
    let dir = crate::config::app_folder();
    let mut replaced = vec![];
    // new layout: ROX-AI.exe + roxai.exe (older releases shipped roxaiw.exe instead of ROX-AI.exe)
    for (want, alt) in [("ROX-AI.exe", Some("roxaiw.exe")), ("roxai.exe", None)] {
        let idx = (0..zip.len())
            .find(|&i| {
                zip.by_index(i)
                    .ok()
                    .and_then(|f| f.enclosed_name())
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .is_some_and(|n| n.eq_ignore_ascii_case(want))
            })
            .or_else(|| {
                alt.and_then(|a| {
                    (0..zip.len()).find(|&i| {
                        zip.by_index(i)
                            .ok()
                            .and_then(|f| f.enclosed_name())
                            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                            .is_some_and(|n| n.eq_ignore_ascii_case(a))
                    })
                })
            });
        let Some(i) = idx else { continue };
        let mut f = zip.by_index(i)?;
        let mut data = vec![];
        f.read_to_end(&mut data)?;
        let target = dir.join(want);
        let tmp = dir.join(format!("{want}.new"));
        std::fs::write(&tmp, &data)
            .with_context(|| format!("cannot write into {}", dir.display()))?;
        if target.exists() {
            let old = dir.join(format!(
                "{}.{}.old.exe",
                want.trim_end_matches(".exe"),
                crate::util::rand_hex(6)
            ));
            std::fs::rename(&target, &old).with_context(|| format!("cannot replace {want}"))?;
        }
        std::fs::rename(&tmp, &target)?;
        replaced.push(want);
    }
    if !replaced.contains(&"ROX-AI.exe") {
        bail!("the release didn't contain ROX-AI.exe");
    }
    let _ = ctx.store.set("last_notice", serde_json::json!({"title": "Updated", "text": format!("v{} → v{}", st.current, rel.version), "at": crate::util::now_iso()}));
    // relaunch the new app; the caller (old app) exits
    std::process::Command::new(dir.join("ROX-AI.exe"))
        .current_dir(&dir)
        .spawn()
        .context("could not start the new version")?;
    Ok(format!("Updated to v{} — restarting…", rel.version))
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
