use super::{http_client, send_json, LlmProvider, ProviderError};
use crate::models::BalanceData;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

pub struct KimiProvider {
    client: Client,
    api_key: String,
}

#[derive(Deserialize)]
struct KimiBalanceResponse {
    code: i32,
    data: KimiBalanceData,
}

#[derive(Deserialize)]
struct KimiBalanceData {
    available_balance: f64,
    voucher_balance: f64,
    cash_balance: f64,
}

impl KimiProvider {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: http_client(),
            api_key: api_key.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for KimiProvider {
    fn name(&self) -> &str {
        "Kimi API"
    }

    async fn fetch_balance(&self) -> Result<BalanceData, ProviderError> {
        let data: KimiBalanceResponse = send_json(
            self.client
                .get("https://api.moonshot.cn/v1/users/me/balance")
                .header("Authorization", format!("Bearer {}", self.api_key)),
            "Kimi API",
        )
        .await?;
        if data.code != 0 {
            return Err(ProviderError(format!("Kimi API error code: {}", data.code)));
        }

        Ok(BalanceData {
            provider: "Kimi API".to_string(),
            available_balance: data.data.available_balance,
            voucher_balance: data.data.voucher_balance,
            cash_balance: data.data.cash_balance,
            total_balance: data.data.available_balance,
            currency: "CNY".to_string(),
        })
    }
}
