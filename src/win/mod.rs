//! Windows-only game control. Every module compiles everywhere: on other OSes the functions are
//! harmless stubs (so scans, the vault and tests work on Linux/macOS).

pub mod capture;
pub mod humanmon;
pub mod input;
pub mod proc;
pub mod roblox;
pub mod window;

/// Window handle as a plain integer (Send/Sync-friendly).
pub type Hwnd = isize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

#[cfg(windows)]
pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
