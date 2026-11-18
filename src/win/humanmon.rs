#![cfg_attr(not(windows), allow(dead_code))]
//! Detects *real* (physical) keyboard/mouse input while the AI is playing.
//!
//! Low-level Windows hooks: input we inject carries the INJECTED flag, so anything without it came
//! from a human. The moment you touch your mouse or keyboard during a playtest, ROX-AI lets go of
//! all controls and stops — you are always in charge.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

static BASE: OnceLock<Instant> = OnceLock::new();
static LAST_HUMAN_MS: AtomicU64 = AtomicU64::new(0);
static MOVED: AtomicI64 = AtomicI64::new(0);
static LAST_X: AtomicI32 = AtomicI32::new(i32::MIN);
static LAST_Y: AtomicI32 = AtomicI32::new(0);
static THREAD: AtomicU32 = AtomicU32::new(0);
static OK: AtomicBool = AtomicBool::new(false);
const MOUSE_THRESHOLD: i64 = 60;

pub fn now_ms() -> u64 {
    BASE.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

/// Has a human touched mouse/keyboard after `since_ms` (a value from `now_ms()`)?
pub fn human_since(since_ms: u64) -> bool {
    LAST_HUMAN_MS.load(Ordering::Relaxed) > since_ms
}

pub fn hooks_ok() -> bool {
    OK.load(Ordering::Relaxed)
}

fn mark() {
    LAST_HUMAN_MS.store(now_ms(), Ordering::Relaxed);
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    unsafe extern "system" fn kb(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code == 0 {
            let k = &*(lp as *const KBDLLHOOKSTRUCT);
            if k.flags & LLKHF_INJECTED == 0 {
                mark();
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, wp, lp)
    }

    unsafe extern "system" fn ms(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code == 0 {
            let m = &*(lp as *const MSLLHOOKSTRUCT);
            if m.flags & LLMHF_INJECTED == 0 {
                if wp as u32 == WM_MOUSEMOVE {
                    let lx = LAST_X.swap(m.pt.x, Ordering::Relaxed);
                    let ly = LAST_Y.swap(m.pt.y, Ordering::Relaxed);
                    if lx != i32::MIN {
                        let d = ((m.pt.x - lx).abs() + (m.pt.y - ly).abs()) as i64;
                        if MOVED.fetch_add(d, Ordering::Relaxed) + d >= MOUSE_THRESHOLD {
                            MOVED.store(0, Ordering::Relaxed);
                            mark();
                        }
                    }
                } else {
                    mark();
                }
            } else {
                LAST_X.store(i32::MIN, Ordering::Relaxed);
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, wp, lp)
    }

    pub fn start() {
        if THREAD.load(Ordering::Relaxed) != 0 {
            return;
        }
        std::thread::spawn(|| unsafe {
            THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);
            let hk = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb), std::ptr::null_mut(), 0);
            let hm = SetWindowsHookExW(WH_MOUSE_LL, Some(ms), std::ptr::null_mut(), 0);
            OK.store(!hk.is_null() && !hm.is_null(), Ordering::Relaxed);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if !hk.is_null() {
                UnhookWindowsHookEx(hk);
            }
            if !hm.is_null() {
                UnhookWindowsHookEx(hm);
            }
            OK.store(false, Ordering::Relaxed);
            THREAD.store(0, Ordering::Relaxed);
        });
    }

    pub fn stop() {
        let t = THREAD.load(Ordering::Relaxed);
        if t != 0 {
            unsafe { PostThreadMessageW(t, WM_QUIT, 0, 0) };
        }
    }
}

#[cfg(windows)]
pub use imp::{start, stop};

#[cfg(not(windows))]
pub fn start() {
    let _ = (&MOVED, &LAST_X, &LAST_Y, &THREAD);
}
#[cfg(not(windows))]
pub fn stop() {}

#[cfg(test)]
mod tests {
    #[test]
    fn marks() {
        let t = super::now_ms();
        assert!(!super::human_since(t + 1_000_000));
        super::mark();
        assert!(super::human_since(t - 1));
    }
}
