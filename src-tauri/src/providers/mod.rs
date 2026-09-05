pub mod deepseek;
pub mod kimi;
pub mod zhipu;
pub mod openai;
pub mod anthropic;

use async_trait::async_trait;
use crate::models::{BalanceData, UsageData, QuotaInfo};

#[derive(Debug)]
pub struct ProviderError(pub String);

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ProviderError {}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError>;
    async fn fetch_usage(&self) -> Result<Option<UsageData>, ProviderError> {
        Ok(None)
    }
    async fn fetch_quota_infos(&self) -> Result<Option<Vec<QuotaInfo>>, ProviderError> {
        Ok(None)
    }
}

pub fn create_provider(name: &str, api_key: &str, platform_token: Option<&str>) -> Option<Box<dyn LlmProvider>> {
    match name {
        "DeepSeek" => Some(Box::new(deepseek::DeepSeekProvider::new(api_key, platform_token))),
        "Kimi" => Some(Box::new(kimi::KimiProvider::new(api_key))),
        "Zhipu" => Some(Box::new(zhipu::ZhipuProvider::new(api_key))),
        "OpenAI" => Some(Box::new(openai::OpenAiProvider::new(api_key))),
        "Anthropic" => Some(Box::new(anthropic::AnthropicProvider::new(api_key))),
        _ => None,
    }
}

/// 宽容地把成本 API 里的 amount 字段解析为 f64。
/// 依次尝试：数字 -> 数字字符串 -> {"value": 数字/数字字符串}；均不命中返回 0.0。
/// OpenAI 与 Anthropic 的金额形态不同（数字 / 字符串 / {value} 对象），此处统一处理，避免命中未支持形态时静默少算。
pub(crate) fn amount_to_f64(v: &serde_json::Value) -> f64 {
    if let Some(n) = v.as_f64() {
        return n;
    }
    if let Some(s) = v.as_str() {
        if let Ok(n) = s.parse::<f64>() {
            return n;
        }
    }
    if let Some(inner) = v.get("value") {
        if let Some(n) = inner.as_f64() {
            return n;
        }
        if let Some(s) = inner.as_str() {
            if let Ok(n) = s.parse::<f64>() {
                return n;
            }
        }
    }
    0.0
}

/// 计算“本月月初”（本地时区 1 号 00:00）对应的 UTC 时刻。
/// 对 UTC+8 用户，可避免每月 1 号 00:00-08:00 之间把“本月”误判为上月（与 last_updated 使用 Local 基准保持一致）。
/// 无 panic 路径：with_day(1) 恒成功（兜底回退当天），and_time 为不可失败构造，DST 空隙退化为按 UTC 解释该 naive 时刻。
pub(crate) fn month_start_utc() -> chrono::DateTime<chrono::Utc> {
    use chrono::{Datelike, TimeZone};
    let now = chrono::Local::now();
    let today = now.date_naive();
    let first_day = today.with_day(1).unwrap_or(today);
    // NaiveTime::MIN 即 00:00:00；and_time 不可失败，无需 expect/unwrap
    let local_midnight = first_day.and_time(chrono::NaiveTime::MIN);
    chrono::Local
        .from_local_datetime(&local_midnight)
        .earliest()
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|| local_midnight.and_utc())
}

