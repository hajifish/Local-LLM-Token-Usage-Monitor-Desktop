use super::{http_client, LlmProvider, ProviderError};
use crate::models::{BalanceData, QuotaInfo};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;

/// Kimi Code 订阅计划配额供应商。
///
/// 从 `~/.kimi-code/config.toml` 读取 API Key，调用 Kimi 网页端
/// `GetSubscriptionStats` 端点获取订阅计划配额（5小时/7天/订阅余额）。
///
/// 注意：Kimi Code (api.kimi.com) 与 Kimi 开放平台 (api.moonshot.cn) 是两套独立系统。
pub struct KimiCodeProvider {
    client: Client,
}

impl KimiCodeProvider {
    pub fn new() -> Self {
        Self {
            client: http_client(),
        }
    }

    /// 从 `~/.kimi-code/config.toml` 读取 Kimi Code API Key。
    ///
    /// 配置文件格式 (TOML):
    /// ```toml
    /// [providers."managed:kimi-code"]
    /// type = "kimi"
    /// base_url = "https://api.kimi.com/coding/v1"
    /// api_key = "sk-xxx"
    /// ```
    fn read_api_key() -> Result<String, ProviderError> {
        let path = Self::config_path()?;
        let content = std::fs::read_to_string(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ProviderError(
                    "未找到 Kimi Code 配置，请先安装并登录 Kimi Code CLI".to_string(),
                )
            } else {
                ProviderError(format!("读取 Kimi Code 配置文件失败: {}", e))
            }
        })?;

        // 简易 TOML 解析：查找 api_key = "..." 行
        // 避免引入 toml crate 依赖，仅提取所需字段
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("api_key") {
                if let Some(val) = trimmed.split('=').nth(1) {
                    let val = val.trim().trim_matches('"').trim_matches('\'');
                    if !val.is_empty() {
                        return Ok(val.to_string());
                    }
                }
            }
        }

        Err(ProviderError(
            "Kimi Code 配置中未找到 api_key，请重新登录 Kimi Code CLI".to_string(),
        ))
    }

    fn config_path() -> Result<PathBuf, ProviderError> {
        let home = dirs::home_dir().ok_or_else(|| {
            ProviderError("无法获取用户主目录".to_string())
        })?;
        Ok(home.join(".kimi-code").join("config.toml"))
    }
}

#[async_trait]
impl LlmProvider for KimiCodeProvider {
    fn name(&self) -> &str {
        "Kimi Code"
    }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        let api_key = Self::read_api_key()?;
        let stats = self.fetch_subscription_stats(&api_key).await?;

        // 主窗口：优先 5h，回退到 7d，再回退到订阅余额
        let remaining = stats
            .ratelimit_code_5h
            .as_ref()
            .filter(|r| r.enabled)
            .map(|r| 100.0 - r.ratio * 100.0)
            .or_else(|| {
                stats
                    .ratelimit_code_7d
                    .as_ref()
                    .filter(|r| r.enabled)
                    .map(|r| 100.0 - r.ratio * 100.0)
            })
            .or_else(|| {
                stats.subscription_balance.as_ref().map(|sb| {
                    let used = sb.kimi_code_used_ratio.unwrap_or(sb.amount_used_ratio);
                    100.0 - used * 100.0
                })
            })
            .unwrap_or(100.0);

