#![cfg_attr(windows, windows_subsystem = "windows")]

#[tauri::command]
async fn api(cmd: String, args: serde_json::Value) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || roxai::ui_api::call(&cmd, args).map_err(|e| format!("{e:#}")))
        .await
        .map_err(|e| e.to_string())?
}

/// Dark window frame: dark mode for the system parts of the window, and a border that matches the app.
#[cfg(windows)]
fn dark_frame(hwnd: *mut core::ffi::c_void) {
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE};
    let dark: i32 = 1;
    // COLORREF is 0x00BBGGRR — #252B3B, the app's line colour
    let border: u32 = 0x003B_2B25;
    unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE as _, &dark as *const i32 as _, 4);
        DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR as _, &border as *const u32 as _, 4);
    }
}

fn main() {
    // With arguments ROX-AI.exe is the command line / MCP server, exactly like roxai.exe.
    if std::env::args().len() > 1 {
        std::process::exit(roxai::cli::run());
    }
    roxai::ui_api::startup();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![api])
        .setup(|_app| {
            #[cfg(windows)]
            {
                use tauri::Manager;
                if let Some(w) = _app.get_webview_window("main") {
                    if let Ok(h) = w.hwnd() {
                        dark_frame(h.0 as _);
                    }
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start ROX-AI");
}
