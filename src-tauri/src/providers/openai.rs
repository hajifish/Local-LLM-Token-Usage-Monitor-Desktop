use super::costs::{fetch_month_cost, usd_cost_balance, CostAuth, CostReportSpec, PrimaryAmount};
use super::{http_client, month_start_utc, LlmProvider, ProviderError};
use crate::models::BalanceData;
use async_trait::async_trait;
use reqwest::Client;

pub struct OpenAiProvider {
    client: Client,
    api_key: String,
}

impl OpenAiProvider {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: http_client(),
            api_key: api_key.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &str {
        "OpenAI"
    }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        // 本月月初（本地时区）转 UTC 的 Unix 秒，作为查询起点
        let start = month_start_utc().timestamp();

        // limit=200（接口最大值，默认仅 20）：本月 ≤31 个日桶，通常单页即返回。
        // OpenAI 分页约定为 has_more + last_id，续查用 after=<last_id>；此处真实跟随翻页累加，
        // 以防日后调低 limit 或改用更细桶宽（如 1h，约 720 桶/月 > 200）时静默停在首页少算。
        let base_url = format!(
            "https://api.openai.com/v1/organization/costs?starting_at={}&bucket_width=1d&limit=200",
            start
        );

        // 翻页循环、金额求和（主 results[].amount、回退 bucket.amount）统一由 costs 引擎处理
        let spec = CostReportSpec {
            provider_label: "OpenAI",
            base_url,
            cursor_param: "after",
            cursor_field: "last_id",
            primary_amount: PrimaryAmount::ResultsAmount,
            auth: CostAuth::Bearer(self.api_key.clone()),
        };
        let total = fetch_month_cost(&self.client, &spec).await?;

        Ok(usd_cost_balance("OpenAI", total))
    }
}

// sum_costs 与翻页/游标测试已迁移至 costs.rs（用 PrimaryAmount::ResultsAmount 参数断言，用例逐字保留）