/// OpenAI 分页游标：从响应取 has_more + last_id（续查参数为 after=<last_id>）。
pub(crate) fn openai_next_cursor(v: &serde_json::Value) -> Option<String> {
    let has_more = v.get("has_more").and_then(|x| x.as_bool()).unwrap_or(false);
    if !has_more {
        return None;
    }
    v.get("last_id")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// Anthropic 分页游标：从响应取 has_more + next_page（续查参数为 page=<next_page>）。
pub(crate) fn anthropic_next_cursor(v: &serde_json::Value) -> Option<String> {
    let has_more = v.get("has_more").and_then(|x| x.as_bool()).unwrap_or(false);
    if !has_more {
        return None;
    }
    v.get("next_page")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// 在已有查询串的 base_url 上追加一个查询参数，并对 value 做百分号编码。
/// 用 reqwest::Url 保证不透明游标含 &/=/+/空格 等字符时续查 URL 不畸形（避免重复取回首页重复累加或请求失败）。
/// base_url 由本模块构造、恒为合法 URL；解析失败时兜底为朴素拼接（不 panic）。
pub(crate) fn append_query_param(base_url: &str, key: &str, value: &str) -> String {
    match reqwest::Url::parse(base_url) {
        Ok(mut url) => {
            url.query_pairs_mut().append_pair(key, value);
            url.to_string()
        }
        Err(_) => format!("{}&{}={}", base_url, key, value),
    }
}

#[cfg(test)]
mod tests {
    use super::{amount_to_f64, openai_next_cursor, anthropic_next_cursor, append_query_param};

    fn val(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn test_amount_to_f64_all_forms() {
        assert_eq!(amount_to_f64(&val("1.5")), 1.5);
        assert_eq!(amount_to_f64(&val("\"2.25\"")), 2.25);
        assert_eq!(amount_to_f64(&val(r#"{"value": 3.0, "currency": "usd"}"#)), 3.0);
        assert_eq!(amount_to_f64(&val(r#"{"value": "4.5"}"#)), 4.5);
        // 未支持形态 -> 0.0
        assert_eq!(amount_to_f64(&val(r#"{"foo": 1}"#)), 0.0);
        assert_eq!(amount_to_f64(&val("null")), 0.0);
    }

    #[test]
    fn test_openai_cursor_uses_last_id() {
        // OpenAI 约定：has_more + last_id
        assert_eq!(openai_next_cursor(&val(r#"{"has_more": true, "last_id": "bucket_abc"}"#)), Some("bucket_abc".to_string()));
        assert_eq!(openai_next_cursor(&val(r#"{"has_more": false, "last_id": "bucket_abc"}"#)), None);
        assert_eq!(openai_next_cursor(&val(r#"{"has_more": true, "last_id": null}"#)), None);
        assert_eq!(openai_next_cursor(&val(r#"{"has_more": true, "last_id": ""}"#)), None);
        // OpenAI 不读 next_page（那是 Anthropic 约定）
        assert_eq!(openai_next_cursor(&val(r#"{"has_more": true, "next_page": "xyz"}"#)), None);
    }

    #[test]
    fn test_anthropic_cursor_uses_next_page() {
        assert_eq!(anthropic_next_cursor(&val(r#"{"has_more": true, "next_page": "abc"}"#)), Some("abc".to_string()));
        assert_eq!(anthropic_next_cursor(&val(r#"{"has_more": false, "next_page": "abc"}"#)), None);
        assert_eq!(anthropic_next_cursor(&val(r#"{"has_more": true, "next_page": null}"#)), None);
        assert_eq!(anthropic_next_cursor(&val(r#"{"has_more": true, "next_page": ""}"#)), None);
        // Anthropic 不读 last_id
        assert_eq!(anthropic_next_cursor(&val(r#"{"has_more": true, "last_id": "xyz"}"#)), None);
    }

    #[test]
    fn test_append_query_param_encodes_cursor() {
        let base = "https://api.openai.com/v1/organization/costs?starting_at=1&limit=200";
        // 含特殊字符 & = + 空格 的游标必须被正确百分号编码
        let url = append_query_param(base, "after", "a&b=c+d e");
        assert!(url.starts_with(base));
        // 用 Url 反解：after 值原样还原，且未污染其它参数
        let parsed = reqwest::Url::parse(&url).unwrap();
        let after: String = parsed
            .query_pairs()
            .find(|(k, _)| k == "after")
            .map(|(_, v)| v.into_owned())
            .expect("after 应存在");
        assert_eq!(after, "a&b=c+d e");
        assert!(parsed.query_pairs().any(|(k, v)| k == "starting_at" && v == "1"));
        assert!(parsed.query_pairs().any(|(k, v)| k == "limit" && v == "200"));
    }
}
