// rpc simulate <chain> --to <address> --data <hex> — eth_call with optional tx fields
// Executes direct provider.call and decodes success or revert payloads

use alloy::eips::BlockId;
use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::Provider;
use alloy::rpc::types::transaction::{TransactionInput, TransactionRequest};
use eyre::{eyre, Report, Result};
use serde_json::json;
use std::str::FromStr;

pub async fn run(
    chain: &str,
    to: &str,
    data: &str,
    from: Option<&str>,
    value: Option<&str>,
    block: Option<&str>,
    sig: Option<&str>,
    rpc_override: Option<&str>,
    provider: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let config = crate::config::Config::load()?;
    let opts = crate::config::RpcOpts {
        rpc: rpc_override.map(String::from),
        provider: provider.map(String::from),
    };
    let rpc_url = config.resolve_rpc(chain, &opts)?;
    let to_addr = parse_address("to", to)?;
    let calldata = parse_data(data)?;
    let block_number = crate::commands::block::parse_block_number(block)?;
    let tx = build_request(to_addr, calldata, from, value)?;
    let provider = crate::rpc::make_provider(&rpc_url)?;
    let block_id = block_number.map(BlockId::number).unwrap_or_else(BlockId::latest);

    match provider.call(tx).block(block_id).await {
        Ok(response) => print_success(&response, sig, json_output),
        Err(err) => {
            if let Some(payload) = err.as_error_resp() {
                if let Some(revert_data) = payload.as_revert_data() {
                    return print_revert(revert_data, json_output);
                }
            }
            Err(Report::new(err))
        }
    }
}

fn build_request(
    to: Address,
    data: Bytes,
    from: Option<&str>,
    value: Option<&str>,
) -> Result<TransactionRequest> {
    let mut tx = TransactionRequest::default()
        .to(to)
        .input(TransactionInput::new(data));
    if let Some(from_raw) = from {
        tx = tx.from(parse_address("from", from_raw)?);
    }
    if let Some(value_raw) = value {
        tx = tx.value(parse_value(value_raw)?);
    }
    Ok(tx)
}

fn parse_address(name: &str, value: &str) -> Result<Address> {
    value
        .parse::<Address>()
        .map_err(|_| eyre!("invalid {name} address: expected 0x + 40 hex chars, got {:?}", value))
}

fn parse_data(value: &str) -> Result<Bytes> {
    let trimmed = value.trim();
    let payload = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    let bytes = hex::decode(payload).map_err(|err| eyre!("invalid data hex: {err}"))?;
    Ok(Bytes::from(bytes))
}

fn parse_value(value: &str) -> Result<U256> {
    let normalized = value.trim();
    if let Some(hex) = normalized.strip_prefix("0x") {
        return U256::from_str_radix(hex, 16).map_err(|err| eyre!("invalid value: {err}"));
    }
    U256::from_str(normalized).map_err(|err| eyre!("invalid value: {err}"))
}

fn print_success(response: &Bytes, sig: Option<&str>, json_output: bool) -> Result<()> {
    if let Some(signature) = sig {
        let decoded = crate::abi::decode_response(signature, response)?;
        if json_output {
            println!(
                "{}",
                json!({
                    "status": "success",
                    "result": format!("0x{}", hex::encode(response)),
                    "decoded": decoded.iter().map(|value| json!({
                        "type": value.ty,
                        "value": value.value,
                    })).collect::<Vec<_>>(),
                    "gas_used": serde_json::Value::Null,
                })
            );
            return Ok(());
        }
        println!("Status:  Success");
        println!("Result:");
        println!("{}", crate::format::format_values(&decoded, crate::format::FormatMode::Decode));
        return Ok(());
    }

    if json_output {
        println!(
            "{}",
            json!({
                "status": "success",
                "result": format!("0x{}", hex::encode(response)),
                "decoded": serde_json::Value::Null,
                "gas_used": serde_json::Value::Null,
            })
        );
        return Ok(());
    }

    println!("Status:  Success");
    println!("Result:  0x{}", hex::encode(response));
    println!();
    println!("{}", crate::format::format_raw_words(response));
    Ok(())
}

fn print_revert(revert_data: Bytes, json_output: bool) -> Result<()> {
    let reason = crate::abi::decode_revert(revert_data.as_ref());
    let encoded = format!("0x{}", hex::encode(revert_data.as_ref()));
    if json_output {
        println!(
            "{}",
            json!({
                "status": "reverted",
                "reason": reason,
                "data": encoded,
                "gas_used": serde_json::Value::Null,
            })
        );
    } else {
        println!("Status:  Reverted");
        println!("Reason:  {reason}");
        println!("Data:    {encoded}");
    }
    Err(Report::new(crate::rpc::RevertError { data: revert_data }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::TxKind;

    #[test]
    fn invalid_hex_data_is_rejected() {
        let err = parse_data("0xzz").unwrap_err();
        assert!(err.to_string().contains("invalid data hex"));
    }

    #[test]
    fn parse_value_accepts_decimal() {
        assert_eq!(parse_value("42").unwrap(), U256::from(42_u64));
    }

    #[test]
    fn parse_value_accepts_hex() {
        assert_eq!(parse_value("0x2a").unwrap(), U256::from(42_u64));
    }

    #[test]
    fn parse_address_rejects_invalid_input() {
        let err = parse_address("to", "0x1234").unwrap_err();
        assert!(err.to_string().contains("invalid to address"));
    }

    #[test]
    fn build_request_applies_optional_fields() {
        let to = parse_address("to", "0x0000000000000000000000000000000000000001").unwrap();
        let from = "0x0000000000000000000000000000000000000002";
        let tx = build_request(to, Bytes::from(vec![0xde, 0xad]), Some(from), Some("0x2a")).unwrap();
        assert_eq!(tx.from, Some(parse_address("from", from).unwrap()));
        assert_eq!(tx.value, Some(U256::from(42_u64)));
        assert_eq!(tx.to, Some(TxKind::Call(to)));
    }

    #[test]
    fn revert_reason_decodes_with_abi_helper() {
        let data = hex::decode("08c379a000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000014696e73756666696369656e742062616c616e6365000000000000000000000000").unwrap();
        assert_eq!(crate::abi::decode_revert(&data), "insufficient balance");
    }
}
