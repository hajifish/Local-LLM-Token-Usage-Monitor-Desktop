//! API 密钥本地加密存储模块。
//!
//! 设计要点：
//! - 密钥由「机器指纹 + 编译期 pepper」经 blake3 派生，解密密钥永不落盘；
//! - 采用 XChaCha20-Poly1305 AEAD，随机 24 字节 nonce，AAD 绑定应用标识、信封版本号与指纹来源；
//! - 密文以版本化「加密信封」形式存储，版本号与指纹来源均参与 AAD，伪造任一项必然解密失败；
//! - 指纹来源变化（platform ↔ fallback）会被显式拒绝，不会静默换一把密钥去试；
//! - 老格式信封（无 `fp_source` 字段）通过旧 AAD 兼容路径解密，下次保存自动升级为新格式。
//!
//! 安全约束：所有错误消息与日志均不得包含密钥、明文、指纹值或密文内容。

use base64::{engine::general_purpose::STANDARD, Engine};
use chacha20poly1305::aead::{Aead, Generate, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// 当前信封版本号，参与 AAD 绑定。
const ENVELOPE_VERSION: u32 = 1;
/// AEAD 附加认证数据（AAD）的应用标识前缀。
const AAD_CONTEXT: &[u8] = b"llmtm-config-v1";
/// blake3 密钥派生上下文（应用标识），与 pepper 一起做域分离。
const KEY_DERIVE_CONTEXT: &str = "com.hajifish.llm-token-monitor/config/v1";
/// XChaCha20-Poly1305 nonce 长度（字节）。
const NONCE_LEN: usize = 24;

/// 指纹来源标识：平台硬件/系统级唯一标识。
pub const FP_SOURCE_PLATFORM: &str = "platform";
/// 指纹来源标识：主机名 + 用户名兜底派生。
pub const FP_SOURCE_FALLBACK: &str = "fallback";

/// 编译期内置 pepper（32 字节一次性随机常量）。
///
/// 作为纵深防御的第二层：即便攻击者拿到目标机器的指纹来源，
/// 缺少该常量也无法离线派生出正确密钥。
const PEPPER: [u8; 32] = [
    0xd9, 0x10, 0x20, 0x66, 0x6c, 0x99, 0x12, 0x3c, 0x1b, 0xc2, 0x8f, 0xe9, 0xd5, 0xe1, 0xcc, 0xb3,
    0x1f, 0x06, 0xbd, 0x63, 0x51, 0xf7, 0x2e, 0xc6, 0xaf, 0xa4, 0xe9, 0x19, 0xdc, 0xe4, 0x4a, 0x06,
];

/// 加密/解密相关错误类型。
#[derive(Debug)]
pub enum SecretsError {
    /// 解密失败：AEAD 校验不过、版本号不符、base64 非法或明文非 UTF-8（含指纹变化/篡改/损坏）。
    DecryptFailed(String),
    /// 加密失败。
    EncryptFailed(String),
    /// 机器指纹不可用（平台源与兜底链均失败）。
    FingerprintUnavailable(String),
    /// 信封记录的指纹来源与本机当前实际可取得的来源不一致（中文可读消息，可安全呈现给用户）。
    FingerprintSourceChanged(String),
    /// 底层 IO / 进程调用错误。
    IoError(String),
}

impl std::fmt::Display for SecretsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecretsError::DecryptFailed(m) => write!(f, "解密失败: {}", m),
            SecretsError::EncryptFailed(m) => write!(f, "加密失败: {}", m),
            SecretsError::FingerprintUnavailable(m) => write!(f, "机器指纹不可用: {}", m),
            // 该变体消息本身即为面向用户的完整中文句子，不再追加前缀。
            SecretsError::FingerprintSourceChanged(m) => write!(f, "{}", m),
            SecretsError::IoError(m) => write!(f, "IO 错误: {}", m),
        }
    }
}

impl std::error::Error for SecretsError {}

/// 机器指纹来源。写入信封并参与 AAD 认证，来源变化即拒绝解密。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FingerprintSource {
    /// 平台硬件/系统级唯一标识（IOPlatformUUID / machine-id / MachineGuid）。
    Platform,
    /// 兜底：主机名 + 用户名派生，绑定强度较弱，且禁止用于首次明文→密文迁移。
    Fallback,
}

