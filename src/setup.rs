//! One-shot system integration (run by the installer, safe to re-run any time):
//!   * create the vault (+ Dataview) and register it in Obsidian
//!   * register the MCP server in Claude Desktop (all install variants) and Claude Code
//!   * create Windows scheduled tasks (daily scan, hourly-retrying playtest check)
//!   * desktop + Start-menu shortcuts with an app icon

use crate::api::RobloxApi;
use crate::autopilot::{find_claude, mcp_entry};
use crate::config::{home_dir, Ctx};
use crate::util::{atomic_write, hidden_command, run_capture};
use crate::vault::Vault;
use anyhow::Result;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

pub const TASK_FOLDER: &str = "ROX-AI";

fn env_path(k: &str) -> Option<PathBuf> {
    std::env::var_os(k).map(PathBuf::from)
}

pub fn claude_desktop_config_paths() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(a) = env_path("APPDATA") {
        v.push(a.join("Claude").join("claude_desktop_config.json"));
    }
    if let Some(l) = env_path("LOCALAPPDATA") {
        if let Ok(rd) = std::fs::read_dir(l.join("Packages")) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with("Claude_") {
                    v.push(
                        e.path()
                            .join("LocalCache")
                            .join("Roaming")
                            .join("Claude")
                            .join("claude_desktop_config.json"),
                    );
                }
            }
        }
    }
    if cfg!(target_os = "macos") {
        v.push(home_dir().join("Library/Application Support/Claude/claude_desktop_config.json"));
    }
    v
}

/// Merge our server into every Claude Desktop config (keeps the user's other servers).
pub fn register_claude_desktop(ctx: &Ctx) -> Vec<String> {
    let paths = claude_desktop_config_paths();
    let existing: Vec<PathBuf> = paths
        .iter()
        .filter(|p| p.parent().is_some_and(|d| d.exists()))
        .cloned()
        .collect();
    let targets = if existing.is_empty() {
        paths.into_iter().take(1).collect()
    } else {
        existing
    };
    let mut done = vec![];
    for p in targets {
        let mut cfg: Value = std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| {
                if p.exists() {
                    let _ = std::fs::copy(&p, p.with_extension("backup.json"));
                }
                json!({})
            });
        if !cfg.is_object() {
            cfg = json!({});
        }
        let servers = cfg
            .as_object_mut()
            .unwrap()
            .entry("mcpServers")
            .or_insert(json!({}));
        if !servers.is_object() {
            *servers = json!({});
        }
        servers
            .as_object_mut()
            .unwrap()
            .insert("roxai".into(), mcp_entry(ctx));
        if atomic_write(&p, serde_json::to_string_pretty(&cfg).unwrap().as_bytes()).is_ok() {
            done.push(p.to_string_lossy().into_owned());
        }
    }
    done
}

pub fn register_claude_code(ctx: &Ctx) -> String {
    let Some(c) = find_claude(ctx) else {
        return "Claude Code CLI not found (skipped)".into();
    };
    let c = c.to_string_lossy().into_owned();
    let _ = run_capture(
        &c,
        &["mcp", "remove", "roxai", "-s", "user"],
        Duration::from_secs(60),
    );
    let exe = crate::util::console_exe().to_string_lossy().into_owned();
    let env = format!("ROXAI_HOME={}", ctx.home.to_string_lossy());
    match run_capture(
        &c,
        &[
            "mcp", "add", "-s", "user", "-e", &env, "roxai", "--", &exe, "mcp",
        ],
        Duration::from_secs(60),
    ) {
        Ok((0, _)) => "registered".into(),
        Ok((_, o)) => format!("failed: {}", o.chars().take(300).collect::<String>()),
        Err(e) => format!("failed: {e}"),
    }
}

// --------------------------------------------------------- scheduler ---

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn current_user() -> String {
    let d = std::env::var("USERDOMAIN").unwrap_or_default();
    let u = std::env::var("USERNAME").unwrap_or_default();
    if d.is_empty() {
        u
    } else {
        format!("{d}\\{u}")
    }
}

