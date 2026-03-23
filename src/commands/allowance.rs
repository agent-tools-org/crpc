// crpc allowance <chain> <token> <owner> <spender> — ERC20 allowance check
// Standard allowance(address,address) call with human-readable output

use alloy::primitives::{Address, U256};
use eyre::{eyre, Result};

pub async fn run(
    chain: &str,
    token: &str,
    owner: &str,
    spender: &str,
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

    let token_address = resolve_token_address(chain, token, &config)?;
    let owner_addr = owner
        .parse::<Address>()
        .map_err(|err| eyre!("invalid owner address: {err}"))?;
    let spender_addr = spender
        .parse::<Address>()
        .map_err(|err| eyre!("invalid spender address: {err}"))?;
    let block_num = crate::commands::block::parse_block_number(block)?;

    let calldata = crate::abi::encode_call(
        "allowance(address,address)",
        &[owner.to_string(), spender.to_string()],
    )?;

    let response = crate::rpc::eth_call_with_fallback(
        &rpc_urls,
        token_address,
        calldata,
        block_num,
        None,
    )
    .await?;

    let decoded = crate::abi::decode_response("allowance(address,address)(uint256)", &response)?;
    let raw_value = decoded
        .first()
        .map(|d| d.value.clone())
        .unwrap_or_else(|| "0".to_string());

    if json {
        println!(
            "{}",
            serde_json::json!({
                "token": token_address.to_string(),
                "owner": owner_addr.to_string(),
                "spender": spender_addr.to_string(),
                "allowance": raw_value,
            })
        );
    } else {
        println!("Token:     {token}");
        println!("Owner:     {owner}");
        println!("Spender:   {spender}");
        if is_max_uint256(&raw_value) {
            println!("Allowance: unlimited (max uint256)");
        } else {
            println!("Allowance: {raw_value}");
        }
    }
    Ok(())
}

fn resolve_token_address(chain: &str, token: &str, config: &crate::config::Config) -> eyre::Result<Address> {
    if let Some(addr) = crate::tokens::resolve_token(chain, token, Some(&config.tokens)) {
        return Ok(addr);
    }
    if token.starts_with("0x") {
        return token
            .parse::<Address>()
            .map_err(|err| eyre!("invalid token address: {err}"));
    }
    Err(eyre!("unknown token {token}"))
}

fn is_max_uint256(value: &str) -> bool {
    value == U256::MAX.to_string()
}
