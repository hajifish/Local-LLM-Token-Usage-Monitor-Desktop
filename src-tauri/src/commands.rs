use tauri::AppHandle;
use std::sync::Arc;
use tokio::sync::{RwLock, Notify};
use crate::models::{AppConfig, UsageSummary};
use crate::config;

#[tauri::command]
pub async fn get_usage(
    summary_state: tauri::State<'_, Arc<RwLock<UsageSummary>>>,
) -> Result<UsageSummary, String> {
    let s = summary_state.read().await;
    Ok(s.clone())
}

#[tauri::command]
pub async fn get_config(
    config_state: tauri::State<'_, Arc<RwLock<AppConfig>>>,
) -> Result<AppConfig, String> {
    let c = config_state.read().await;
    Ok(c.clone())
}

#[tauri::command]
pub async fn save_config(
    app: AppHandle,
    config_state: tauri::State<'_, Arc<RwLock<AppConfig>>>,
    notify: tauri::State<'_, Arc<Notify>>,
    new_config: AppConfig,
) -> Result<(), String> {
    config::save(&app, &new_config)?;
    let mut c = config_state.write().await;
    *c = new_config;
    drop(c);
    // 通知调度器立即轮询
    notify.notify_one();
    Ok(())
}
