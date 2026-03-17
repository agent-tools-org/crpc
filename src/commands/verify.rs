// crpc verify <chain> <addresses...> — batch check code existence
// Exports run entrypoint and compact size formatting for output

use alloy::primitives::Address;
use eyre::Result;
use serde::Serialize;

#[derive(Serialize)]
struct VerifyResult<'a> {
    address: &'a str,
    is_contract: bool,
    code_size: usize,
}

pub async fn run(
    chain: &str,
    addresses: &[String],
    json: bool,
    rpc: Option<&str>,
    provider: Option<&str>,
) -> Result<()> {
    let config = crate::config::Config::load()?;
    let opts = crate::config::RpcOpts { rpc: rpc.map(String::from), provider: provider.map(String::from) };
    let rpc_url = config.resolve_rpc(chain, &opts)?;
    let mut rows = Vec::new();
    if !json {
        println!("{:<42} {:<9} {}", "Address", "Status", "Size");
    }
    for address in addresses {
        let addr = match address.parse::<Address>() {
            Ok(addr) => addr,
            Err(err) => {
                eprintln!("invalid address {address}: {err}");
                continue;
            }
        };
        let size = crate::rpc::get_code(&rpc_url, addr).await?.len();
        let is_contract = size > 0;
        if json {
            rows.push(VerifyResult { address, is_contract, code_size: size });
        } else {
            println!(
                "{:<42} {:<9} {}",
                address,
                if is_contract { "CONTRACT" } else { "EOA" },
                if is_contract { format_size(size) } else { "-".into() }
            );
        }
    }
    if json {
        println!("{}", serde_json::to_string(&rows)?);
    }
    Ok(())
}

fn format_size(size: usize) -> String {
    if size < 1024 { format!("{size} B") } else { format!("{:.1} KB", size as f64 / 1024.0) }
}

#[cfg(test)]
mod tests {
    use super::{format_size, run, VerifyResult};
    use serde_json::Value;

    #[test]
    fn format_size_handles_edge_cases() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(4200), "4.1 KB");
    }

    #[tokio::test]
    async fn run_skips_multiple_invalid_addresses_without_error() {
        let addresses = vec![
            "invalid".to_string(),
            "0x1234".to_string(),
            "not-an-address".to_string(),
        ];
        assert!(run("eth", &addresses, false, Some("http://127.0.0.1:9"), None).await.is_ok());
    }

    #[test]
    fn json_output_shape_is_valid() {
        let rows = vec![VerifyResult {
            address: "0x0000000000000000000000000000000000000001",
            is_contract: true,
            code_size: 1024,
        }];
        let output = serde_json::to_string(&rows).unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();
        let first = &parsed.as_array().unwrap()[0];
        assert_eq!(
            first.get("address").and_then(Value::as_str),
            Some("0x0000000000000000000000000000000000000001")
        );
        assert_eq!(
            first.get("is_contract").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            first.get("code_size").and_then(Value::as_u64),
            Some(1024)
        );
    }
}