impl FingerprintSource {
    /// 落盘与 AAD 使用的稳定字符串标识。
    pub fn as_str(self) -> &'static str {
        match self {
            FingerprintSource::Platform => FP_SOURCE_PLATFORM,
            FingerprintSource::Fallback => FP_SOURCE_FALLBACK,
        }
    }

    /// 中文可读标签，用于面向用户的错误消息。
    pub fn label_zh(self) -> &'static str {
        match self {
            FingerprintSource::Platform => "平台硬件标识",
            FingerprintSource::Fallback => "主机名+用户名兜底标识",
        }
    }

    /// 从落盘字符串解析来源；无法识别时返回 None。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            FP_SOURCE_PLATFORM => Some(FingerprintSource::Platform),
            FP_SOURCE_FALLBACK => Some(FingerprintSource::Fallback),
            _ => None,
        }
    }
}

impl std::fmt::Debug for FingerprintSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// 本机指纹及其来源。
///
/// 注意：刻意手写 `Debug` 以遮蔽指纹值，避免任何日志/断言输出意外泄露标识内容。
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceFingerprint {
    /// 指纹原值（禁止写入日志或错误消息）。
    pub value: String,
    /// 指纹来源。
    pub source: FingerprintSource,
}

impl std::fmt::Debug for DeviceFingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceFingerprint")
            .field("value", &"<redacted>")
            .field("source", &self.source)
            .finish()
    }
}

/// 版本化加密信封（整体作为配置文件落盘）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncryptedEnvelope {
    /// 版本号，兼作格式标识；值固定为 1，参与 AAD 绑定。
    pub llmtm_encrypted: u32,
    /// base64 编码的 24 字节随机 nonce。
    pub nonce: String,
    /// base64 编码的密文（含 Poly1305 认证标签）。
    pub ciphertext: String,
    /// 加密时使用的指纹来源（`"platform"` / `"fallback"`），并入 AAD 参与认证。
    ///
    /// 向后兼容：升级前写入的老信封没有该字段，`#[serde(default)]` 会反序列化为空串，
    /// 空串即「字段缺失」哨兵，由 [`EncryptedEnvelope::effective_fp_source`] 视为
    /// `"platform"`，并由 [`EncryptedEnvelope::is_legacy_format`] 触发旧 AAD 兼容解密路径。
    /// 新写入的信封总会显式带上非空来源，因此空串不会出现在新格式中。
    #[serde(default)]
    pub fp_source: String,
}

impl EncryptedEnvelope {
    /// 生效的指纹来源：字段缺失或无法识别（老格式）时视为 `platform`。
    pub fn effective_fp_source(&self) -> FingerprintSource {
        FingerprintSource::parse(&self.fp_source).unwrap_or(FingerprintSource::Platform)
    }

    /// 是否为不含 `fp_source` 字段的老格式信封。
    pub fn is_legacy_format(&self) -> bool {
        self.fp_source.is_empty()
    }
}

/// 获取当前机器的稳定指纹及其来源。
///
/// 优先使用平台硬件/系统级唯一标识；失败时降级为「主机名 + 用户名」派生并记录 warn，
/// 降级链要求 host 与 user 同时存在，两者皆不可用时返回错误，绝不退化为固定常量。
pub fn device_fingerprint() -> Result<DeviceFingerprint, SecretsError> {
    match platform_fingerprint() {
        Ok(fp) => {
            let fp = fp.trim().to_string();
            if !fp.is_empty() {
                return Ok(DeviceFingerprint {
                    value: fp,
                    source: FingerprintSource::Platform,
                });
            }
            log::warn!(
                "Secrets: 平台指纹为空，降级为主机名+用户名兜底派生；后果：密钥绑定强度下降，且暂缓明文配置迁移"
            );
        }
        Err(e) => {
            log::warn!(
                "Secrets: 平台指纹不可用，降级为主机名+用户名兜底派生；后果：密钥绑定强度下降，且暂缓明文配置迁移。原因: {}",
                e
            );
        }
    }
    match fallback_fingerprint() {
        Some(value) => Ok(DeviceFingerprint {
            value,
            source: FingerprintSource::Fallback,
        }),
        None => Err(SecretsError::FingerprintUnavailable(
            "平台指纹与主机名+用户名兜底指纹均不可用".to_string(),
        )),
    }
}

