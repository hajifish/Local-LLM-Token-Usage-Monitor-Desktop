use async_trait::async_trait;
use reqwest::Client;
use crate::models::BalanceData;
use super::{amount_to_f64, append_query_param, month_start_utc, openai_next_cursor, LlmProvider, ProviderError};

pub struct OpenAiProvider {
    client: Client,
    api_key: String,
}

/// 防御式翻页上限：本月 ≤31 个日桶，正常单页即返回；此上限仅避免异常游标导致死循环。
const MAX_PAGES: usize = 12;

impl OpenAiProvider {
    pub fn new(api_key: &str) -> Self {
        // 带 15s 超时，避免串行轮询被单个请求挂起
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client, api_key: api_key.to_string() }
    }
}

/// 从单页组织成本响应中防御式求和所有 amount。
/// 主字段：data[].results[].amount；仅当某 bucket 确实没有 results 明细时，才回退读 data[].amount。
/// 用存在性标志（而非 bucket_sum==0.0）区分“字段缺失”与“金额恰为 0”，避免浮点判等脆弱与误回退。
fn sum_costs(v: &serde_json::Value) -> f64 {
    let mut total = 0.0;
    if let Some(data) = v.get("data").and_then(|d| d.as_array()) {
        for bucket in data {
            let mut saw_primary = false;
            let mut bucket_sum = 0.0;
            if let Some(results) = bucket.get("results").and_then(|r| r.as_array()) {
                for r in results {
                    if let Some(amount) = r.get("amount") {
                        saw_primary = true;
                        bucket_sum += amount_to_f64(amount);
                    }
                }
            }
            // 主字段确实缺失时才回退读 bucket.amount
            if !saw_primary {
                if let Some(amount) = bucket.get("amount") {
                    bucket_sum += amount_to_f64(amount);
                }
            }
            total += bucket_sum;
        }
    }
    total
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &str { "OpenAI" }

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

        let mut total = 0.0;
        let mut after: Option<String> = None;
        let mut pages = 0usize;
        loop {
            // 用 reqwest::Url 追加 after 游标，对不透明游标做百分号编码，避免续查 URL 畸形
            let url = match &after {
                Some(cursor) => append_query_param(&base_url, "after", cursor),
                None => base_url.clone(),
            };

            let resp = self.client.get(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send().await.map_err(|e| ProviderError(e.to_string()))?;

            let status = resp.status();
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(ProviderError("需要组织管理员密钥（Admin Key），普通 API Key 无法查询用量/花费".into()));
            }
            if !status.is_success() {
                return Err(ProviderError(format!("OpenAI API error: {}", status)));
            }

            let json: serde_json::Value = resp.json().await.map_err(|e| ProviderError(e.to_string()))?;
            total += sum_costs(&json);

            pages += 1;
            match openai_next_cursor(&json) {
                Some(cursor) if pages < MAX_PAGES => after = Some(cursor),
                _ => break,
            }
        }

        Ok(BalanceData {
            provider: "OpenAI".to_string(),
            available_balance: total,
            voucher_balance: 0.0,
            cash_balance: 0.0,
            total_balance: total,
            currency: "USD".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::sum_costs;

    fn val(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn test_sum_amount_as_number() {
        let json = val(r#"{
            "data": [
                {"results": [{"amount": 1.5}, {"amount": 2.25}]},
                {"results": [{"amount": 0.75}]}
            ]
        }"#);
        assert!((sum_costs(&json) - 4.5).abs() < 1e-9);
    }

    #[test]
    fn test_sum_amount_as_object() {
        // amount 为 {"value": 数字, "currency": "usd"}
        let json = val(r#"{
            "data": [
                {"results": [{"amount": {"value": 1.5, "currency": "usd"}}]},
                {"results": [{"amount": {"value": 2.0, "currency": "usd"}}]}
            ]
        }"#);
        assert!((sum_costs(&json) - 3.5).abs() < 1e-9);
    }

    #[test]
    fn test_sum_amount_as_string() {
        // 交叉形态：OpenAI 也宽容处理数字字符串（共享解析器）
        let json = val(r#"{
            "data": [
                {"results": [{"amount": "1.5"}, {"amount": "2.5"}]}
            ]
        }"#);
        assert!((sum_costs(&json) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn test_sum_fallback_bucket_amount() {
        // 无 results 明细，回退读 bucket.amount
        let json = val(r#"{
            "data": [
                {"amount": 3.0},
                {"amount": {"value": 1.0, "currency": "usd"}}
            ]
        }"#);
        assert!((sum_costs(&json) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn test_no_fallback_when_primary_present_but_zero() {
        // 存在性判据：results 明细存在且金额为 0 时，不应回退叠加 bucket.amount（旧 ==0.0 判据会误加 5.0）
        let json = val(r#"{
            "data": [
                {"results": [{"amount": 0.0}], "amount": 5.0}
            ]
        }"#);
        assert_eq!(sum_costs(&json), 0.0);
    }

    #[test]
    fn test_sum_empty_and_missing() {
        assert_eq!(sum_costs(&val(r#"{"data": []}"#)), 0.0);
        assert_eq!(sum_costs(&val(r#"{"object": "page"}"#)), 0.0);
    }

    #[test]
    fn test_openai_pagination_cursor_and_url() {
        use super::{openai_next_cursor, append_query_param};
        // 游标：has_more + last_id
        let page1 = val(r#"{"data": [{"results": [{"amount": 1.0}]}], "has_more": true, "last_id": "bkt&a=1 x"}"#);
        let cursor = openai_next_cursor(&page1).expect("应取到 last_id 游标");
        // 续查 URL 用 after= 且对特殊字符百分号编码
        let base = "https://api.openai.com/v1/organization/costs?starting_at=1&limit=200";
        let url = append_query_param(base, "after", &cursor);
        let parsed = reqwest::Url::parse(&url).unwrap();
        let after: String = parsed.query_pairs().find(|(k, _)| k == "after").map(|(_, v)| v.into_owned()).unwrap();
        assert_eq!(after, "bkt&a=1 x");
        // 下一页 has_more=false 时游标为 None，终止翻页
        let page2 = val(r#"{"data": [], "has_more": false, "last_id": "z"}"#);
        assert_eq!(openai_next_cursor(&page2), None);
    }
}
