// rpc diff <chain> <contract> <sig> [args...] — compare local vs on-chain

use alloy::primitives::Address;
use eyre::{eyre, Result};
use serde_json::json;

pub async fn run(
    chain: &str,
    contract: &str,
    sig: &str,
    args: &[String],
    from: &str,
    to: &str,
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
    let contract_addr = contract.parse::<Address>().map_err(|_| {
        eyre!("invalid address: expected 0x + 40 hex chars, got {:?}", contract)
    })?;
    let from_block = crate::commands::block::parse_block_number(Some(from))?
        .ok_or_else(|| eyre!("from block must be numeric: {from}"))?;
    let to_block = crate::commands::block::parse_block_number(Some(to))?;
    let calldata = crate::abi::encode_call(sig, args)?;
    let (old_response, new_response) = tokio::try_join!(
        crate::rpc::eth_call_with_fallback(
            &rpc_urls,
            contract_addr,
            calldata.clone(),
            Some(from_block),
        ),
        crate::rpc::eth_call_with_fallback(&rpc_urls, contract_addr, calldata, to_block),
    )?;
    let old_decoded = crate::abi::decode_response(sig, &old_response)?;
    let new_decoded = crate::abi::decode_response(sig, &new_response)?;
    let diff_output = crate::format::format_diff(&old_decoded, &new_decoded, from, to);
    if json {
        println!("{}", json!({ "diff": diff_output }));
    } else {
        println!("{}", diff_output);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_unknown_chain() {
        let args: Vec<String> = vec![];
        assert!(run(
            "missing",
            "0x0000000000000000000000000000000000000000",
            "foo()",
            &args,
            "0",
            "0",
            None,
            None,
            false,
        )
        .await
        .is_err());
    }
}
