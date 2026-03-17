// rpc balance <chain> <token> <holder> — ERC20 balanceOf with formatting
use alloy::primitives::{Address, U256};
use eyre::{bail, eyre, Result};
use crate::commands::block::parse_block_number;
use serde_json::json;

pub async fn run(
    chain: &str,
    token: &str,
    holder: &str,
    raw: bool,
    block: Option<&str>,
    rpc_override: Option<&str>,
    provider: Option<&str>,
    json: bool,
) -> Result<()> {
    let config = crate::config::Config::load()?;
    let opts = crate::config::RpcOpts {
        rpc: rpc_override.map(String::from),
        provider: provider.map(String::from),
    };
    let rpc_urls = config.resolve_rpc_all(chain, &opts)?;
    let token_addr = resolve_token_address(chain, token, &config)?;
    let holder_addr = holder.parse::<Address>()?;
    let calldata = crate::abi::encode_call("balanceOf(address)", &[holder_addr.to_string()])?;
    let block_number = parse_block_number(block)?;
    let response = crate::rpc::eth_call_with_fallback(&rpc_urls, token_addr, calldata, block_number).await?;
    if raw && !json {
        println!("{}", crate::format::format_raw(&response));
        return Ok(());
    }
    let decoded = crate::abi::decode_response("balanceOf(address)(uint256)", &response)?;
    let balance = decoded
        .get(0)
        .ok_or_else(|| eyre!("balanceOf returned no value"))?
        .value
        .clone();
    let decimals = try_read_decimals(&rpc_urls, token_addr, block_number).await;
    let symbol = try_read_symbol(&rpc_urls, token_addr, block_number).await;
    let human_output = if let (Some(decimals), Ok(amount)) = (decimals, balance.parse::<U256>()) {
        let human = format_token_amount(amount, decimals);
        let suffix = symbol.as_ref().map(|sym| format!(" {sym}")).unwrap_or_default();
        Some(format!("{human}{suffix}"))
    } else {
        None
    };
    let human_for_json = human_output.clone();
    if json {
        println!(
            "{}",
            json!({
                "balance": balance,
                "human": human_for_json,
                "symbol": symbol.clone(),
                "decimals": decimals,
            })
        );
        return Ok(());
    }
    println!("Balance: {balance}");
    if let Some(human_value) = human_output {
        println!("Human: {human_value}");
    }
    if decimals.is_none() {
        if let Some(sym) = symbol.as_deref() {
            println!("Symbol: {sym}");
        }
    }
    Ok(())
}
fn resolve_token_address(chain: &str, token: &str, config: &crate::config::Config) -> Result<Address> {
    if let Some(addr) = crate::tokens::resolve_token(chain, token, Some(&config.tokens)) {
        return Ok(addr);
    }
    if token.starts_with("0x") {
        return token
            .parse::<Address>()
            .map_err(|err| eyre!("invalid token address: {err}"));
    }
    bail!("unknown token {token}")
}
async fn try_read_decimals(rpc_urls: &[String], token: Address, block: Option<u64>) -> Option<u8> {
    let calldata = crate::abi::encode_call("decimals()", &[]).ok()?;
    let response = crate::rpc::eth_call_with_fallback(rpc_urls, token, calldata, block)
        .await
        .ok()?;
    let decoded = crate::abi::decode_response("decimals()(uint8)", &response).ok()?;
    decoded.get(0).and_then(|value| value.value.parse::<u8>().ok())
}
async fn try_read_symbol(rpc_urls: &[String], token: Address, block: Option<u64>) -> Option<String> {
    let calldata = crate::abi::encode_call("symbol()", &[]).ok()?;
    let response = crate::rpc::eth_call_with_fallback(rpc_urls, token, calldata, block)
        .await
        .ok()?;
    let decoded = crate::abi::decode_response("symbol()(string)", &response).ok()?;
    decoded.get(0).map(|value| value.value.clone())
}
fn format_token_amount(amount: U256, decimals: u8) -> String {
    if decimals == 0 {
        return amount.to_string();
    }
    let scale = U256::from(10u64).pow(U256::from(decimals as u64));
    let integer = amount / scale;
    let fractional = amount % scale;
    if fractional.is_zero() {
        return integer.to_string();
    }
    let frac_str = fractional.to_string();
    let padded = format!("{:0>width$}", frac_str, width = decimals as usize);
    let trimmed = padded.trim_end_matches('0');
    format!("{integer}.{trimmed}")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rejects_unknown_chain() {
        assert!(run(
            "missing",
            "0x0000000000000000000000000000000000000000",
            "0x0000000000000000000000000000000000000000",
            false,
            None,
            None,
            None,
            false,
        )
        .await
        .is_err());
    }
    #[test]
    fn formats_token_amount_with_decimals() {
        let amount = U256::from(1500u64);
        assert_eq!(format_token_amount(amount, 3), "1.5");
    }
    #[test]
    fn resolves_direct_address_tokens() {
        let config = crate::config::Config {
            keys: None,
            default_provider: None,
            chains: std::collections::HashMap::new(),
            tokens: std::collections::HashMap::new(),
        };
        let addr = resolve_token_address("eth", "0x0000000000000000000000000000000000000001", &config).unwrap();
        assert_eq!(addr.to_string(), "0x0000000000000000000000000000000000000001");
    }
}