/// 由指纹派生 32 字节对称密钥：blake3 KDF（上下文含应用标识，材料含 pepper + 指纹）。
///
/// 返回值用 [`Zeroizing`] 承载，离开作用域即清零，减少密钥在内存中的滞留。
fn derive_key(fp: &str) -> Zeroizing<[u8; 32]> {
    let mut hasher = blake3::Hasher::new_derive_key(KEY_DERIVE_CONTEXT);
    hasher.update(&PEPPER);
    hasher.update(fp.as_bytes());
    let mut out = Zeroizing::new([0u8; 32]);
    hasher.finalize_xof().fill(&mut out[..]);
    out
}

/// 加密明文为版本化信封（AAD 绑定版本号与当前指纹来源）。
pub fn encrypt(plaintext: &str) -> Result<EncryptedEnvelope, SecretsError> {
    let fp = device_fingerprint()?;
    let key = derive_key(&fp.value);
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_slice())
        .map_err(|e| SecretsError::EncryptFailed(format!("密钥长度非法: {}", e)))?;
    // 使用 try_generate 而非 generate：Cargo.toml 设了 panic = "abort"，
    // 若 OS RNG 失败，generate 的 panic 会直接终止整个进程。
    let nonce = XNonce::try_generate().map_err(|e| {
        SecretsError::EncryptFailed(format!("无法生成随机 nonce（系统随机源失败）: {}", e))
    })?;
    let aad = build_aad(ENVELOPE_VERSION, fp.source.as_str());
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext.as_bytes(),
                aad: aad.as_slice(),
            },
        )
        .map_err(|e| SecretsError::EncryptFailed(format!("AEAD 加密失败: {}", e)))?;
    Ok(EncryptedEnvelope {
        llmtm_encrypted: ENVELOPE_VERSION,
        nonce: STANDARD.encode(nonce.as_slice()),
        ciphertext: STANDARD.encode(ciphertext.as_slice()),
        fp_source: fp.source.as_str().to_string(),
    })
}

