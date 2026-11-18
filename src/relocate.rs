//! "Where should ROX-AI live?" — the setup wizard's location step.
//! ROX-AI stays portable: moving it just copies the exe(s) + its data and vault to the folder you pick,
//! restarts from there and cleans up the old copy. Optional Desktop / Start-menu shortcuts.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const APP_FILES: [&str; 3] = ["ROX-AI.exe", "roxai.exe", "README.txt"];

/// Recommended home: Documents\ROX-AI (visible, easy to find, survives cleanups of Downloads).
pub fn suggested_folder() -> PathBuf {
    crate::config::home_dir().join("Documents").join("ROX-AI")
}

/// Running from Downloads, a temp folder or straight out of a ZIP? Then moving is recommended.
pub fn is_temporary(dir: &Path) -> bool {
    let p = dir.to_string_lossy().to_lowercase().replace('/', "\\");
    let temp = std::env::temp_dir().to_string_lossy().to_lowercase();
    p.contains("\\downloads")
        || p.starts_with(&temp)
        || p.contains("\\temp\\")
        || p.contains("\\temp1_")
        || p.contains(".zip")
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let (s, d) = (e.path(), to.join(e.file_name()));
        if s.is_dir() {
            copy_dir(&s, &d)?;
        } else {
            std::fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

/// Copy the app (and its data + vault, if they live beside it) into `to`. Returns the new ROX-AI.exe.
pub fn move_app(to: &Path) -> Result<PathBuf> {
    let from = crate::config::app_folder();
    if same(&from, to) {
        bail!("ROX-AI is already in that folder");
    }
    std::fs::create_dir_all(to).with_context(|| format!("can't create {}", to.display()))?;
    let me = std::env::current_exe()?;
    let mut copied_main = false;
    for f in APP_FILES {
        let src = from.join(f);
        if src.exists() {
            std::fs::copy(&src, to.join(f)).with_context(|| format!("can't copy {f}"))?;
            copied_main |= f == "ROX-AI.exe";
        }
    }
    if !copied_main {
        // single-file download under another name (e.g. "ROX-AI (1).exe")
        std::fs::copy(&me, to.join("ROX-AI.exe"))?;
    }
    for d in ["data", "ROX-AI Vault"] {
        let src = from.join(d);
        if src.is_dir() && !to.join(d).exists() {
            copy_dir(&src, &to.join(d)).with_context(|| format!("can't copy {d}"))?;
        }
    }
    Ok(to.join("ROX-AI.exe"))
}

fn same(a: &Path, b: &Path) -> bool {
    let n = |p: &Path| {
        std::fs::canonicalize(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .to_lowercase()
    };
    n(a) == n(b)
}

/// Called by the *new* copy after a move: delete what we copied out of the old folder.
pub fn finish_move(old: &Path) {
    std::thread::sleep(Duration::from_secs(2)); // let the old process exit
    for _ in 0..10 {
        let mut left = false;
        for f in APP_FILES {
            let p = old.join(f);
            if p.exists() && std::fs::remove_file(&p).is_err() {
                left = true;
            }
        }
        if let Ok(rd) = std::fs::read_dir(old) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if n.starts_with("ROX-AI") && n.ends_with(".exe") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        for d in ["data", "ROX-AI Vault"] {
            let p = old.join(d);
            if p.exists() && std::fs::remove_dir_all(&p).is_err() {
                left = true;
            }
        }
        if !left {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    let _ = std::fs::remove_dir(old); // only if now empty
}

/// Desktop and/or Start-menu shortcuts to ROX-AI.exe (created silently).
pub fn create_shortcuts(exe: &Path, desktop: bool, start_menu: bool) -> Result<()> {
    if !cfg!(windows) {
        return Ok(());
    }
    let mut targets = vec![];
    if desktop {
        targets.push("[Environment]::GetFolderPath('Desktop')".to_string());
    }
    if start_menu {
        targets.push("[Environment]::GetFolderPath('Programs')".to_string());
    }
    let q = |s: &str| s.replace('\'', "''");
    for t in targets {
        let ps = format!(
            "$s=(New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path ({t}) 'ROX-AI.lnk'));$s.TargetPath='{}';$s.WorkingDirectory='{}';$s.IconLocation='{},0';$s.Description='ROX-AI';$s.Save()",
            q(&exe.to_string_lossy()),
            q(&exe.parent().unwrap_or(Path::new(".")).to_string_lossy()),
            q(&exe.to_string_lossy())
        );
        crate::util::run_capture(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &ps,
            ],
            Duration::from_secs(30),
        )?;
    }
    Ok(())
}

pub fn remove_shortcuts() {
    let home = crate::config::home_dir();
    let _ = std::fs::remove_file(home.join("Desktop").join("ROX-AI.lnk"));
    if let Some(a) = std::env::var_os("APPDATA") {
        let _ = std::fs::remove_file(
            PathBuf::from(a).join("Microsoft/Windows/Start Menu/Programs/ROX-AI.lnk"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn temp_detection() {
        assert!(is_temporary(Path::new("C:\\Users\\a\\Downloads\\ROX-AI")));
        assert!(!is_temporary(Path::new("C:\\Users\\a\\Documents\\ROX-AI")));
    }
}
