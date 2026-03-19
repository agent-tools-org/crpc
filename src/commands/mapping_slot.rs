// crpc mapping-slot <chain> <contract> <slot> <key> — compute & read mapping storage slot
// Computes Solidity mapping slots and reads them via RPC fallback
use alloy::primitives::{keccak256, Address, B256, U256};
use eyre::{eyre, Result};
use serde_json::json;
use std::str::FromStr;
use crate::commands::block::parse_block_number;
use crate::config::{Config, RpcOpts};

pub async fn run(
    chain: &str,
    contract: &str,
    mapping_slot: &str,
    key: &str,
    block: Option<&str>,
    rpc_override: Option<&str>,
    provider: Option<&str>,
    json: bool,
) -> Result<()> {
    let config = Config::load()?;
    let opts = RpcOpts { rpc: rpc_override.map(String::from), provider: provider.map(String::from) };
    let rpc_urls = config.resolve_rpc_all(chain, &opts)?;
    let contract_addr = contract.parse::<Address>()?;
    let computed_slot = compute_mapping_slot(mapping_slot, key)?;
    let block_number = parse_block_number(block)?;
    let storage = crate::rpc::get_storage_at_with_fallback(
        &rpc_urls,
        contract_addr,
        U256::from_be_bytes(computed_slot.0),
        block_number,
    )
    .await?;
    let slot_bytes: &[u8; 32] = storage.as_ref();
    let as_uint = U256::from_be_bytes(*slot_bytes);
    if json {
        println!(
            "{}",
            json!({
                "mapping_slot": mapping_slot,
                "key": key,
                "computed_slot": format!("0x{}", hex::encode(computed_slot)),
                "hex": format!("0x{}", hex::encode(slot_bytes)),
                "uint256": as_uint.to_string(),
            })
        );
        return Ok(());
    }
    println!("Computed slot: 0x{}", hex::encode(computed_slot));
    println!("Value: 0x{}", hex::encode(slot_bytes));
    println!("As uint256: {as_uint}");
    Ok(())
}

fn compute_mapping_slot(mapping_slot: &str, key: &str) -> Result<B256> {
    let mut input = [0u8; 64];
    input[..32].copy_from_slice(&parse_key(key)?);
    input[32..].copy_from_slice(&parse_u256(mapping_slot)?.to_be_bytes::<32>());
    Ok(keccak256(input))
}

fn parse_key(key: &str) -> Result<[u8; 32]> {
    let normalized = key.trim();
    if normalized.len() == 42 && normalized.starts_with("0x") {
        return Ok(Address::from_str(normalized)?.into_word().0);
    }
    Ok(parse_u256(normalized)?.to_be_bytes::<32>())
}

fn parse_u256(value: &str) -> Result<U256> {
    let normalized = value.trim();
    if let Some(hex) = normalized.strip_prefix("0x") {
        U256::from_str_radix(hex, 16).map_err(|err| eyre!("invalid value: {err}"))
    } else if normalized.is_empty() {
        eyre::bail!("value cannot be empty")
    } else {
        U256::from_str(normalized).map_err(|err| eyre!("invalid value: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_address_key() {
        let parsed = parse_key("0x00000000000000000000000000000000000000ff").unwrap();
        assert_eq!(hex::encode(parsed), format!("{:064x}", 255u64));
    }

    #[test]
    fn computes_mapping_slot_for_uint_key() {
        let computed = compute_mapping_slot("5", "7").unwrap();
        let mut input = [0u8; 64];
        input[..32].copy_from_slice(&U256::from(7u64).to_be_bytes::<32>());
        input[32..].copy_from_slice(&U256::from(5u64).to_be_bytes::<32>());
        assert_eq!(computed, keccak256(input));
    }
}
