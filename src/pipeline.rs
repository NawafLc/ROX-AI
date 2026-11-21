//! The headless build pipeline: every game is plain files, built and tested without any window.
//!
//!   files ──Rojo──► place.rbxl ──Lune──► scripts compile + unit tests (this PC)
//!                        └──Open Cloud──► upload to the private test place ──Luau Execution──► tests on Roblox
//!
//! Rojo and Lune are downloaded into ROX-AI's own `tools` folder (portable, nothing installed in Windows).

use crate::config::Ctx;
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Pinned, known-good versions (bumped on purpose, never "latest" by surprise).
pub const TOOLS: [(&str, &str, &str); 2] = [("rojo", "rojo-rbx/rojo", "7.7.1"), ("lune", "lune-org/lune", "0.11.1")];

const HARNESS: &str = include_str!("luau/harness.luau");
const CHECK: &str = include_str!("luau/check.luau");
const RUN: &str = include_str!("luau/run.luau");
const REMOTE: &str = include_str!("luau/remote.luau");

/// The starter project every new game begins from: (path inside the project, contents).
const TEMPLATE: [(&str, &str); 6] = [
    ("default.project.json", include_str!("template/default.project.json")),
    ("src/shared/Currency.luau", include_str!("template/shared/Currency.luau")),
    ("src/server/Main.server.luau", include_str!("template/server/Main.server.luau")),
    ("src/client/Main.client.luau", include_str!("template/client/Main.client.luau")),
    ("tests/Currency.spec.luau", include_str!("template/tests/Currency.spec.luau")),
    ("tests/Server.spec.luau", include_str!("template/tests/Server.spec.luau")),
];

// ------------------------------------------------------------------ tools

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

pub fn tool_path(ctx: &Ctx, name: &str) -> Option<PathBuf> {
    let p = ctx.tools_dir().join(exe(name));
    p.is_file().then_some(p)
}

/// Installed version of a tool (from the marker written next to it), if it is the pinned one.
fn tool_ok(ctx: &Ctx, name: &str, version: &str) -> bool {
    tool_path(ctx, name).is_some() && std::fs::read_to_string(ctx.tools_dir().join(format!("{name}.version"))).is_ok_and(|v| v.trim() == version)
}

pub fn tools_ready(ctx: &Ctx) -> bool {
    TOOLS.iter().all(|(n, _, v)| tool_ok(ctx, n, v))
}

pub fn tools_status(ctx: &Ctx) -> Value {
    json!(TOOLS.iter().map(|(n, _, v)| json!({"name": n, "version": v, "ok": tool_ok(ctx, n, v)})).collect::<Vec<_>>())
}

fn platform() -> Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("macos", "aarch64") => "macos-aarch64",
        ("macos", "x86_64") => "macos-x86_64",
        (os, arch) => bail!("no game building tools for {os}/{arch}"),
    })
}

/// Download Rojo and Lune (official GitHub releases) into the tools folder. Skips what's already there.
pub fn install_tools(ctx: &Ctx, report: crate::deps::Report) -> Result<()> {
    let dir = ctx.tools_dir();
    std::fs::create_dir_all(&dir)?;
    for (name, repo, version) in TOOLS {
        if tool_ok(ctx, name, version) {
            continue;
        }
        let pretty = if name == "rojo" { "Rojo (place builder)" } else { "Lune (script checker)" };
        let url = format!("https://github.com/{repo}/releases/download/v{version}/{name}-{version}-{}.zip", platform()?);
        let zip_path = crate::deps::download(&url, &format!("roxai-{name}-{version}.zip"), pretty, report)?;
        report(&format!("Unpacking {pretty}…"), None);
        let mut z = zip::ZipArchive::new(std::fs::File::open(&zip_path)?).context("the download wasn't a valid zip")?;
        let want = exe(name);
        let mut found = false;
        for i in 0..z.len() {
            let mut f = z.by_index(i)?;
            if f.name().rsplit('/').next() == Some(want.as_str()) {
                let tmp = dir.join(format!("{want}.new"));
                let mut out = std::fs::File::create(&tmp)?;
                std::io::copy(&mut f, &mut out)?;
                drop(out);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
                }
                std::fs::rename(&tmp, dir.join(&want))?;
                found = true;
                break;
            }
        }
        let _ = std::fs::remove_file(&zip_path);
        if !found {
            bail!("{pretty}: {want} wasn't in the download");
        }
        std::fs::write(dir.join(format!("{name}.version")), version)?;
        crate::info!("installed {name} {version}");
    }
    Ok(())
}

