use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use crate::models::{BalanceData, QuotaInfo};
use super::{LlmProvider, ProviderError};

pub struct ZhipuProvider {
    client: Client,
    api_key: String,
}

#[derive(Deserialize)]
struct ZhipuQuotaResponse {
    data: Option<ZhipuQuotaData>,
}

#[derive(Deserialize)]
struct ZhipuQuotaData {
    #[serde(rename = "planName")]
    plan_name: Option<String>,
    limits: Option<Vec<ZhipuLimit>>,
    level: Option<String>,
}

#[derive(Deserialize)]
struct ZhipuLimit {
    #[serde(rename = "type")]
    limit_type: Option<String>,
    unit: Option<u64>,
    number: Option<u64>,
    name: Option<String>,
    usage: Option<f64>,
    used: Option<f64>,
    limit: Option<f64>,
    #[serde(rename = "currentValue")]
    current_value: Option<f64>,
    remaining: Option<f64>,
    #[serde(rename = "nextResetTime")]
    next_reset_time: Option<u64>,
    percentage: Option<f64>,
}

impl ZhipuProvider {
    pub fn new(api_key: &str) -> Self {
        Self { client: Client::new(), api_key: api_key.to_string() }
    }

    fn format_reset_time(epoch_ms: u64) -> String {
        let secs = epoch_ms / 1000;
        let dt = chrono::DateTime::from_timestamp(secs as i64, 0);
        match dt {
            Some(dt) => dt.with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string(),
            None => String::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for ZhipuProvider {
    fn name(&self) -> &str { "Zhipu" }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        let resp = self.client.get("https://open.bigmodel.cn/api/monitor/usage/quota/limit")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send().await.map_err(|e| ProviderError(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(ProviderError(format!("Zhipu API error: {}", resp.status())));
        }

        let data: ZhipuQuotaResponse = resp.json().await.map_err(|e| ProviderError(e.to_string()))?;

        let remaining = data.data
            .as_ref()
            .and_then(|d| d.limits.as_ref())
            .and_then(|limits| limits.first())
            .and_then(|l| l.percentage)
            .map(|p| 100.0 - p)
            .unwrap_or(0.0);

        Ok(BalanceData {
            provider: "Zhipu".to_string(),
            available_balance: remaining,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: 100.0,
            currency: "%".to_string(),
        })
    }

    async fn fetch_quota_infos(&self) -> Result<Option<Vec<QuotaInfo>>, ProviderError> {
        let resp = self.client.get("https://open.bigmodel.cn/api/monitor/usage/quota/limit")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send().await.map_err(|e| ProviderError(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(ProviderError(format!("Zhipu API error: {}", resp.status())));
        }

        let data: ZhipuQuotaResponse = resp.json().await.map_err(|e| ProviderError(e.to_string()))?;

        let plan_name = data.data.as_ref().and_then(|d| d.plan_name.clone());
        let level = data.data.as_ref().and_then(|d| d.level.clone());
        let limits = match data.data.and_then(|d| d.limits) {
            Some(l) => l,
            None => return Ok(None),
        };

        let quota_infos: Vec<QuotaInfo> = limits.into_iter().filter_map(|l| {
            // Build a short descriptive name from unit/number (abbreviated)
            let base_name = l.name.map(|n| {
                // Strip any leading "每" and trailing "限额/限制" for brevity
                n.trim_start_matches('每').trim_end_matches("限额").trim_end_matches("限制").to_string()
            }).unwrap_or_else(|| {
                match (l.unit, l.number) {
                    (Some(3), Some(n)) => format!("{}H", n),
                    (Some(6), Some(n)) if n == 1 => "周".to_string(),
                    (Some(6), Some(n)) => format!("{}W", n),
                    _ => "限额".to_string(),
                }
            });
            // Name is just the short label; plan/level shown separately via plan_label tag
            let name = base_name;
            let used = l.used.or(l.usage).unwrap_or(0.0);
            let total = l.limit.unwrap_or(0.0);
            let current_value = l.current_value;
            let remaining = l.remaining;
            let remaining_percent = l.percentage.map(|p| 100.0 - p).unwrap_or(0.0);
            let reset_time = l.next_reset_time.map(Self::format_reset_time);
            // plan_label shows the plan type once (prefer plan_name, fallback to level)
            let plan_label = plan_name.clone().filter(|p| !p.is_empty())
                .or_else(|| level.clone().filter(|v| !v.is_empty()));

            Some(QuotaInfo {
                name,
                used,
                total,
                current_value,
                remaining,
                remaining_percent,
                reset_time,
                plan_label,
            })
        }).collect();

        if quota_infos.is_empty() {
            Ok(None)
        } else {
            Ok(Some(quota_infos))
        }
    }
}
