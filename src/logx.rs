//! Tiny logger: rotating file in the app folder + warnings to stderr.
//! Never writes to stdout (stdout is the MCP channel).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

static FILE: Mutex<Option<File>> = Mutex::new(None);

pub fn init(logs_dir: &Path) {
    let _ = std::fs::create_dir_all(logs_dir);
    let path = logs_dir.join("roxai.log");
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 2_000_000 {
            for i in (1..5).rev() {
                let _ = std::fs::rename(
                    logs_dir.join(format!("roxai.log.{i}")),
                    logs_dir.join(format!("roxai.log.{}", i + 1)),
                );
            }
            let _ = std::fs::rename(&path, logs_dir.join("roxai.log.1"));
        }
    }
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&path) {
        *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
    }
}

pub fn write(level: &str, msg: &str) {
    let line = format!(
        "{} {} [{}] {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        level,
        std::process::id(),
        msg
    );
    if let Ok(mut g) = FILE.lock() {
        if let Some(f) = g.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }
    if level != "INFO" {
        let _ = std::io::stderr().write_all(format!("roxai {level}: {msg}\n").as_bytes());
    }
}

#[macro_export]
macro_rules! info { ($($t:tt)*) => { $crate::logx::write("INFO", &format!($($t)*)) } }
#[macro_export]
macro_rules! warn { ($($t:tt)*) => { $crate::logx::write("WARN", &format!($($t)*)) } }
#[macro_export]
macro_rules! error { ($($t:tt)*) => { $crate::logx::write("ERROR", &format!($($t)*)) } }
