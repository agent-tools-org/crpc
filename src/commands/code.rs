// crpc code <chain> <address> — show contract bytecode size/existence
// Useful for verifying if an address is a contract or EOA

use alloy::primitives::Address;
use eyre::{eyre, Result};

pub async fn run(
    chain: &str,
    address: &str,
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
    let addr = address
        .parse::<Address>()
        .map_err(|err| eyre!("invalid address: {err}"))?;

    let code = crate::rpc::get_code(&rpc_url, addr).await?;
    let size = code.len();
    let is_contract = size > 0;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "address": address,
                "is_contract": is_contract,
                "bytecode_size": size,
                "bytecode": format!("0x{}", hex::encode(&code)),
            })
        );
    } else {
        println!("Address: {address}");
        if is_contract {
            println!("Type:    Contract");
            println!("Size:    {size} bytes");
            // Show first/last few bytes for identification
            if size > 0 {
                let preview = if size <= 64 {
                    format!("0x{}", hex::encode(&code))
                } else {
                    format!(
                        "0x{}...{}",
                        hex::encode(&code[..32]),
                        hex::encode(&code[code.len() - 4..])
                    )
                };
                println!("Code:    {preview}");
            }
        } else {
            println!("Type:    EOA (no bytecode)");
        }
    }
    Ok(())
}
