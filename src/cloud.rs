//! Roblox Open Cloud: the private test place, uploads and Luau Execution.
//!
//! The API key is stored only on this PC, encrypted for the signed-in Windows user (DPAPI), in
//! `data\secrets\roblox.key`. It is never written to settings, logs, the vault or any tool output, and no
//! agent tool can read it: agents ask ROX-AI to upload/test, ROX-AI uses the key itself.

use crate::config::Ctx;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const API: &str = "https://apis.roblox.com";

/// What the app shows about the connection (no secret in here).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Connection {
    pub universe_id: u64,
    pub place_id: u64,
    pub place_name: String,
    pub connected_at: String,
}

pub fn connection(ctx: &Ctx) -> Option<Connection> {
    let c: Connection = serde_json::from_value(ctx.store.get("roblox_connection")?).ok()?;
    (c.place_id > 0 && key_file(ctx).is_file()).then_some(c)
}

fn key_file(ctx: &Ctx) -> std::path::PathBuf {
    ctx.home.join("secrets").join("roblox.key")
}

pub(crate) fn api_key(ctx: &Ctx) -> Result<String> {
    let raw = std::fs::read(key_file(ctx)).map_err(|_| anyhow!("Roblox isn't connected yet (Settings → Roblox connection)"))?;
    let plain = unprotect(&raw).context("the saved Roblox key can't be read on this Windows account — connect again")?;
    Ok(String::from_utf8(plain)?.trim().to_string())
}

fn save_key(ctx: &Ctx, key: &str) -> Result<()> {
    let p = key_file(ctx);
    std::fs::create_dir_all(p.parent().unwrap())?;
    crate::util::atomic_write(&p, &protect(key.as_bytes())?)
}

pub fn disconnect(ctx: &Ctx) -> Result<()> {
    let _ = std::fs::remove_file(key_file(ctx));
    ctx.store.set("roblox_connection", Value::Null)?;
    Ok(())
}

// ------------------------------------------------------------------ key encryption

#[cfg(windows)]
fn dpapi(data: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    const UI_FORBIDDEN: u32 = 0x1;
    let ok = unsafe {
        if encrypt {
            CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), UI_FORBIDDEN, &mut out)
        } else {
            CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), UI_FORBIDDEN, &mut out)
        }
    };
    if ok == 0 || out.pbData.is_null() {
        bail!("Windows data protection failed");
    }
    let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
    unsafe { LocalFree(out.pbData as _) };
    Ok(v)
}

#[cfg(windows)]
fn protect(data: &[u8]) -> Result<Vec<u8>> {
    dpapi(data, true)
}
#[cfg(windows)]
fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
    dpapi(data, false)
}

// Other systems (tests / CI on Linux): owner-only file.
#[cfg(not(windows))]
fn protect(data: &[u8]) -> Result<Vec<u8>> {
    Ok(data.to_vec())
}
#[cfg(not(windows))]
fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
    Ok(data.to_vec())
}

// ------------------------------------------------------------------ HTTP

fn agent() -> Result<ureq::Agent> {
    Ok(ureq::AgentBuilder::new()
        .tls_connector(std::sync::Arc::new(native_tls::TlsConnector::new()?))
        .try_proxy_from_env(true)
        .timeout_connect(Duration::from_secs(30))
        .timeout(Duration::from_secs(300))
        .user_agent("ROX-AI")
        .build())
}

/// Turn an HTTP failure into a plain-language message (never contains the key).
fn explain(e: ureq::Error, what: &str) -> anyhow::Error {
    match e {
        ureq::Error::Status(code, resp) => {
            let body: String = resp.into_string().unwrap_or_default().chars().take(300).collect();
            let hint = match code {
                401 => "Roblox didn't accept the API key. Copy it again from Creator Dashboard → Open Cloud → API Keys.",
                403 => "The API key isn't allowed to do this. Check that it has both permissions for the test experience and that \"Accepted IP addresses\" includes 0.0.0.0/0.",
                404 => "Roblox couldn't find that place, or it has no published version yet. Publish it once from Studio.",
                409 => "Roblox is busy with this place right now. Try again in a minute.",
                429 => "Roblox's rate limit was hit (5 test runs per minute). Try again in a minute.",
                _ => "",
            };
            crate::warn!("Roblox: HTTP {code} while trying to {what}: {body}");
            if hint.is_empty() {
                anyhow!("Couldn't {what} (Roblox said HTTP {code}): {body}")
            } else {
                anyhow!("{hint}")
            }
        }
        ureq::Error::Transport(t) => anyhow!("{what}: can't reach Roblox ({t})"),
    }
}

