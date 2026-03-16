// crpc history <chain> <address> [--from <block>] [--to <block>] [--limit N]
// Fetches recent address transactions from Etherscan. Falls back to recent block
// scanning via RPC on L2s where Etherscan requires paid API key.

use alloy_consensus::transaction::Transaction as ConsensusTransaction;
use eyre::Result;
use serde_json::Value;

pub async fn run(
    chain: &str,
    address: &str,
    from: Option<&str>,
    to: Option<&str>,
    limit: usize,
    rpc_override: Option<&str>,
    provider: Option<&str>,
    json_output: bool,
) -> Result<()> {
    if limit == 0 {
        return Ok(());
    }
    let chain_id = resolve_chain_id(chain)?;
    let client = crate::etherscan::EtherscanClient::new();
    match client
        .get_tx_list(chain_id, address, parse_block(from)?, parse_block(to)?, 1, limit as u32, "desc")
        .await
    {
        Ok(txs) => render(address, &txs, json_output),
        Err(e) => {
            eprintln!("Etherscan failed: {e}");
            eprintln!("Falling back to recent block scan via RPC...");
            run_rpc_fallback(chain, address, from, to, limit, rpc_override, provider, json_output).await
        }
    }
}

fn render(address: &str, txs: &[Value], json_output: bool) -> Result<()> {
    if json_output {
        println!("{}", Value::Array(txs.to_vec()));
        return Ok(());
    }
    if txs.is_empty() {
        println!("No transactions found for {address}");
        return Ok(());
    }
    for tx in txs {
        let block = tx.get("blockNumber").and_then(Value::as_str).unwrap_or("?");
        let hash = shorten(tx.get("hash").and_then(Value::as_str).unwrap_or("?"));
        let from = shorten(tx.get("from").and_then(Value::as_str).unwrap_or("?"));
        let to = shorten(tx.get("to").and_then(Value::as_str).unwrap_or("?"));
        let gas = tx.get("gasUsed").or_else(|| tx.get("gas")).and_then(Value::as_str).unwrap_or("?");
        let function = tx.get("functionName").and_then(Value::as_str).filter(|v| !v.is_empty()).unwrap_or("<unknown>");
        let value = format_eth(tx.get("value").and_then(Value::as_str).unwrap_or("0"));
        println!("Block {block} | {hash}");
        println!("  From: {from} → To: {to}");
        println!("  Value: {value} ETH | Gas: {gas}");
        println!("  Function: {function}");
    }
    Ok(())
}

/// Scan recent blocks via RPC looking for transactions involving the address.
/// Limited approach — only checks recent blocks, but works without Etherscan.
async fn run_rpc_fallback(
    chain: &str,
    address: &str,
    from: Option<&str>,
    to: Option<&str>,
    limit: usize,
    rpc_override: Option<&str>,
    provider: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let config = crate::config::Config::load()?;
    let opts = crate::config::RpcOpts {
        rpc: rpc_override.map(String::from),
        provider: provider.map(String::from),
    };
    let rpc_url = config.resolve_rpc(chain, &opts)?;

    let latest = crate::rpc::get_block_number(&rpc_url).await?;
    let to_block = match parse_block(to)? {
        Some(b) => b,
        None => latest,
    };
    let from_block = match parse_block(from)? {
        Some(b) => b,
        None => to_block.saturating_sub(100), // scan last 100 blocks
    };

    let target = address.to_lowercase();
    let mut found = Vec::new();

    for block_num in (from_block..=to_block).rev() {
        if found.len() >= limit {
            break;
        }
        let block = crate::rpc::get_block(&rpc_url, Some(block_num)).await?;
        let Some(block) = block else { continue };
        for tx in block.transactions.txns() {
            if found.len() >= limit {
                break;
            }
            let tx_from = tx.inner.signer().to_string().to_lowercase();
            let tx_to = tx.inner.to().map(|a| a.to_string().to_lowercase()).unwrap_or_default();
            if tx_from == target || tx_to == target {
                let entry = serde_json::json!({
                    "blockNumber": block_num.to_string(),
                    "hash": tx.inner.tx_hash().to_string(),
                    "from": tx_from,
                    "to": tx_to,
                    "value": tx.inner.value().to_string(),
                    "gas": tx.inner.gas_limit().to_string(),
                });
                found.push(entry);
            }
        }
    }

    if found.is_empty() {
        eprintln!("No transactions found in blocks {from_block}..{to_block}");
        eprintln!("Tip: use --from/--to for a wider range, or set ETHERSCAN_API_KEY for full history");
        return Ok(());
    }

    render(address, &found, json_output)
}

fn resolve_chain_id(chain: &str) -> Result<u64> {
    crate::config::resolve_chain_id(chain)
}

fn parse_block(block: Option<&str>) -> Result<Option<u64>> {
    crate::commands::block::parse_block_number(block)
}

fn shorten(value: &str) -> String {
    if value.len() <= 14 { value.to_string() } else { format!("{}...{}", &value[..10], &value[value.len() - 4..]) }
}

fn format_eth(wei: &str) -> String {
    let wei = wei.parse::<u128>().unwrap_or(0);
    let whole = wei / 1_000_000_000_000_000_000;
    let frac = (wei % 1_000_000_000_000_000_000) / 1_000_000_000_000_000;
    if frac == 0 { whole.to_string() } else { format!("{whole}.{frac:03}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_eth_chain_alias() {
        assert_eq!(resolve_chain_id("eth").unwrap(), 1);
    }

    #[test]
    fn resolves_numeric_chain_id() {
        assert_eq!(resolve_chain_id("8453").unwrap(), 8453);
    }

    #[test]
    fn formats_eth_and_shortens_ids() {
        assert_eq!(format_eth("500000000000000000"), "0.500");
        assert_eq!(shorten("0x1234567890abcdef"), "0x12345678...cdef");
    }

    #[test]
    fn parses_blocks() {
        assert_eq!(parse_block(Some("42")).unwrap(), Some(42));
    }
}
