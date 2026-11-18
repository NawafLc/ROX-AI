#![cfg_attr(not(windows), allow(dead_code))]
//! Keyboard/mouse injection for Roblox via Win32 SendInput.
//!
//! Roblox reads raw input, so keys are sent as *scan codes* and camera turns as *relative* mouse
//! moves — that's what makes them register in-game. Every pressed key/button is tracked so
//! `release_all()` guarantees nothing stays stuck after an abort or error.

use anyhow::{bail, Result};
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Marker in dwExtraInfo so our own input is recognisable.
pub const TAG: usize = 0x524F58; // "ROX"

/// Keys Claude may never send (could close the game or trigger OS shortcuts).
pub const FORBIDDEN: [&str; 5] = ["alt", "f4", "win", "lwin", "rwin"];

pub fn scan_code(key: &str) -> Option<(u16, bool)> {
    let k = norm_key(key);
    let ext = match k.as_str() {
        "up" => Some(0x48),
        "down" => Some(0x50),
        "left" => Some(0x4B),
        "right" => Some(0x4D),
        "home" => Some(0x47),
        "end" => Some(0x4F),
        "pageup" => Some(0x49),
        "pagedown" => Some(0x51),
        "insert" => Some(0x52),
        "delete" => Some(0x53),
        _ => None,
    };
    if let Some(c) = ext {
        return Some((c, true));
    }
    let c = match k.as_str() {
        "esc" => 0x01,
        "1" => 0x02,
        "2" => 0x03,
        "3" => 0x04,
        "4" => 0x05,
        "5" => 0x06,
        "6" => 0x07,
        "7" => 0x08,
        "8" => 0x09,
        "9" => 0x0A,
        "0" => 0x0B,
        "-" => 0x0C,
        "=" => 0x0D,
        "backspace" => 0x0E,
        "tab" => 0x0F,
        "q" => 0x10,
        "w" => 0x11,
        "e" => 0x12,
        "r" => 0x13,
        "t" => 0x14,
        "y" => 0x15,
        "u" => 0x16,
        "i" => 0x17,
        "o" => 0x18,
        "p" => 0x19,
        "[" => 0x1A,
        "]" => 0x1B,
        "enter" => 0x1C,
        "ctrl" => 0x1D,
        "a" => 0x1E,
        "s" => 0x1F,
        "d" => 0x20,
        "f" => 0x21,
        "g" => 0x22,
        "h" => 0x23,
        "j" => 0x24,
        "k" => 0x25,
        "l" => 0x26,
        ";" => 0x27,
        "'" => 0x28,
        "`" => 0x29,
        "shift" => 0x2A,
        "\\" => 0x2B,
        "z" => 0x2C,
        "x" => 0x2D,
        "c" => 0x2E,
        "v" => 0x2F,
        "b" => 0x30,
        "n" => 0x31,
        "m" => 0x32,
        "," => 0x33,
        "." => 0x34,
        "/" => 0x35,
        "rshift" => 0x36,
        "alt" => 0x38,
        "space" => 0x39,
        "capslock" => 0x3A,
        "f1" => 0x3B,
        "f2" => 0x3C,
        "f3" => 0x3D,
        "f4" => 0x3E,
        "f5" => 0x3F,
        "f6" => 0x40,
        "f7" => 0x41,
        "f8" => 0x42,
        "f9" => 0x43,
        "f10" => 0x44,
        "f11" => 0x57,
        "f12" => 0x58,
        _ => return None,
    };
    Some((c, false))
}

pub fn norm_key(k: &str) -> String {
    let k = if k == " " {
        "space".to_string()
    } else {
        k.trim().to_lowercase()
    };
    match k.as_str() {
        "return" => "enter",
        "escape" => "esc",
        "control" => "ctrl",
        "spacebar" | "jump" => "space",
        "lshift" | "sprint" => "shift",
        "pgup" => "pageup",
        "pgdn" => "pagedown",
        "del" => "delete",
        other => other,
    }
    .to_string()
}

