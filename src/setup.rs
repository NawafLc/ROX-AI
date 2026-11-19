//! One-shot system integration (run by the installer, safe to re-run any time):
//!   * create the vault (+ Dataview) and register it in Obsidian
//!   * register the MCP server in Claude Desktop (all install variants) and Claude Code
//!   * create Windows scheduled tasks (daily scan, hourly-retrying playtest check)
//!   * desktop + Start-menu shortcuts with an app icon

use crate::api::RobloxApi;
use crate::autopilot::{find_claude, mcp_entry};
use crate::config::{home_dir, Ctx};
use crate::util::{atomic_write, run_capture};
use crate::vault::Vault;
use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
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
        if servers.get("roxai") == Some(&mcp_entry(ctx)) {
            done.push(p.to_string_lossy().into_owned());
            continue;
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
    let mut out = json!({"Daily Chart Scan": create_task(ctx, "Daily Chart Scan", &scan)});
    // legacy name from the playtesting-only versions
    let _ = run_capture(
        "schtasks",
        &[
            "/Delete",
            "/TN",
            &format!("{TASK_FOLDER}\\Weekly Playtests"),
            "/F",
        ],
        Duration::from_secs(30),
    );
    if ctx.is_active() {
        let learn = task_xml(ctx, "ROX-AI: checks hourly if it's time for Claude to learn new chart games from YouTube videos, then does it (no screen use).", &s.playtest_check_time, "autopilot --scheduled", "PT11H", "PT20M",
            "<Repetition><Interval>PT1H</Interval><Duration>PT12H</Duration><StopAtDurationEnd>false</StopAtDurationEnd></Repetition>");
        out["Learning"] = json!(create_task(ctx, "Learning", &learn));
    } else {
        let _ = run_capture(
            "schtasks",
            &["/Delete", "/TN", &format!("{TASK_FOLDER}\\Learning"), "/F"],
            Duration::from_secs(30),
        );
    }
    out
}

