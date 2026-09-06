use super::{http_client, LlmProvider, ProviderError};
use crate::models::{BalanceData, QuotaInfo};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;

/// Claude Code 订阅计划配额供应商。
///
/// 从 `~/.claude/credentials.json` 读取 OAuth 令牌，调用 Anthropic OAuth 用量 API
/// 获取订阅计划配额（5小时/7天窗口）。
///
/// 与 Anthropic 组织成本 API（需管理员密钥）完全独立，适用于 Claude Pro/Max 订阅用户。
pub struct ClaudeCodeProvider {
    client: Client,
}

impl ClaudeCodeProvider {
    pub fn new() -> Self {
        Self {
            client: http_client(),
        }
    }

    /// 读取 Claude Code CLI 凭证文件 (`~/.claude/credentials.json`)。
    fn read_credentials() -> Result<ClaudeCredentials, ProviderError> {
        let path = Self::credentials_path()?;
        let content = std::fs::read_to_string(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ProviderError("未找到 Claude Code 凭证，请先运行 `claude login`".to_string())
            } else {
                ProviderError(format!("读取 Claude Code 凭证文件失败: {}", e))
            }
        })?;

        let creds: ClaudeCredentialsFile = serde_json::from_str(&content).map_err(|e| {
            ProviderError(format!("Claude Code 凭证格式错误: {}", e))
        })?;

        let oauth = creds.claude_ai_oauth.ok_or_else(|| {
            ProviderError("Claude Code 凭证中未找到 OAuth 信息，请先运行 `claude login`".to_string())
        })?;

        Ok(ClaudeCredentials {
            access_token: oauth.access_token,
        })
    }

    fn credentials_path() -> Result<PathBuf, ProviderError> {
        let home = dirs::home_dir().ok_or_else(|| {
            ProviderError("无法获取用户主目录".to_string())
        })?;
        Ok(home.join(".claude").join("credentials.json"))
    }
}

#[async_trait]
impl LlmProvider for ClaudeCodeProvider {
    fn name(&self) -> &str {
        "Claude Code"
    }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        let creds = Self::read_credentials()?;
        let usage = self.fetch_usage_response(&creds).await?;

        // 主窗口：优先 five_hour，回退到 seven_day
        let remaining = usage
            .five_hour
            .as_ref()
            .map(|w| 100.0 - w.utilization)
            .or_else(|| usage.seven_day.as_ref().map(|w| 100.0 - w.utilization))
            .unwrap_or(100.0);

        Ok(BalanceData {
            provider: "Claude Code".to_string(),
            available_balance: remaining,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: remaining,
            currency: "%".to_string(),
        })
    }

    async fn fetch_quota_infos(&self) -> Result<Option<Vec<QuotaInfo>>, ProviderError> {
        let creds = Self::read_credentials()?;
        let usage = self.fetch_usage_response(&creds).await?;

        let mut quotas = Vec::new();

        if let Some(ref w) = usage.five_hour {
            quotas.push(QuotaInfo {
                name: "5小时".to_string(),
                used: w.utilization,
                total: 100.0,
                current_value: None,
                remaining: Some(100.0 - w.utilization),
                remaining_percent: 100.0 - w.utilization,
                reset_time: w.resets_at.as_ref().map(|s| format_iso_reset(s)),
                plan_label: None,
            });
        }

        if let Some(ref w) = usage.seven_day {
            quotas.push(QuotaInfo {
                name: "7天".to_string(),
                used: w.utilization,
                total: 100.0,
                current_value: None,
                remaining: Some(100.0 - w.utilization),
                remaining_percent: 100.0 - w.utilization,
                reset_time: w.resets_at.as_ref().map(|s| format_iso_reset(s)),
                plan_label: None,
            });
        }

        if let Some(ref w) = usage.seven_day_opus {
            quotas.push(QuotaInfo {
                name: "7天Opus".to_string(),
                used: w.utilization,
                total: 100.0,
                current_value: None,
                remaining: Some(100.0 - w.utilization),
                remaining_percent: 100.0 - w.utilization,
                reset_time: w.resets_at.as_ref().map(|s| format_iso_reset(s)),
                plan_label: None,
            });
        }

        if let Some(ref w) = usage.seven_day_sonnet {
            quotas.push(QuotaInfo {
                name: "7天Sonnet".to_string(),
                used: w.utilization,
                total: 100.0,
                current_value: None,
                remaining: Some(100.0 - w.utilization),
                remaining_percent: 100.0 - w.utilization,
                reset_time: w.resets_at.as_ref().map(|s| format_iso_reset(s)),
                plan_label: None,
            });
        }

        if quotas.is_empty() {
            Ok(None)
        } else {
            Ok(Some(quotas))
        }
    }
}

impl ClaudeCodeProvider {
    async fn fetch_usage_response(
        &self,
        creds: &ClaudeCredentials,
    ) -> Result<ClaudeUsageResponse, ProviderError> {
        let resp = self
            .client
            .get("https://api.anthropic.com/api/oauth/usage")
            .header("Authorization", format!("Bearer {}", creds.access_token))
            .header("anthropic-beta", "oauth-2025-04-20")
            .header("User-Agent", "claude-code/2.1.0")
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| ProviderError(format!("Claude Code API 请求失败: {}", e)))?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError(
                "Claude Code 登录已过期，请重新运行 `claude login`".to_string(),
            ));
        }
        if !status.is_success() {
            return Err(ProviderError(format!(
                "Claude Code API 返回错误: {}（可能需要更新）",
                status
            )));
        }

        resp.json().await.map_err(|e| {
            ProviderError(format!("Claude Code API 响应解析失败: {}", e))
        })
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
struct ClaudeCredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<ClaudeAiOauth>,
}

#[derive(Deserialize)]
struct ClaudeAiOauth {
    #[serde(rename = "accessToken")]
    access_token: String,
}

struct ClaudeCredentials {
    access_token: String,
}

#[derive(Deserialize)]
struct ClaudeUsageResponse {
    five_hour: Option<ClaudeUsageWindow>,
    seven_day: Option<ClaudeUsageWindow>,
    #[serde(rename = "seven_day_opus")]
    seven_day_opus: Option<ClaudeUsageWindow>,
    #[serde(rename = "seven_day_sonnet")]
    seven_day_sonnet: Option<ClaudeUsageWindow>,
}

#[derive(Deserialize)]
struct ClaudeUsageWindow {
    utilization: f64,
    #[serde(rename = "resets_at")]
    resets_at: Option<String>,
}
