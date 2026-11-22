#![cfg_attr(windows, windows_subsystem = "windows")]

mod core;
mod rec;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

#[tauri::command]
async fn api(cmd: String, args: serde_json::Value) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || core::call(&cmd, args).map_err(|e| format!("{e:#}")))
        .await
        .map_err(|e| e.to_string())?
}

/// The guide that floats over Roblox while recording. It is hidden from screen capture, so it never shows in a video.
#[tauri::command]
async fn overlay_open(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let url = format!("overlay.html?id={id}");
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.eval(&format!("window.location.replace('{url}')"));
        let _ = w.show();
        let _ = w.set_focus();
    } else {
        let w = WebviewWindowBuilder::new(&app, "overlay", WebviewUrl::App(url.into()))
            .title("ROX Research guide")
            .inner_size(380.0, 640.0)
            .min_inner_size(320.0, 200.0)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(false)
            .resizable(true)
            .shadow(true)
            .build()
            .map_err(|e| e.to_string())?;
        // top-right corner of the screen
        if let Ok(Some(m)) = w.current_monitor() {
            let size = m.size();
            let scale = m.scale_factor();
            let x = size.width as f64 / scale - 380.0 - 24.0;
            let _ = w.set_position(tauri::LogicalPosition::new(x.max(0.0), 80.0));
        }
        // checks can photograph the guide; normally it never shows in a video
        if std::env::var("ROXRESEARCH_GUIDE_VISIBLE").is_err() {
            hide_from_capture(&w);
        }
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.minimize();
    }
    Ok(())
}

#[tauri::command]
async fn overlay_close(app: tauri::AppHandle, show_main: bool, route: String) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.close();
    }
    if show_main {
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.unminimize();
            let _ = main.show();
            let _ = main.set_focus();
            if !route.is_empty() {
                let _ = main.eval(&format!("window.location.hash = '{}'", route.replace('\'', "")));
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn hide_from_capture(w: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};
    if let Ok(h) = w.hwnd() {
        unsafe {
            SetWindowDisplayAffinity(h.0 as _, WDA_EXCLUDEFROMCAPTURE);
        }
    }
}
#[cfg(not(windows))]
fn hide_from_capture(_w: &tauri::WebviewWindow) {}

#[cfg(windows)]
fn dark_frame(hwnd: *mut std::ffi::c_void) {
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE};
    let dark: i32 = 1;
    let border: u32 = 0x003B_2B25;
    unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE as _, &dark as *const i32 as _, 4);
        DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR as _, &border as *const u32 as _, 4);
    }
}

/// `ROX-Research.exe <command> [json args]` runs one engine command and prints the result (for checks and support).
fn cli(args: &[String]) -> i32 {
    let a: serde_json::Value = args.get(1).and_then(|s| serde_json::from_str(s).ok()).unwrap_or(serde_json::json!({}));
    match core::call(&args[0], a) {
        Ok(mut v) => {
            // background work (planning, teardown) must finish before this process exits
            if let Some(id) = v.get("id").and_then(|x| x.as_str()).map(String::from).or_else(|| args.get(1).and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()).and_then(|j| j["id"].as_str().map(String::from)).filter(|_| args[0] == "push")) {
                while let Ok(s) = core::load(&id) {
                    if s.status != "planning" && s.status != "analyzing" {
                        v = serde_json::to_value(&s).unwrap_or(v);
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            0
        }
        Err(e) => {
            eprintln!("ERROR: {e:#}");
            1
        }
    }
}

fn main() {
    core::startup();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        std::process::exit(cli(&args));
    }
    // get the video recorder ready in the background the first time
    std::thread::spawn(|| {
        if core::ffmpeg_path().is_none() {
            if let Err(e) = core::ensure_ffmpeg() {
                roxai::warn!("ffmpeg setup: {e:#}");
            }
        }
    });
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![api, overlay_open, overlay_close])
        .setup(|_app| {
            #[cfg(windows)]
            if let Some(w) = _app.get_webview_window("main") {
                if let Ok(h) = w.hwnd() {
                    dark_frame(h.0 as _);
                }
            }
            // checks: open on a given page (and the guide for a session) without clicking
            if let Ok(route) = std::env::var("ROXRESEARCH_ROUTE") {
                let app = _app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.eval(&format!("window.location.hash = '{}'", route.replace('\'', "")));
                    }
                    if let Ok(id) = std::env::var("ROXRESEARCH_OVERLAY") {
                        let _ = tauri::async_runtime::block_on(overlay_open(app.clone(), id));
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start ROX Research");
}