pub fn remove_tasks() {
    for n in ["Daily Chart Scan", "Learning", "Weekly Playtests"] {
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

// ---------------------------------------------------------- Obsidian ---

pub fn obsidian_exe() -> Option<PathBuf> {
    let la = env_path("LOCALAPPDATA");
    let mut cands: Vec<PathBuf> = [
        la.as_ref()
            .map(|l| l.join("Programs").join("Obsidian").join("Obsidian.exe")),
        la.as_ref().map(|l| l.join("Obsidian").join("Obsidian.exe")),
        env_path("ProgramFiles").map(|p| p.join("Obsidian").join("Obsidian.exe")),
        env_path("ProgramFiles(x86)").map(|p| p.join("Obsidian").join("Obsidian.exe")),
    ]
    .into_iter()
    .flatten()
    .collect();
    // winget / custom installs: ask the uninstall registry where it went
    if cfg!(windows) {
        for root in ["HKCU", "HKLM"] {
            let key = format!("{root}\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall");
            if let Ok((0, out)) = run_capture(
                "reg",
                &["query", &key, "/s", "/f", "Obsidian.exe", "/d"],
                Duration::from_secs(20),
            ) {
                let rx = regex::Regex::new(r#"(?i)([a-z]:\\[^"\r\n]*?obsidian\.exe)"#).unwrap();
                for c in rx.captures_iter(&out) {
                    cands.push(PathBuf::from(&c[1]));
                }
            }
        }
    }
    cands.into_iter().find(|p| p.is_file())
}

/// Obsidian registers the obsidian:// link only after it has been opened once.
pub fn obsidian_protocol_registered() -> bool {
    cfg!(windows)
        && ["HKCU\\Software\\Classes\\obsidian", "HKCR\\obsidian"]
            .iter()
            .any(|k| {
                run_capture("reg", &["query", k], Duration::from_secs(10))
                    .is_ok_and(|(c, _)| c == 0)
            })
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

/// Open the vault in Obsidian, whatever state Obsidian is in. Never shows an error dialog:
///   link works → use it · Obsidian installed but never opened → register vault + start Obsidian
///   Obsidian missing → open the folder in Explorer.
/// Open one note (path relative to the vault) in Obsidian, or with the default app.
pub fn open_note(ctx: &Ctx, rel: &str) {
    let file = ctx.vault().join(rel);
    let path = file.to_string_lossy().into_owned();
    if cfg!(windows) && obsidian_protocol_registered() {
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
        crate::util::open_path(&format!("obsidian://open?path={enc}"));
    } else {
        crate::util::open_path(&path);
    }
}

pub fn open_vault(ctx: &Ctx) -> String {
    let path = ctx.vault().to_string_lossy().into_owned();
    if !cfg!(windows) {
        crate::util::open_path(&path);
        return "opened folder".into();
    }
    if obsidian_protocol_registered() {
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
        crate::util::open_path(&format!("obsidian://open?path={enc}"));
        return "opened in Obsidian".into();
    }
    if let Some(exe) = obsidian_exe() {
        // first launch: our vault is pre-registered (marked open), so Obsidian opens straight into it
        let reg = register_obsidian_vault(ctx);
        let _ = std::process::Command::new(&exe).spawn();
        return format!("started Obsidian ({reg})");
    }
    crate::util::open_path(&path);
    "Obsidian not found — opened the vault folder in Explorer".into()
}

/// Silent setup on every launch (fast when nothing changed). Portable: everything points at the exe's
/// current folder, so moving the folder just works — paths are refreshed automatically.
pub fn ensure(ctx: &Ctx, api: Option<&dyn RobloxApi>) -> Value {
    migrate_legacy(ctx);
    cleanup_old_exes();
    let v = Vault::new(ctx.vault());
    let vault = match v.init(api) {
        Ok(()) => v.root.to_string_lossy().into_owned(),
        Err(e) => format!("failed: {e}"),
    };
    let exe = crate::util::console_exe().to_string_lossy().into_owned();
    let mut res = json!({"vault": vault, "data": ctx.home.to_string_lossy()});
    // A development build (cargo's target folder) must never take over the real app's schedule or
    // Claude connections, e.g. while previewing the interface.
    let dev_build = exe.to_lowercase().replace('/', "\\").contains("\\target\\") && std::env::var_os("ROXAI_DEV_REGISTER").is_none();
    if dev_build {
        res["registration"] = json!("skipped (development build)");
    }
    if cfg!(windows) && !dev_build {
        res["obsidian"] = json!(register_obsidian_vault(ctx));
        res["claude_desktop"] = json!(register_claude_desktop(ctx));
        // Claude Code registration is slow-ish: only redo it when the exe moved
        if find_claude(ctx).is_some()
            && ctx
                .store
                .get("claude_code_exe")
                .and_then(|v| v.as_str().map(String::from))
                != Some(exe.clone())
        {
            let r = register_claude_code(ctx);
            if r == "registered" {
                let _ = ctx.store.set("claude_code_exe", json!(exe));
            }
            res["claude_code"] = json!(r);
        }
        res["auto_run"] = json!(apply_auto_run(ctx));
    }
    let _ = make_icon(ctx);
    res
}

/// Scheduled tasks on/off (the only thing ROX-AI registers in Windows, and only when enabled).
pub fn apply_auto_run(ctx: &Ctx) -> String {
    if !cfg!(windows) {
        return "n/a".into();
    }
    let exe = crate::util::bg_exe().to_string_lossy().into_owned();
    if ctx.settings.auto_run {
        let current = ctx
            .store
            .get("tasks_exe")
            .and_then(|v| v.as_str().map(String::from));
        let want = format!("{exe}|{}", ctx.settings.mode);
        if current.as_deref() != Some(want.as_str())
            || task_status("Daily Chart Scan").is_none()
            || (ctx.is_active() == task_status("Learning").is_none())
        {
            let r = create_tasks(ctx);
            if r.as_object().is_some_and(|o| o.values().all(|v| v == "ok")) {
                let _ = ctx.store.set("tasks_exe", json!(want));
                return "on".into();
            }
            return format!("failed: {r}");
        }
        "on".into()
    } else {
        if ctx.store.get("tasks_exe").is_some() || task_status("Daily Chart Scan").is_some() {
            remove_tasks();
            let _ = ctx.store.set("tasks_exe", Value::Null);
        }
        "off".into()
    }
}

/// "Remove from this PC": undo everything ROX-AI registered. Your folder (app, data, vault) is untouched.
pub fn remove_from_pc(ctx: &Ctx) -> String {
    remove_tasks();
    let _ = ctx.store.set("tasks_exe", Value::Null);
    let _ = ctx.store.set("claude_code_exe", Value::Null);
    for p in claude_desktop_config_paths() {
        if let Some(mut cfg) = std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        {
            if let Some(m) = cfg.get_mut("mcpServers").and_then(|m| m.as_object_mut()) {
                if m.remove("roxai").is_some() {
                    let _ = atomic_write(
                        &p,
                        serde_json::to_string_pretty(&cfg)
                            .unwrap_or_default()
                            .as_bytes(),
                    );
                }
            }
        }
    }
    if let Some(c) = find_claude(ctx) {
        let _ = run_capture(
            &c.to_string_lossy(),
            &["mcp", "remove", "roxai", "-s", "user"],
            Duration::from_secs(60),
        );
    }
    let mut s = ctx.settings.clone();
    s.auto_run = false;
    let _ = s.save(&ctx.home);
    "Removed the schedule and Claude connections. Delete the folder to remove ROX-AI completely."
        .into()
}

/// Leftovers from an update (Windows can't delete a running exe, so updates rename them).
pub fn cleanup_old_exes() {
    let dir = crate::config::app_folder();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().ends_with(".old.exe") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else if !dst.exists() {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// One-time move from the old *installed* version (%LOCALAPPDATA%\ROX-AI + Documents\ROX-AI Vault):
/// copies history/settings and the vault into this portable folder, then removes the old install's
/// shortcuts, PATH entry and program files. Nothing is deleted from your vault.
pub fn migrate_legacy(ctx: &Ctx) {
    let Some(old) = crate::config::legacy_home() else {
        return;
    };
    if !cfg!(windows)
        || old == ctx.home
        || !old.join("data").join("games.json").exists()
        || ctx.store.get("legacy_migrated").is_some()
    {
        return;
    }
    if ctx.store.games().is_empty() {
        let _ = copy_dir(&old.join("data"), ctx.store.dir());
        if let Ok(t) = std::fs::read_to_string(old.join("settings.json")) {
            if let Ok(mut v) = serde_json::from_str::<Value>(&t) {
                v["vault_path"] = json!(""); // portable vault
                if let Ok(s) = serde_json::from_value::<crate::config::Settings>(v) {
                    let _ = s.save(&ctx.home);
                }
            }
        }
        let old_vault = crate::config::home_dir()
            .join("Documents")
            .join("ROX-AI Vault");
        if old_vault.exists() {
            let _ = copy_dir(&old_vault, &ctx.vault());
        }
    }
    // remove the old install's footprint (data in the old folder is kept as a backup)
    for lnk in [
        crate::config::home_dir().join("Desktop").join("ROX-AI.lnk"),
        env_path("APPDATA")
            .unwrap_or_default()
            .join("Microsoft/Windows/Start Menu/Programs/ROX-AI.lnk"),
    ] {
        let _ = std::fs::remove_file(lnk);
    }
    let _ = std::fs::remove_dir_all(old.join("bin"));
    let _ = std::fs::remove_dir_all(old.join("venv"));
    let _ = std::fs::remove_dir_all(old.join("app"));
    if let Ok((0, out)) = run_capture(
        "reg",
        &["query", "HKCU\\Environment", "/v", "Path"],
        Duration::from_secs(10),
    ) {
        if let Some(line) = out.lines().find(|l| l.trim_start().starts_with("Path")) {
            let val = line
                .split("REG_")
                .nth(1)
                .and_then(|r| r.split_once("    "))
                .map(|(_, v)| v.trim().to_string())
                .unwrap_or_default();
            let bin = old.join("bin").to_string_lossy().into_owned();
            if val.to_lowercase().contains(&bin.to_lowercase()) {
                let new: Vec<&str> = val
                    .split(';')
                    .filter(|p| !p.eq_ignore_ascii_case(&bin) && !p.is_empty())
                    .collect();
                let _ = run_capture(
                    "reg",
                    &[
                        "add",
                        "HKCU\\Environment",
                        "/v",
                        "Path",
                        "/t",
                        "REG_EXPAND_SZ",
                        "/d",
                        &new.join(";"),
                        "/f",
                    ],
                    Duration::from_secs(10),
                );
            }
        }
    }
    let _ = ctx
        .store
        .set("legacy_migrated", json!(crate::util::now_iso()));
    crate::info!("migrated old installed version into the portable folder");
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
