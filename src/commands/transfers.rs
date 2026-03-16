// crpc transfers <chain> <address> — ERC20 transfer history via Etherscan.
// Resolves chain IDs, queries explorer data, and prints text or JSON output.

use eyre::{eyre, Result};
use serde_json::Value;

pub async fn run(
    chain: &str,
    address: &str,
    token: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
    limit: usize,
    json_output: bool,
) -> Result<()> {
    if limit == 0 {
        println!("[]");
        return Ok(());
    }
    let chain_id = resolve_chain_id(chain)?;
    let client = crate::etherscan::EtherscanClient::new();
    let entries = client
        .get_token_transfers(
            chain_id,
            address,
            token,
            crate::commands::block::parse_block_number(from)?,
            crate::commands::block::parse_block_number(to)?,
            1,
            limit as u32,
            "desc",
        )
        .await?;
    if json_output {
        println!("{}", Value::Array(entries));
        return Ok(());
    }
    for entry in &entries {
        println!(
            "Block {} | {}",
            field(entry, "blockNumber"),
            field(entry, "hash")
        );
        println!("  From: {} -> To: {}", field(entry, "from"), field(entry, "to"));
        println!(
            "  Token: {} ({})",
            field(entry, "tokenSymbol"),
            field(entry, "contractAddress")
        );
        println!(
            "  Amount: {} {}",
            format_amount(field(entry, "value"), decimals(entry))?,
            field(entry, "tokenSymbol")
        );
    }
    Ok(())
}

fn resolve_chain_id(chain: &str) -> Result<u64> {
    crate::config::resolve_chain_id(chain)
}

fn field<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry.get(key).and_then(Value::as_str).unwrap_or("<missing>")
}

fn decimals(entry: &Value) -> u32 {
    field(entry, "tokenDecimal").parse::<u32>().unwrap_or(0)
}

fn format_amount(value: &str, decimals: u32) -> Result<String> {
    if !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(eyre!("invalid token value"));
    }
    if decimals == 0 {
        return Ok(value.to_string());
    }
    let width = decimals as usize;
    let padded = if value.len() <= width {
        format!("{:0>width$}", value, width = width + 1)
    } else {
        value.to_string()
    };
    let split = padded.len() - width;
    Ok(format!("{}.{}", &padded[..split], &padded[split..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_chain_id_from_alias_and_number() {
        assert_eq!(resolve_chain_id("eth").unwrap(), 1);
        assert_eq!(resolve_chain_id("1").unwrap(), 1);
    }

    #[test]
    fn formats_amount_with_fixed_decimals() {
        assert_eq!(format_amount("1000000", 6).unwrap(), "1.000000");
    }
}
