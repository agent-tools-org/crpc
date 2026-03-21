// crpc cost <chain> <tx_hash> — calculate actual transaction cost in native token + USD
// Fetches receipt for gasUsed × effectiveGasPrice, plus L1 data fees on L2 chains (OP Stack)

use eyre::{eyre, Result};

struct NativeToken {
    symbol: &'static str,
    coingecko_id: &'static str,
    decimals: u32,
}

fn native_token_for_chain(chain_id: u64) -> NativeToken {
    match chain_id {
        56 => NativeToken { symbol: "BNB", coingecko_id: "binancecoin", decimals: 18 },
        137 => NativeToken { symbol: "POL", coingecko_id: "matic-network", decimals: 18 },
        43114 => NativeToken { symbol: "AVAX", coingecko_id: "avalanche-2", decimals: 18 },
        250 => NativeToken { symbol: "FTM", coingecko_id: "fantom", decimals: 18 },
        // ETH-based: mainnet, Base, Arbitrum, Optimism, Linea, Scroll, zkSync, etc.
        _ => NativeToken { symbol: "ETH", coingecko_id: "ethereum", decimals: 18 },
    }
}

/// OP Stack chain IDs: L1 data fee is separate from L2 execution fee
fn is_op_stack(chain_id: u64) -> bool {
    matches!(chain_id, 10 | 8453 | 7777777 | 34443 | 1135 | 255 | 291)
}

/// Arbitrum: gasUsed in receipt already includes L1 component
fn is_arbitrum(chain_id: u64) -> bool {
    matches!(chain_id, 42161 | 42170)
}

fn format_token_amount(wei: u128, decimals: u32) -> String {
    let divisor = 10u128.pow(decimals);
    let whole = wei / divisor;
    let frac = wei % divisor;
    if frac == 0 {
        return format!("{whole}");
    }
    let frac_str = format!("{frac:0>width$}", width = decimals as usize);
    let trimmed = frac_str.trim_end_matches('0');
    format!("{whole}.{trimmed}")
}

async fn fetch_usd_price(coingecko_id: &str) -> Result<f64> {
    let url = format!(
        "https://api.coingecko.com/api/v3/simple/price?ids={coingecko_id}&vs_currencies=usd"
    );
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await?;
    let body: serde_json::Value = resp.json().await?;
    body.get(coingecko_id)
        .and_then(|v| v.get("usd"))
        .and_then(|v| v.as_f64())
        .ok_or_else(|| eyre!("failed to parse price from CoinGecko"))
}

/// Fetch receipt as raw JSON to access chain-specific fields (l1Fee, l1GasUsed, etc.)
async fn fetch_raw_receipt(rpc_url: &str, tx_hash: &str) -> Result<serde_json::Value> {
    let provider = crate::rpc::make_provider(rpc_url)?;
    use alloy::providers::Provider;
    let result: serde_json::Value = provider
        .raw_request("eth_getTransactionReceipt".into(), (tx_hash,))
        .await?;
    Ok(result)
}

fn parse_hex_u128(hex: &str) -> Option<u128> {
    let stripped = hex.strip_prefix("0x").unwrap_or(hex);
    u128::from_str_radix(stripped, 16).ok()
}

