//! 配置持久化模块。
//!
//! 配置文件整体以「加密信封」形式落盘（见 [`crate::secrets`]），密钥由机器指纹派生、永不落盘。
//! - [`load`]：兼容加密信封与旧明文格式，明文格式将自动、幂等地迁移为加密信封；
//!   同时产出 [`ConfigHealth`]，把「无法解密」「迁移被暂缓」「已完成加密升级」等状态暴露给 UI；
//! - [`save`]：序列化 -> 加密 -> 写唯一临时文件 -> `fs::rename` 原子替换，Unix 下限制权限为 0600。
//!
//! 数据安全的两道防线：
//! 1. **load 路径**：解密失败时只返回默认配置，绝不写盘，原密文文件逐字不动；
//! 2. **save 路径**：若本次会话 load 时该文件无法解密，覆盖前先备份为
//!    `config.json.unreadable-<unix 秒>.bak`（0600），避免用户随手一次保存就永久销毁全部密钥。
//!
//! 核心逻辑抽为路径参数化的纯函数以便单元测试，且不污染真实 app data 目录。

use crate::models::AppConfig;
use crate::secrets::{self, EncryptedEnvelope, FingerprintSource, SecretsError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tauri::Manager;
use zeroize::Zeroizing;

/// 配置健康状态：本次启动加载配置时发生的关键事件，供 UI 明确提示用户。
///
/// 字段名为 snake_case，直接作为 IPC 返回值序列化给前端。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigHealth {
    /// 配置文件存在，但本次会话无法解密/读取（换机、系统重装、篡改或损坏）。
    pub unreadable: bool,
    /// 覆盖前备份守卫生成的 `.bak` 文件路径；仅在 unreadable 状态下执行保存时才会产生。
    pub backup_path: Option<String>,
    /// 明文→密文迁移被暂缓（当前仅能取得兜底指纹，或迁移校验失败），下次启动会重试。
    pub migration_deferred: bool,
    /// 本次启动已完成明文→密文迁移（原明文已被加密信封安全替换）。
    pub migrated_from_plaintext: bool,
}

/// 明文配置迁移决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MigrationDecision {
    /// 取得平台级指纹，可以安全迁移。
    Proceed,
    /// 暂缓迁移，保留明文文件不动。
    Defer,
}

/// 保存失败的内部原因分类，用于映射为中文用户可读消息（技术细节只写日志）。
enum ConfigErrorKind {
    Serialize,
    TempWrite,
    Verify,
    Rename,
    Backup,
}

fn config_path(app: &AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .expect("failed to get app data dir");
    if let Err(e) = fs::create_dir_all(&dir) {
        log::warn!("Config: 创建配置目录失败: {}", e);
    }
    dir.join("config.json")
}

/// 加载配置（对外入口），同时返回本次加载的健康状态。
pub fn load(app: &AppHandle) -> (AppConfig, ConfigHealth) {
    let path = config_path(app);
    load_from_path(&path)
}

/// 保存配置（对外入口）。
///
/// `unreadable_at_load` 为真表示本次会话加载时该文件无法解密：覆盖前会先备份原文件。
/// 返回本次生成的备份文件路径（若有），供调用方写入 [`ConfigHealth::backup_path`]。
pub fn save(
    app: &AppHandle,
    config: &AppConfig,
    unreadable_at_load: bool,
) -> Result<Option<String>, String> {
    let path = config_path(app);
    save_to_path(&path, config, unreadable_at_load)
}

