//! Same program as `roxai`, built without a console window: used by scheduled tasks, shortcuts and the panel
//! so nothing flashes on screen.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    std::process::exit(roxai::cli::run());
}
