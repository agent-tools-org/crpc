// rpc block <chain> [number|latest] — block info
use eyre::{eyre, Result};

pub async fn run(
    chain: &str,
    number: Option<&str>,
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
    let block_label = number.unwrap_or("latest");
    let block_number = parse_block_number(number)?;
    let block = crate::rpc::get_block_with_fallback(&rpc_urls, block_number)
        .await?
        .ok_or_else(|| eyre!("block {block_label} not found"))?;
    let header = block.header;
    let hash_bytes: &[u8] = header.hash.as_ref();
    if json {
        println!(
            "{}",
            crate::format::format_block_json(
                header.number,
                hash_bytes,
                header.timestamp,
                header.gas_used,
                header.gas_limit,
                block.transactions.len(),
                header.base_fee_per_gas.map(|fee| fee.to_string()),
            )
        );
        return Ok(());
    }
    println!("Number: {}", header.number);
    println!("Hash: 0x{}", hex::encode(hash_bytes));
    println!("Timestamp: {}", header.timestamp);
    println!("Gas used: {}", header.gas_used);
    println!("Gas limit: {}", header.gas_limit);
    println!("Transactions: {}", block.transactions.len());
    if let Some(base_fee) = header.base_fee_per_gas {
        println!("Base fee per gas: {base_fee}");
    } else {
        println!("Base fee per gas: <none>");
    }
    Ok(())
}

pub fn parse_block_number(input: Option<&str>) -> Result<Option<u64>> {
    match input {
        None => Ok(None),
        Some(raw) => {
            let normalized = raw.trim();
            if normalized.is_empty() || normalized.eq_ignore_ascii_case("latest") {
                return Ok(None);
            }
            normalized
                .parse::<u64>()
                .map(Some)
                .map_err(|err| eyre!("invalid block number: {err}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_returns_none() {
        assert!(parse_block_number(Some("latest")).unwrap().is_none());
    }

    #[test]
    fn parses_decimal_number() {
        assert_eq!(parse_block_number(Some("42")).unwrap(), Some(42));
    }
}
