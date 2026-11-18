//! Win32 window helpers: find/focus/resize, idle & lock detection, hotkey polling, DPI awareness.

use super::{Hwnd, Rect};

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::System::StationsAndDesktops::{
        CloseDesktop, OpenInputDesktop, SwitchDesktop, DESKTOP_SWITCHDESKTOP,
    };
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, GetAsyncKeyState, GetLastInputInfo, SetFocus, KEYEVENTF_KEYUP, LASTINPUTINFO,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    fn h(x: Hwnd) -> HWND {
        x as HWND
    }

    pub fn make_dpi_aware() {
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
    }

    pub fn window_pid(hwnd: Hwnd) -> u32 {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h(hwnd), &mut pid) };
        pid
    }

    unsafe extern "system" fn enum_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        let v = &mut *(lp as *mut Vec<Hwnd>);
        if IsWindowVisible(hwnd) != 0 {
            v.push(hwnd as Hwnd);
        }
        1
    }

    pub fn visible_windows() -> Vec<Hwnd> {
        let mut v: Vec<Hwnd> = vec![];
        unsafe { EnumWindows(Some(enum_cb), &mut v as *mut Vec<Hwnd> as LPARAM) };
        v
    }

    pub fn title(hwnd: Hwnd) -> String {
        let mut buf = [0u16; 512];
        let n = unsafe { GetWindowTextW(h(hwnd), buf.as_mut_ptr(), 512) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    pub fn client_rect(hwnd: Hwnd) -> Option<Rect> {
        let mut r: RECT = unsafe { std::mem::zeroed() };
        if unsafe { GetClientRect(h(hwnd), &mut r) } == 0 {
            return None;
        }
        let mut p = POINT { x: 0, y: 0 };
        unsafe { ClientToScreen(h(hwnd), &mut p) };
        Some(Rect {
            left: p.x,
            top: p.y,
            width: r.right - r.left,
            height: r.bottom - r.top,
        })
    }

    pub fn is_minimized(hwnd: Hwnd) -> bool {
        unsafe { IsIconic(h(hwnd)) != 0 }
    }

    pub fn is_hung(hwnd: Hwnd) -> bool {
        unsafe { IsHungAppWindow(h(hwnd)) != 0 }
    }

    pub fn foreground() -> Hwnd {
        unsafe { GetForegroundWindow() as Hwnd }
    }

    pub fn focus(hwnd: Hwnd) -> bool {
        unsafe {
            ShowWindow(
                h(hwnd),
                if IsIconic(h(hwnd)) != 0 {
                    SW_RESTORE
                } else {
                    SW_SHOW
                },
            );
            if foreground() == hwnd {
                return true;
            }
            let cur = GetCurrentThreadId();
            let fg = GetForegroundWindow();
            let fg_t = if fg.is_null() {
                0
            } else {
                GetWindowThreadProcessId(fg, std::ptr::null_mut())
            };
            let tg_t = GetWindowThreadProcessId(h(hwnd), std::ptr::null_mut());
            let mut attached = vec![];
            for t in [fg_t, tg_t] {
                if t != 0 && t != cur && !attached.contains(&t) && AttachThreadInput(cur, t, 1) != 0
                {
                    attached.push(t);
                }
            }
            BringWindowToTop(h(hwnd));
            SetForegroundWindow(h(hwnd));
            SetFocus(h(hwnd));
            for t in attached {
                AttachThreadInput(cur, t, 0);
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
            if foreground() != hwnd {
                // a harmless synthetic Alt tap unlocks SetForegroundWindow
                keybd_event(0x12, 0, 0, 0);
                keybd_event(0x12, 0, KEYEVENTF_KEYUP, 0);
                SetForegroundWindow(h(hwnd));
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
            foreground() == hwnd
        }
    }

    pub fn screen_size() -> (i32, i32) {
        unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
    }

    pub fn resize_client(hwnd: Hwnd, w: i32, hgt: i32) {
        unsafe {
            let style = GetWindowLongW(h(hwnd), GWL_STYLE) as u32;
            let ex = GetWindowLongW(h(hwnd), GWL_EXSTYLE) as u32;
            let (sw, sh) = screen_size();
            let (w, hgt) = (w.min(sw - 40), hgt.min(sh - 80));
            let mut r = RECT {
                left: 0,
                top: 0,
                right: w,
                bottom: hgt,
            };
            AdjustWindowRectEx(&mut r, style, 0, ex);
            let (ww, wh) = (r.right - r.left, r.bottom - r.top);
            SetWindowPos(
                h(hwnd),
                std::ptr::null_mut(),
                ((sw - ww) / 2).max(0),
                ((sh - wh) / 2).max(0),
                ww,
                wh,
                SWP_NOZORDER | SWP_SHOWWINDOW,
            );
        }
    }

    pub fn idle_seconds() -> f64 {
        let mut li = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        unsafe {
            if GetLastInputInfo(&mut li) == 0 {
                return 0.0;
            }
            GetTickCount().wrapping_sub(li.dwTime) as f64 / 1000.0
        }
    }

    pub fn is_locked() -> bool {
        unsafe {
            let d = OpenInputDesktop(0, 0, DESKTOP_SWITCHDESKTOP);
            if d.is_null() {
                return true;
            }
            let ok = SwitchDesktop(d) != 0;
            CloseDesktop(d);
            !ok
        }
    }

    pub fn key_down(vk: i32) -> bool {
        unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
    }
}

#[cfg(windows)]
pub use imp::{
    client_rect, focus, foreground, idle_seconds, is_hung, is_locked, is_minimized, make_dpi_aware,
    resize_client, screen_size, title, visible_windows, window_pid,
};

#[cfg(windows)]
pub fn find_windows_for_pids(pids: &[u32]) -> Vec<Hwnd> {
    visible_windows()
        .into_iter()
        .filter(|h| pids.contains(&window_pid(*h)))
        .collect()
}

#[cfg(windows)]
pub fn is_fullscreen(hwnd: Hwnd) -> bool {
    let (w, h) = screen_size();
    client_rect(hwnd).map_or(false, |r| r.width >= w && r.height >= h)
}

/// Poll a hotkey like "ctrl+shift+q" (physical keyboard state).
pub fn hotkey_down(combo: &str) -> bool {
    #[cfg(windows)]
    {
        let parts: Vec<String> = combo
            .to_lowercase()
            .split('+')
            .map(|s| s.trim().to_string())
            .collect();
        if parts.is_empty() {
            return false;
        }
        for p in parts {
            let vk = match p.as_str() {
                "ctrl" => 0x11,
                "shift" => 0x10,
                "alt" => 0x12,
                "esc" => 0x1B,
                "pause" => 0x13,
                "end" => 0x23,
                f if f.starts_with('f') && f.len() <= 3 && f[1..].parse::<i32>().is_ok() => {
                    0x6F + f[1..].parse::<i32>().unwrap()
                }
                c if c.len() == 1 => c.to_uppercase().chars().next().unwrap() as i32,
                _ => return false,
            };
            if !imp::key_down(vk) {
                return false;
            }
        }
        true
    }
    #[cfg(not(windows))]
    {
        let _ = combo;
        false
    }
}

// ------------------------------------------------------------- stubs ---
#[cfg(not(windows))]
mod stubs {
    use super::*;
    pub fn make_dpi_aware() {}
    pub fn client_rect(_h: Hwnd) -> Option<Rect> {
        None
    }
    pub fn focus(_h: Hwnd) -> bool {
        false
    }
    pub fn foreground() -> Hwnd {
        0
    }
    pub fn idle_seconds() -> f64 {
        1e9
    }
    pub fn is_hung(_h: Hwnd) -> bool {
        false
    }
    pub fn is_locked() -> bool {
        false
    }
    pub fn is_minimized(_h: Hwnd) -> bool {
        false
    }
    pub fn resize_client(_h: Hwnd, _w: i32, _hh: i32) {}
    pub fn screen_size() -> (i32, i32) {
        (1920, 1080)
    }
    pub fn title(_h: Hwnd) -> String {
        String::new()
    }
    pub fn visible_windows() -> Vec<Hwnd> {
        vec![]
    }
    pub fn window_pid(_h: Hwnd) -> u32 {
        0
    }
    pub fn find_windows_for_pids(_p: &[u32]) -> Vec<Hwnd> {
        vec![]
    }
    pub fn is_fullscreen(_h: Hwnd) -> bool {
        false
    }
}
#[cfg(not(windows))]
pub use stubs::*;
