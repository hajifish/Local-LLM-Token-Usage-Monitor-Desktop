//! OpenAI / Anthropic 组织成本报告的统一翻页引擎。
//! 两家 cost 接口高度同构（分页 + data[] 桶 + amount 求和），仅在以下维度不同，
//! 全部参数化收编到本模块，openai.rs / anthropic.rs 只保留各自 spec 组装：
//! - 主/回退金额方向相反（OpenAI 主 results[].amount；Anthropic 主 bucket.amount）
//! - 游标参数与字段（after/last_id vs page/next_page）
//! - 认证头（Bearer vs x-api-key + anthropic-version）

use super::{amount_to_f64, append_query_param, send_json_with, ProviderError};
use crate::models::BalanceData;

/// 防御式翻页上限：本月 ≤31 个日桶，正常单页即返回；此上限仅避免异常游标导致死循环。
const MAX_PAGES: usize = 12;

/// 成本报告接口的认证方式。
pub(crate) enum CostAuth {
    Bearer(String),
    XApiKey { key: String, version: &'static str },
}

/// sum_costs 的主字段方向：决定先读哪个字段、缺失时回退读哪个。
pub(crate) enum PrimaryAmount {
    /// 主字段 data[].amount，回退 data[].results[].amount（Anthropic）
    BucketAmount,
    /// 主字段 data[].results[].amount，回退 data[].amount（OpenAI）
    ResultsAmount,
}

/// 一次成本报告查询的全部差异点，由各 Provider 的 fetch_balance 组装。
pub(crate) struct CostReportSpec {
    /// "OpenAI" | "Anthropic"，驱动错误文案
    pub provider_label: &'static str,
    /// 由调用点组装（含 starting_at/bucket_width/limit）
    pub base_url: String,
    /// 续查游标参数名："after" | "page"
    pub cursor_param: &'static str,
    /// 响应游标字段名："last_id" | "next_page"
    pub cursor_field: &'static str,
    pub primary_amount: PrimaryAmount,
    pub auth: CostAuth,
}

/// 从单页组织成本响应中防御式求和所有 amount。
/// OpenAI（ResultsAmount）主字段：data[].results[].amount；仅当某 bucket 确实没有 results 明细时，才回退读 data[].amount。
/// Anthropic（BucketAmount）主字段：data[].amount（数字或十进制字符串）；仅当某 bucket 确实没有 amount 时，才回退读 data[].results[].amount。
/// 用存在性标志（而非 bucket_sum==0.0）区分“字段缺失”与“金额恰为 0”，避免浮点判等脆弱与误回退。
pub(crate) fn sum_costs(v: &serde_json::Value, primary: &PrimaryAmount) -> f64 {
    let mut total = 0.0;
    if let Some(data) = v.get("data").and_then(|d| d.as_array()) {
        for bucket in data {
            let mut saw_primary = false;
            let mut bucket_sum = 0.0;
            match primary {
                PrimaryAmount::ResultsAmount => {
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
                }
                PrimaryAmount::BucketAmount => {
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
                }
            }
            total += bucket_sum;
        }
    }
    total
}

/// 分页游标：从响应取 has_more + 指定游标字段（field）。
/// OpenAI 约定：has_more + last_id（续查参数为 after=<last_id>）；
/// Anthropic 约定：has_more + next_page（续查参数为 page=<next_page>）。
/// 按 field 取对应字段，OpenAI 不读 next_page、Anthropic 不读 last_id 的语义保持不变。
pub(crate) fn next_cursor(v: &serde_json::Value, field: &str) -> Option<String> {
    let has_more = v.get("has_more").and_then(|x| x.as_bool()).unwrap_or(false);
    if !has_more {
        return None;
    }
    v.get(field)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// 按 spec 翻页拉取本月成本报告并逐页累加，返回总花费。
/// 收编原 openai.rs / anthropic.rs fetch_balance 中完全同构的翻页循环：
/// 游标追加、401/403 管理员密钥错误映射、非 2xx 错误、JSON 解析、逐页累加。
pub(crate) async fn fetch_month_cost(
    client: &reqwest::Client,
    spec: &CostReportSpec,
) -> Result<f64, ProviderError> {
    let mut total = 0.0;
    let mut cursor: Option<String> = None;
    let mut pages = 0usize;
    loop {
        // 用 reqwest::Url 追加游标，对不透明游标做百分号编码，避免续查 URL 畸形
        let url = match &cursor {
            Some(c) => append_query_param(&spec.base_url, spec.cursor_param, c),
            None => spec.base_url.clone(),
        };

        let mut req = client.get(&url);
        match &spec.auth {
            CostAuth::Bearer(key) => {
                req = req.header("Authorization", format!("Bearer {}", key));
            }
            CostAuth::XApiKey { key, version } => {
                req = req
                    .header("x-api-key", key.clone())
                    .header("anthropic-version", *version);
            }
        }

        // 401/403 映射为管理员密钥错误（固定文案，不含 label）；其余非 2xx 走 send_json_with 默认 "<label> API error: <status>"。
        // 请求发送、状态检查、JSON 解析的公共样板统一由 mod.rs 的 send_json_with 收编，避免错误文案模板双份维护。
        let json: serde_json::Value = send_json_with(req, spec.provider_label, |status| {
            if status.as_u16() == 401 || status.as_u16() == 403 {
                Some(ProviderError(
                    "需要组织管理员密钥（Admin Key），普通 API Key 无法查询用量/花费".into(),
                ))
            } else {
                None
            }
        })
        .await?;
        total += sum_costs(&json, &spec.primary_amount);

        pages += 1;
        match next_cursor(&json, spec.cursor_field) {
            Some(next) if pages < MAX_PAGES => cursor = Some(next),
            _ => break,
        }
    }
    Ok(total)
}

/// OpenAI / Anthropic 成本报告的余额组装：两处 fetch_balance 尾部同构的 6 字段构造收编于此。
/// 字段值与各 Provider 原有实现逐项完全一致：provider=label、available_balance=total、
/// voucher_balance=0.0、cash_balance=0.0、total_balance=total、currency="USD"。
pub(crate) fn usd_cost_balance(label: &str, total: f64) -> BalanceData {
    BalanceData {
        provider: label.to_string(),
        available_balance: total,
        voucher_balance: 0.0,
        cash_balance: 0.0,
        total_balance: total,
        currency: "USD".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::append_query_param;
    use super::super::test_util::val;
    use super::{next_cursor, sum_costs, PrimaryAmount};

    // ---------- OpenAI 方向（主 results[].amount，回退 bucket.amount）----------
    // 以下 6 个用例自 openai.rs 原样迁移，JSON 字面量与断言值逐字保留

    #[test]
    fn test_sum_amount_as_number() {
        let json = val(r#"{
            "data": [
                {"results": [{"amount": 1.5}, {"amount": 2.25}]},
                {"results": [{"amount": 0.75}]}
            ]
        }"#);
        assert!((sum_costs(&json, &PrimaryAmount::ResultsAmount) - 4.5).abs() < 1e-9);
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
        assert!((sum_costs(&json, &PrimaryAmount::ResultsAmount) - 3.5).abs() < 1e-9);
    }

    #[test]
    fn test_sum_amount_as_string() {
        // 交叉形态：OpenAI 也宽容处理数字字符串（共享解析器）
        let json = val(r#"{
            "data": [
                {"results": [{"amount": "1.5"}, {"amount": "2.5"}]}
            ]
        }"#);
        assert!((sum_costs(&json, &PrimaryAmount::ResultsAmount) - 4.0).abs() < 1e-9);
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
        assert!((sum_costs(&json, &PrimaryAmount::ResultsAmount) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn test_no_fallback_when_primary_present_but_zero() {
        // 存在性判据：results 明细存在且金额为 0 时，不应回退叠加 bucket.amount（旧 ==0.0 判据会误加 5.0）
        let json = val(r#"{
            "data": [
                {"results": [{"amount": 0.0}], "amount": 5.0}
            ]
        }"#);
        assert_eq!(sum_costs(&json, &PrimaryAmount::ResultsAmount), 0.0);
    }

    #[test]
    fn test_sum_empty_and_missing() {
        assert_eq!(
            sum_costs(&val(r#"{"data": []}"#), &PrimaryAmount::ResultsAmount),
            0.0
        );
        assert_eq!(
            sum_costs(&val(r#"{"object": "page"}"#), &PrimaryAmount::ResultsAmount),
            0.0
        );
    }

    // ---------- Anthropic 方向（主 bucket.amount，回退 results[].amount）----------
    // 以下 6 个用例自 anthropic.rs 原样迁移，JSON 字面量与断言值逐字保留

    #[test]
    fn test_sum_data_amount_as_number() {
        let json = val(r#"{
            "data": [
                {"amount": 1.5},
                {"amount": 2.25}
            ]
        }"#);
        assert!((sum_costs(&json, &PrimaryAmount::BucketAmount) - 3.75).abs() < 1e-9);
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
        assert!((sum_costs(&json, &PrimaryAmount::BucketAmount) - 4.0).abs() < 1e-9);
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
        assert!((sum_costs(&json, &PrimaryAmount::BucketAmount) - 4.0).abs() < 1e-9);
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
        assert!((sum_costs(&json, &PrimaryAmount::BucketAmount) - 3.5).abs() < 1e-9);
    }

    #[test]
    fn test_no_fallback_when_primary_present_but_zero_bucket_direction() {
        // 存在性判据：data[].amount 存在且为 0 时，不应回退叠加 results[].amount（旧 ==0.0 判据会误加 7.0）
        let json = val(r#"{
            "data": [
                {"amount": "0", "results": [{"amount": 7.0}]}
            ]
        }"#);
        assert_eq!(sum_costs(&json, &PrimaryAmount::BucketAmount), 0.0);
    }

    #[test]
    fn test_sum_empty_and_missing_bucket_direction() {
        assert_eq!(
            sum_costs(&val(r#"{"data": []}"#), &PrimaryAmount::BucketAmount),
            0.0
        );
        assert_eq!(
            sum_costs(&val(r#"{"object": "page"}"#), &PrimaryAmount::BucketAmount),
            0.0
        );
    }

    // ---------- 游标语义（原 mod.rs / openai.rs / anthropic.rs 游标测试迁移合并）----------

    #[test]
    fn test_openai_cursor_uses_last_id() {
        // OpenAI 约定：has_more + last_id
        assert_eq!(
            next_cursor(
                &val(r#"{"has_more": true, "last_id": "bucket_abc"}"#),
                "last_id"
            ),
            Some("bucket_abc".to_string())
        );
        assert_eq!(
            next_cursor(
                &val(r#"{"has_more": false, "last_id": "bucket_abc"}"#),
                "last_id"
            ),
            None
        );
        assert_eq!(
            next_cursor(&val(r#"{"has_more": true, "last_id": null}"#), "last_id"),
            None
        );
        assert_eq!(
            next_cursor(&val(r#"{"has_more": true, "last_id": ""}"#), "last_id"),
            None
        );
        // OpenAI 不读 next_page（那是 Anthropic 约定）
        assert_eq!(
            next_cursor(&val(r#"{"has_more": true, "next_page": "xyz"}"#), "last_id"),
            None
        );
    }

    #[test]
    fn test_anthropic_cursor_uses_next_page() {
        assert_eq!(
            next_cursor(
                &val(r#"{"has_more": true, "next_page": "abc"}"#),
                "next_page"
            ),
            Some("abc".to_string())
        );
        assert_eq!(
            next_cursor(
                &val(r#"{"has_more": false, "next_page": "abc"}"#),
                "next_page"
            ),
            None
        );
        assert_eq!(
            next_cursor(
                &val(r#"{"has_more": true, "next_page": null}"#),
                "next_page"
            ),
            None
        );
        assert_eq!(
            next_cursor(&val(r#"{"has_more": true, "next_page": ""}"#), "next_page"),
            None
        );
        // Anthropic 不读 last_id
        assert_eq!(
            next_cursor(&val(r#"{"has_more": true, "last_id": "xyz"}"#), "next_page"),
            None
        );
    }

    #[test]
    fn test_openai_pagination_cursor_and_url() {
        // 游标：has_more + last_id
        let page1 = val(
            r#"{"data": [{"results": [{"amount": 1.0}]}], "has_more": true, "last_id": "bkt&a=1 x"}"#,
        );
        let cursor = next_cursor(&page1, "last_id").expect("应取到 last_id 游标");
        // 续查 URL 用 after= 且对特殊字符百分号编码
        let base = "https://api.openai.com/v1/organization/costs?starting_at=1&limit=200";
        let url = append_query_param(base, "after", &cursor);
        let parsed = reqwest::Url::parse(&url).unwrap();
        let after: String = parsed
            .query_pairs()
            .find(|(k, _)| k == "after")
            .map(|(_, v)| v.into_owned())
            .unwrap();
        assert_eq!(after, "bkt&a=1 x");
        // 下一页 has_more=false 时游标为 None，终止翻页
        let page2 = val(r#"{"data": [], "has_more": false, "last_id": "z"}"#);
        assert_eq!(next_cursor(&page2, "last_id"), None);
    }

    #[test]
    fn test_anthropic_pagination_cursor_and_url() {
        // 游标：has_more + next_page（不读 last_id）
        let page1 =
            val(r#"{"data": [{"amount": "1.0"}], "has_more": true, "next_page": "cur&sor=x y"}"#);
        let cursor = next_cursor(&page1, "next_page").expect("应取到 next_page 游标");
        // 续查 URL 用 page= 且对特殊字符百分号编码
        let base = "https://api.anthropic.com/v1/organizations/cost_report?starting_at=2026-09-01T00:00:00Z&limit=100";
        let url = append_query_param(base, "page", &cursor);
        let parsed = reqwest::Url::parse(&url).unwrap();
        let page_val: String = parsed
            .query_pairs()
            .find(|(k, _)| k == "page")
            .map(|(_, v)| v.into_owned())
            .unwrap();
        assert_eq!(page_val, "cur&sor=x y");
        // 不读 last_id
        assert_eq!(
            next_cursor(&val(r#"{"has_more": true, "last_id": "z"}"#), "next_page"),
            None
        );
        // has_more=false 时终止翻页
        assert_eq!(
            next_cursor(
                &val(r#"{"has_more": false, "next_page": "z"}"#),
                "next_page"
            ),
            None
        );
    }
}
