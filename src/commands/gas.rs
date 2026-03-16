// crpc gas <chain> — gas prices via Etherscan with RPC fallback
// Exports run; uses Etherscan gas oracle, falls back to eth_gasPrice

use eyre::Result;

pub async fn run(
    chain: &str,
    rpc_override: Option<&str>,
    provider: Option<&str>,
    json: bool,
) -> Result<()> {
    let chain_id = crate::config::resolve_chain_id(chain)?;

    // Try Etherscan first
    match crate::etherscan::EtherscanClient::new().gas_oracle(chain_id).await {
        Ok(gas) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "safe": gas.safe_gas_price,
                        "standard": gas.propose_gas_price,
                        "fast": gas.fast_gas_price,
                        "base_fee": gas.suggested_base_fee,
                    })
                );
            } else {
                println!("Gas Prices (Gwei):");
                println!("  Safe:     {}", gas.safe_gas_price);
                println!("  Standard: {}", gas.propose_gas_price);
                println!("  Fast:     {}", gas.fast_gas_price);
                if let Some(base_fee) = gas.suggested_base_fee {
                    println!("  Base fee: {base_fee}");
                }
            }
            Ok(())
        }
        Err(_) => run_rpc_fallback(chain, rpc_override, provider, json).await,
    }
}

async fn run_rpc_fallback(
    chain: &str,
    rpc_override: Option<&str>,
    provider: Option<&str>,
    json: bool,
) -> Result<()> {
    let config = crate::config::Config::load()?;
    let opts = crate::config::RpcOpts {
        rpc: rpc_override.map(String::from),
        provider: provider.map(String::from),
    };
    let rpc_url = config.resolve_rpc(chain, &opts)?;

    let gas_price = crate::rpc::get_gas_price(&rpc_url).await?;
    let priority_fee = crate::rpc::get_max_priority_fee(&rpc_url).await?;

    let gas_gwei = wei_to_gwei(gas_price);
    let priority_gwei = priority_fee.map(wei_to_gwei);

    if json {
        println!(
            "{}",
            serde_json::json!({
                "gas_price": gas_gwei,
                "priority_fee": priority_gwei,
                "source": "rpc",
            })
        );
    } else {
        println!("Gas Price (via RPC):");
        println!("  Gas price: {gas_gwei} Gwei");
        if let Some(pf) = priority_gwei {
            println!("  Priority:  {pf} Gwei");
        }
    }
    Ok(())
}

fn wei_to_gwei(wei: u128) -> String {
    let whole = wei / 1_000_000_000;
    let frac = wei % 1_000_000_000;
    if frac == 0 {
        whole.to_string()
    } else {
        let frac_str = format!("{frac:09}");
        let trimmed = frac_str.trim_end_matches('0');
        format!("{whole}.{trimmed}")
    }
}
