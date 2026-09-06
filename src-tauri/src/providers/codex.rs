use super::{http_client, LlmProvider, ProviderError};
use crate::models::{BalanceData, QuotaInfo};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;

/// Codex (OpenAI) 订阅计划配额供应商。
///
/// 从 `~/.codex/auth.json` 读取 OAuth 令牌，调用 ChatGPT 用量 API 获取订阅计划配额。
/// 与 OpenAI 组织成本 API（需管理员密钥）完全独立，适用于 Plus/Pro/Team 订阅用户。
pub struct CodexProvider {
    client: Client,
    cached_response: tokio::sync::OnceCell<CodexUsageResponse>,
}

impl CodexProvider {
    pub fn new() -> Self {
        Self {
            client: http_client(),
            cached_response: tokio::sync::OnceCell::new(),
        }
    }

    /// 读取 Codex CLI 凭证文件 (`~/.codex/auth.json`)。
    fn read_credentials() -> Result<CodexCredentials, ProviderError> {
        let path = Self::credentials_path()?;
        let content = std::fs::read_to_string(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ProviderError("未找到 Codex CLI 凭证，请先运行 `codex login`".to_string())
            } else {
                ProviderError(format!("读取 Codex 凭证文件失败: {}", e))
            }
        })?;

        let creds: CodexAuthFile = serde_json::from_str(&content)
            .map_err(|e| ProviderError(format!("Codex CLI 凭证格式错误: {}", e)))?;

        // 支持两种格式：OAuth tokens 或 API Key
        if let Some(tokens) = creds.tokens {
            Ok(CodexCredentials {
                access_token: tokens.access_token,
                account_id: tokens.account_id,
            })
        } else if let Some(api_key) = creds.openai_api_key {
            // API Key 模式：account_id 为空
            Ok(CodexCredentials {
                access_token: api_key,
                account_id: None,
            })
        } else if let Some(pat) = creds.personal_access_token {
            Ok(CodexCredentials {
                access_token: pat,
                account_id: None,
            })
        } else {
            Err(ProviderError(
                "Codex CLI 凭证中未找到有效的 access_token 或 API Key".to_string(),
            ))
        }
    }

    fn credentials_path() -> Result<PathBuf, ProviderError> {
        let home =
            dirs::home_dir().ok_or_else(|| ProviderError("无法获取用户主目录".to_string()))?;
        Ok(home.join(".codex").join("auth.json"))
    }
}

#[async_trait]
impl LlmProvider for CodexProvider {
    fn name(&self) -> &str {
        "Codex"
    }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        let usage = self.fetch_usage_response().await?;

        // 主窗口：优先 primary_window (5小时)，回退到 secondary_window (7天)
        let remaining = usage
            .rate_limit
            .as_ref()
            .and_then(|rl| rl.primary_window.as_ref())
            .map(|w| 100.0 - w.used_percent as f64)
            .or_else(|| {
                usage
                    .rate_limit
                    .as_ref()
                    .and_then(|rl| rl.secondary_window.as_ref())
                    .map(|w| 100.0 - w.used_percent as f64)
            })
            .unwrap_or(100.0);

        Ok(BalanceData {
            provider: "Codex".to_string(),
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
        let plan_label = usage.plan_type.clone();

        if let Some(ref rl) = usage.rate_limit {
            if let Some(ref pw) = rl.primary_window {
                quotas.push(QuotaInfo {
                    name: "5小时窗口".to_string(),
                    used: pw.used_percent as f64,
                    total: 100.0,
                    current_value: None,
                    remaining: Some(100.0 - pw.used_percent as f64),
                    remaining_percent: 100.0 - pw.used_percent as f64,
                    reset_time: Some(format_reset_time(pw.reset_at)),
                    plan_label: plan_label.clone(),
                });
            }
            if let Some(ref sw) = rl.secondary_window {
                quotas.push(QuotaInfo {
                    name: "7天窗口".to_string(),
                    used: sw.used_percent as f64,
                    total: 100.0,
                    current_value: None,
                    remaining: Some(100.0 - sw.used_percent as f64),
                    remaining_percent: 100.0 - sw.used_percent as f64,
                    reset_time: Some(format_reset_time(sw.reset_at)),
                    plan_label: plan_label.clone(),
                });
            }
        }

        if quotas.is_empty() {
            Ok(None)
        } else {
            Ok(Some(quotas))
        }
    }
}

impl CodexProvider {
    /// 通过 OnceCell 缓存只发一次请求，同一轮调度内 fetch_balance 与 fetch_quota_infos 共享结果。
    async fn fetch_usage_response(&self) -> Result<CodexUsageResponse, ProviderError> {
        if let Some(cached) = self.cached_response.get() {
            return Ok(cached.clone());
        }
        let creds = Self::read_credentials()?;
        let mut request = self
            .client
            .get("https://chatgpt.com/backend-api/wham/usage")
            .header("Authorization", format!("Bearer {}", creds.access_token))
            .header("Accept", "application/json");

        if let Some(ref account_id) = creds.account_id {
            request = request.header("ChatGPT-Account-Id", account_id);
        }

        let resp = request
            .send()
            .await
            .map_err(|e| ProviderError(format!("Codex API 请求失败: {}", e)))?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError(
                "Codex 登录已过期，请重新运行 `codex login`".to_string(),
            ));
        }
        if !status.is_success() {
            return Err(ProviderError(format!("Codex API error: {}", status)));
        }

        let resp: CodexUsageResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError(format!("Codex API 响应解析失败: {}", e)))?;
        let _ = self.cached_response.set(resp.clone());
        Ok(resp)
    }
}