/// 从指定路径加载配置：兼容加密信封与旧明文格式（明文格式自动迁移）。
///
/// 任何解析/解密失败都返回默认配置并记录日志，绝不 panic；
/// 且 **load 路径绝不写盘**，原文件逐字保持不动（save 路径另有备份守卫兜底）。
fn load_from_path(path: &Path) -> (AppConfig, ConfigHealth) {
    let mut health = ConfigHealth::default();
    // 顺手清理上次异常退出可能残留的临时文件
    cleanup_stale_temp_files(path);

    if !path.exists() {
        log::info!("Config: 配置文件不存在，使用默认配置");
        return (AppConfig::default(), health);
    }
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            log::error!(
                "Config: 读取配置文件失败，本次使用默认配置；原文件保持不动，之后若执行保存会先自动备份。原因: {}",
                e
            );
            health.unreadable = true;
            return (AppConfig::default(), health);
        }
    };
    let value: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            log::error!(
                "Config: 配置文件不是合法 JSON，本次使用默认配置；原文件保持不动，之后若执行保存会先自动备份。原因: {}",
                e
            );
            health.unreadable = true;
            return (AppConfig::default(), health);
        }
    };

    if value.get("llmtm_encrypted").is_some() {
        let cfg = load_encrypted(value, path, &mut health);
        (cfg, health)
    } else {
        let cfg = load_plaintext_and_migrate(value, path, &mut health);
        (cfg, health)
    }
}

/// 解析并解密「加密信封」格式的配置。
fn load_encrypted(value: serde_json::Value, path: &Path, health: &mut ConfigHealth) -> AppConfig {
    let envelope: EncryptedEnvelope = match serde_json::from_value(value) {
        Ok(env) => env,
        Err(e) => {
            log::error!(
                "Config: 加密信封结构不完整，本次使用默认配置；原文件保持不动，之后若执行保存会先自动备份。原因: {}",
                e
            );
            health.unreadable = true;
            return AppConfig::default();
        }
    };
    match secrets::decrypt(&envelope) {
        Ok(plaintext) => match serde_json::from_str::<AppConfig>(plaintext.as_str()) {
            Ok(cfg) => {
                log::info!("Config: 已解密并加载配置: {:?}", path);
                cfg
            }
            Err(e) => {
                log::error!(
                    "Config: 解密成功但内容不是合法配置结构，本次使用默认配置；原文件保持不动，之后若执行保存会先自动备份。原因: {}",
                    e
                );
                health.unreadable = true;
                AppConfig::default()
            }
        },
        // 换机 / 系统重装 / 篡改 / 损坏 / 指纹来源变化。
        // 注意：这里的「不覆盖」保护只在 load 路径成立——load 全程不写盘；
        // save 路径无法靠"不写"来保护（用户确实需要保存新配置），
        // 因此由 backup_unreadable_file 的覆盖前备份守卫兜底。
        Err(e) => {
            log::error!(
                "Config: 无法解密本机配置文件（可能已换机、重装系统或文件被篡改），本次使用默认配置。\
                 原文件保持不动；若之后执行保存，会先自动备份为 .unreadable-<时间戳>.bak。原因: {}",
                e
            );
            health.unreadable = true;
            AppConfig::default()
        }
    }
}

/// 解析旧明文配置并按需触发自动迁移；无论迁移成功与否都返回可用的内存态配置。
fn load_plaintext_and_migrate(
    value: serde_json::Value,
    path: &Path,
    health: &mut ConfigHealth,
) -> AppConfig {
    let cfg: AppConfig = match serde_json::from_value(value) {
        Ok(cfg) => cfg,
        Err(e) => {
            log::error!(
                "Config: 明文配置结构不合法，本次使用默认配置；原文件保持不动，之后若执行保存会先自动备份。原因: {}",
                e
            );
            health.unreadable = true;
            return AppConfig::default();
        }
    };

    // 兜底指纹下禁止首次明文→密文迁移：主机名/用户名一旦变化密钥即永久失效，
    // 把用户密钥锁进这种不稳定标识的代价远大于晚一点迁移。
    let fp_source = match secrets::device_fingerprint() {
        Ok(fp) => Some(fp.source),
        Err(e) => {
            log::error!(
                "Config: 无法取得任何机器指纹，已暂缓明文配置加密迁移。原因: {}",
                e
            );
            None
        }
    };
    apply_migration_decision(cfg, path, health, decide_migration(fp_source))
}

