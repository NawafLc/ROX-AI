//! Shared helpers: atomic writes, cross-process locks, time, processes.

use anyhow::{bail, Result};
use chrono::{DateTime, Local, NaiveDate, Utc};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Write via temp file + rename so readers (Obsidian, other processes) never see half a file.
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp{}",
        path.extension().and_then(|e| e.to_str()).unwrap_or(""),
        std::process::id()
    ));
    std::fs::write(&tmp, data)?;
    let mut last = None;
    for i in 0..10 {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(50 * (i + 1)));
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last.unwrap().into())
}

pub fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
}

pub fn today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

pub fn parse_dt(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Some(d.with_timezone(&Utc));
    }
    // Roblox sometimes sends 7 fractional digits; trim to 6
    if let Some(dot) = s.find('.') {
        let tail = &s[dot + 1..];
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        let rest = &tail[digits.len()..];
        let fixed = format!(
            "{}.{}{}",
            &s[..dot],
            &digits[..digits.len().min(6)],
            if rest.is_empty() { "Z" } else { rest }
        );
        if let Ok(d) = DateTime::parse_from_rfc3339(&fixed) {
            return Some(d.with_timezone(&Utc));
        }
    }
    if s.len() >= 10 {
        if let Ok(d) = NaiveDate::parse_from_str(&s[..10], "%Y-%m-%d") {
            return Some(d.and_hms_opt(0, 0, 0)?.and_utc());
        }
    }
    None
}

pub fn days_since(s: &str) -> Option<f64> {
    parse_dt(s).map(|d| (Utc::now() - d).num_seconds() as f64 / 86400.0)
}

pub fn day_gap(a: &str, b: &str) -> Option<i64> {
    let a = NaiveDate::parse_from_str(a, "%Y-%m-%d").ok()?;
    let b = NaiveDate::parse_from_str(b, "%Y-%m-%d").ok()?;
    Some((b - a).num_days())
}

pub fn rand_hex(n: usize) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    while out.len() < n {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
        );
        h.write_u32(std::process::id());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out[..n].to_string()
}

pub fn uuid4() -> String {
    let h = rand_hex(32);
    format!(
        "{}-{}-4{}-a{}-{}",
        &h[0..8],
        &h[8..12],
        &h[13..16],
        &h[17..20],
        &h[20..32]
    )
}

// ------------------------------------------------------------ locks ---

pub struct InstanceLock {
    path: PathBuf,
    _file: File,
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Single-instance lock. The OS releases it automatically if the process dies, so a crash can never
/// block future runs.
pub fn single_instance(state_dir: &Path, name: &str) -> Result<InstanceLock> {
    std::fs::create_dir_all(state_dir)?;
    let path = state_dir.join(format!("{name}.lock"));
    let f = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)?;
    match f.try_lock() {
        Ok(()) => {}
        Err(_) => bail!("{name} already running"),
    }
    use std::io::Write;
    let mut f2 = &f;
    let _ = f2.write_all(format!("{}", std::process::id()).as_bytes());
    Ok(InstanceLock { path, _file: f })
}

pub fn is_locked(state_dir: &Path, name: &str) -> bool {
    let path = state_dir.join(format!("{name}.lock"));
    if !path.exists() {
        return false;
    }
    match OpenOptions::new().read(true).write(true).open(&path) {
        Ok(f) => {
            
            f.try_lock().is_err()
        }
        Err(_) => false,
    }
}

// ------------------------------------------------------- processes ---

pub fn pid_exists(pid: u32) -> bool {
    crate::win::proc::pid_exists(pid)
}

/// Hidden child process on Windows (no console flash).
pub fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

pub fn run_capture(program: &str, args: &[&str], timeout: Duration) -> Result<(i32, String)> {
    let mut child = hidden_command(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let start = std::time::Instant::now();
    loop {
        if let Some(st) = child.try_wait()? {
            let out = child.wait_with_output()?;
            let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
            s.push_str(&String::from_utf8_lossy(&out.stderr));
            return Ok((st.code().unwrap_or(-1), s));
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            bail!("{program} timed out");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Path of the windowless twin (roxaiw.exe) next to the current exe, or the current exe.
pub fn bg_exe() -> PathBuf {
    let me = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("roxai"));
    let name = if cfg!(windows) {
        "roxaiw.exe"
    } else {
        "roxaiw"
    };
    let w = me.with_file_name(name);
    if w.exists() {
        w
    } else {
        me
    }
}

pub fn console_exe() -> PathBuf {
    let me = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("roxai"));
    let name = if cfg!(windows) { "roxai.exe" } else { "roxai" };
    let c = me.with_file_name(name);
    if c.exists() {
        c
    } else {
        me
    }
}

pub fn open_path(p: &str) {
    #[cfg(windows)]
    {
        let _ = hidden_command("cmd").args(["/c", "start", "", p]).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(p).spawn();
    }
}