pub fn task_xml(
    ctx: &Ctx,
    desc: &str,
    time_hhmm: &str,
    args: &str,
    limit: &str,
    logon_delay: &str,
    repetition: &str,
) -> String {
    let start = format!(
        "{}T{}:00",
        chrono::Local::now().format("%Y-%m-%d"),
        time_hhmm
    );
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>{desc}</Description><Author>ROX-AI</Author></RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>{start}</StartBoundary>
      <Enabled>true</Enabled>
      {repetition}
      <ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay>
    </CalendarTrigger>
    <LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId><Delay>{logon_delay}</Delay></LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>true</RunOnlyIfNetworkAvailable>
    <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>{limit}</ExecutionTimeLimit>
    <Priority>7</Priority>
    <RestartOnFailure><Interval>PT15M</Interval><Count>3</Count></RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec><Command>{cmd}</Command><Arguments>{args}</Arguments><WorkingDirectory>{cwd}</WorkingDirectory></Exec>
  </Actions>
</Task>
"#,
        desc = xml_escape(desc),
        user = xml_escape(&current_user()),
        cmd = xml_escape(&crate::util::bg_exe().to_string_lossy()),
        args = xml_escape(args),
        cwd = xml_escape(&ctx.home.to_string_lossy()),
    )
}

pub fn create_task(ctx: &Ctx, name: &str, xml: &str) -> String {
    let tmp = std::env::temp_dir().join(format!("roxai-task-{}.xml", crate::util::rand_hex(8)));
    // Task Scheduler wants UTF-16 LE with BOM
    let mut bytes = vec![0xFF, 0xFE];
    for u in xml.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    if let Err(e) = std::fs::write(&tmp, bytes) {
        return format!("failed: {e}");
    }
    let r = run_capture(
        "schtasks",
        &[
            "/Create",
            "/TN",
            &format!("{TASK_FOLDER}\\{name}"),
            "/XML",
            &tmp.to_string_lossy(),
            "/F",
        ],
        Duration::from_secs(60),
    );
    let _ = std::fs::remove_file(&tmp);
    let _ = ctx;
    match r {
        Ok((0, _)) => "ok".into(),
        Ok((_, o)) => format!("failed: {}", o.trim().chars().take(300).collect::<String>()),
        Err(e) => format!("failed: {e}"),
    }
}

pub fn create_tasks(ctx: &Ctx) -> Value {
    if !cfg!(windows) {
        return json!({"skipped": "not Windows"});
    }
    let s = &ctx.settings;
    let scan = task_xml(
        ctx,
        "ROX-AI: refresh Roblox charts, trends and the Obsidian vault (no Claude needed).",
        &s.scan_time,
        "scan --if-needed",
        "PT1H",
        "PT5M",
        "",
    );
    let play = task_xml(ctx, "ROX-AI: checks hourly if the weekly AI playtest run is due and the PC is idle, then runs it.", &s.playtest_check_time, "autopilot --scheduled", "PT11H", "PT20M",
        "<Repetition><Interval>PT1H</Interval><Duration>PT12H</Duration><StopAtDurationEnd>false</StopAtDurationEnd></Repetition>");
    json!({"Daily Chart Scan": create_task(ctx, "Daily Chart Scan", &scan), "Weekly Playtests": create_task(ctx, "Weekly Playtests", &play)})
}

pub fn remove_tasks() {
    for n in ["Daily Chart Scan", "Weekly Playtests"] {
        let _ = run_capture(
            "schtasks",
            &["/Delete", "/TN", &format!("{TASK_FOLDER}\\{n}"), "/F"],
            Duration::from_secs(30),
        );
    }
}

pub fn task_status(name: &str) -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    let (code, out) = run_capture(
        "schtasks",
        &[
            "/Query",
            "/TN",
            &format!("{TASK_FOLDER}\\{name}"),
            "/FO",
            "LIST",
        ],
        Duration::from_secs(30),
    )
    .ok()?;
    if code != 0 {
        return None;
    }
    Some(
        out.lines()
            .find(|l| l.trim().to_lowercase().starts_with("status:"))
            .map(|l| l.split_once(':').unwrap().1.trim().to_string())
            .unwrap_or("exists".into()),
    )
}