// ------------------------------------------------------------------ projects

pub fn games_dir(ctx: &Ctx) -> PathBuf {
    ctx.home.join("games")
}

pub fn slug(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() {
        "game".into()
    } else {
        s.chars().take(48).collect()
    }
}

/// Write the starter project to `dir` (only files that don't exist yet, so nothing of a game is overwritten).
pub fn write_template(dir: &Path, name: &str) -> Result<()> {
    for (rel, text) in TEMPLATE {
        let p = dir.join(rel);
        if p.exists() {
            continue;
        }
        std::fs::create_dir_all(p.parent().unwrap())?;
        let text = text.replace("{{NAME}}", &name.replace('"', "'"));
        std::fs::write(&p, text)?;
    }
    let gi = dir.join(".gitignore");
    if !gi.exists() {
        std::fs::write(gi, "build/\n.roxai/\n")?;
    }
    Ok(())
}

pub fn new_project(ctx: &Ctx, name: &str) -> Result<PathBuf> {
    let dir = games_dir(ctx).join(slug(name));
    if dir.join("default.project.json").exists() {
        bail!("a game project called {} already exists", slug(name));
    }
    write_template(&dir, name)?;
    Ok(dir)
}

// ------------------------------------------------------------------ build + test

#[derive(Clone, Debug, Default, Serialize)]
pub struct Step {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub seconds: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    pub project: String,
    pub ok: bool,
    pub steps: Vec<Step>,
    /// local test results (Lune)
    pub tests: Vec<Value>,
    /// test results on Roblox's servers
    pub remote_tests: Vec<Value>,
    pub remote_logs: Vec<String>,
    pub place_version: Option<u64>,
    pub finished: String,
}

