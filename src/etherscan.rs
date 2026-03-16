// Etherscan V2 API client for ABI, gas, and account queries.
// Exports: EtherscanClient, GasOracle.
// Deps: reqwest, eyre, serde_json, std::env.
use eyre::{bail, eyre, Result};
use serde_json::Value;

pub struct EtherscanClient {
    client: reqwest::Client,
    api_key: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct GasOracle {
    pub safe_gas_price: String,
    pub propose_gas_price: String,
    pub fast_gas_price: String,
    pub suggested_base_fee: Option<String>,
}

impl EtherscanClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: std::env::var("ETHERSCAN_API_KEY").ok().filter(|key| !key.is_empty()),
        }
    }

    fn build_url(&self, chain_id: u64, params: &[(&str, &str)]) -> String {
        let mut url = reqwest::Url::parse("https://api.etherscan.io/v2/api").expect("valid url");
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("chainid", &chain_id.to_string());
            for (key, value) in params {
                query.append_pair(key, value);
            }
            if let Some(api_key) = &self.api_key {
                query.append_pair("apikey", api_key);
            }
        }
        url.into()
    }

    pub async fn get_abi(&self, chain_id: u64, address: &str) -> Result<Vec<Value>> {
        let result = self
            .api_call(chain_id, &[("module", "contract"), ("action", "getabi"), ("address", address)])
            .await?;
        let abi = result.as_str().ok_or_else(|| eyre!("missing ABI payload"))?;
        Ok(serde_json::from_str(abi)?)
    }

    pub async fn gas_oracle(&self, chain_id: u64) -> Result<GasOracle> {
        let result = self
            .api_call(chain_id, &[("module", "gastracker"), ("action", "gasoracle")])
            .await?;
        Self::parse_gas_oracle(&result)
    }

    pub async fn get_tx_list(&self, chain_id: u64, address: &str, start_block: Option<u64>, end_block: Option<u64>, page: u32, offset: u32, sort: &str) -> Result<Vec<Value>> {
        self.get_account_rows(chain_id, "txlist", address, None, start_block, end_block, page, offset, sort).await
    }

    pub async fn get_token_transfers(&self, chain_id: u64, address: &str, contract: Option<&str>, start_block: Option<u64>, end_block: Option<u64>, page: u32, offset: u32, sort: &str) -> Result<Vec<Value>> {
        self.get_account_rows(chain_id, "tokentx", address, contract, start_block, end_block, page, offset, sort).await
    }

    async fn get_account_rows(&self, chain_id: u64, action: &'static str, address: &str, contract: Option<&str>, start_block: Option<u64>, end_block: Option<u64>, page: u32, offset: u32, sort: &str) -> Result<Vec<Value>> {
        let start = start_block.unwrap_or(0).to_string();
        let end = end_block.unwrap_or(99_999_999).to_string();
        let page = page.to_string();
        let offset = offset.to_string();
        let mut params = vec![("module", "account"), ("action", action), ("address", address), ("startblock", start.as_str()), ("endblock", end.as_str()), ("page", page.as_str()), ("offset", offset.as_str()), ("sort", sort)];
        if let Some(contract) = contract {
            params.push(("contractaddress", contract));
        }
        let result = self.api_call(chain_id, &params).await?;
        match result {
            Value::Array(rows) => Ok(rows),
            _ => bail!("expected array result"),
        }
    }

    async fn api_call(&self, chain_id: u64, params: &[(&str, &str)]) -> Result<Value> {
        let response = self.client.get(self.build_url(chain_id, params)).send().await?.error_for_status()?;
        let body: Value = response.json().await?;
        let status = body.get("status").and_then(Value::as_str).unwrap_or("0");
        if status == "1" {
            return body.get("result").cloned().ok_or_else(|| eyre!("missing result field"));
        }
        let detail = body.get("result").and_then(Value::as_str).or_else(|| body.get("message").and_then(Value::as_str)).unwrap_or("unknown error");
        if detail.contains("API Key") && self.api_key.is_none() {
            bail!("Etherscan API key required. Set ETHERSCAN_API_KEY env var (free at https://etherscan.io/apis)")
        }
        bail!("etherscan api error: {detail}")
    }

    fn parse_gas_oracle(result: &Value) -> Result<GasOracle> {
        let get = |key| result.get(key).and_then(Value::as_str).map(str::to_owned).ok_or_else(|| eyre!("missing {key}"));
        Ok(GasOracle {
            safe_gas_price: get("SafeGasPrice")?,
            propose_gas_price: get("ProposeGasPrice")?,
            fast_gas_price: get("FastGasPrice")?,
            suggested_base_fee: result.get("suggestBaseFee").and_then(Value::as_str).map(str::to_owned),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn new_works_without_api_key() {
        let _guard = env_lock().lock().expect("lock");
        let original = std::env::var("ETHERSCAN_API_KEY").ok();
        unsafe { std::env::remove_var("ETHERSCAN_API_KEY") };
        let client = EtherscanClient::new();
        assert_eq!(client.api_key, None);
        if let Some(value) = original {
            unsafe { std::env::set_var("ETHERSCAN_API_KEY", value) };
        }
    }

    #[test]
    fn build_url_includes_expected_params() {
        let client = EtherscanClient { client: reqwest::Client::new(), api_key: Some("secret".into()) };
        let url = client.build_url(8453, &[("module", "account"), ("action", "txlist"), ("address", "0xabc")]);
        assert_eq!(url, "https://api.etherscan.io/v2/api?chainid=8453&module=account&action=txlist&address=0xabc&apikey=secret");
    }

    #[test]
    fn parses_gas_oracle_fields() {
        let sample = serde_json::json!({
            "SafeGasPrice": "1",
            "ProposeGasPrice": "2",
            "FastGasPrice": "3",
            "suggestBaseFee": "0.5"
        });
        let gas = EtherscanClient::parse_gas_oracle(&sample).expect("gas oracle");
        assert_eq!(gas, GasOracle { safe_gas_price: "1".into(), propose_gas_price: "2".into(), fast_gas_price: "3".into(), suggested_base_fee: Some("0.5".into()) });
    }
}
