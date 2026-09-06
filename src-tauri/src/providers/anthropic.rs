use super::costs::{fetch_month_cost, usd_cost_balance, CostAuth, CostReportSpec, PrimaryAmount};
use super::{http_client, month_start_utc, LlmProvider, ProviderError};
use crate::models::BalanceData;
use async_trait::async_trait;
use reqwest::Client;

pub struct AnthropicProvider {
    client: Client,
    api_key: String,
}

impl AnthropicProvider {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: http_client(),
            api_key: api_key.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "Anthropic"
    }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        // 本月月初（本地时区）转 UTC 的 ISO8601 字符串，形如 2026-09-01T00:00:00Z
        let start = month_start_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        // cost_report 与 OpenAI 同构，同样分页（响应含 has_more/next_page，默认 limit 较小）。
        // 传 limit 减少往返，并防御式跟随 next_page 游标翻页累加所有页，避免本月后段日桶被丢弃、少算花费。
        let base_url = format!(
            "https://api.anthropic.com/v1/organizations/cost_report?starting_at={}&bucket_width=1d&limit=100",
            start
        );

        // 翻页循环、金额求和（主 bucket.amount、回退 results[].amount）统一由 costs 引擎处理
        let spec = CostReportSpec {
            provider_label: "Anthropic",
            base_url,
            cursor_param: "page",
            cursor_field: "next_page",
            primary_amount: PrimaryAmount::BucketAmount,
            auth: CostAuth::XApiKey {
                key: self.api_key.clone(),
                version: "2023-06-01",
            },
        };
        let total = fetch_month_cost(&self.client, &spec).await?;

        Ok(usd_cost_balance("Anthropic", total))
    }
}

// sum_costs 与翻页/游标测试已迁移至 costs.rs（用 PrimaryAmount::BucketAmount 参数断言，用例逐字保留）