/// The experience (universe) a place belongs to. Public endpoint, no key needed.
pub fn universe_of(place_id: u64) -> Result<u64> {
    let v: Value = agent()?
        .get(&format!("{API}/universes/v1/places/{place_id}/universe"))
        .call()
        .map_err(|e| explain(e, "look up the place"))?
        .into_json()?;
    v["universeId"].as_u64().filter(|u| *u > 0).ok_or_else(|| anyhow!("Roblox doesn't know a place with id {place_id}"))
}

/// A pasted place link, Start Place ID, or Creator Dashboard link → (place id, universe id if known).
pub fn parse_place(input: &str) -> Result<(Option<u64>, Option<u64>)> {
    let s = input.trim();
    let num_after = |marker: &str| -> Option<u64> {
        let i = s.find(marker)? + marker.len();
        s[i..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()
    };
    if let Some(p) = num_after("/games/").or_else(|| num_after("placeId=")).or_else(|| num_after("/places/")) {
        return Ok((Some(p), None));
    }
    if let Some(u) = num_after("/experiences/") {
        return Ok((None, Some(u)));
    }
    if let Ok(p) = s.parse::<u64>() {
        return Ok((Some(p), None));
    }
    bail!("That doesn't look like a place link or Start Place ID. In Creator Dashboard click ⋯ on the experience → Copy Start Place ID, and paste that.")
}

fn root_place(universe_id: u64) -> Result<u64> {
    let v: Value = agent()?
        .get(&format!("https://games.roblox.com/v1/games?universeIds={universe_id}"))
        .call()
        .map_err(|e| explain(e, "look up the experience"))?
        .into_json()?;
    v["data"][0]["rootPlaceId"]
        .as_u64()
        .ok_or_else(|| anyhow!("Roblox doesn't show this experience's start place. In Creator Dashboard click ⋯ on it → Copy Start Place ID, and paste that instead."))
}

/// Upload a place file (.rbxl) as a new version of the test place. Returns the version number.
pub fn upload_place(ctx: &Ctx, rbxl: &[u8]) -> Result<u64> {
    let c = connection(ctx).ok_or_else(|| anyhow!("Roblox isn't connected yet"))?;
    let key = api_key(ctx)?;
    let a = agent()?;
    // Roblox answers 409 "server is busy" while the place is open in Studio, and sometimes just briefly:
    // wait and retry for a few minutes before giving up.
    let waits = [0u64, 15, 30, 60, 90];
    for (i, wait) in waits.iter().enumerate() {
        std::thread::sleep(Duration::from_secs(*wait));
        // "Saved" keeps it a draft version: nothing changes for players, the test place stays private.
        let r = a
            .post(&format!("{API}/universes/v1/{}/places/{}/versions?versionType=Saved", c.universe_id, c.place_id))
            .set("x-api-key", &key)
            .set("Content-Type", "application/octet-stream")
            .send_bytes(rbxl);
        match r {
            Ok(resp) => {
                let v: Value = resp.into_json()?;
                return v["versionNumber"].as_u64().ok_or_else(|| anyhow!("Roblox didn't return a version number: {v}"));
            }
            Err(ureq::Error::Status(code, resp)) if (code == 409 || code >= 500) && i + 1 < waits.len() => {
                let body: String = resp.into_string().unwrap_or_default().chars().take(200).collect();
                crate::warn!("Roblox: upload got HTTP {code} ({body}), retrying");
            }
            Err(ureq::Error::Status(409, _)) if studio_open() => {
                bail!("Roblox won't take the upload while the test place is open in Roblox Studio. Close Studio (your team never needs it open), then press Test pipeline again.")
            }
            Err(e) => return Err(explain(e, "upload the place")),
        }
    }
    unreachable!()
}

/// Roblox Studio is running (an open place blocks uploads to it).
fn studio_open() -> bool {
    cfg!(windows)
        && crate::util::run_capture("tasklist", &["/FI", "IMAGENAME eq RobloxStudioBeta.exe", "/NH"], Duration::from_secs(15))
            .is_ok_and(|(_, out)| out.to_lowercase().contains("robloxstudiobeta"))
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct LuauRun {
    pub state: String,
    /// return values of the script
    pub results: Vec<Value>,
    pub error: Option<String>,
    pub logs: Vec<String>,
    pub seconds: f64,
}

/// Run a Luau script inside the test place on Roblox's servers and wait for it.
pub fn run_luau(ctx: &Ctx, script: &str, version: Option<u64>, timeout: Duration) -> Result<LuauRun> {
    let c = connection(ctx).ok_or_else(|| anyhow!("Roblox isn't connected yet"))?;
    let key = api_key(ctx)?;
    run_luau_with(&key, c.universe_id, c.place_id, script, version, timeout)
}

fn run_luau_with(key: &str, universe: u64, place: u64, script: &str, version: Option<u64>, timeout: Duration) -> Result<LuauRun> {
    let a = agent()?;
    let start = Instant::now();
    let url = match version {
        Some(v) => format!("{API}/cloud/v2/universes/{universe}/places/{place}/versions/{v}/luau-execution-session-tasks"),
        None => format!("{API}/cloud/v2/universes/{universe}/places/{place}/luau-execution-session-tasks"),
    };
    let task: Value = a
        .post(&url)
        .set("x-api-key", key)
        .send_json(json!({"script": script, "timeout": format!("{}s", timeout.as_secs().clamp(1, 300))}))
        .map_err(|e| explain(e, "start a test run"))?
        .into_json()?;
    let path = task["path"].as_str().ok_or_else(|| anyhow!("Roblox didn't start the run: {task}"))?.to_string();
    let deadline = timeout + Duration::from_secs(120);
    let mut t = task;
    loop {
        let state = t["state"].as_str().unwrap_or("").to_string();
        if matches!(state.as_str(), "COMPLETE" | "FAILED" | "CANCELLED") {
            break;
        }
        if start.elapsed() > deadline {
            bail!("the test run on Roblox's servers didn't finish in time");
        }
        std::thread::sleep(Duration::from_secs(2));
        t = a
            .get(&format!("{API}/cloud/v2/{path}"))
            .set("x-api-key", key)
            .call()
            .map_err(|e| explain(e, "check the test run"))?
            .into_json()?;
    }
    let mut logs = vec![];
    if let Ok(r) = a.get(&format!("{API}/cloud/v2/{path}/logs?maxPageSize=10000")).set("x-api-key", key).call() {
        if let Ok(v) = r.into_json::<Value>() {
            for l in v["luauExecutionSessionTaskLogs"].as_array().into_iter().flatten() {
                for m in l["messages"].as_array().into_iter().flatten() {
                    if let Some(s) = m.as_str() {
                        logs.push(s.to_string());
                    }
                }
            }
        }
    }
    Ok(LuauRun {
        state: t["state"].as_str().unwrap_or("").to_string(),
        results: t["output"]["results"].as_array().cloned().unwrap_or_default(),
        error: t["error"].is_object().then(|| format!("{}: {}", t["error"]["code"].as_str().unwrap_or("ERROR"), t["error"]["message"].as_str().unwrap_or(""))),
        logs,
        seconds: start.elapsed().as_secs_f64(),
    })
}

/// Check a key + test place, and save them if Roblox accepts them.
pub fn connect(ctx: &Ctx, key: &str, place_input: &str) -> Result<Connection> {
    let key = key.trim();
    if key.len() < 20 || key.contains(char::is_whitespace) {
        bail!("That doesn't look like an Open Cloud API key. Use the Copy button after \"Save & Generate key\".");
    }
    let (place, universe) = parse_place(place_input)?;
    let (place, universe) = match (place, universe) {
        (Some(p), _) => (p, universe_of(p)?),
        (None, Some(u)) => (root_place(u)?, u),
        _ => unreachable!(),
    };
    let r = run_luau_with(key, universe, place, "return { placeId = game.PlaceId, name = game.Name }", None, Duration::from_secs(60))?;
    if r.state != "COMPLETE" {
        bail!("Roblox started the test place but the check failed: {}", r.error.unwrap_or(r.state));
    }
    let got = r.results.first().cloned().unwrap_or_default();
    if got["placeId"].as_u64().is_some_and(|p| p != place) {
        bail!("Roblox ran a different place than expected");
    }
    save_key(ctx, key)?;
    let c = Connection {
        universe_id: universe,
        place_id: place,
        place_name: got["name"].as_str().unwrap_or("Test place").to_string(),
        connected_at: crate::util::now_iso(),
    };
    ctx.store.set("roblox_connection", serde_json::to_value(&c)?)?;
    crate::info!("Roblox connected: test place {place} (experience {universe})");
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_links() {
        assert_eq!(parse_place("https://www.roblox.com/games/1234567/My-Test").unwrap(), (Some(1234567), None));
        assert_eq!(parse_place(" 98765 ").unwrap(), (Some(98765), None));
        assert_eq!(parse_place("https://create.roblox.com/dashboard/creations/experiences/555/overview").unwrap(), (None, Some(555)));
        assert_eq!(parse_place("https://create.roblox.com/dashboard/creations/experiences/555/places/777/configure").unwrap(), (Some(777), None));
        assert!(parse_place("hello").is_err());
    }

    #[test]
    fn key_round_trip_stays_encrypted() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::at(dir.path());
        save_key(&ctx, "secret-key-value-1234567890").unwrap();
        assert_eq!(api_key(&ctx).unwrap(), "secret-key-value-1234567890");
        if cfg!(windows) {
            let raw = std::fs::read(key_file(&ctx)).unwrap();
            assert!(!String::from_utf8_lossy(&raw).contains("secret-key-value"));
        }
        assert!(connection(&ctx).is_none(), "no place yet");
        disconnect(&ctx).unwrap();
        assert!(api_key(&ctx).is_err());
    }
}
