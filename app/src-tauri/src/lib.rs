//! P56 Studio — Tauri backend. Every UI command goes through `p56-service`, the same
//! layer the Home Assistant add-on server uses.

use std::sync::Arc;

use p56_service::{Defaults, Service};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
async fn p56(state: State<'_, Arc<Service>>, cmd: String, args: Option<Value>) -> Result<Value, String> {
    let svc = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || svc.call(&cmd, &args.unwrap_or(Value::Null))).await.map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).try_init();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle: AppHandle = app.handle().clone();
            let emit: p56_service::Emit = Arc::new(move |t: &str, data: Value| {
                let _ = handle.emit("p56", serde_json::json!({ "type": t, "data": data }));
            });
            // previews of uploaded files survive a restart in the app's data folder
            let thumb_dir = app.path().app_data_dir().ok().map(|d| d.join("thumbs"));
            // P56_FAN_ALLOW=<ip>[,<ip>…]: accept fans from these addresses only (tests use it so a
            // real fan on the network never joins a test run)
            let allow_fans = std::env::var("P56_FAN_ALLOW").map(|v| v.split(',').filter_map(|a| a.trim().parse().ok()).collect()).unwrap_or_default();
            app.manage(Service::new(emit, Defaults { thumb_dir, allow_fans, ..Defaults::default() }));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![p56])
        .run(tauri::generate_context!())
        .expect("error while running P56 Studio");
}