/// 按给定决策执行（或暂缓）明文→密文迁移。
///
/// 决策以参数注入，使「暂缓」分支在平台指纹恒可用的开发机上也能被确定性测试。
/// 无论走哪个分支，都返回可用的内存态配置，应用功能不受影响。
fn apply_migration_decision(
    cfg: AppConfig,
    path: &Path,
    health: &mut ConfigHealth,
    decision: MigrationDecision,
) -> AppConfig {
    if decision == MigrationDecision::Defer {
        log::warn!(
            "Config: 检测到明文配置，但当前指纹来源不足以安全绑定密钥，已暂缓加密迁移、保留明文文件不动；\
             待平台级指纹恢复可用后，下次启动会自动完成迁移（应用功能不受影响）"
        );
        health.migration_deferred = true;
        return cfg;
    }

    log::info!("Config: 检测到旧版明文配置，开始迁移为机器绑定加密存储");
    // verify=true：rename 前回读解密校验，失败则保留原明文文件不动，下次启动幂等重试。
    match write_encrypted_atomic(path, &cfg, true, false) {
        Ok(_) => {
            health.migrated_from_plaintext = true;
            log::info!(
                "Config: 明文配置已迁移为加密信封，原明文已被安全替换: {:?}",
                path
            );
        }
        Err(e) => {
            log::error!(
                "Config: 明文→密文迁移失败，已保留原明文文件不动，下次启动将重试。原因: {}",
                e
            );
            health.migration_deferred = true;
        }
    }
    cfg
}

/// 迁移决策纯函数：仅当取得平台级指纹时才允许首次明文→密文迁移。
///
/// 抽成可注入来源的纯函数以便单元测试（真实环境无法随意切换指纹来源）。
fn decide_migration(fp_source: Option<FingerprintSource>) -> MigrationDecision {
    match fp_source {
        Some(FingerprintSource::Platform) => MigrationDecision::Proceed,
        // 兜底指纹或指纹完全不可用：一律暂缓
        _ => MigrationDecision::Defer,
    }
}

/// 保存配置到指定路径：加密后原子写入，必要时先备份无法解密的原文件。
fn save_to_path(
    path: &Path,
    config: &AppConfig,
    unreadable_at_load: bool,
) -> Result<Option<String>, String> {
    write_encrypted_atomic(path, config, false, unreadable_at_load)
}

/// 将配置加密后原子写入目标路径。
///
/// 流程：序列化 -> 加密 -> 写同目录唯一临时文件 ->（可选）回读校验
/// ->（可选）覆盖前备份守卫 -> `fs::rename` 原子替换。
/// - `verify=true`：迁移场景，校验失败会清理临时文件并保留原文件不动（幂等重试）；
/// - `backup_unreadable=true`：覆盖前把无法解密的原文件备份为 `.unreadable-<ts>.bak`；
///   备份失败则中止保存，绝不冒着不可逆丢失密钥的风险覆盖。
///
/// 返回 `Ok(Some(备份路径))` 表示本次产生了备份文件。任一步骤失败都会清理临时文件。
fn write_encrypted_atomic(
    path: &Path,
    config: &AppConfig,
    verify: bool,
    backup_unreadable: bool,
) -> Result<Option<String>, String> {
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            log::warn!("Config: 创建配置目录失败: {}", e);
        }
    }

    // 明文配置用 Zeroizing 承载，离开作用域即清零，减少密钥在内存中的滞留
    let plaintext = Zeroizing::new(serde_json::to_string_pretty(config).map_err(|e| {
        log::error!("Config: 序列化配置失败: {}", e);
        user_error(ConfigErrorKind::Serialize)
    })?);
    let envelope = secrets::encrypt(plaintext.as_str()).map_err(|e| {
        log::error!("Config: 加密配置失败: {}", e);
        user_error_for_secrets(&e)
    })?;
    let envelope_json = serde_json::to_string_pretty(&envelope).map_err(|e| {
        log::error!("Config: 序列化加密信封失败: {}", e);
        user_error(ConfigErrorKind::Serialize)
    })?;

    let tmp = temp_path_for(path);
    if let Err(e) = write_temp_secure(&tmp, &envelope_json) {
        let _ = fs::remove_file(&tmp);
        log::error!("Config: 写入临时配置文件失败: {}", e);
        return Err(user_error(ConfigErrorKind::TempWrite));
    }
    if verify {
        if let Err(e) = verify_roundtrip(&tmp, config) {
            let _ = fs::remove_file(&tmp);
            log::error!(
                "Config: 迁移回读校验失败，已取消写入并保留原文件不动: {}",
                e
            );
            return Err(user_error(ConfigErrorKind::Verify));
        }
    }

    // 覆盖前备份守卫：正常可读配置的保存绝不会走到这里，因此不会产生任何 .bak。
    let backup_path = if backup_unreadable && path.exists() {
        match backup_unreadable_file(path) {
            Ok(bp) => {
                if let Some(ref p) = bp {
                    log::warn!("Config: 原配置文件本次无法解密，覆盖前已备份为: {}", p);
                }
                bp
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                log::error!(
                    "Config: 原配置文件无法解密且备份失败，为避免密钥不可逆丢失已中止保存: {}",
                    e
                );
                return Err(user_error(ConfigErrorKind::Backup));
            }
        }
    } else {
        None
    };

    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        log::error!("Config: 原子替换配置文件失败: {}", e);
        return Err(user_error(ConfigErrorKind::Rename));
    }
    log::info!("Config: 配置已保存: {:?}", path);
    Ok(backup_path)
}

