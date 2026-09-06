use super::{http_client, LlmProvider, ProviderError};
use crate::models::{BalanceData, QuotaInfo};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;

const USER_AGENT: &str = "claude-code/2.1.0";

/// Claude Code 订阅计划配额供应商。
///
/// 从 `~/.claude/credentials.json` 读取 OAuth 令牌，调用 Anthropic OAuth 用量 API
/// 获取订阅计划配额（5小时/7天窗口）。
///
/// 与 Anthropic 组织成本 API（需管理员密钥）完全独立，适用于 Claude Pro/Max 订阅用户。
pub struct ClaudeCodeProvider {
    client: Client,
    cached_response: tokio::sync::OnceCell<ClaudeUsageResponse>,
}

impl ClaudeCodeProvider {
    pub fn new() -> Self {
        Self {
            client: http_client(),
            cached_response: tokio::sync::OnceCell::new(),
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

        let creds: ClaudeCredentialsFile = serde_json::from_str(&content)
            .map_err(|e| ProviderError(format!("Claude Code 凭证格式错误: {}", e)))?;

        let oauth = creds.claude_ai_oauth.ok_or_else(|| {
            ProviderError(
                "Claude Code 凭证中未找到 OAuth 信息，请先运行 `claude login`".to_string(),
            )
        })?;

        Ok(ClaudeCredentials {
            access_token: oauth.access_token,
        })
    }

    fn credentials_path() -> Result<PathBuf, ProviderError> {
        let home =
            dirs::home_dir().ok_or_else(|| ProviderError("无法获取用户主目录".to_string()))?;
        Ok(home.join(".claude").join("credentials.json"))
    }
}

#[async_trait]
impl LlmProvider for ClaudeCodeProvider {
    fn name(&self) -> &str {
        "Claude Code"
    }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        let usage = self.fetch_usage_response().await?;

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
        let usage = self.fetch_usage_response().await?;

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
    /// 通过 OnceCell 缓存只发一次请求，同一轮调度内 fetch_balance 与 fetch_quota_infos 共享结果。
    async fn fetch_usage_response(&self) -> Result<ClaudeUsageResponse, ProviderError> {
        if let Some(cached) = self.cached_response.get() {
            return Ok(cached.clone());
        }
        let creds = Self::read_credentials()?;
        let resp = self
            .client
            .get("https://api.anthropic.com/api/oauth/usage")
            .header("Authorization", format!("Bearer {}", creds.access_token))
            .header("anthropic-beta", "oauth-2025-04-20")
            .header("User-Agent", USER_AGENT)
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

        let resp: ClaudeUsageResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError(format!("Claude Code API 响应解析失败: {}", e)))?;
        let _ = self.cached_response.set(resp.clone());
        Ok(resp)
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

#[derive(Clone, Deserialize)]
struct ClaudeUsageResponse {
    five_hour: Option<ClaudeUsageWindow>,
    seven_day: Option<ClaudeUsageWindow>,
    #[serde(rename = "seven_day_opus")]
    seven_day_opus: Option<ClaudeUsageWindow>,
    #[serde(rename = "seven_day_sonnet")]
    seven_day_sonnet: Option<ClaudeUsageWindow>,
}

#[derive(Clone, Deserialize)]
struct ClaudeUsageWindow {
    utilization: f64,
    #[serde(rename = "resets_at")]
    resets_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ClaudeUsageResponse 反序列化测试 ---

    #[test]
    fn test_claude_usage_response_with_five_hour_and_seven_day() {
        let json = serde_json::json!({
            "five_hour": {
                "utilization": 35.5,
                "resets_at": "2025-09-06T18:00:00Z"
            },
            "seven_day": {
                "utilization": 60.0,
                "resets_at": "2025-09-12T00:00:00Z"
            },
            "seven_day_opus": {
                "utilization": 10.0,
                "resets_at": "2025-09-12T00:00:00Z"
            },
            "seven_day_sonnet": {
                "utilization": 20.0,
                "resets_at": "2025-09-12T00:00:00Z"
            }
        });
        let resp: ClaudeUsageResponse = serde_json::from_value(json).unwrap();
        let fh = resp.five_hour.unwrap();
        assert!((fh.utilization - 35.5).abs() < f64::EPSILON);
        assert_eq!(fh.resets_at.as_deref(), Some("2025-09-06T18:00:00Z"));
        let sd = resp.seven_day.unwrap();
        assert!((sd.utilization - 60.0).abs() < f64::EPSILON);
        let opus = resp.seven_day_opus.unwrap();
        assert!((opus.utilization - 10.0).abs() < f64::EPSILON);
        let sonnet = resp.seven_day_sonnet.unwrap();
        assert!((sonnet.utilization - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_claude_usage_response_empty() {
        let json = serde_json::json!({});
        let resp: ClaudeUsageResponse = serde_json::from_value(json).unwrap();
        assert!(resp.five_hour.is_none());
        assert!(resp.seven_day.is_none());
        assert!(resp.seven_day_opus.is_none());
        assert!(resp.seven_day_sonnet.is_none());
    }

    #[test]
    fn test_claude_usage_response_partial() {
        let json = serde_json::json!({
            "five_hour": {
                "utilization": 0.0,
                "resets_at": null
            }
        });
        let resp: ClaudeUsageResponse = serde_json::from_value(json).unwrap();
        let fh = resp.five_hour.unwrap();
        assert!((fh.utilization - 0.0).abs() < f64::EPSILON);
        assert!(fh.resets_at.is_none());
        assert!(resp.seven_day.is_none());
    }

    // --- ClaudeCredentialsFile 反序列化测试 ---

    #[test]
    fn test_claude_credentials_file() {
        let json = serde_json::json!({
            "claudeAiOauth": {
                "accessToken": "sk-ant-oauth-tok"
            }
        });
        let creds: ClaudeCredentialsFile = serde_json::from_value(json).unwrap();
        let oauth = creds.claude_ai_oauth.unwrap();
        assert_eq!(oauth.access_token, "sk-ant-oauth-tok");
    }

    #[test]
    fn test_claude_credentials_file_missing_oauth() {
        let json = serde_json::json!({});
        let creds: ClaudeCredentialsFile = serde_json::from_value(json).unwrap();
        assert!(creds.claude_ai_oauth.is_none());
    }

    // --- format_iso_reset 测试 ---

    #[test]
    fn test_format_iso_reset_future_days() {
        use chrono::Utc;
        let future = (Utc::now() + chrono::Duration::days(3)).to_rfc3339();
        let result = format_iso_reset(&future);
        assert!(result.contains("天后重置"), "got: {}", result);
    }

    #[test]
    fn test_format_iso_reset_future_hours() {
        use chrono::Utc;
        let future = (Utc::now() + chrono::Duration::hours(4)).to_rfc3339();
        let result = format_iso_reset(&future);
        assert!(result.contains("小时后重置"), "got: {}", result);
    }

    #[test]
    fn test_format_iso_reset_invalid_string() {
        let result = format_iso_reset("not-a-date");
        assert_eq!(result, "not-a-date");
    }
}