        Ok(BalanceData {
            provider: "Kimi Code".to_string(),
            available_balance: remaining,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: remaining,
            currency: "%".to_string(),
        })
    }

    async fn fetch_quota_infos(&self) -> Result<Option<Vec<QuotaInfo>>, ProviderError> {
        let api_key = Self::read_api_key()?;
        let stats = self.fetch_subscription_stats(&api_key).await?;

        let mut quotas = Vec::new();

        if let Some(ref r) = stats.ratelimit_code_5h {
            if r.enabled {
                let used_pct = r.ratio * 100.0;
                quotas.push(QuotaInfo {
                    name: "5小时".to_string(),
                    used: used_pct,
                    total: 100.0,
                    current_value: None,
                    remaining: Some(100.0 - used_pct),
                    remaining_percent: 100.0 - used_pct,
                    reset_time: r.reset_time.as_ref().map(|s| format_iso_reset(s)),
                    plan_label: Some("Kimi Code".to_string()),
                });
            }
        }

        if let Some(ref r) = stats.ratelimit_code_7d {
            if r.enabled {
                let used_pct = r.ratio * 100.0;
                quotas.push(QuotaInfo {
                    name: "7天".to_string(),
                    used: used_pct,
                    total: 100.0,
                    current_value: None,
                    remaining: Some(100.0 - used_pct),
                    remaining_percent: 100.0 - used_pct,
                    reset_time: r.reset_time.as_ref().map(|s| format_iso_reset(s)),
                    plan_label: Some("Kimi Code".to_string()),
                });
            }
        }

        if let Some(ref sb) = stats.subscription_balance {
            let used_ratio = sb.kimi_code_used_ratio.unwrap_or(sb.amount_used_ratio);
            let used_pct = used_ratio * 100.0;
            quotas.push(QuotaInfo {
                name: "订阅余额".to_string(),
                used: used_pct,
                total: 100.0,
                current_value: None,
                remaining: Some(100.0 - used_pct),
                remaining_percent: 100.0 - used_pct,
                reset_time: sb.expire_time.as_ref().map(|s| format_iso_reset(s)),
                plan_label: Some("Kimi Code".to_string()),
            });
        }

        if quotas.is_empty() {
            Ok(None)
        } else {
            Ok(Some(quotas))
        }
    }
}

impl KimiCodeProvider {
    async fn fetch_subscription_stats(
        &self,
        api_key: &str,
    ) -> Result<KimiSubscriptionStats, ProviderError> {
        // 尝试通过 Kimi Code API 获取订阅统计
        // 使用 connect-RPC 风格的 POST 请求
        let resp = self
            .client
            .post("https://www.kimi.com/api/gateway/membership.v2.MembershipService/GetSubscriptionStats")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("User-Agent", "kimi-code-cli")
            .body("{}")
            .send()
            .await
            .map_err(|e| ProviderError(format!("Kimi Code API 请求失败: {}", e)))?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(ProviderError(
                "Kimi Code 登录已过期，请重新登录 Kimi Code CLI".to_string(),
            ));
        }
        if !status.is_success() {
            return Err(ProviderError(format!(
                "Kimi Code API 返回错误: {}（可能需要更新）",
                status
            )));
        }

        let wrapper: KimiStatsWrapper = resp.json().await.map_err(|e| {
            ProviderError(format!("Kimi Code API 响应解析失败: {}", e))
        })?;

        Ok(wrapper.data.unwrap_or_default())
    }
}

/// 将 ISO 8601 时间字符串转换为相对时间描述。
fn format_iso_reset(iso: &str) -> String {
    use chrono::{DateTime, Utc};
    match iso.parse::<DateTime<Utc>>() {
        Ok(dt) => {
            let now = Utc::now();
            let duration = dt.signed_duration_since(now);
            if duration.num_days() > 0 {
                format!("{}天后重置", duration.num_days())
            } else if duration.num_hours() > 0 {
                format!("{}小时后重置", duration.num_hours())
            } else if duration.num_minutes() > 0 {
                format!("{}分钟后重置", duration.num_minutes())
            } else {
                "即将重置".to_string()
            }
        }
        Err(_) => iso.to_string(),
    }
}

// --- 响应数据结构 ---

#[derive(Deserialize)]
struct KimiStatsWrapper {
    data: Option<KimiSubscriptionStats>,
}

#[derive(Deserialize, Default)]
struct KimiSubscriptionStats {
    #[serde(rename = "ratelimitCode5h")]
    ratelimit_code_5h: Option<KimiRateLimit>,
    #[serde(rename = "ratelimitCode7d")]
    ratelimit_code_7d: Option<KimiRateLimit>,
    #[serde(rename = "subscriptionBalance")]
    subscription_balance: Option<KimiSubscriptionBalance>,
}

#[derive(Deserialize)]
struct KimiRateLimit {
    /// 使用比例，0~1 的小数（如 0.0002 表示 0.02%）
    ratio: f64,
    enabled: bool,
    #[serde(rename = "resetTime")]
    reset_time: Option<String>,
}

#[derive(Deserialize)]
struct KimiSubscriptionBalance {
    #[serde(rename = "amountUsedRatio")]
    amount_used_ratio: f64,
    /// Kimi Code 专用使用比例，优先于 amount_used_ratio
    #[serde(rename = "kimiCodeUsedRatio")]
    kimi_code_used_ratio: Option<f64>,
    #[serde(rename = "expireTime")]
    expire_time: Option<String>,
}