/// 把当前无法解密但确实存在的配置文件复制为 `<原名>.unreadable-<unix 秒>.bak` 并设 0600。
///
/// 返回 `Ok(None)` 表示原文件不存在（无需备份）。备份内容与原密文逐字一致，
/// 用户回到原设备后仍可用它恢复密钥。
fn backup_unreadable_file(path: &Path) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let base = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "config.json".to_string());
    let backup = path.with_file_name(format!("{}.unreadable-{}.bak", base, ts));
    fs::copy(path, &backup).map_err(|e| format!("复制原文件失败: {}", e))?;
    // 备份同样是密文，但仍收紧权限，避免同机其他用户旁路读取
    set_owner_only(&backup).map_err(|e| format!("设置备份文件权限失败: {}", e))?;
    Ok(Some(backup.to_string_lossy().to_string()))
}

/// 回读临时文件、解密并与内存配置逐字段（序列化后）比对，确保迁移无损。
fn verify_roundtrip(tmp: &Path, expected: &AppConfig) -> Result<(), String> {
    let back = fs::read_to_string(tmp).map_err(|e| e.to_string())?;
    let envelope: EncryptedEnvelope = serde_json::from_str(&back).map_err(|e| e.to_string())?;
    let decrypted = secrets::decrypt(&envelope).map_err(|e| e.to_string())?;
    let decrypted_cfg: AppConfig =
        serde_json::from_str(decrypted.as_str()).map_err(|e| e.to_string())?;
    let a = serde_json::to_string(expected).map_err(|e| e.to_string())?;
    let b = serde_json::to_string(&decrypted_cfg).map_err(|e| e.to_string())?;
    if a != b {
        return Err("解密回读的配置与内存态不一致".to_string());
    }
    Ok(())
}

/// 生成与目标文件同目录、带唯一后缀（pid + 纳秒时间戳）的临时文件路径。
///
/// 唯一后缀避免多实例/多线程同时保存时互相覆盖对方的临时文件。
fn temp_path_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "config.json".to_string());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    path.with_file_name(format!("{}.{}.{}.tmp", name, std::process::id(), nanos))
}

/// 清理同目录下上次异常退出可能残留的 `<原名>.*.tmp` 文件。
fn cleanup_stale_temp_files(path: &Path) {
    let (Some(parent), Some(file_name)) = (path.parent(), path.file_name()) else {
        return;
    };
    let base = file_name.to_string_lossy().to_string();
    let prefix = format!("{}.", base);
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        let name = match entry_path.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };
        if name.starts_with(&prefix) && name.ends_with(".tmp") {
            match fs::remove_file(&entry_path) {
                Ok(()) => log::info!("Config: 已清理残留临时文件 {:?}", entry_path),
                Err(e) => log::warn!("Config: 清理残留临时文件 {:?} 失败: {}", entry_path, e),
            }
        }
    }
}

/// 以 0600 权限「一步创建并写入」临时文件。
///
/// Unix 下用 `create_new(true) + mode(0o600)`：
/// - 消除「先按默认 0644 创建、再 chmod」之间密文可被同机其他用户读取的窗口；
/// - `create_new` 在目标已存在时报错且拒绝跟随符号链接，避免符号链接竞态把内容写到别处。
#[cfg(unix)]
fn write_temp_secure(tmp: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(tmp)?;
    file.write_all(content.as_bytes())
}

