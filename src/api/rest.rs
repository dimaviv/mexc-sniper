use crate::models::ContractDetailResponse;
use anyhow::Result;
use reqwest::Client;

pub struct MexcRestClient {
    client: Client,
    base_url: String,
}

impl MexcRestClient {
    pub fn new(base_url: String) -> Self {
        Self {
            client: Client::new(),
            base_url,
        }
    }

    pub async fn get_all_contracts(&self) -> Result<Vec<String>> {
        let url = format!("{}/api/v1/contract/detail", self.base_url);

        let response = self.client
            .get(&url)
            .send()
            .await?;

        let data: ContractDetailResponse = response.json().await?;

        if !data.success {
            anyhow::bail!("API returned success=false, code={}", data.code);
        }

        let symbols: Vec<String> = data.data.iter()
            .filter(|contract| contract.state == 0)
            .map(|contract| contract.symbol.clone())
            .collect();

        Ok(symbols)
    }
}
