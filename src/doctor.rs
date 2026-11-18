//! Health checks — used by the panel, the dashboard status box, `roxai doctor` and the system_check tool.

use crate::config::Ctx;
use crate::util::{days_since, today};

pub struct Check {
    pub name: String,
    pub ok: Option<bool>, // None = warning / unknown
    pub detail: String,
    pub fix: String,
}

fn c(name: &str, ok: Option<bool>, detail: impl Into<String>, fix: &str) -> Check {
    Check {
        name: name.into(),
        ok,
        detail: detail.into(),
        fix: fix.into(),
    }
}

pub fn checks(ctx: &Ctx, network: bool) -> Vec<Check> {
    let s = &ctx.settings;
    let mut out = vec![
        c(
            "Windows",
            Some(cfg!(windows)),
            std::env::consts::OS,
            "Playtests need Windows; scans work anywhere.",
        ),
        c("ROX-AI", Some(true), format!("v{}", crate::VERSION), ""),
    ];
    let v = ctx.vault();
    out.push(c(
        "Vault",
        Some(v.exists()),
        v.to_string_lossy(),
        "roxai setup",
    ));
    if v.exists() {
        out.push(c(
            "Dataview plugin",
            Some(v.join(".obsidian/plugins/dataview/main.js").exists()),
            "live dashboard tables",
            "Optional: Obsidian → Community plugins → Dataview",
        ));
    }
    if cfg!(windows) {
        use crate::setup as S;
        use crate::win::roblox as RB;
        out.push(c(
            "Obsidian",
            Some(S::obsidian_exe().is_some()),
            S::obsidian_exe()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or("not found".into()),
            "winget install Obsidian.Obsidian",
        ));
        let rb = RB::is_installed();
        out.push(c(
            "Roblox Player",
            Some(rb),
            if rb {
                RB::player_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or("protocol only".into())
            } else {
                "not found".into()
            },
            "Install from roblox.com/download and log in with your ALT account",
        ));
        let (ok, info) = crate::autopilot::claude_ready(ctx);
        out.push(c(
            "Claude Code CLI",
            Some(ok),
            info,
            "Re-run INSTALL.bat, then run `claude` once to log in",
        ));
        let cd: Vec<_> = S::claude_desktop_config_paths()
            .into_iter()
            .filter(|p| std::fs::read_to_string(p).is_ok_and(|t| t.contains("roxai")))
            .collect();
        out.push(c(
            "Claude Desktop MCP",
            Some(!cd.is_empty()),
            cd.first()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or("not registered".into()),
            "roxai setup",
        ));
        for t in ["Daily Chart Scan", "Learning"] {
            let st = S::task_status(t);
            out.push(c(
                &format!("Task: {t}"),
                Some(
                    st.as_ref()
                        .is_some_and(|x| !x.to_lowercase().contains("disabled")),
                ),
                st.unwrap_or("missing".into()),
                "roxai setup",
            ));
        }
    }
    match ctx.store.last_run("scan", None) {
        Some(r) => {
            let age = days_since(r.finished.as_deref().unwrap_or(&r.started)).unwrap_or(0.0);
            out.push(c(
                "Last chart scan",
                Some(r.status == "ok" && age < 2.0),
                format!(
                    "{} · {} ({age:.1} d ago)",
                    r.status,
                    r.started.chars().take(16).collect::<String>()
                ),
                if r.status != "ok" { "roxai scan" } else { "" },
            ));
        }
        None => out.push(c("Last chart scan", None, "never", "roxai scan")),
    }
    match ctx.store.last_run("autopilot", None) {
        Some(r) => out.push(c(
            "Last playtest run",
            Some(["ok", "skipped", "running", "partial"].contains(&r.status.as_str())),
            format!(
                "{} · {} · {}",
                r.status,
                r.started.chars().take(16).collect::<String>(),
                r.detail.chars().take(120).collect::<String>()
            ),
            "",
        )),
        None => out.push(c("Last playtest run", None, "never ran yet", "")),
    }
    let b = ctx.store.get("blocked_reason");
    out.push(c(
        "Account blocker",
        Some(b.is_none()),
        b.as_ref()
            .map(|b| b["reason"].as_str().unwrap_or("?").to_string())
            .unwrap_or("none".into()),
        "Fix the Roblox/Claude login, then click 'I fixed the login' (roxai unblock)",
    ));
    if network {
        match crate::api::HttpApi::new()
            .and_then(|api| crate::api::RobloxApi::chart(&api, &s.sort_id, 3, &s.country))
        {
            Ok(v) => out.push(c(
                "Roblox API",
                Some(!v.is_empty()),
                format!("{} games returned", v.len()),
                "",
            )),
            Err(e) => out.push(c(
                "Roblox API",
                Some(false),
                e.to_string().chars().take(200).collect::<String>(),
                "Check internet connection",
            )),
        }
    }
    out.push(c("Data folder", Some(true), ctx.home.to_string_lossy(), ""));
    out
}

pub fn report_text(ctx: &Ctx, network: bool) -> String {
    checks(ctx, network)
        .iter()
        .map(|k| {
            let icon = match k.ok {
                Some(true) => "✅",
                Some(false) => "❌",
                None => "⚠️",
            };
            let fix = if k.ok != Some(true) && !k.fix.is_empty() {
                format!("  → {}", k.fix)
            } else {
                String::new()
            };
            format!("{icon} {}: {}{fix}", k.name, k.detail)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn short_status(ctx: &Ctx) -> Vec<String> {
    let st = &ctx.store;
    let mut out = vec![];
    let ls = st
        .last_run("scan", Some("ok"))
        .map(|r| {
            r.started
                .chars()
                .take(16)
                .collect::<String>()
                .replace('T', " ")
        })
        .unwrap_or("never".into());
    out.push(format!("📊 Last successful scan: **{ls}**"));
    match st.last_run("autopilot", None) {
        Some(r) => {
            let extra = serde_json::from_str::<serde_json::Value>(&r.detail)
                .ok()
                .filter(|d| d.get("queue").is_some())
                .map(|d| format!(" · {}/{} games", d["completed"], d["queue"]))
                .unwrap_or_default();
            out.push(format!(
                "🕹️ Last playtest run: **{}** {}{extra}",
                r.status,
                r.started.chars().take(10).collect::<String>()
            ));
        }
        None => out.push("🕹️ Last playtest run: not yet (runs weekly when your PC is idle)".into()),
    }
    out.push(format!(
        "🗂️ {} games tracked · {} completed playtests · today {}",
        st.games().len(),
        st.count_playtests("completed"),
        today()
    ));
    if let Some(b) = st.get("blocked_reason") {
        out.push(format!(
            "⛔ **Needs you:** {} → fix it, then press *I fixed the login* in the ROX-AI panel.",
            b["reason"].as_str().unwrap_or("?")
        ));
    }
    out
}