/// 解密版本化信封为明文（返回值用 [`Zeroizing`] 承载）。
///
/// 失败场景：版本号不符、指纹来源与信封记录不一致、AEAD 校验不过、base64 非法、
/// nonce 长度错误或明文非 UTF-8。
pub fn decrypt(envelope: &EncryptedEnvelope) -> Result<Zeroizing<String>, SecretsError> {
    // 版本号显式校验（版本号同时参与 AAD，二者共同保证伪造版本必然失败）。
    if envelope.llmtm_encrypted != ENVELOPE_VERSION {
        return Err(SecretsError::DecryptFailed(format!(
            "不支持的信封版本: {}",
            envelope.llmtm_encrypted
        )));
    }

    let fp = device_fingerprint()?;
    let envelope_source = envelope.effective_fp_source();
    // 来源不一致时明确拒绝：绝不静默用另一把密钥尝试解密。
    if envelope_source != fp.source {
        return Err(SecretsError::FingerprintSourceChanged(format!(
            "配置文件是在「{}」下加密的，但本机当前只能取得「{}」，为避免用错误密钥解密已直接拒绝。\
             请回到原设备，或确认本机硬件标识是否可用。",
            envelope_source.label_zh(),
            fp.source.label_zh()
        )));
    }

    let key = derive_key(&fp.value);
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_slice())
        .map_err(|e| SecretsError::DecryptFailed(format!("密钥长度非法: {}", e)))?;

    let nonce_bytes = STANDARD
        .decode(envelope.nonce.as_bytes())
        .map_err(|e| SecretsError::DecryptFailed(format!("nonce base64 非法: {}", e)))?;
    if nonce_bytes.len() != NONCE_LEN {
        return Err(SecretsError::DecryptFailed(format!(
            "nonce 长度非法: {}（应为 {}）",
            nonce_bytes.len(),
            NONCE_LEN
        )));
    }
    let nonce = XNonce::try_from(nonce_bytes.as_slice())
        .map_err(|_| SecretsError::DecryptFailed("nonce 字节非法".to_string()))?;

    let ciphertext = STANDARD
        .decode(envelope.ciphertext.as_bytes())
        .map_err(|e| SecretsError::DecryptFailed(format!("密文 base64 非法: {}", e)))?;

    // 先按新 AAD（含 fp_source）尝试。
    let new_aad = build_aad(envelope.llmtm_encrypted, envelope_source.as_str());
    let plaintext = match cipher.decrypt(
        &nonce,
        Payload {
            msg: ciphertext.as_slice(),
            aad: new_aad.as_slice(),
        },
    ) {
        Ok(pt) => pt,
        Err(first_err) => {
            // 兼容路径：仅当信封确实缺失 fp_source 字段（老格式）时，按旧 AAD 重试一次。
            // 两次尝试使用同一把密钥，且都受 Poly1305 标签保护，多试一个 AAD 不降低安全性。
            if envelope.is_legacy_format() {
                let legacy_aad = build_legacy_aad(envelope.llmtm_encrypted);
                match cipher.decrypt(
                    &nonce,
                    Payload {
                        msg: ciphertext.as_slice(),
                        aad: legacy_aad.as_slice(),
                    },
                ) {
                    Ok(pt) => {
                        log::info!(
                            "Secrets: 老格式信封（无 fp_source 字段）已通过兼容路径解密，下次保存将自动升级为新格式"
                        );
                        pt
                    }
                    Err(_) => {
                        return Err(SecretsError::DecryptFailed(format!(
                            "AEAD 校验失败（含旧格式兼容尝试）: {}",
                            first_err
                        )))
                    }
                }
            } else {
                return Err(SecretsError::DecryptFailed(format!(
                    "AEAD 校验失败: {}",
                    first_err
                )));
            }
        }
    };

    String::from_utf8(plaintext)
        .map(Zeroizing::new)
        .map_err(|e| SecretsError::DecryptFailed(format!("解密结果不是合法 UTF-8: {}", e)))
}

/// 构造 AAD：应用标识前缀 + 大端版本号 + 指纹来源，使版本号与来源均被密码学绑定。
fn build_aad(version: u32, fp_source: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(AAD_CONTEXT.len() + 4 + fp_source.len());
    aad.extend_from_slice(AAD_CONTEXT);
    aad.extend_from_slice(&version.to_be_bytes());
    aad.extend_from_slice(fp_source.as_bytes());
    aad
}

/// 旧格式 AAD（不含 fp_source），仅用于解密本次升级之前写入的老信封。
fn build_legacy_aad(version: u32) -> Vec<u8> {
    let mut aad = Vec::with_capacity(AAD_CONTEXT.len() + 4);
    aad.extend_from_slice(AAD_CONTEXT);
    aad.extend_from_slice(&version.to_be_bytes());
    aad
}

/// 兜底指纹组合纯函数：**必须 host 与 user 同时非空**才返回。
///
/// 任一缺失即返回 None。这是刻意消除的弱绑定：若允许退化为裸用户名，
/// 两台机器上的同名用户会派生出同一把密钥，攻击者拿到密文文件即可在任意机器离线解密。
fn compose_fallback_fingerprint(host: Option<&str>, user: Option<&str>) -> Option<String> {
    let host = host.map(str::trim).filter(|s| !s.is_empty())?;
    let user = user.map(str::trim).filter(|s| !s.is_empty())?;
    Some(format!("{}@{}", host, user))
}

/// 采集兜底指纹（主机名 + 用户名）。
fn fallback_fingerprint() -> Option<String> {
    compose_fallback_fingerprint(detect_hostname().as_deref(), detect_username().as_deref())
}

/// 主机名：优先环境变量，缺失时用 `hostname` 命令兜底。
fn detect_hostname() -> Option<String> {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(hostname_from_command)
}

