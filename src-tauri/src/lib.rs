mod commands;
mod config;
mod models;
mod providers;
mod scheduler;
mod secrets;
mod tray;

use std::sync::Arc;
use tauri::Manager;
use tokio::sync::{Notify, RwLock};

use commands::{get_config, get_config_health, get_usage, save_config, trigger_refresh};
use models::UsageSummary;
use scheduler::Scheduler;
use tauri_plugin_log::{Target, TargetKind};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 日志落地：未安装 logger 时 log crate 走 NopLogger（max_level = Off），
        // release 构建下迁移失败 / 解密失败 / 指纹降级等诊断信息会全部丢失。
        // 写入应用日志目录 + 标准输出；日志内容严禁包含密钥、明文、指纹值与密文。
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .targets([
                    Target::new(TargetKind::LogDir { file_name: None }),
                    Target::new(TargetKind::Stdout),
                ])
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .setup(|app| {
            tray::create_tray(app.handle())?;
            let (app_config, config_health) = config::load(app.handle());
            let config_state = Arc::new(RwLock::new(app_config));
            let summary_state = Arc::new(RwLock::new(UsageSummary::default()));
            let health_state = Arc::new(RwLock::new(config_health));
            let notify = Arc::new(Notify::new());
            app.manage(config_state.clone());
            app.manage(summary_state.clone());
            app.manage(health_state.clone());
            app.manage(notify.clone());
            let sched = Scheduler::new_with_notify(
                app.handle().clone(),
                config_state,
                summary_state,
                notify,
            );
            sched.start();
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_usage,
            get_config,
            get_config_health,
            save_config,
            trigger_refresh
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
