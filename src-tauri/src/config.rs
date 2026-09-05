use crate::models::AppConfig;
use std::fs;
use std::path::PathBuf;
use tauri::AppHandle;
use tauri::Manager;

fn config_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_data_dir().expect("failed to get app data dir");
    if let Err(e) = fs::create_dir_all(&dir) {
        log::warn!("Config error: failed to create config directory: {}", e);
    }
    dir.join("config.json")
}

pub fn load(app: &AppHandle) -> AppConfig {
    let path = config_path(app);
    if path.exists() {
        match fs::read_to_string(&path) {
            Ok(content) => {
                match serde_json::from_str(&content) {
                    Ok(config) => {
                        log::info!("Config loaded successfully from {:?}", path);
                        config
                    }
                    Err(e) => {
                        log::warn!("Config error: failed to parse config: {}", e);
                        AppConfig::default()
                    }
                }
            }
            Err(e) => {
                log::warn!("Config error: failed to read config file: {}", e);
                AppConfig::default()
            }
        }
    } else {
        log::info!("Config file not found, using defaults");
        AppConfig::default()
    }
}

pub fn save(app: &AppHandle, config: &AppConfig) -> Result<(), String> {
    let path = config_path(app);
    let content = serde_json::to_string_pretty(config).map_err(|e| {
        log::warn!("Config error: failed to serialize config: {}", e);
        e.to_string()
    })?;
    fs::write(&path, content).map_err(|e| {
        log::warn!("Config error: failed to write config file: {}", e);
        e.to_string()
    })?;
    log::info!("Config saved successfully to {:?}", path);
    Ok(())
}