#[cfg(not(unix))]
fn write_temp_secure(tmp: &Path, content: &str) -> std::io::Result<()> {
    fs::write(tmp, content)?;
    set_owner_only(tmp)
}

/// Unix 下将文件权限收紧为仅属主可读写（0600）。
#[cfg(unix)]
fn set_owner_only(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// 把内部保存失败原因映射为中文用户可读消息（技术细节只写日志，不外泄给 UI）。
fn user_error(kind: ConfigErrorKind) -> String {
    match kind {
        ConfigErrorKind::Serialize => "配置内容序列化失败，未能保存".to_string(),
        ConfigErrorKind::TempWrite => "写入临时配置文件失败，未能保存".to_string(),
        ConfigErrorKind::Verify => "加密结果校验未通过，为避免损坏配置已取消保存".to_string(),
        ConfigErrorKind::Rename => "替换配置文件失败，未能保存".to_string(),
        ConfigErrorKind::Backup => {
            "原配置文件备份失败，为避免密钥不可逆丢失已中止保存，请检查磁盘空间与权限后重试"
                .to_string()
        }
    }
}

/// 把 secrets 层错误映射为中文用户可读消息，避免把英文内部术语直接渲染到界面。
fn user_error_for_secrets(e: &SecretsError) -> String {
    match e {
        SecretsError::FingerprintUnavailable(_) => "无法获取本机安全标识，配置未能保存".to_string(),
        // 该变体消息本身即为面向用户的完整中文说明，可直接呈现
        SecretsError::FingerprintSourceChanged(_) => e.to_string(),
        SecretsError::EncryptFailed(_) => "配置加密失败，未能保存".to_string(),
        SecretsError::DecryptFailed(_) | SecretsError::IoError(_) => {
            "读写加密配置时发生错误，未能保存".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderConfig;
    use base64::{engine::general_purpose::STANDARD, Engine};

    /// 生成位于系统临时目录、带唯一后缀的路径，避免污染真实 app data 目录及并发冲突。
    fn unique_temp_path(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "llmtm-config-test-{}-{}-{}.json",
            tag,
            std::process::id(),
            nanos
        ))
    }

    /// 列出同目录下、以本测试文件名为前缀且以指定后缀结尾的文件（用于断言 .bak / .tmp）。
    fn related_files(path: &Path, suffix: &str) -> Vec<PathBuf> {
        let parent = path.parent().expect("测试路径应有父目录");
        let base = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let Ok(entries) = fs::read_dir(parent) else {
            return Vec::new();
        };
        entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().starts_with(&base))
                    .unwrap_or(false)
                    && p.to_string_lossy().ends_with(suffix)
            })
            .collect()
    }

    fn sample_config() -> AppConfig {
        AppConfig {
            providers: vec![
                ProviderConfig {
                    name: "deepseek".to_string(),
                    api_key: "sk-DEEP-SECRET-abc123".to_string(),
                    enabled: true,
                    platform_token: Some("plat-token-xyz".to_string()),
                    alias: Some("工作账号".to_string()),
                },
                ProviderConfig {
                    name: "kimi".to_string(),
                    api_key: "sk-KIMI-SECRET-def456".to_string(),
                    enabled: false,
                    platform_token: None,
                    alias: None,
                },
            ],
            refresh_interval: 120,
        }
    }

    /// 构造一个「必然无法解密」的信封内容（密文首字节被篡改）。
    fn corrupt_envelope_json() -> String {
        let mut envelope = secrets::encrypt("payload").expect("测试环境应能加密");
        let mut ct = STANDARD
            .decode(envelope.ciphertext.as_bytes())
            .expect("自产密文应可解码");
        ct[0] ^= 0x01;
        envelope.ciphertext = STANDARD.encode(ct.as_slice());
        serde_json::to_string_pretty(&envelope).unwrap()
    }

    /// 旧明文格式 load 后自动迁移：文件含 llmtm_encrypted，且不含任何 api_key 明文子串。
    #[test]
    fn migrate_plaintext_to_encrypted() {
        let path = unique_temp_path("migrate");
        let cfg = sample_config();
        fs::write(&path, serde_json::to_string_pretty(&cfg).unwrap()).unwrap();

        let (loaded, health) = load_from_path(&path);
        assert_eq!(loaded.providers.len(), 2);
        assert_eq!(loaded.providers[0].api_key, "sk-DEEP-SECRET-abc123");
        assert_eq!(loaded.refresh_interval, 120);
        assert!(!health.unreadable);

        // 本机为平台指纹时应完成迁移；仅兜底指纹时按设计暂缓（见 migration_deferred 测试）
        let after = fs::read_to_string(&path).unwrap();
        if health.migrated_from_plaintext {
            assert!(after.contains("llmtm_encrypted"));
            assert!(!after.contains("sk-DEEP-SECRET-abc123"));
            assert!(!after.contains("sk-KIMI-SECRET-def456"));
            assert!(!after.contains("plat-token-xyz"));
            // 再次加载应从加密信封正确解密（幂等）
            let (reloaded, health2) = load_from_path(&path);
            assert_eq!(reloaded.providers[1].api_key, "sk-KIMI-SECRET-def456");
            assert!(!health2.unreadable);
            assert!(!health2.migrated_from_plaintext);
        } else {
            assert!(health.migration_deferred);
            assert_eq!(after, serde_json::to_string_pretty(&cfg).unwrap());
        }
        // 临时文件不应残留
        assert!(related_files(&path, ".tmp").is_empty());
        // 迁移路径绝不产生 .bak
        assert!(related_files(&path, ".bak").is_empty());

        let _ = fs::remove_file(&path);
    }

    /// save 后文件含 llmtm_encrypted，且不含 api_key 明文子串，可被正确解密回读。
    #[test]
    fn save_writes_encrypted_without_plaintext() {
        let path = unique_temp_path("save");
        let cfg = sample_config();
        save_to_path(&path, &cfg, false).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("llmtm_encrypted"));
        assert!(content.contains("fp_source"));
        assert!(!content.contains("sk-DEEP-SECRET-abc123"));
        assert!(!content.contains("sk-KIMI-SECRET-def456"));
        assert!(related_files(&path, ".tmp").is_empty());

        let (loaded, health) = load_from_path(&path);
        assert_eq!(loaded.providers.len(), 2);
        assert_eq!(
            loaded.providers[0].platform_token.as_deref(),
            Some("plat-token-xyz")
        );
        assert!(!health.unreadable);

        let _ = fs::remove_file(&path);
    }

    /// Unix 下最终文件权限应为 0600。
    #[cfg(unix)]
    #[test]
    fn saved_file_permissions_are_0600() {
        use std::os::unix::fs::PermissionsExt;
        let path = unique_temp_path("perm");
        save_to_path(&path, &sample_config(), false).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = fs::remove_file(&path);
    }

    /// 新增①：伪造/损坏信封 → load 返回默认配置 + unreadable，且文件逐字未变、无 tmp 残留、无 .bak。
    #[test]
    fn decrypt_failure_keeps_ciphertext_file_untouched() {
        let path = unique_temp_path("untouched");
        let original = corrupt_envelope_json();
        fs::write(&path, &original).unwrap();

        let (loaded, health) = load_from_path(&path);
        assert!(loaded.providers.is_empty());
        assert_eq!(loaded.refresh_interval, 300);
        assert!(health.unreadable, "解密失败必须标记 unreadable");
        assert!(health.backup_path.is_none(), "load 路径不应产生备份");
        // 文件逐字未变
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert!(related_files(&path, ".tmp").is_empty());
        assert!(related_files(&path, ".bak").is_empty());

        let _ = fs::remove_file(&path);
    }

    /// 新增②：unreadable 状态下 save → 生成 .unreadable-<ts>.bak（0600、内容与原密文一致），
    /// 新信封正常写入且可解密回读。
    #[test]
    fn save_after_unreadable_load_creates_backup() {
        let path = unique_temp_path("backup");
        let original = corrupt_envelope_json();
        fs::write(&path, &original).unwrap();

        let (_, health) = load_from_path(&path);
        assert!(health.unreadable);

        let backup = save_to_path(&path, &sample_config(), true)
            .expect("保存应成功")
            .expect("unreadable 状态下必须产生备份路径");
        let backup_path = PathBuf::from(&backup);
        assert!(backup_path.exists(), "备份文件应真实存在");

        let name = backup_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        assert!(
            name.contains(".unreadable-") && name.ends_with(".bak"),
            "备份文件名不符合约定: {}",
            name
        );
        // 备份内容与原密文逐字一致（用户回到原设备仍可恢复）
        assert_eq!(fs::read_to_string(&backup_path).unwrap(), original);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&backup_path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "备份文件权限应为 0600");
        }

        // 新信封已正常写入，且可被本机会话解密
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("llmtm_encrypted"));
        assert_ne!(after, original);
        let (reloaded, health2) = load_from_path(&path);
        assert!(!health2.unreadable);
        assert_eq!(reloaded.providers.len(), 2);
        assert_eq!(reloaded.providers[0].api_key, "sk-DEEP-SECRET-abc123");
        assert!(related_files(&path, ".tmp").is_empty());

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&backup_path);
    }

    /// 新增③：可读配置的正常保存绝不产生任何 .bak。
    #[test]
    fn normal_save_creates_no_backup() {
        let path = unique_temp_path("nobackup");
        let cfg = sample_config();

        let backup = save_to_path(&path, &cfg, false).unwrap();
        assert!(backup.is_none(), "正常保存不应产生备份路径");
        assert!(
            related_files(&path, ".bak").is_empty(),
            "正常保存不得在目录中留下任何 .bak"
        );

        // 连续保存多次同样不产生备份
        let mut cfg2 = cfg.clone();
        cfg2.refresh_interval = 240;
        let backup2 = save_to_path(&path, &cfg2, false).unwrap();
        assert!(backup2.is_none());
        assert!(related_files(&path, ".bak").is_empty());

        let (loaded, health) = load_from_path(&path);
        assert!(!health.unreadable);
        assert_eq!(loaded.refresh_interval, 240);

        let _ = fs::remove_file(&path);
    }

    /// 新增⑦：兜底指纹（或指纹不可用）下必须暂缓迁移；端到端验证明文文件保持不动。
    #[test]
    fn migration_deferred_on_fallback_fingerprint() {
        // 决策纯函数：只有平台级指纹才允许迁移
        assert_eq!(
            decide_migration(Some(FingerprintSource::Platform)),
            MigrationDecision::Proceed
        );
        assert_eq!(
            decide_migration(Some(FingerprintSource::Fallback)),
            MigrationDecision::Defer,
            "兜底指纹必须暂缓迁移，避免把密钥锁进不稳定标识"
        );
        assert_eq!(
            decide_migration(None),
            MigrationDecision::Defer,
            "指纹完全不可用时同样必须暂缓"
        );

        // 端到端：按本机实际指纹来源断言对应分支
        let path = unique_temp_path("defer");
        let cfg = sample_config();
        let plaintext = serde_json::to_string_pretty(&cfg).unwrap();
        fs::write(&path, &plaintext).unwrap();

        let (loaded, health) = load_from_path(&path);
        // 无论是否迁移，配置都必须正常可用（应用功能不受影响）
        assert_eq!(loaded.providers.len(), 2);
        assert_eq!(loaded.providers[0].api_key, "sk-DEEP-SECRET-abc123");
        assert!(!health.unreadable);

        let source = secrets::device_fingerprint().map(|fp| fp.source).ok();
        if source == Some(FingerprintSource::Platform) {
            assert!(health.migrated_from_plaintext);
            assert!(!health.migration_deferred);
        } else {
            assert!(
                health.migration_deferred,
                "非平台指纹必须置 migration_deferred"
            );
            assert!(!health.migrated_from_plaintext);
            // 明文文件逐字未变
            assert_eq!(fs::read_to_string(&path).unwrap(), plaintext);
        }
        assert!(related_files(&path, ".tmp").is_empty());

        let _ = fs::remove_file(&path);
    }

    /// 新增⑧-a：直接注入 `Defer` 决策，确定性验证「暂缓迁移」分支：
    /// 明文文件逐字不动、返回配置正常可用、health.migration_deferred 为真、不产生任何 tmp/bak。
    ///
    /// 与上一个测试互补：上一个依赖本机实际指纹来源，本测试在任意机器上都会执行到暂缓分支。
    #[test]
    fn deferred_migration_keeps_plaintext_file_untouched() {
        let path = unique_temp_path("defer-forced");
        let cfg = sample_config();
        let plaintext = serde_json::to_string_pretty(&cfg).unwrap();
        fs::write(&path, &plaintext).unwrap();

        let mut health = ConfigHealth::default();
        let loaded = apply_migration_decision(cfg, &path, &mut health, MigrationDecision::Defer);

        assert!(
            health.migration_deferred,
            "暂缓分支必须置 migration_deferred"
        );
        assert!(!health.migrated_from_plaintext, "未实际迁移不得置 true");
        assert!(!health.unreadable, "明文可读，不应误报 unreadable");
        assert!(health.backup_path.is_none());
        // 配置正常返回，应用功能不受影响
        assert_eq!(loaded.providers.len(), 2);
        assert_eq!(loaded.providers[0].api_key, "sk-DEEP-SECRET-abc123");
        assert_eq!(loaded.refresh_interval, 120);
        // 明文文件逐字未变（未被加密、未被删除、未被覆盖）
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            plaintext,
            "暂缓迁移时明文文件必须逐字不动"
        );
        assert!(related_files(&path, ".tmp").is_empty());
        assert!(related_files(&path, ".bak").is_empty());

        // 下次启动若平台指纹恢复可用，迁移会幂等完成（此处直接验证 Proceed 分支）
        let loaded2 =
            apply_migration_decision(loaded, &path, &mut health, MigrationDecision::Proceed);
        assert_eq!(loaded2.providers.len(), 2);
        assert!(health.migrated_from_plaintext);
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("llmtm_encrypted"), "应已升级为加密信封");
        assert!(
            !content.contains("sk-DEEP-SECRET-abc123"),
            "不得残留明文密钥"
        );

        let _ = fs::remove_file(&path);
    }

    /// 新增⑨：load 入口会清理同目录残留的临时文件。
    #[test]
    fn temp_files_cleaned_on_load() {
        let path = unique_temp_path("tmpclean");
        fs::write(
            &path,
            serde_json::to_string_pretty(&sample_config()).unwrap(),
        )
        .unwrap();

        let base = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap();
        // 预置两个残留临时文件，模拟上次异常退出
        let stale1 = path.with_file_name(format!("{}.999999.111.tmp", base));
        let stale2 = path.with_file_name(format!("{}.888888.222.tmp", base));
        fs::write(&stale1, "stale").unwrap();
        fs::write(&stale2, "stale").unwrap();
        assert!(stale1.exists() && stale2.exists());

        let _ = load_from_path(&path);
        assert!(!stale1.exists(), "残留临时文件应被清理");
        assert!(!stale2.exists(), "残留临时文件应被清理");
        assert!(
            related_files(&path, ".tmp").is_empty(),
            "load 后不应留下任何 .tmp"
        );

        let _ = fs::remove_file(&path);
    }

    /// 中文错误映射：内部英文技术细节不得泄露给 UI。
    #[test]
    fn user_errors_are_chinese_and_leak_free() {
        let msg = user_error_for_secrets(&SecretsError::FingerprintUnavailable(
            "both platform and host/user based fingerprints unavailable".to_string(),
        ));
        assert_eq!(msg, "无法获取本机安全标识，配置未能保存");
        assert!(!msg.contains("fingerprint unavailable"));
        for kind in [
            ConfigErrorKind::Serialize,
            ConfigErrorKind::TempWrite,
            ConfigErrorKind::Verify,
            ConfigErrorKind::Rename,
            ConfigErrorKind::Backup,
        ] {
            let m = user_error(kind);
            assert!(!m.is_empty());
            assert!(!m.is_ascii(), "应为中文消息: {}", m);
        }
    }
}