pub fn run_task_now(name: &str) -> bool {
    run_capture(
        "schtasks",
        &["/Run", "/TN", &format!("{TASK_FOLDER}\\{name}")],
        Duration::from_secs(30),
    )
    .is_ok_and(|(c, _)| c == 0)
}

// --------------------------------------------------------- shortcuts ---

/// Generate the app icon (RX on a dark rounded square).
pub fn make_icon(ctx: &Ctx) -> Result<PathBuf> {
    let p = ctx.home.join("roxai.ico");
    if p.exists() {
        return Ok(p);
    }
    let mut frames = vec![];
    for size in [16u32, 32, 48, 64, 128, 256] {
        let img = icon_image(size);
        frames.push(image::codecs::ico::IcoFrame::as_png(
            img.as_raw(),
            size,
            size,
            image::ExtendedColorType::Rgba8,
        )?);
    }
    let f = std::fs::File::create(&p)?;
    image::codecs::ico::IcoEncoder::new(f).encode_images(&frames)?;
    Ok(p)
}

pub fn icon_image(size: u32) -> image::RgbaImage {
    let mut img = image::RgbaImage::new(size, size);
    let s = size as f32;
    let r = s * 0.22;
    let border = (s * 0.04).max(1.0);
    // 5x7 glyphs for R and X
    const R: [u8; 7] = [
        0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
    ];
    const X: [u8; 7] = [
        0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
    ];
    let cell = s * 0.06;
    let gx0 = s / 2.0 - cell * 5.5;
    let gy0 = s / 2.0 - cell * 3.5;
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let corner = (fx < r || fx > s - r) && (fy < r || fy > s - r);
            // distance from the outer edge of the rounded square (negative = outside)
            let inner = if corner {
                let (cx, cy) = (fx.clamp(r, s - r), fy.clamp(r, s - r));
                r - ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt()
            } else {
                fx.min(fy).min(s - fx).min(s - fy)
            };
            if inner < 0.0 {
                continue;
            }
            let mut c = if inner < border {
                [0, 220, 170, 255]
            } else {
                [20, 24, 38, 255]
            };
            let gx = ((fx - gx0) / cell).floor();
            let gy = ((fy - gy0) / cell).floor();
            if (0.0..7.0).contains(&gy) {
                let row = gy as usize;
                if (0.0..5.0).contains(&gx) && R[row] & (0b10000 >> gx as u32) != 0 {
                    c = [0, 230, 180, 255];
                }
                if (6.0..11.0).contains(&gx) && X[row] & (0b10000 >> (gx as u32 - 6)) != 0 {
                    c = [0, 230, 180, 255];
                }
            }
            img.put_pixel(x, y, image::Rgba(c));
        }
    }
    img
}

fn shortcut(
    lnk: &std::path::Path,
    target: &str,
    args: &str,
    icon: &str,
    cwd: &str,
    desc: &str,
) -> bool {
    let q = |s: &str| s.replace('\'', "''");
    let ps = format!("$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.Arguments='{}';$s.WorkingDirectory='{}';$s.IconLocation='{}';$s.Description='{}';$s.Save()",
        q(&lnk.to_string_lossy()), q(target), q(args), q(cwd), q(icon), q(desc));
    run_capture(
        "powershell",
        &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps],
        Duration::from_secs(60),
    )
    .is_ok_and(|(c, _)| c == 0)
}

pub fn create_shortcuts(ctx: &Ctx) -> Vec<String> {
    if !cfg!(windows) {
        return vec![];
    }
    let icon = make_icon(ctx)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let desktop = run_capture(
        "powershell",
        &[
            "-NoProfile",
            "-Command",
            "[Environment]::GetFolderPath('Desktop')",
        ],
        Duration::from_secs(30),
    )
    .ok()
    .map(|(_, o)| PathBuf::from(o.trim()))
    .filter(|p| p.exists())
    .unwrap_or_else(|| home_dir().join("Desktop"));
    let start = env_path("APPDATA")
        .unwrap_or_default()
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs");
    let target = crate::util::bg_exe().to_string_lossy().into_owned();
    let mut out = vec![];
    for folder in [desktop, start] {
        if folder.exists() {
            let lnk = folder.join("ROX-AI.lnk");
            if shortcut(
                &lnk,
                &target,
                "panel",
                &icon,
                &ctx.home.to_string_lossy(),
                "ROX-AI control panel",
            ) {
                out.push(lnk.to_string_lossy().into_owned());
            }
        }
    }
    out
}

