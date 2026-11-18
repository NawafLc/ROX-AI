//! `roxai selftest`: exercises the real Win32 code paths (run in CI on windows-latest, or by you).
//! Exit 0 = all hard checks passed. Soft checks (things a headless CI desktop may not allow) print WARN.

use crate::config::Ctx;
use crate::win::{capture, humanmon, input as IN, proc, roblox as RB, window as W, Rect};
#[allow(unused_imports)]
use anyhow::{anyhow, bail, Result};
use std::time::Duration;

pub fn run(ctx: &Ctx) -> i32 {
    let mut fails = vec![];
    let mut check = |name: &str, hard: bool, f: &dyn Fn() -> Result<String>| match f() {
        Ok(r) => println!("PASS {name}: {r}"),
        Err(e) => {
            println!("{} {name}: {e:#}", if hard { "FAIL" } else { "WARN" });
            if hard {
                fails.push(name.to_string());
            }
        }
    };
    W::make_dpi_aware();
    check("screen size", true, &|| {
        Ok(format!("{:?}", W::screen_size()))
    });
    check("idle seconds", true, &|| {
        Ok(format!("{:.1}", W::idle_seconds()))
    });
    check("is_locked", true, &|| Ok(W::is_locked().to_string()));
    check("process list", true, &|| {
        let n = proc::list().len();
        if n == 0 {
            bail!("no processes listed")
        } else {
            Ok(format!("{n} processes"))
        }
    });
    check("pid_exists(self)", true, &|| {
        if proc::pid_exists(std::process::id()) {
            Ok("yes".into())
        } else {
            bail!("own pid not found")
        }
    });
    check("roblox find_window", true, &|| {
        Ok(format!("{:?}", RB::find_window()))
    });
    check("roblox installed?", false, &|| {
        Ok(RB::is_installed().to_string())
    });
    check("hotkey poll", true, &|| {
        Ok(W::hotkey_down("ctrl+shift+q").to_string())
    });
    check("forbidden keys blocked", true, &|| {
        if IN::check_key("alt").is_err() && IN::check_key("f4").is_err() {
            Ok("blocked".into())
        } else {
            bail!("not blocked")
        }
    });
    check("human input monitor", false, &|| {
        humanmon::start();
        std::thread::sleep(Duration::from_millis(800));
        let ok = humanmon::hooks_ok();
        humanmon::stop();
        if ok {
            Ok("hooks installed".into())
        } else {
            bail!("hooks not installed")
        }
    });
    check("screen capture", false, &|| {
        let (w, h) = W::screen_size();
        let shot = capture::grab(
            Rect {
                left: 0,
                top: 0,
                width: w.min(1600),
                height: h.min(900),
            },
            800,
            60,
            true,
        )?;
        let out = std::env::var("RUNNER_TEMP")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir())
            .join("roxai-capture.jpg");
        std::fs::write(&out, &shot.jpeg)?;
        Ok(format!(
            "{}x{} scale={:.2} bright={:.0} bytes={}",
            shot.width,
            shot.height,
            shot.scale,
            shot.brightness,
            shot.jpeg.len()
        ))
    });
    check("SendInput typing into Notepad", false, &notepad_typing);
    check("Task Scheduler create/query/delete", false, &|| {
        if !cfg!(windows) {
            bail!("not Windows");
        }
        let xml = crate::setup::task_xml(ctx, "ci", "03:33", "doctor", "PT5M", "PT1M", "");
        let r = crate::setup::create_task(ctx, "CI Test Task", &xml);
        let st = crate::setup::task_status("CI Test Task");
        let _ = crate::util::run_capture(
            "schtasks",
            &["/Delete", "/TN", "ROX-AI\\CI Test Task", "/F"],
            Duration::from_secs(30),
        );
        if r != "ok" {
            bail!(r)
        } else {
            Ok(st.unwrap_or_default())
        }
    });
    check("icon generation", true, &|| {
        Ok(crate::setup::make_icon(ctx)?.to_string_lossy().into_owned())
    });
    println!(
        "\nHARD FAILURES: {}",
        if fails.is_empty() {
            "none".to_string()
        } else {
            fails.join(", ")
        }
    );
    i32::from(!fails.is_empty())
}

#[cfg(windows)]
fn notepad_typing() -> Result<String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, SendMessageW, WM_GETTEXT};
    let mut child = std::process::Command::new("notepad.exe").spawn()?;
    let res = (|| {
        let mut hwnd = None;
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(200));
            if let Some(h) = W::find_windows_for_pids(&[child.id()]).into_iter().next() {
                hwnd = Some(h);
                break;
            }
        }
        let h = hwnd.ok_or_else(|| anyhow!("notepad window not found"))?;
        W::focus(h);
        std::thread::sleep(Duration::from_millis(500));
        IN::type_text("rox ai 123")?;
        IN::tap("space", 0.05)?;
        for k in ["w", "a", "s", "d"] {
            IN::tap(k, 0.05)?;
        }
        std::thread::sleep(Duration::from_millis(500));
        IN::release_all();
        let cls: Vec<u16> = "Edit\0".encode_utf16().collect();
        let edit =
            unsafe { FindWindowExW(h as _, std::ptr::null_mut(), cls.as_ptr(), std::ptr::null()) };
        let mut buf = [0u16; 256];
        unsafe { SendMessageW(edit, WM_GETTEXT, 256, buf.as_mut_ptr() as isize) };
        let txt = String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(0)]);
        if !txt.contains("rox ai 123") {
            bail!("typed text not found, got {txt:?}");
        }
        if !txt.contains("wasd") {
            bail!("scan-code keys not received, got {txt:?}");
        }
        Ok(format!("{txt:?}"))
    })();
    let _ = child.kill();
    res
}

#[cfg(not(windows))]
fn notepad_typing() -> Result<String> {
    bail!("not Windows")
}
