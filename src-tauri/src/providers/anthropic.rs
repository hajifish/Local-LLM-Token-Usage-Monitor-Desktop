use async_trait::async_trait;
use reqwest::Client;
use crate::models::BalanceData;
use super::{amount_to_f64, anthropic_next_cursor, append_query_param, month_start_utc, LlmProvider, ProviderError};

pub struct AnthropicProvider {
    client: Client,
    api_key: String,
}

/// 防御式翻页上限：本月 ≤31 个日桶，正常单页即返回；此上限仅避免异常游标导致死循环。
const MAX_PAGES: usize = 12;

impl AnthropicProvider {
    pub fn new(api_key: &str) -> Self {
        // 带 15s 超时，避免串行轮询被单个请求挂起
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client, api_key: api_key.to_string() }
    }
}

/// 从单页组织成本报告响应中防御式求和所有 amount。
/// 主字段：data[].amount（数字或十进制字符串）；仅当某 bucket 确实没有 amount 时，才回退读 data[].results[].amount。
/// 用存在性标志（而非 bucket_sum==0.0）区分“字段缺失”与“金额恰为 0”，避免浮点判等脆弱与误回退。
fn sum_costs(v: &serde_json::Value) -> f64 {
    let mut total = 0.0;
    if let Some(data) = v.get("data").and_then(|d| d.as_array()) {
        for bucket in data {
            let mut saw_primary = false;
            let mut bucket_sum = 0.0;
            if let Some(amount) = bucket.get("amount") {
                saw_primary = true;
                bucket_sum += amount_to_f64(amount);
            }
            // 主字段确实缺失时才回退读 results[].amount
            if !saw_primary {
                if let Some(results) = bucket.get("results").and_then(|r| r.as_array()) {
                    for r in results {
                        if let Some(amount) = r.get("amount") {
                            bucket_sum += amount_to_f64(amount);
                        }
                    }
                }
            }
            total += bucket_sum;
        }
    }
    total
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str { "Anthropic" }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        // 本月月初（本地时区）转 UTC 的 ISO8601 字符串，形如 2026-09-01T00:00:00Z
        let start = month_start_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        // cost_report 与 OpenAI 同构，同样分页（响应含 has_more/next_page，默认 limit 较小）。
        // 传 limit 减少往返，并防御式跟随 next_page 游标翻页累加所有页，避免本月后段日桶被丢弃、少算花费。
        let base_url = format!(
            "https://api.anthropic.com/v1/organizations/cost_report?starting_at={}&bucket_width=1d&limit=100",
            start
        );

        let mut total = 0.0;
        let mut page: Option<String> = None;
        let mut pages = 0usize;
        loop {
            // 用 reqwest::Url 追加 page 游标，对不透明游标做百分号编码，避免续查 URL 畸形
            let url = match &page {
                Some(cursor) => append_query_param(&base_url, "page", cursor),
                None => base_url.clone(),
            };

            let resp = self.client.get(&url)
                .header("x-api-key", self.api_key.clone())
                .header("anthropic-version", "2023-06-01")
                .send().await.map_err(|e| ProviderError(e.to_string()))?;

            let status = resp.status();
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(ProviderError("需要组织管理员密钥（Admin Key），普通 API Key 无法查询用量/花费".into()));
            }
            if !status.is_success() {
                return Err(ProviderError(format!("Anthropic API error: {}", status)));
            }

            let json: serde_json::Value = resp.json().await.map_err(|e| ProviderError(e.to_string()))?;
            total += sum_costs(&json);

            pages += 1;
            match anthropic_next_cursor(&json) {
                Some(cursor) if pages < MAX_PAGES => page = Some(cursor),
                _ => break,
            }
        }

        Ok(BalanceData {
            provider: "Anthropic".to_string(),
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
    fn test_sum_data_amount_as_number() {
        let json = val(r#"{
            "data": [
                {"amount": 1.5},
                {"amount": 2.25}
            ]
        }"#);
        assert!((sum_costs(&json) - 3.75).abs() < 1e-9);
    }

    #[test]
    fn test_sum_data_amount_as_string() {
        // data[].amount 为十进制字符串（Anthropic 文档形态）
        let json = val(r#"{
            "data": [
                {"amount": "1.5"},
                {"amount": "2.5"}
            ]
        }"#);
        assert!((sum_costs(&json) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn test_sum_data_amount_as_object() {
        // 交叉形态：Anthropic 也宽容处理 {value} 对象（共享解析器）
        let json = val(r#"{
            "data": [
                {"amount": {"value": "1.5", "currency": "usd"}},
                {"amount": {"value": 2.5}}
            ]
        }"#);
        assert!((sum_costs(&json) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn test_sum_fallback_results_amount() {
        // 无 data[].amount，回退读 data[].results[].amount
        let json = val(r#"{
            "data": [
                {"results": [{"amount": 1.0}, {"amount": "2.0"}]},
                {"results": [{"amount": 0.5}]}
            ]
        }"#);
        assert!((sum_costs(&json) - 3.5).abs() < 1e-9);
    }

    #[test]
    fn test_no_fallback_when_primary_present_but_zero() {
        // 存在性判据：data[].amount 存在且为 0 时，不应回退叠加 results[].amount（旧 ==0.0 判据会误加 7.0）
        let json = val(r#"{
            "data": [
                {"amount": "0", "results": [{"amount": 7.0}]}
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
    fn test_anthropic_pagination_cursor_and_url() {
        use super::{anthropic_next_cursor, append_query_param};
        // 游标：has_more + next_page（不读 last_id）
        let page1 = val(r#"{"data": [{"amount": "1.0"}], "has_more": true, "next_page": "cur&sor=x y"}"#);
        let cursor = anthropic_next_cursor(&page1).expect("应取到 next_page 游标");
        // 续查 URL 用 page= 且对特殊字符百分号编码
        let base = "https://api.anthropic.com/v1/organizations/cost_report?starting_at=2026-09-01T00:00:00Z&limit=100";
        let url = append_query_param(base, "page", &cursor);
        let parsed = reqwest::Url::parse(&url).unwrap();
        let page_val: String = parsed.query_pairs().find(|(k, _)| k == "page").map(|(_, v)| v.into_owned()).unwrap();
        assert_eq!(page_val, "cur&sor=x y");
        // 不读 last_id
        assert_eq!(anthropic_next_cursor(&val(r#"{"has_more": true, "last_id": "z"}"#)), None);
        // has_more=false 时终止翻页
        assert_eq!(anthropic_next_cursor(&val(r#"{"has_more": false, "next_page": "z"}"#)), None);
    }
}
