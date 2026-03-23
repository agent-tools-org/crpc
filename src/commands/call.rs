// rpc call <chain> <contract> <sig> [args...] — eth_call with auto decode

use alloy::primitives::Address;
use eyre::Result;
use crate::commands::block::parse_block_number;
use hex::encode;

pub async fn run(
    chain: &str,
    contract: &str,
    sig: &str,
    args: &[String],
    raw: bool,
    human: bool,
    block: Option<&str>,
    from: Option<&str>,
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
        eyre::eyre!("invalid address: expected 0x + 40 hex chars, got {:?}", contract)
    })?;
    let from_addr = from
        .map(|f| f.parse::<Address>())
        .transpose()
        .map_err(|_| eyre::eyre!("invalid --from address"))?;
    let calldata = crate::abi::encode_call(sig, args)?;
    let block_number = parse_block_number(block)?;
    let response = match crate::rpc::eth_call_with_fallback(&rpc_urls, contract_addr, calldata, block_number, from_addr).await {
        Ok(bytes) => bytes,
        Err(err) => {
            if let Some(revert) = err.downcast_ref::<crate::rpc::RevertError>() {
                let reason = crate::abi::decode_revert(revert.data.as_ref());
                println!("Error: execution reverted: {reason}");
                println!("Revert data: 0x{}", encode(revert.data.as_ref()));
            }
            return Err(err);
        }
    };
    if raw && !json {
        println!("{}", crate::format::format_raw_words(&response));
        return Ok(());
    }
    let decoded = crate::abi::decode_response(sig, &response)?;
    if json {
        println!("{}", crate::format::format_json(&decoded));
        return Ok(());
    }
    let mode = crate::format::FormatMode::from_flags(raw, human);
    println!("{}", crate::format::format_values(&decoded, mode));
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
            false,
            false,
            None,
            None,
            None,
            None,
            false,
        )
        .await
        .is_err());
    }
}