// ---------------------------------------------------------- Obsidian ---

pub fn obsidian_exe() -> Option<PathBuf> {
    [
        env_path("LOCALAPPDATA").map(|l| l.join("Programs").join("Obsidian").join("Obsidian.exe")),
        env_path("ProgramFiles").map(|p| p.join("Obsidian").join("Obsidian.exe")),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.exists())
}

pub fn register_obsidian_vault(ctx: &Ctx) -> String {
    let Some(appdata) = env_path("APPDATA") else {
        return "skipped".into();
    };
    if crate::win::proc::list()
        .iter()
        .any(|p| p.name.eq_ignore_ascii_case("obsidian.exe"))
    {
        return "Obsidian is running — the vault will be opened via link instead".into();
    }
    let cfg = appdata.join("obsidian").join("obsidian.json");
    let vault = ctx.vault().to_string_lossy().into_owned();
    let mut data: Value = std::fs::read_to_string(&cfg)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(json!({}));
    if !data.is_object() {
        data = json!({});
    }
    let vaults = data
        .as_object_mut()
        .unwrap()
        .entry("vaults")
        .or_insert(json!({}));
    let Some(m) = vaults.as_object_mut() else {
        return "failed: unexpected obsidian.json".into();
    };
    if m.values().any(|v| {
        v["path"]
            .as_str()
            .is_some_and(|p| p.eq_ignore_ascii_case(&vault))
    }) {
        return "already registered".into();
    }
    for v in m.values_mut() {
        if let Some(o) = v.as_object_mut() {
            o.remove("open");
        }
    }
    m.insert(
        crate::util::rand_hex(16),
        json!({"path": vault, "ts": chrono::Utc::now().timestamp_millis(), "open": true}),
    );
    match atomic_write(&cfg, data.to_string().as_bytes()) {
        Ok(()) => "registered".into(),
        Err(e) => format!("failed: {e}"),
    }
}

pub fn open_vault(ctx: &Ctx) {
    let path = ctx.vault().to_string_lossy().into_owned();
    let enc: String = path
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    if cfg!(windows) && obsidian_exe().is_some() {
        let _ = hidden_command("cmd")
            .args(["/c", "start", "", &format!("obsidian://open?path={enc}")])
            .spawn();
    } else {
        crate::util::open_path(&path);
    }
}

pub fn setup_all(ctx: &Ctx, api: &dyn RobloxApi, open_after: bool) -> Value {
    let v = Vault::new(ctx.vault());
    let vault = match v.init(Some(api)) {
        Ok(()) => v.root.to_string_lossy().into_owned(),
        Err(e) => format!("failed: {e}"),
    };
    let res = json!({
        "vault": vault,
        "obsidian": if cfg!(windows) { register_obsidian_vault(ctx) } else { "skipped".into() },
        "claude_desktop": register_claude_desktop(ctx),
        "claude_code": register_claude_code(ctx),
        "tasks": create_tasks(ctx),
        "shortcuts": create_shortcuts(ctx),
        "icon": make_icon(ctx).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
    });
    crate::info!("setup: {res}");
    if open_after {
        open_vault(ctx);
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_xml_is_well_formed() {
        let d = tempfile::tempdir().unwrap();
        let ctx = Ctx::at(d.path());
        let x = task_xml(
            &ctx,
            "a & b",
            "10:17",
            "scan --if-needed",
            "PT1H",
            "PT5M",
            "",
        );
        assert!(
            x.contains("a &amp; b")
                && x.contains("T10:17:00")
                && x.contains("<Arguments>scan --if-needed</Arguments>")
        );
    }
    #[test]
    fn icon_renders() {
        let i = icon_image(64);
        assert_eq!(i.dimensions(), (64, 64));
        assert!(i.pixels().any(|p| p[1] > 200));
    }
}
