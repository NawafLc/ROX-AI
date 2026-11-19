//! ROX-AI.exe — the app you double-click.
//! With arguments it runs as the background worker (scans, learning, schedule), with no window at all.
//! Without arguments it opens the app window.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[tauri::command]
async fn api(cmd: String, args: serde_json::Value) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || roxai::ui_api::call(&cmd, args).map_err(|e| format!("{e:#}")))
        .await
        .map_err(|e| e.to_string())?
}

fn main() {
    if std::env::args().len() > 1 {
        std::process::exit(roxai::cli::run());
    }
    roxai::ui_api::startup();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![api])
        .run(tauri::generate_context!())
        .expect("failed to start ROX-AI");
}