impl Report {
    fn step(&mut self, name: &str, started: Instant, r: Result<String>) -> bool {
        let (ok, detail) = match r {
            Ok(d) => (true, d),
            Err(e) => (false, format!("{e:#}")),
        };
        self.steps.push(Step { name: name.into(), ok, detail, seconds: (started.elapsed().as_secs_f64() * 10.0).round() / 10.0 });
        ok
    }
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn script_file(ctx: &Ctx, name: &str, body: &str) -> Result<PathBuf> {
    let dir = ctx.tools_dir().join("roxai");
    std::fs::create_dir_all(&dir)?;
    let p = dir.join(name);
    std::fs::write(&p, format!("{HARNESS}\n{body}"))?;
    Ok(p)
}

fn summarize(tests: &[Value]) -> (usize, usize, usize) {
    let skipped = tests.iter().filter(|t| t["skipped"].as_bool() == Some(true)).count();
    let failed = tests.iter().filter(|t| t["ok"].as_bool() != Some(true)).count();
    (tests.len() - skipped - failed, failed, skipped)
}

fn failures(tests: &[Value]) -> String {
    tests
        .iter()
        .filter(|t| t["ok"].as_bool() != Some(true))
        .map(|t| format!("✗ {} › {}: {}", t["spec"].as_str().unwrap_or(""), t["test"].as_str().unwrap_or(""), t["error"].as_str().unwrap_or("")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build a project and run every check. `remote` = also upload to the test place and test on Roblox.
pub fn run(ctx: &Ctx, dir: &Path, remote: bool, report_progress: crate::deps::Report) -> Result<Report> {
    let _lock = crate::util::single_instance(&ctx.state_dir(), "pipeline")?;
    let dir = dir.canonicalize().with_context(|| format!("no game project at {}", dir.display()))?;
    // plain path (no \\?\ prefix) so every tool understands it
    let dir = PathBuf::from(dir.to_string_lossy().trim_start_matches(r"\\?\").to_string());
    if !dir.join("default.project.json").is_file() {
        bail!("{} has no default.project.json", dir.display());
    }
    let mut rep = Report { project: dir.file_name().unwrap_or_default().to_string_lossy().into(), ..Default::default() };
    let project = rep.project.clone();
    let progress = |text: &str| {
        report_progress(text, None);
        let _ = ctx.store.set("pipeline_now", json!({"project": project, "step": text, "since": crate::util::now_iso()}));
    };

    // 1. tools
    let t = Instant::now();
    progress("Getting the build tools ready");
    if !rep.step("Build tools", t, install_tools(ctx, report_progress).map(|_| TOOLS.iter().map(|(n, _, v)| format!("{n} {v}")).collect::<Vec<_>>().join(", "))) {
        return finish(ctx, &dir, rep);
    }

    // 2. Rojo: files → place file
    let t = Instant::now();
    progress("Building the place file");
    let out = dir.join("build").join("place.rbxl");
    std::fs::create_dir_all(out.parent().unwrap())?;
    let _ = std::fs::remove_file(&out);
    let rojo = tool_path(ctx, "rojo").ok_or_else(|| anyhow!("Rojo missing"))?;
    let r = crate::util::run_capture(&rojo.to_string_lossy(), &["build", &dir.to_string_lossy(), "--output", &out.to_string_lossy()], Duration::from_secs(300)).and_then(|(code, text)| {
        if code != 0 || !out.is_file() {
            bail!("Rojo couldn't build the place:\n{}", tail(&text, 12));
        }
        Ok(format!("{} KB place file", std::fs::metadata(&out)?.len() / 1024))
    });
    if !rep.step("Build place", t, r) {
        return finish(ctx, &dir, rep);
    }

    // 3. Lune: every script compiles + unit tests
    let t = Instant::now();
    progress("Checking scripts and running unit tests");
    let lune = tool_path(ctx, "lune").ok_or_else(|| anyhow!("Lune missing"))?;
    let check = script_file(ctx, "check.luau", CHECK)?;
    let r = crate::util::run_capture(&lune.to_string_lossy(), &["run", &check.to_string_lossy(), &out.to_string_lossy()], Duration::from_secs(300)).and_then(|(code, text)| {
        let line = text.lines().find_map(|l| l.trim().strip_prefix("ROXAI_RESULT ")).ok_or_else(|| anyhow!("the script checker didn't finish:\n{}", tail(&text, 15)))?;
        let v: Value = serde_json::from_str(line)?;
        let syntax = v["syntax"].as_array().cloned().unwrap_or_default();
        rep.tests = v["tests"].as_array().cloned().unwrap_or_default();
        if !syntax.is_empty() {
            bail!(
                "{} script(s) don't compile:\n{}",
                syntax.len(),
                syntax.iter().map(|s| format!("✗ {}: {}", s["script"].as_str().unwrap_or(""), s["error"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n")
            );
        }
        let (pass, fail, skip) = summarize(&rep.tests);
        if fail > 0 || code != 0 {
            bail!("{fail} test(s) failed, {pass} passed:\n{}", failures(&rep.tests));
        }
        Ok(format!("{} scripts compile · {pass} tests passed{}", v["scripts"].as_u64().unwrap_or(0), if skip > 0 { format!(" · {skip} run on Roblox only") } else { String::new() }))
    });
    if !rep.step("Scripts and unit tests", t, r) || !remote {
        return finish(ctx, &dir, rep);
    }

    // 4. upload to the private test place
    let t = Instant::now();
    progress("Uploading to your private test place");
    let r = std::fs::read(&out).map_err(anyhow::Error::from).and_then(|b| crate::cloud::upload_place(ctx, &b));
    let version = match r {
        Ok(v) => {
            rep.place_version = Some(v);
            rep.step("Upload to test place", t, Ok(format!("version {v} (saved, not public)")));
            v
        }
        Err(e) => {
            rep.step("Upload to test place", t, Err(e));
            return finish(ctx, &dir, rep);
        }
    };

    // 5. tests on Roblox's servers
    let t = Instant::now();
    progress("Running the tests on Roblox's servers");
    let script = format!("{HARNESS}\n{REMOTE}");
    let r = crate::cloud::run_luau(ctx, &script, Some(version), Duration::from_secs(240)).and_then(|run| {
        rep.remote_logs = run.logs.clone();
        if run.state != "COMPLETE" {
            bail!("the test run stopped: {}\n{}", run.error.unwrap_or(run.state), tail(&run.logs.join("\n"), 10));
        }
        let out = run.results.first().cloned().unwrap_or_default();
        rep.remote_tests = out["tests"].as_array().cloned().unwrap_or_default();
        let (pass, fail, _) = summarize(&rep.remote_tests);
        if fail > 0 {
            bail!("{fail} test(s) failed on Roblox, {pass} passed:\n{}", failures(&rep.remote_tests));
        }
        Ok(format!("{pass} tests passed on Roblox's servers in {:.0}s", run.seconds))
    });
    rep.step("Tests on Roblox", t, r);
    finish(ctx, &dir, rep)
}

fn finish(ctx: &Ctx, dir: &Path, mut rep: Report) -> Result<Report> {
    rep.ok = !rep.steps.is_empty() && rep.steps.iter().all(|s| s.ok);
    rep.finished = crate::util::now_iso();
    let v = serde_json::to_value(&rep)?;
    let meta = dir.join(".roxai");
    std::fs::create_dir_all(&meta)?;
    crate::util::atomic_write(&meta.join("last_build.json"), serde_json::to_string_pretty(&v)?.as_bytes())?;
    ctx.store.set("pipeline_last", v)?;
    ctx.store.set("pipeline_now", Value::Null)?;
    Ok(rep)
}

/// Builds and tests the starter project end to end: proves the whole pipeline works on this PC.
/// Build the place and run one Luau snippet against it on this PC; returns what it printed.
pub fn run_snippet(ctx: &Ctx, dir: &Path, code: &str) -> Result<String> {
    let _lock = crate::util::single_instance(&ctx.state_dir(), "pipeline")?;
    let dir = PathBuf::from(dir.canonicalize()?.to_string_lossy().trim_start_matches(r"\\?\").to_string());
    let rojo = tool_path(ctx, "rojo").ok_or_else(|| anyhow!("Rojo missing: run project_build once"))?;
    let lune = tool_path(ctx, "lune").ok_or_else(|| anyhow!("Lune missing: run project_build once"))?;
    let build = dir.join("build");
    std::fs::create_dir_all(&build)?;
    let place = build.join("run.rbxl");
    let (code_r, text) = crate::util::run_capture(&rojo.to_string_lossy(), &["build", &dir.to_string_lossy(), "--output", &place.to_string_lossy()], Duration::from_secs(300))?;
    if code_r != 0 || !place.is_file() {
        bail!("Rojo couldn't build the place:
{}", tail(&text, 12));
    }
    let snippet = build.join("snippet.luau");
    std::fs::write(&snippet, code)?;
    let run = script_file(ctx, "run.luau", RUN)?;
    let (_, text) = crate::util::run_capture(&lune.to_string_lossy(), &["run", &run.to_string_lossy(), &place.to_string_lossy(), &snippet.to_string_lossy()], Duration::from_secs(120))?;
    let line = text.lines().find_map(|l| l.trim().strip_prefix("ROXAI_RUN ")).ok_or_else(|| anyhow!("the snippet didn't finish (endless loop?):
{}", tail(&text, 12)))?;
    let v: Value = serde_json::from_str(line)?;
    let out = v["output"].as_str().unwrap_or("").to_string();
    let mut s = if out.is_empty() { "(nothing printed)".to_string() } else { out };
    if v["cut"].as_bool() == Some(true) {
        s.push_str("
(output cut at 20 KB: print less)");
    }
    if v["ok"].as_bool() != Some(true) {
        s.push_str(&format!("
ERROR: {}", v["error"].as_str().unwrap_or("?")));
    }
    Ok(s)
}

pub fn selftest(ctx: &Ctx, report: crate::deps::Report) -> Result<Report> {
    let dir = games_dir(ctx).join("_pipeline-check");
    let _ = std::fs::remove_dir_all(&dir);
    write_template(&dir, "ROX-AI pipeline check")?;
    let remote = crate::cloud::connection(ctx).is_some();
    run(ctx, &dir, remote, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("Pet Simulator: Ultra!"), "pet-simulator-ultra");
        assert_eq!(slug("  "), "game");
    }

    #[test]
    fn template_is_complete_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        write_template(dir.path(), "My \"Game\"").unwrap();
        let proj: Value = serde_json::from_str(&std::fs::read_to_string(dir.path().join("default.project.json")).unwrap()).unwrap();
        assert_eq!(proj["name"], "My 'Game'");
        assert_eq!(proj["tree"]["ServerStorage"]["Tests"]["$path"], "tests");
        let f = dir.path().join("src/shared/Currency.luau");
        std::fs::write(&f, "-- mine").unwrap();
        write_template(dir.path(), "x").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "-- mine");
        assert!(dir.path().join("tests/Currency.spec.luau").is_file());
    }

    #[test]
    fn test_summaries() {
        let t = vec![json!({"ok": true}), json!({"ok": false, "spec": "a", "test": "b", "error": "boom"}), json!({"ok": true, "skipped": true})];
        assert_eq!(summarize(&t), (1, 1, 1));
        assert!(failures(&t).contains("boom"));
    }
}
