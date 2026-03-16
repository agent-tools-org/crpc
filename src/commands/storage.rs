// rpc slot <chain> <contract> <slot> — storage slot read
use alloy::primitives::{Address, U256};
use eyre::{eyre, Result};
use serde_json::json;
use std::str::FromStr;
use crate::commands::block::parse_block_number;

pub async fn run(
    chain: &str,
    contract: &str,
    slot: &str,
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
    let contract_addr = contract.parse::<Address>()?;
    let slot_value = parse_slot(slot)?;
    let block_number = parse_block_number(block)?;
    let storage = crate::rpc::get_storage_at_with_fallback(
        &rpc_urls,
        contract_addr,
        slot_value,
        block_number,
    )
    .await?;
    let slot_bytes: &[u8; 32] = storage.as_ref();
    let as_uint = U256::from_be_bytes(*slot_bytes);
    if json {
        println!(
            "{}",
            json!({
                "slot": slot,
                "hex": format!("0x{}", hex::encode(slot_bytes)),
                "uint256": as_uint.to_string(),
            })
        );
        return Ok(());
    }
    println!("Slot {slot}: 0x{}", hex::encode(slot_bytes));
    println!("As uint256: {as_uint}");
    Ok(())
}

fn parse_slot(slot: &str) -> Result<U256> {
    let normalized = slot.trim();
    if let Some(hex) = normalized.strip_prefix("0x") {
        U256::from_str_radix(hex, 16).map_err(|err| eyre!("invalid slot: {err}"))
    } else if normalized.is_empty() {
        eyre::bail!("slot cannot be empty")
    } else {
        U256::from_str(normalized).map_err(|err| eyre!("invalid slot: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_slot() {
        assert_eq!(parse_slot("0x10").unwrap(), U256::from(16u64));
    }

    #[test]
    fn parses_decimal_slot() {
        assert_eq!(parse_slot("255").unwrap(), U256::from(255u64));
    }
}
