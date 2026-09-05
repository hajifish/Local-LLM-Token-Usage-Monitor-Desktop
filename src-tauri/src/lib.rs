mod commands;
mod config;
mod models;
mod providers;
mod scheduler;
mod tray;

use std::sync::Arc;
use tokio::sync::{RwLock, Notify};
use tauri::Manager;

use commands::{get_config, get_usage, save_config};
use models::UsageSummary;
use scheduler::Scheduler;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .setup(|app| {
            tray::create_tray(app.handle())?;
            let app_config = config::load(app.handle());
            let config_state = Arc::new(RwLock::new(app_config));
            let summary_state = Arc::new(RwLock::new(UsageSummary::default()));
            let notify = Arc::new(Notify::new());
            app.manage(config_state.clone());
            app.manage(summary_state.clone());
            app.manage(notify.clone());
            let sched = Scheduler::new_with_notify(
                app.handle().clone(), config_state, summary_state, notify,
            );
            sched.start();
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_usage, get_config, save_config])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
