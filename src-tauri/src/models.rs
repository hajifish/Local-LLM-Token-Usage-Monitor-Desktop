use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceData {
    pub provider: String,
    pub available_balance: f64,
    pub voucher_balance: f64,
    pub cash_balance: f64,
    pub total_balance: f64,
    pub currency: String,
}

impl BalanceData {
    /// 配额百分比型供应商（currency = "%"）
    pub fn is_percent(&self) -> bool {
        self.currency == "%"
    }
    /// 美元成本型供应商（currency = "USD"，如 OpenAI / Anthropic）
    pub fn is_usd(&self) -> bool {
        self.currency == "USD"
    }
    /// 人民币余额型供应商（currency = "CNY"，如 DeepSeek / Kimi），用于 ¥ 总额正向白名单
    pub fn is_cny(&self) -> bool {
        self.currency == "CNY"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageData {
    pub provider: String,
    pub total_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub today_cost: Option<f64>,
    pub today_requests: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub api_key: String,
    pub enabled: bool,
    #[serde(default)]
    pub platform_token: Option<String>,
    /// 用户自定义别名（用于区分同一服务商的多个账号）；为空时回退到 name
    #[serde(default)]
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub providers: Vec<ProviderConfig>,
    pub refresh_interval: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            refresh_interval: 300,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaInfo {
    /// 限制类型名称（如 "每5小时限额"、"每周限额"）
    pub name: String,
    /// 已使用量
    pub used: f64,
    /// 总量
    pub total: f64,
    /// 当前值（剩余可用）
    pub current_value: Option<f64>,
    /// 剩余量
    pub remaining: Option<f64>,
    /// 剩余百分比
    pub remaining_percent: f64,
    /// 重置时间
    pub reset_time: Option<String>,
    /// 配额类型标签（如 "coding plan"）
    pub plan_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub name: String,
    /// 展示用别名（alias 或回退到 name）
    pub alias: String,
    pub enabled: bool,
    pub balance: Option<BalanceData>,
    pub usage: Option<UsageData>,
    pub error: Option<String>,
    pub quota_infos: Option<Vec<QuotaInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageSummary {
    pub providers: Vec<ProviderStatus>,
    pub total_balance: f64,
    pub last_updated: String,
}
