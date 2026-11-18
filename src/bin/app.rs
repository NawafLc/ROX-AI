//! ROX-AI.exe — the app you double-click. Same program as `roxai`, built without a console window,
//! so nothing ever flashes on screen (also what the background schedule runs).
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    std::process::exit(roxai::cli::run());
}