/// Validate a key name (shared by all platforms so errors are consistent).
pub fn check_key(key: &str) -> Result<(u16, bool)> {
    let k = norm_key(key);
    if FORBIDDEN.contains(&k.as_str()) {
        bail!("key '{k}' is blocked for safety");
    }
    scan_code(&k).ok_or_else(|| anyhow::anyhow!("unknown key '{k}' (use letters, digits, space, shift, ctrl, enter, esc, tab, arrows, f1-f12…)"))
}

static PRESSED_KEYS: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static PRESSED_BTNS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn track(set: &Mutex<Option<HashSet<String>>>, k: &str, down: bool) {
    let mut g = set.lock().unwrap_or_else(|e| e.into_inner());
    let s = g.get_or_insert_with(HashSet::new);
    if down {
        s.insert(k.to_string());
    } else {
        s.remove(k);
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    pub fn send(inputs: &[INPUT]) -> Result<()> {
        let n = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if n as usize != inputs.len() {
            bail!("SendInput was blocked (PC locked, or an admin/UAC window has focus)");
        }
        Ok(())
    }

    pub fn key_input(code: u16, ext: bool, up: bool) -> INPUT {
        let mut flags = KEYEVENTF_SCANCODE;
        if up {
            flags |= KEYEVENTF_KEYUP;
        }
        if ext {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: code,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: TAG,
                },
            },
        }
    }

    pub fn unicode_input(ch: u16, up: bool) -> INPUT {
        let flags = KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { 0 };
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: ch,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: TAG,
                },
            },
        }
    }

    pub fn mouse_input(dx: i32, dy: i32, data: i32, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data as u32,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: TAG,
                },
            },
        }
    }

    pub fn virtual_screen() -> (i32, i32, i32, i32) {
        unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        }
    }

    pub fn btn_flags(btn: &str) -> Option<(u32, u32)> {
        match btn {
            "left" => Some((MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP)),
            "right" => Some((MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP)),
            "middle" => Some((MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP)),
            _ => None,
        }
    }

    pub const MOVE: u32 = MOUSEEVENTF_MOVE;
    pub const ABS: u32 = MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
    pub const WHEEL: u32 = MOUSEEVENTF_WHEEL;
}

#[cfg(windows)]
pub fn key(key: &str, down: bool) -> Result<()> {
    let (c, ext) = check_key(key)?;
    imp::send(&[imp::key_input(c, ext, !down)])?;
    track(&PRESSED_KEYS, &norm_key(key), down);
    Ok(())
}

#[cfg(not(windows))]
pub fn key(key: &str, _down: bool) -> Result<()> {
    check_key(key)?;
    bail!("input control only works on Windows")
}

pub fn tap(k: &str, hold: f64) -> Result<()> {
    key(k, true)?;
    std::thread::sleep(Duration::from_secs_f64(hold.clamp(0.01, 15.0)));
    key(k, false)
}

/// Hold several keys together (e.g. w+shift) for `secs`, polling `check` so aborts are instant.
pub fn hold_keys(keys: &[String], secs: f64, check: &dyn Fn() -> Result<()>) -> Result<()> {
    for k in keys {
        check_key(k)?;
    }
    let mut pressed = vec![];
    let res = (|| {
        for k in keys {
            key(k, true)?;
            pressed.push(k.clone());
        }
        let end = Instant::now() + Duration::from_secs_f64(secs.clamp(0.0, 30.0));
        while Instant::now() < end {
            check()?;
            std::thread::sleep(Duration::from_millis(40));
        }
        Ok(())
    })();
    for k in pressed.iter().rev() {
        let _ = key(k, false);
    }
    res
}

#[cfg(windows)]
pub fn type_text(text: &str) -> Result<()> {
    for ch in text.encode_utf16() {
        if ch == '\n' as u16 {
            tap("enter", 0.05)?;
            continue;
        }
        imp::send(&[imp::unicode_input(ch, false), imp::unicode_input(ch, true)])?;
        std::thread::sleep(Duration::from_millis(12));
    }
    Ok(())
}

#[cfg(windows)]
pub fn move_abs(x: i32, y: i32) -> Result<()> {
    let (vx, vy, vw, vh) = imp::virtual_screen();
    let nx = ((x - vx) as f64 * 65535.0 / (vw - 1).max(1) as f64).round() as i32;
    let ny = ((y - vy) as f64 * 65535.0 / (vh - 1).max(1) as f64).round() as i32;
    imp::send(&[imp::mouse_input(nx, ny, 0, imp::MOVE | imp::ABS)])
}