/// 将 Unix 时间戳转换为相对时间描述（如 "2小时后重置"）。
fn format_reset_time(reset_at: i64) -> String {
    use chrono::{DateTime, Utc};
    let reset_dt = DateTime::<Utc>::from_timestamp(reset_at, 0);
    match reset_dt {
        Some(dt) => {
            let now = Utc::now();
            let duration = dt.signed_duration_since(now);
            if duration.num_hours() > 0 {
                format!("{}小时后重置", duration.num_hours())
            } else if duration.num_minutes() > 0 {
                format!("{}分钟后重置", duration.num_minutes())
            } else {
                "即将重置".to_string()
            }
        }
        None => String::new(),
    }
}

// --- 响应数据结构 ---

#[derive(Deserialize)]
struct CodexAuthFile {
    tokens: Option<CodexTokens>,
    #[serde(rename = "OPENAI_API_KEY")]
    openai_api_key: Option<String>,
    #[serde(rename = "personal_access_token")]
    personal_access_token: Option<String>,
}

#[derive(Deserialize)]
struct CodexTokens {
    access_token: String,
    #[serde(rename = "account_id")]
    account_id: Option<String>,
}

struct CodexCredentials {
    access_token: String,
    account_id: Option<String>,
}

#[derive(Clone, Deserialize)]
struct CodexUsageResponse {
    #[serde(rename = "plan_type")]
    plan_type: Option<String>,
    #[serde(rename = "rate_limit")]
    rate_limit: Option<CodexRateLimit>,
}

#[derive(Clone, Deserialize)]
struct CodexRateLimit {
    #[serde(rename = "primary_window")]
    primary_window: Option<CodexWindow>,
    #[serde(rename = "secondary_window")]
    secondary_window: Option<CodexWindow>,
}

#[derive(Clone, Deserialize)]
struct CodexWindow {
    #[serde(rename = "used_percent")]
    used_percent: i32,
    #[serde(rename = "reset_at")]
    reset_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- CodexUsageResponse 反序列化测试 ---

    #[test]
    fn test_codex_usage_response_with_primary_window() {
        let json = serde_json::json!({
            "plan_type": "Plus",
            "rate_limit": {
                "primary_window": {
                    "used_percent": 42,
                    "reset_at": 1700000000
                },
                "secondary_window": {
                    "used_percent": 10,
                    "reset_at": 1700500000
                }
            }
        });
        let resp: CodexUsageResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.plan_type.as_deref(), Some("Plus"));
        let rl = resp.rate_limit.unwrap();
        let pw = rl.primary_window.unwrap();
        assert_eq!(pw.used_percent, 42);
        assert_eq!(pw.reset_at, 1700000000);
        let sw = rl.secondary_window.unwrap();
        assert_eq!(sw.used_percent, 10);
    }

    #[test]
    fn test_codex_usage_response_with_secondary_only() {
        let json = serde_json::json!({
            "plan_type": "Team",
            "rate_limit": {
                "secondary_window": {
                    "used_percent": 55,
                    "reset_at": 1700600000
                }
            }
        });
        let resp: CodexUsageResponse = serde_json::from_value(json).unwrap();
        let rl = resp.rate_limit.unwrap();
        assert!(rl.primary_window.is_none());
        let sw = rl.secondary_window.unwrap();
        assert_eq!(sw.used_percent, 55);
    }

    #[test]
    fn test_codex_usage_response_empty() {
        let json = serde_json::json!({});
        let resp: CodexUsageResponse = serde_json::from_value(json).unwrap();
        assert!(resp.plan_type.is_none());
        assert!(resp.rate_limit.is_none());
    }

    // --- CodexAuthFile 反序列化测试 ---

    #[test]
    fn test_codex_auth_file_oauth_tokens() {
        let json = serde_json::json!({
            "tokens": {
                "access_token": "tok_abc123",
                "account_id": "acct_xyz"
            }
        });
        let auth: CodexAuthFile = serde_json::from_value(json).unwrap();
        let tokens = auth.tokens.unwrap();
        assert_eq!(tokens.access_token, "tok_abc123");
        assert_eq!(tokens.account_id.as_deref(), Some("acct_xyz"));
        assert!(auth.openai_api_key.is_none());
        assert!(auth.personal_access_token.is_none());
    }

    #[test]
    fn test_codex_auth_file_api_key() {
        let json = serde_json::json!({
            "OPENAI_API_KEY": "sk-test-key-123"
        });
        let auth: CodexAuthFile = serde_json::from_value(json).unwrap();
        assert!(auth.tokens.is_none());
        assert_eq!(auth.openai_api_key.as_deref(), Some("sk-test-key-123"));
    }

    #[test]
    fn test_codex_auth_file_personal_access_token() {
        let json = serde_json::json!({
            "personal_access_token": "pat_secret_value"
        });
        let auth: CodexAuthFile = serde_json::from_value(json).unwrap();
        assert!(auth.tokens.is_none());
        assert!(auth.openai_api_key.is_none());
        assert_eq!(
            auth.personal_access_token.as_deref(),
            Some("pat_secret_value")
        );
    }

    // --- format_reset_time 测试 ---

    #[test]
    fn test_format_reset_time_future_hours() {
        use chrono::Utc;
        let future_ts = Utc::now().timestamp() + 7200;
        let result = format_reset_time(future_ts);
        assert!(result.contains("小时后重置"), "got: {}", result);
    }

    #[test]
    fn test_format_reset_time_future_minutes() {
        use chrono::Utc;
        let future_ts = Utc::now().timestamp() + 1800;
        let result = format_reset_time(future_ts);
        assert!(result.contains("分钟后重置"), "got: {}", result);
    }

    #[test]
    fn test_format_reset_time_invalid_timestamp() {
        let result = format_reset_time(i64::MAX);
        assert!(result.is_empty(), "got: {}", result);
    }
}
