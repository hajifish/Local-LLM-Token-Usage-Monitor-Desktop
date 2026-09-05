use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use crate::models::{BalanceData, UsageData};
use super::{LlmProvider, ProviderError};

pub struct DeepSeekProvider {
    client: Client,
    api_key: String,
    platform_token: Option<String>,
}

#[derive(Deserialize)]
struct BalanceResponse {
    is_available: bool,
    balance_infos: Vec<BalanceInfo>,
}

#[derive(Deserialize)]
struct BalanceInfo {
    currency: String,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
}

#[derive(Deserialize)]
struct UsageResponse {
    biz_data: Option<UsageBizData>,
}

#[derive(Deserialize)]
struct UsageBizData {
    days: Option<Vec<UsageDay>>,
}

#[derive(Deserialize)]
struct UsageDay {
    date: String,
    data: Option<Vec<UsageDayData>>,
}

#[derive(Deserialize)]
struct UsageDayData {
    usage: Option<Vec<UsageItem>>,
}

#[derive(Deserialize)]
struct UsageItem {
    #[serde(rename = "type")]
    item_type: Option<String>,
    amount: Option<String>,
}

impl DeepSeekProvider {
    pub fn new(api_key: &str, platform_token: Option<&str>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            platform_token: platform_token.map(|s| s.to_string()),
        }
    }
}

#[async_trait]
impl LlmProvider for DeepSeekProvider {
    fn name(&self) -> &str { "DeepSeek" }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        let resp = self.client.get("https://api.deepseek.com/user/balance")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send().await.map_err(|e| ProviderError(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(ProviderError(format!("DeepSeek API error: {}", resp.status())));
        }

        let data: BalanceResponse = resp.json().await.map_err(|e| ProviderError(e.to_string()))?;
        let info = data.balance_infos.first()
            .ok_or_else(|| ProviderError("No balance info".to_string()))?;

        let total: f64 = info.total_balance.parse().unwrap_or(0.0);
        let granted: f64 = info.granted_balance.parse().unwrap_or(0.0);
        let topped_up: f64 = info.topped_up_balance.parse().unwrap_or(0.0);

        // 币种归一化：total_balance 累加采用严格白名单 is_cny()（精确 "CNY"），
        // 若接口返回大小写/别名等非精确 "CNY"，DeepSeek 余额会被静默排除出“总余额 ¥”。
        // 接口返回 "CNY" 时行为与现状完全一致。
        let currency = if info.currency.eq_ignore_ascii_case("cny") {
            "CNY".to_string()
        } else {
            info.currency.clone()
        };

        Ok(BalanceData {
            provider: "DeepSeek".to_string(),
            available_balance: total,
            voucher_balance: granted,
            cash_balance: topped_up,
            total_balance: total,
            currency,
        })
    }

    async fn fetch_usage(&self) -> Result<Option<UsageData>, ProviderError> {
        let platform_token = match &self.platform_token {
            Some(t) => t,
            None => return Ok(None),
        };

        let now = chrono::Local::now();
        let month = now.format("%m").to_string();
        let year = now.format("%Y").to_string();
        let today = now.format("%Y-%m-%d").to_string();

        let url = format!(
            "https://platform.deepseek.com/api/v0/usage/amount?month={}&year={}",
            month, year
        );

        let resp = self.client.get(&url)
            .header("Authorization", format!("Bearer {}", platform_token))
            .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
            .send().await.map_err(|e| ProviderError(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(ProviderError(format!("DeepSeek platform API error: {}", resp.status())));
        }

        let data: UsageResponse = resp.json().await.map_err(|e| ProviderError(e.to_string()))?;

        let days = match data.biz_data.and_then(|b| b.days) {
            Some(d) => d,
            None => return Ok(None),
        };

        // Find today's data
        let today_data = days.iter().find(|d| d.date == today);
        let today_entries = match today_data.and_then(|d| d.data.as_ref()) {
            Some(entries) => entries,
            None => return Ok(None),
        };

        // Sum up today's usage
        let mut total_tokens: u64 = 0;
        let mut prompt_tokens: u64 = 0;
        let mut completion_tokens: u64 = 0;
        let mut requests: u64 = 0;

        for entry in today_entries {
            if let Some(ref items) = entry.usage {
                for item in items {
                    let amount: u64 = item.amount.as_ref()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    
                    match item.item_type.as_deref() {
                        Some("REQUEST") => requests += amount,
                        Some("PROMPT_TOKEN") | Some("PROMPT_CACHE_MISS_TOKEN") | Some("PROMPT_CACHE_HIT_TOKEN") => {
                            prompt_tokens += amount;
                            total_tokens += amount;
                        }
                        Some("RESPONSE_TOKEN") => {
                            completion_tokens += amount;
                            total_tokens += amount;
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(Some(UsageData {
            provider: "DeepSeek".to_string(),
            total_tokens,
            prompt_tokens,
            completion_tokens,
            today_cost: None, // Cost requires a separate API call
            today_requests: Some(requests),
        }))
    }
}