#[cfg(windows)]
pub fn move_rel(dx: i32, dy: i32) -> Result<()> {
    imp::send(&[imp::mouse_input(dx, dy, 0, imp::MOVE)])
}

#[cfg(windows)]
pub fn button(btn: &str, down: bool) -> Result<()> {
    let (d, u) =
        imp::btn_flags(btn).ok_or_else(|| anyhow::anyhow!("button must be left/right/middle"))?;
    imp::send(&[imp::mouse_input(0, 0, 0, if down { d } else { u })])?;
    track(&PRESSED_BTNS, btn, down);
    Ok(())
}

#[cfg(windows)]
pub fn wheel(clicks: i32) -> Result<()> {
    imp::send(&[imp::mouse_input(
        0,
        0,
        clicks.clamp(-30, 30) * 120,
        imp::WHEEL,
    )])
}

#[cfg(not(windows))]
pub fn type_text(_t: &str) -> Result<()> {
    bail!("input control only works on Windows")
}
#[cfg(not(windows))]
pub fn move_abs(_x: i32, _y: i32) -> Result<()> {
    bail!("input control only works on Windows")
}
#[cfg(not(windows))]
pub fn move_rel(_x: i32, _y: i32) -> Result<()> {
    bail!("input control only works on Windows")
}
#[cfg(not(windows))]
pub fn button(_b: &str, _d: bool) -> Result<()> {
    bail!("input control only works on Windows")
}
#[cfg(not(windows))]
pub fn wheel(_c: i32) -> Result<()> {
    bail!("input control only works on Windows")
}

pub fn click_at(x: i32, y: i32, btn: &str, double: bool, hold: f64) -> Result<()> {
    move_abs(x, y)?;
    std::thread::sleep(Duration::from_millis(50));
    // Roblox only registers hover/click on GUI after a real move event
    move_rel(1, 0)?;
    std::thread::sleep(Duration::from_millis(30));
    move_rel(-1, 0)?;
    std::thread::sleep(Duration::from_millis(50));
    for i in 0..(if double { 2 } else { 1 }) {
        button(btn, true)?;
        std::thread::sleep(Duration::from_secs_f64(hold.clamp(0.02, 15.0)));
        button(btn, false)?;
        if double && i == 0 {
            std::thread::sleep(Duration::from_millis(80));
        }
    }
    Ok(())
}

/// Right-drag = rotate the camera in Roblox.
pub fn drag_camera(dx: i32, dy: i32) -> Result<()> {
    button("right", true)?;
    let steps = 20;
    let res = (|| {
        let (mut ax, mut ay) = (0f64, 0f64);
        for _ in 0..steps {
            ax += dx as f64 / steps as f64;
            ay += dy as f64 / steps as f64;
            let (mx, my) = (ax.trunc() as i32, ay.trunc() as i32);
            ax -= mx as f64;
            ay -= my as f64;
            if mx != 0 || my != 0 {
                move_rel(mx, my)?;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    let _ = button("right", false);
    res
}

/// Fail-safe: let go of every key/button we might be holding.
pub fn release_all() {
    #[cfg(windows)]
    {
        let keys: Vec<String> = PRESSED_KEYS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .map(|s| s.into_iter().collect())
            .unwrap_or_default();
        for k in keys
            .iter()
            .map(|s| s.as_str())
            .chain(["w", "a", "s", "d", "space", "shift", "ctrl"])
        {
            if let Some((c, ext)) = scan_code(k) {
                let _ = imp::send(&[imp::key_input(c, ext, true)]);
            }
        }
        let _ = PRESSED_BTNS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        for b in ["left", "right", "middle"] {
            if let Some((_, up)) = imp::btn_flags(b) {
                let _ = imp::send(&[imp::mouse_input(0, 0, 0, up)]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys() {
        assert!(check_key("alt").is_err());
        assert!(check_key("F4").is_err());
        assert_eq!(check_key("W").unwrap(), (0x11, false));
        assert_eq!(check_key("jump").unwrap(), (0x39, false));
        assert_eq!(check_key("up").unwrap(), (0x48, true));
        assert!(check_key("banana").is_err());
    }
}