pub async fn run(
    chain: &str,
    hash: &str,
    rpc_override: Option<&str>,
    provider: Option<&str>,
    json: bool,
) -> Result<()> {
    let is_valid_hash = hash.starts_with("0x")
        && hash.len() == 66
        && hash.as_bytes()[2..]
            .iter()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F'));
    if !is_valid_hash {
        return Err(eyre!(
            "Error: invalid transaction hash: expected 0x + 64 hex chars, got \"{hash}\""
        ));
    }

    let chain_id = crate::config::resolve_chain_id(chain)?;
    let config = crate::config::Config::load()?;
    let opts = crate::config::RpcOpts {
        rpc: rpc_override.map(String::from),
        provider: provider.map(String::from),
    };
    let rpc_urls = config.resolve_rpc_all(chain, &opts)?;
    let rpc_url = rpc_urls.first().ok_or_else(|| eyre!("no RPC URL for {chain}"))?;

    let raw = fetch_raw_receipt(rpc_url, hash).await?;
    if raw.is_null() {
        return Err(eyre!("receipt not found for {hash} (transaction may be pending)"));
    }

    let gas_used = raw.get("gasUsed")
        .and_then(|v| v.as_str())
        .and_then(parse_hex_u128)
        .ok_or_else(|| eyre!("missing gasUsed in receipt"))?;

    let effective_gas_price = raw.get("effectiveGasPrice")
        .and_then(|v| v.as_str())
        .and_then(parse_hex_u128)
        .ok_or_else(|| eyre!("missing effectiveGasPrice in receipt"))?;

    let l2_cost = gas_used * effective_gas_price;

    // L1 data fee for OP Stack chains (Base, Optimism, etc.)
    let l1_fee = if is_op_stack(chain_id) {
        raw.get("l1Fee")
            .and_then(|v| v.as_str())
            .and_then(parse_hex_u128)
            .unwrap_or(0)
    } else {
        0
    };

    let total_cost = l2_cost + l1_fee;

    let token = native_token_for_chain(chain_id);
    let cost_str = format_token_amount(total_cost, token.decimals);

    // Fetch USD price (best-effort)
    let usd_result = fetch_usd_price(token.coingecko_id).await;

    if json {
        let cost_f64 = total_cost as f64 / 10f64.powi(token.decimals as i32);
        let mut obj = serde_json::json!({
            "gas_used": gas_used,
            "effective_gas_price": effective_gas_price,
            "l2_cost_wei": l2_cost.to_string(),
            "cost_wei": total_cost.to_string(),
            "cost": cost_str,
            "symbol": token.symbol,
        });
        if l1_fee > 0 {
            obj.as_object_mut().unwrap().insert("l1_fee_wei".into(), serde_json::json!(l1_fee.to_string()));
            obj.as_object_mut().unwrap().insert("l1_fee".into(), serde_json::json!(format_token_amount(l1_fee, token.decimals)));
        }
        if let Ok(price) = &usd_result {
            let usd = cost_f64 * price;
            obj.as_object_mut().unwrap().insert("usd_price".into(), serde_json::json!(price));
            obj.as_object_mut().unwrap().insert("cost_usd".into(), serde_json::json!(format!("{usd:.6}")));
        }
        println!("{obj}");
    } else {
        let gas_price_gwei = effective_gas_price as f64 / 1e9;
        println!("Gas used:    {gas_used}");
        println!("Gas price:   {gas_price_gwei:.4} Gwei");
        if l1_fee > 0 {
            let l2_str = format_token_amount(l2_cost, token.decimals);
            let l1_str = format_token_amount(l1_fee, token.decimals);
            println!("L2 exec:     {l2_str} {}", token.symbol);
            println!("L1 data:     {l1_str} {}", token.symbol);
        }
        match usd_result {
            Ok(price) => {
                let cost_f64 = total_cost as f64 / 10f64.powi(token.decimals as i32);
                let usd = cost_f64 * price;
                if is_arbitrum(chain_id) {
                    println!("Cost:        {cost_str} {} (${usd:.6})  [includes L1+L2]", token.symbol);
                } else {
                    println!("Cost:        {cost_str} {} (${usd:.6})", token.symbol);
                }
            }
            Err(_) => {
                println!("Cost:        {cost_str} {}", token.symbol);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_token_amount_zero_frac() {
        assert_eq!(format_token_amount(1_000_000_000_000_000_000, 18), "1");
    }

    #[test]
    fn format_token_amount_with_frac() {
        assert_eq!(format_token_amount(500_000_000_000_000_000, 18), "0.5");
    }

    #[test]
    fn format_token_amount_small() {
        // 976776683561 wei = 0.000000976776683561 ETH
        assert_eq!(
            format_token_amount(976_776_683_561, 18),
            "0.000000976776683561"
        );
    }

    #[test]
    fn native_token_eth_chains() {
        assert_eq!(native_token_for_chain(1).symbol, "ETH");
        assert_eq!(native_token_for_chain(8453).symbol, "ETH");
        assert_eq!(native_token_for_chain(42161).symbol, "ETH");
        assert_eq!(native_token_for_chain(10).symbol, "ETH");
    }

    #[test]
    fn native_token_bsc() {
        assert_eq!(native_token_for_chain(56).symbol, "BNB");
    }

    #[test]
    fn op_stack_detection() {
        assert!(is_op_stack(10));   // Optimism
        assert!(is_op_stack(8453)); // Base
        assert!(!is_op_stack(1));   // Mainnet
        assert!(!is_op_stack(42161)); // Arbitrum
    }

    #[test]
    fn arbitrum_detection() {
        assert!(is_arbitrum(42161));
        assert!(is_arbitrum(42170)); // Nova
        assert!(!is_arbitrum(1));
    }

    #[test]
    fn parse_hex_values() {
        assert_eq!(parse_hex_u128("0x1"), Some(1));
        assert_eq!(parse_hex_u128("0xff"), Some(255));
        assert_eq!(parse_hex_u128("0x0"), Some(0));
    }

    #[tokio::test]
    async fn rejects_invalid_hash() {
        let err = run("base", "0x123", None, None, false).await.unwrap_err();
        assert!(err.to_string().contains("invalid transaction hash"));
    }
}
