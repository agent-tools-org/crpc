// crpc balance-slot <chain> <token> <holder> — auto-detect balanceOf mapping storage slot
// Brute-forces common slot positions and matches against actual balanceOf result
use alloy::primitives::{keccak256, Address, U256};
use eyre::{eyre, Result};
use serde_json::json;

/// Common balanceOf mapping slot positions across ERC20 implementations
const CANDIDATE_SLOTS: &[u64] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
    11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    51, 101,
];

pub async fn run(
    chain: &str,
    token: &str,
    holder: &str,
    rpc_override: Option<&str>,
    provider: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let config = crate::config::Config::load()?;
    let opts = crate::config::RpcOpts {
        rpc: rpc_override.map(String::from),
        provider: provider.map(String::from),
    };
    let rpc_urls = config.resolve_rpc_all(chain, &opts)?;
    let token_addr = token.parse::<Address>()?;
    let holder_addr = holder.parse::<Address>()?;

    // Get expected balance via balanceOf call
    let calldata = crate::abi::encode_call("balanceOf(address)", &[holder_addr.to_string()])?;
    let response = crate::rpc::eth_call_with_fallback(&rpc_urls, token_addr, calldata, None).await?;
    let decoded = crate::abi::decode_response("balanceOf(address)(uint256)", &response)?;
    let balance_str = decoded
        .first()
        .ok_or_else(|| eyre!("balanceOf returned no value"))?
        .value
        .clone();
    let expected: U256 = balance_str.parse().map_err(|e| eyre!("parse balance: {e}"))?;

    if expected.is_zero() {
        eyre::bail!("holder has zero balance — cannot detect slot (need a non-zero balance to match)");
    }

    // Brute-force candidate slot positions
    let holder_word = holder_addr.into_word();
    for &pos in CANDIDATE_SLOTS {
        let mut input = [0u8; 64];
        input[..32].copy_from_slice(holder_word.as_ref());
        input[32..].copy_from_slice(&U256::from(pos).to_be_bytes::<32>());
        let computed = keccak256(input);

        let storage = crate::rpc::get_storage_at_with_fallback(
            &rpc_urls,
            token_addr,
            U256::from_be_bytes(computed.0),
            None,
        )
        .await?;
        let value = U256::from_be_bytes(*storage.as_ref());

        if value == expected {
            if json_output {
                println!(
                    "{}",
                    json!({
                        "slot_position": pos,
                        "computed_slot": format!("0x{}", hex::encode(computed)),
                        "balance": expected.to_string(),
                        "holder": format!("{holder_addr}"),
                    })
                );
            } else {
                println!("balanceOf storage slot: {pos} (computed: 0x{})", hex::encode(computed));
            }
            return Ok(());
        }
    }

    eyre::bail!("could not detect balanceOf slot in positions [0-20, 51, 101]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::Address;

    #[test]
    fn computes_expected_mapping_hash() {
        let holder = Address::ZERO;
        let holder_word = holder.into_word();
        let mut input = [0u8; 64];
        input[..32].copy_from_slice(holder_word.as_ref());
        input[32..].copy_from_slice(&U256::from(3u64).to_be_bytes::<32>());
        let hash = keccak256(input);
        // Just verify it produces a deterministic 32-byte hash
        assert_eq!(hash.len(), 32);
        assert_ne!(hash, keccak256([0u8; 64]));
    }
}
