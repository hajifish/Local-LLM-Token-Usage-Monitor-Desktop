use crate::config::{self, ConfigHealth};
use crate::models::{AppConfig, UsageSummary};
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::{Notify, RwLock};

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

/// 返回本次启动加载配置时的健康状态，供前端在设置页明确提示用户。
///
/// 字段名为 snake_case，与 `src/types.ts` 中的 `ConfigHealth` 严格一致。
#[tauri::command]
pub async fn get_config_health(
    health_state: tauri::State<'_, Arc<RwLock<ConfigHealth>>>,
) -> Result<ConfigHealth, String> {
    let h = health_state.read().await;
    Ok(h.clone())
}

/// 通知调度器立即执行一次供应商轮询，用于前端刷新按钮。
#[tauri::command]
pub async fn trigger_refresh(notify: tauri::State<'_, Arc<Notify>>) -> Result<(), String> {
    notify.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn save_config(
    app: AppHandle,
    config_state: tauri::State<'_, Arc<RwLock<AppConfig>>>,
    health_state: tauri::State<'_, Arc<RwLock<ConfigHealth>>>,
    notify: tauri::State<'_, Arc<Notify>>,
    new_config: AppConfig,
) -> Result<(), String> {
    log::info!("Saving configuration");
    // 读取本次会话的加载健康状态：unreadable 为真时，config::save 会在覆盖前先备份原密文，
    // 避免用户随手一次保存就永久销毁本机无法解密的全部密钥。
    // 已备份过（backup_path 非空）则不再重复备份：此时磁盘上已是本次会话新写入的可读信封，
    // 再备份只会产生多余的 .bak；unreadable 本身保持为真，以便前端持续展示备份文件名。
    let need_backup = {
        let h = health_state.read().await;
        h.unreadable && h.backup_path.is_none()
    };
    let backup_path = config::save(&app, &new_config, need_backup)?;
    if let Some(path) = backup_path {
        log::warn!("Config: 已在覆盖前备份无法解密的原配置文件（后果：原密钥仍可从备份恢复）");
        let mut h = health_state.write().await;
        h.backup_path = Some(path);
        drop(h);
    }
    let mut c = config_state.write().await;
    *c = new_config;
    drop(c);
    // 通知调度器立即轮询
    notify.notify_one();
    log::info!("Configuration saved, triggering immediate refresh");
    Ok(())
}