/// 用户名：依次尝试 USER / USERNAME / LOGNAME。
fn detect_username() -> Option<String> {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .or_else(|_| std::env::var("LOGNAME"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 通过 `hostname` 命令获取主机名（兜底链的补充来源）。
fn hostname_from_command() -> Option<String> {
    let out = std::process::Command::new("hostname").output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

// ---------------------------------------------------------------------------
// 平台相关的机器指纹来源（cfg 条件编译）
// ---------------------------------------------------------------------------

/// macOS：执行 `ioreg -rd1 -c IOPlatformExpertDevice`，解析 `"IOPlatformUUID" = "XXXX"`。
///
/// 优先使用绝对路径 `/usr/sbin/ioreg`：GUI/launchd 等精简 PATH 环境下裸命令名可能查不到，
/// 静默失败会降级到兜底指纹，导致密钥悄然变化；绝对路径失败后才回退 PATH 查找。
#[cfg(target_os = "macos")]
fn platform_fingerprint() -> Result<String, SecretsError> {
    const ARGS: [&str; 3] = ["-rd1", "-c", "IOPlatformExpertDevice"];
    let out = std::process::Command::new("/usr/sbin/ioreg")
        .args(ARGS)
        .output()
        .or_else(|_| std::process::Command::new("ioreg").args(ARGS).output())
        .map_err(|e| SecretsError::IoError(format!("调用 ioreg 失败: {}", e)))?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some(idx) = line.find("IOPlatformUUID") {
            if let Some(val) = extract_quoted_after_eq(&line[idx..]) {
                if !val.is_empty() {
                    return Ok(val);
                }
            }
        }
    }
    Err(SecretsError::FingerprintUnavailable(
        "ioreg 输出中未找到 IOPlatformUUID".to_string(),
    ))
}

/// 从形如 `IOPlatformUUID" = "XXXX"` 的片段中提取等号后第一对双引号内的值。
#[cfg(target_os = "macos")]
fn extract_quoted_after_eq(s: &str) -> Option<String> {
    let eq = s.find('=')?;
    let rest = &s[eq + 1..];
    let first = rest.find('"')?;
    let after = &rest[first + 1..];
    let second = after.find('"')?;
    Some(after[..second].to_string())
}

/// Linux：读取 `/etc/machine-id`，失败回退 `/var/lib/dbus/machine-id`。
#[cfg(target_os = "linux")]
fn platform_fingerprint() -> Result<String, SecretsError> {
    let mut last_io_err: Option<SecretsError> = None;
    for p in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        match std::fs::read_to_string(p) {
            Ok(s) => {
                let t = s.trim().to_string();
                if !t.is_empty() {
                    return Ok(t);
                }
            }
            Err(e) => {
                last_io_err = Some(SecretsError::IoError(format!("读取 {} 失败: {}", p, e)));
            }
        }
    }
    Err(last_io_err.unwrap_or_else(|| {
        SecretsError::FingerprintUnavailable("machine-id 文件缺失或为空".to_string())
    }))
}

/// Windows：读取 `HKLM\SOFTWARE\Microsoft\Cryptography` 下的 `MachineGuid`。
#[cfg(target_os = "windows")]
fn platform_fingerprint() -> Result<String, SecretsError> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey("SOFTWARE\\Microsoft\\Cryptography")
        .map_err(|e| SecretsError::IoError(format!("打开注册表键失败: {}", e)))?;
    let guid: String = key
        .get_value("MachineGuid")
        .map_err(|e| SecretsError::IoError(format!("读取 MachineGuid 失败: {}", e)))?;
    let t = guid.trim().to_string();
    if t.is_empty() {
        return Err(SecretsError::FingerprintUnavailable(
            "MachineGuid 为空".to_string(),
        ));
    }
    Ok(t)
}

/// 其它平台：无稳定指纹来源，交由兜底链处理。
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn platform_fingerprint() -> Result<String, SecretsError> {
    Err(SecretsError::FingerprintUnavailable(
        "当前平台无可用指纹来源".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};

    /// 仅测试用：按「老格式」构造信封（AAD 不含 fp_source、字段留空模拟缺失）。
    fn encrypt_legacy_for_test(plaintext: &str) -> EncryptedEnvelope {
        let fp = device_fingerprint().expect("测试环境应能取得指纹");
        let key = derive_key(&fp.value);
        let cipher = XChaCha20Poly1305::new_from_slice(key.as_slice()).unwrap();
        let nonce = XNonce::try_generate().unwrap();
        let aad = build_legacy_aad(ENVELOPE_VERSION);
        let ct = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: aad.as_slice(),
                },
            )
            .unwrap();
        EncryptedEnvelope {
            llmtm_encrypted: ENVELOPE_VERSION,
            nonce: STANDARD.encode(nonce.as_slice()),
            ciphertext: STANDARD.encode(ct.as_slice()),
            // 空串模拟「老信封没有该字段」
            fp_source: String::new(),
        }
    }

    /// ① 多 provider 配置 JSON 加解密往返，逐字段一致。
    #[test]
    fn roundtrip_multi_provider_json() {
        let json = r#"{"providers":[{"name":"deepseek","api_key":"sk-AAA","enabled":true,"platform_token":"pt-1","alias":"a1"},{"name":"kimi","api_key":"sk-BBB","enabled":false,"platform_token":null,"alias":null},{"name":"zhipu","api_key":"sk-CCC","enabled":true,"platform_token":"pt-3","alias":"a3"}],"refresh_interval":60}"#;
        let envelope = encrypt(json).expect("encrypt should succeed");
        let decrypted = decrypt(&envelope).expect("decrypt should succeed");
        assert_eq!(decrypted.as_str(), json);
        // 逐字段一致（结构化比对）
        let a: serde_json::Value = serde_json::from_str(json).unwrap();
        let b: serde_json::Value = serde_json::from_str(decrypted.as_str()).unwrap();
        assert_eq!(a, b);
    }

    /// ② 密文或 nonce 改 1 字节，解密必须失败。
    #[test]
    fn tampered_ciphertext_or_nonce_fails() {
        let envelope = encrypt("secret-payload").unwrap();

        let mut ct = STANDARD.decode(envelope.ciphertext.as_bytes()).unwrap();
        assert!(!ct.is_empty());
        ct[0] ^= 0x01;
        let bad_ct = EncryptedEnvelope {
            ciphertext: STANDARD.encode(ct.as_slice()),
            ..envelope.clone()
        };
        assert!(decrypt(&bad_ct).is_err());

        let mut nonce = STANDARD.decode(envelope.nonce.as_bytes()).unwrap();
        assert!(!nonce.is_empty());
        nonce[0] ^= 0x01;
        let bad_nonce = EncryptedEnvelope {
            nonce: STANDARD.encode(nonce.as_slice()),
            ..envelope
        };
        assert!(decrypt(&bad_nonce).is_err());
    }

    /// ③ 版本号伪造为 2，解密失败。
    #[test]
    fn forged_version_fails() {
        let envelope = encrypt("x").unwrap();
        let forged = EncryptedEnvelope {
            llmtm_encrypted: 2,
            ..envelope
        };
        assert!(decrypt(&forged).is_err());
    }

    /// ④ base64 非法内容，解密失败。
    #[test]
    fn invalid_base64_fails() {
        let envelope = encrypt("x").unwrap();
        let bad = EncryptedEnvelope {
            ciphertext: "@@@not-valid-base64@@@".to_string(),
            ..envelope
        };
        assert!(decrypt(&bad).is_err());
    }

    /// ⑤ 指纹函数：本机返回非空且两次调用一致（不断言具体值）。
    #[test]
    fn fingerprint_nonempty_and_stable() {
        let a = device_fingerprint().expect("fingerprint should be available");
        let b = device_fingerprint().expect("fingerprint should be available");
        assert!(!a.value.trim().is_empty());
        assert_eq!(a.value, b.value);
        assert_eq!(a.source, b.source);
    }

    /// ⑥ derive_key 确定性：同一指纹两次派生一致，不同指纹派生不同。
    #[test]
    fn derive_key_deterministic_and_distinct() {
        let k1 = derive_key("fp-same");
        let k2 = derive_key("fp-same");
        let k3 = derive_key("fp-other");
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    /// 新增 ④-a：信封 fp_source 与本机当前来源不一致时，必须以 FingerprintSourceChanged 拒绝。
    #[test]
    fn fp_source_mismatch_rejected() {
        let current = device_fingerprint().unwrap();
        let mismatched = match current.source {
            FingerprintSource::Platform => FP_SOURCE_FALLBACK,
            FingerprintSource::Fallback => FP_SOURCE_PLATFORM,
        };
        let mut envelope = encrypt("payload").unwrap();
        // 加密时写入的来源必然与本机当前来源一致
        assert_eq!(envelope.fp_source, current.source.as_str());
        // 篡改为相反来源：应被来源校验拒绝，而不是静默换密钥尝试
        envelope.fp_source = mismatched.to_string();
        let err = decrypt(&envelope).unwrap_err();
        assert!(
            matches!(err, SecretsError::FingerprintSourceChanged(_)),
            "期望 FingerprintSourceChanged，实际: {}",
            err
        );
    }

    /// 新增 ⑤-a：老格式信封（无 fp_source 字段）经兼容 AAD 路径仍可成功解密。
    #[test]
    fn legacy_envelope_without_fp_source_still_decrypts() {
        let legacy = encrypt_legacy_for_test("legacy-payload");
        // 模拟磁盘上的老 JSON：完全不含 fp_source 键
        let json = serde_json::json!({
            "llmtm_encrypted": legacy.llmtm_encrypted,
            "nonce": legacy.nonce,
            "ciphertext": legacy.ciphertext,
        });
        let parsed: EncryptedEnvelope = serde_json::from_value(json).unwrap();
        assert!(parsed.is_legacy_format(), "缺失字段应被识别为老格式");
        assert_eq!(parsed.effective_fp_source(), FingerprintSource::Platform);

        let current = device_fingerprint().unwrap();
        if current.source == FingerprintSource::Platform {
            let dec = decrypt(&parsed).expect("老格式信封应通过兼容路径解密成功");
            assert_eq!(dec.as_str(), "legacy-payload");
        } else {
            // 本机恰好只能取得兜底指纹：老信封按规则视为 platform 来源，
            // 应被来源校验明确拒绝（这同样是预期且可预测的行为）。
            assert!(matches!(
                decrypt(&parsed),
                Err(SecretsError::FingerprintSourceChanged(_))
            ));
        }
    }

    /// 新增 ⑥-a：兜底指纹必须 host 与 user 同时存在，缺一即 None（消除弱绑定）。
    #[test]
    fn fallback_fingerprint_requires_host_and_user() {
        assert_eq!(
            compose_fallback_fingerprint(Some("host-a"), Some("user-b")),
            Some("host-a@user-b".to_string())
        );
        // host 缺失：绝不能退化为裸用户名（否则跨机器同名用户会得到同一把密钥）
        assert_eq!(compose_fallback_fingerprint(None, Some("user-b")), None);
        // user 缺失：同样返回 None
        assert_eq!(compose_fallback_fingerprint(Some("host-a"), None), None);
        // 空白字符串等同于缺失
        assert_eq!(
            compose_fallback_fingerprint(Some("   "), Some("user-b")),
            None
        );
        assert_eq!(compose_fallback_fingerprint(Some("host-a"), Some("")), None);
        assert_eq!(compose_fallback_fingerprint(None, None), None);
        // 两端空白应被裁剪后再组合
        assert_eq!(
            compose_fallback_fingerprint(Some(" host-a "), Some(" user-b ")),
            Some("host-a@user-b".to_string())
        );
    }

    /// 新写入的信封必须显式带上非空 fp_source，且与本机当前来源一致。
    #[test]
    fn new_envelope_records_fp_source() {
        let current = device_fingerprint().unwrap();
        let envelope = encrypt("payload").unwrap();
        assert!(!envelope.is_legacy_format());
        assert_eq!(envelope.fp_source, current.source.as_str());
        assert_eq!(envelope.effective_fp_source(), current.source);
    }
}
