// crpc abi-check <chain> <contract> <signature> — verify a function selector exists
// Checks verified ABI first, then falls back to PUSH4 selectors in on-chain bytecode

use alloy::primitives::Address;
use eyre::{eyre, Result};
use serde_json::Value;

pub async fn run(
    chain: &str,
    contract: &str,
    sig: &str,
    rpc_override: Option<&str>,
    provider: Option<&str>,
    json: bool,
) -> Result<()> {
    let request = build_request(contract, sig)?;
    let config = crate::config::Config::load()?;
    let opts = crate::config::RpcOpts {
        rpc: rpc_override.map(String::from),
        provider: provider.map(String::from),
    };
    let rpc_url = config.resolve_rpc(chain, &opts)?;
    let chain_id = crate::config::resolve_chain_id(chain)?;
    let outcome = check_selector(
        chain_id,
        request.address,
        request.selector,
        sig,
        &request.selector_hex,
        &rpc_url,
    )
    .await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&outcome)?);
    } else {
        print_text(contract, &request.selector_hex, &outcome);
    }
    Ok(())
}

#[derive(Debug)]
struct Request {
    address: Address,
    selector: [u8; 4],
    selector_hex: String,
}

#[derive(serde::Serialize)]
struct AbiCheckOutcome {
    selector: String,
    contract: String,
    found: bool,
    source: &'static str,
    matching_function: Option<String>,
    similar_functions: Vec<String>,
}

async fn check_selector(
    chain_id: u64,
    contract: Address,
    selector: [u8; 4],
    queried_sig: &str,
    selector_hex: &str,
    rpc_url: &str,
) -> Result<AbiCheckOutcome> {
    let client = crate::etherscan::EtherscanClient::new();
    let contract_text = contract.to_string();
    match client.get_abi(chain_id, &contract_text).await {
        Ok(entries) => Ok(check_abi_entries(selector_hex, queried_sig, contract_text, &entries)?),
        Err(_) => check_bytecode(contract, contract_text, selector, selector_hex, rpc_url).await,
    }
}

fn check_abi_entries(
    selector_hex: &str,
    queried_sig: &str,
    contract: String,
    entries: &[Value],
) -> Result<AbiCheckOutcome> {
    let query_name = function_name(queried_sig)?;
    let mut matching_function = None;
    let mut similar_functions = Vec::new();
    for entry in entries {
        let Some(signature) = abi_function_signature(entry)? else {
            continue;
        };
        let entry_selector_hex = hex::encode(crate::abi::compute_selector(&signature)?);
        if entry_selector_hex == selector_hex.trim_start_matches("0x") {
            matching_function = Some(signature);
            break;
        }
        if function_name(&signature)? == query_name {
            similar_functions.push(signature);
        }
    }
    Ok(AbiCheckOutcome {
        selector: selector_hex.to_string(),
        contract,
        found: matching_function.is_some(),
        source: "abi",
        matching_function,
        similar_functions,
    })
}

async fn check_bytecode(
    contract: Address,
    contract_text: String,
    selector: [u8; 4],
    selector_hex: &str,
    rpc_url: &str,
) -> Result<AbiCheckOutcome> {
    let code = crate::rpc::get_code(rpc_url, contract).await?;
    let found = crate::abi::extract_selectors_from_bytecode(&code).contains(&selector);
    Ok(AbiCheckOutcome {
        selector: selector_hex.to_string(),
        contract: contract_text,
        found,
        source: "bytecode",
        matching_function: None,
        similar_functions: Vec::new(),
    })
}

fn build_request(contract: &str, sig: &str) -> Result<Request> {
    let address = contract
        .parse::<Address>()
        .map_err(|err| eyre!("invalid address: {err}"))?;
    let selector = crate::abi::compute_selector(sig)?;
    Ok(Request {
        address,
        selector,
        selector_hex: format!("0x{}", hex::encode(selector)),
    })
}

fn print_text(contract: &str, selector_hex: &str, outcome: &AbiCheckOutcome) {
    println!("Selector: {selector_hex}");
    println!("Contract: {contract}");
    println!();
    if outcome.found {
        match (&outcome.matching_function, outcome.source) {
            (Some(signature), "abi") => println!("✓ FOUND — {signature}"),
            _ => println!("✓ FOUND (bytecode match, no ABI available)"),
        }
        return;
    }
    println!("✗ NOT FOUND — function selector {selector_hex} does not exist in this contract");
    if outcome.source == "abi" && !outcome.similar_functions.is_empty() {
        println!();
        println!("Available functions (from ABI):");
        for signature in &outcome.similar_functions {
            println!("  {signature}");
        }
    }
}

fn abi_function_signature(entry: &Value) -> Result<Option<String>> {
    if entry.get("type").and_then(Value::as_str) != Some("function") {
        return Ok(None);
    }
    let Some(name) = entry.get("name").and_then(Value::as_str) else {
        return Ok(None);
    };
    let params = abi_inputs_signature(entry.get("inputs"))?;
    Ok(Some(format!("{name}({params})")))
}

fn abi_inputs_signature(field: Option<&Value>) -> Result<String> {
    let Some(Value::Array(items)) = field else {
        return Ok(String::new());
    };
    let mut parts = Vec::with_capacity(items.len());
    for item in items {
        parts.push(abi_type(item)?);
    }
    Ok(parts.join(","))
}

fn abi_type(item: &Value) -> Result<String> {
    let raw = item
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("missing abi input type"))?;
    let Some(tuple_suffix) = raw.strip_prefix("tuple") else {
        return Ok(raw.to_string());
    };
    let components = item
        .get("components")
        .and_then(Value::as_array)
        .ok_or_else(|| eyre!("tuple input missing components"))?;
    let mut types = Vec::with_capacity(components.len());
    for component in components {
        types.push(abi_type(component)?);
    }
    Ok(format!("({}){tuple_suffix}", types.join(",")))
}

fn function_name(sig: &str) -> Result<&str> {
    sig.split_once('(')
        .map(|(name, _)| name)
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| eyre!("invalid function signature"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_rejects_invalid_signature() {
        let err = build_request("0x0000000000000000000000000000000000000001", "transfer").unwrap_err();
        assert!(err.to_string().contains("missing argument list"));
    }

    #[test]
    fn abi_function_signature_supports_tuple_arrays() {
        let entry = serde_json::json!({
            "type": "function",
            "name": "foo",
            "inputs": [
                {
                    "type": "tuple[]",
                    "components": [
                        {"type": "address"},
                        {"type": "uint256"}
                    ]
                }
            ]
        });
        assert_eq!(
            abi_function_signature(&entry).unwrap(),
            Some("foo((address,uint256)[])".to_string())
        );
    }
}
