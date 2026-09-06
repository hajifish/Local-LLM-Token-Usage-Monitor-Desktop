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
}

impl CodexProvider {
    pub fn new() -> Self {
        Self {
            client: http_client(),
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

        let creds: CodexAuthFile = serde_json::from_str(&content).map_err(|e| {
            ProviderError(format!("Codex CLI 凭证格式错误: {}", e))
        })?;

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
        let home = dirs::home_dir().ok_or_else(|| {
            ProviderError("无法获取用户主目录".to_string())
        })?;
        Ok(home.join(".codex").join("auth.json"))
    }
}

#[async_trait]
impl LlmProvider for CodexProvider {
    fn name(&self) -> &str {
        "Codex"
    }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        let creds = Self::read_credentials()?;
        let usage = self.fetch_usage_response(&creds).await?;

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
        let creds = Self::read_credentials()?;
        let usage = self.fetch_usage_response(&creds).await?;

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
    async fn fetch_usage_response(
        &self,
        creds: &CodexCredentials,
    ) -> Result<CodexUsageResponse, ProviderError> {
        let mut request = self
            .client
            .get("https://chatgpt.com/backend-api/wham/usage")
            .header("Authorization", format!("Bearer {}", creds.access_token))
            .header("Accept", "application/json");

        if let Some(ref account_id) = creds.account_id {
            request = request.header("ChatGPT-Account-Id", account_id);
        }

        let resp = request.send().await.map_err(|e| {
            ProviderError(format!("Codex API 请求失败: {}", e))
        })?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError(
                "Codex 登录已过期，请重新运行 `codex login`".to_string(),
            ));
        }
        if !status.is_success() {
            return Err(ProviderError(format!("Codex API error: {}", status)));
        }

        resp.json().await.map_err(|e| {
            ProviderError(format!("Codex API 响应解析失败: {}", e))
        })
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

#[derive(Deserialize)]
struct CodexUsageResponse {
    #[serde(rename = "plan_type")]
    plan_type: Option<String>,
    #[serde(rename = "rate_limit")]
    rate_limit: Option<CodexRateLimit>,
}

#[derive(Deserialize)]
struct CodexRateLimit {
    #[serde(rename = "primary_window")]
    primary_window: Option<CodexWindow>,
    #[serde(rename = "secondary_window")]
    secondary_window: Option<CodexWindow>,
}

#[derive(Deserialize)]
struct CodexWindow {
    #[serde(rename = "used_percent")]
    used_percent: i32,
    #[serde(rename = "reset_at")]
    reset_at: i64,
}
