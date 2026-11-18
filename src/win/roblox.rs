//! Launch / find / kill the Roblox Player.

use super::{proc, window, Hwnd};
use anyhow::{bail, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const PLAYER_EXES: [&str; 1] = ["RobloxPlayerBeta.exe"];
pub const ALL_EXES: [&str; 4] = [
    "RobloxPlayerBeta.exe",
    "RobloxPlayerLauncher.exe",
    "RobloxCrashHandler.exe",
    "RobloxPlayerInstaller.exe",
];

pub fn player_pids() -> Vec<u32> {
    proc::list()
        .into_iter()
        .filter(|p| PLAYER_EXES.iter().any(|n| n.eq_ignore_ascii_case(&p.name)))
        .map(|p| p.pid)
        .collect()
}

pub fn is_running() -> bool {
    !player_pids().is_empty()
}

pub fn player_path() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let mut cands = vec![];
    for base in [
        std::env::var("LOCALAPPDATA").ok(),
        std::env::var("ProgramFiles(x86)").ok(),
        std::env::var("ProgramFiles").ok(),
    ]
    .into_iter()
    .flatten()
    {
        let versions = PathBuf::from(base).join("Roblox").join("Versions");
        if let Ok(rd) = std::fs::read_dir(&versions) {
            for e in rd.flatten() {
                let exe = e.path().join("RobloxPlayerBeta.exe");
                if exe.exists() {
                    cands.push(exe);
                }
            }
        }
    }
    cands
        .into_iter()
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

pub fn protocol_registered() -> bool {
    if !cfg!(windows) {
        return false;
    }
    [
        "HKCU\\Software\\Classes\\roblox-player",
        "HKCR\\roblox-player",
        "HKCU\\Software\\Classes\\roblox",
        "HKCR\\roblox",
    ]
    .iter()
    .any(|k| {
        crate::util::run_capture("reg", &["query", k], Duration::from_secs(10))
            .is_ok_and(|(c, _)| c == 0)
    })
}

pub fn is_installed() -> bool {
    player_path().is_some() || protocol_registered()
}

pub fn kill_all() -> usize {
    let n = proc::kill_by_name(&ALL_EXES);
    if n > 0 {
        crate::info!("closed {n} Roblox process(es)");
    }
    n
}

/// Join a place using the logged-in Roblox app's own session (ROX-AI never touches cookies).
pub fn launch(place_id: u64) -> Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        for url in [
            format!("roblox://experiences/start?placeId={place_id}"),
            format!("roblox://placeId={place_id}"),
        ] {
            let op = super::wide("open");
            let u = super::wide(&url);
            let r = unsafe {
                ShellExecuteW(
                    std::ptr::null_mut(),
                    op.as_ptr(),
                    u.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    SW_SHOWNORMAL,
                )
            };
            if r as isize > 32 {
                crate::info!("launched {url}");
                return Ok(());
            }
        }
        bail!("Could not launch Roblox. Is Roblox installed?")
    }
    #[cfg(not(windows))]
    {
        let _ = place_id;
        bail!("Roblox can only be launched on Windows")
    }
}

pub fn find_window() -> Option<Hwnd> {
    let pids = player_pids();
    if pids.is_empty() {
        return None;
    }
    window::find_windows_for_pids(&pids)
        .into_iter()
        .filter_map(|h| window::client_rect(h).map(|r| (h, r)))
        .filter(|(_, r)| r.width > 200)
        .max_by_key(|(_, r)| r.width as i64 * r.height as i64)
        .map(|(h, _)| h)
}

/// Wait for the game window (Roblox may first auto-update, which can take a while).
pub fn wait_for_window(timeout: Duration, abort: &dyn Fn() -> Result<()>) -> Result<Option<Hwnd>> {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        abort()?;
        if let Some(h) = find_window() {
            return Ok(Some(h));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    Ok(None)
}
