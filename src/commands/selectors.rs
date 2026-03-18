// crpc selectors <chain> <contract> — extract bytecode selectors and resolve names.
// Uses RPC bytecode fetch plus optional Etherscan/OpenChain lookups.
// Exports the run entrypoint for CLI dispatch.

use alloy::primitives::{keccak256, Address};
use eyre::{eyre, Result};
use serde_json::Value;
use std::collections::HashMap;

pub async fn run(
    chain: &str,
    contract: &str,
    offline: bool,
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
    let address = contract
        .parse::<Address>()
        .map_err(|err| eyre!("invalid address: {err}"))?;
    let bytecode = crate::rpc::get_code(&rpc_url, address).await?;
    if bytecode.is_empty() {
        return Err(eyre!("address is an EOA, no bytecode"));
    }

    let selectors = crate::abi::extract_selectors_from_bytecode(&bytecode);
    let names = if offline {
        HashMap::new()
    } else {
        resolve_selector_names(chain, &format!("{address}"), &selectors).await
    };

    if json {
        let items = selectors
            .iter()
            .map(|selector| {
                serde_json::json!({
                    "selector": selector_hex(selector),
                    "name": names.get(selector),
                })
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::json!({
                "contract": format!("{address}"),
                "selectors": items,
            })
        );
    } else {
        println!("Contract: {address}");
        println!("Selectors: {} found", selectors.len());
        if !selectors.is_empty() {
            println!();
        }
        for selector in &selectors {
            let name = names
                .get(selector)
                .cloned()
                .unwrap_or_else(|| "??? (unknown)".to_string());
            println!("{}  {name}", selector_hex(selector));
        }
    }
    Ok(())
}

async fn resolve_selector_names(
    chain: &str,
    contract: &str,
    selectors: &[[u8; 4]],
) -> HashMap<[u8; 4], String> {
    let mut names = HashMap::new();
    if let Ok(chain_id) = crate::config::resolve_chain_id(chain) {
        if let Ok(entries) = crate::etherscan::EtherscanClient::new().get_abi(chain_id, contract).await {
            names.extend(selectors_from_abi(&entries));
        }
    }
    let unresolved = selectors
        .iter()
        .copied()
        .filter(|selector| !names.contains_key(selector))
        .collect::<Vec<_>>();
    if unresolved.is_empty() {
        return names;
    }
    let client = reqwest::Client::new();
    names.extend(lookup_selectors(&client, &unresolved).await);
    names
}

fn selectors_from_abi(entries: &[Value]) -> HashMap<[u8; 4], String> {
    entries
        .iter()
        .filter_map(function_signature)
        .map(|signature| (selector_bytes(&signature), signature))
        .collect()
}

fn function_signature(entry: &Value) -> Option<String> {
    if entry.get("type").and_then(Value::as_str) != Some("function") {
        return None;
    }
    let name = entry.get("name").and_then(Value::as_str)?;
    let inputs = entry.get("inputs").and_then(Value::as_array)?;
    let params = inputs
        .iter()
        .map(abi_param_type)
        .collect::<Option<Vec<_>>>()?
        .join(",");
    Some(format!("{name}({params})"))
}

fn abi_param_type(param: &Value) -> Option<String> {
    let ty = param.get("type").and_then(Value::as_str)?;
    if let Some(suffix) = ty.strip_prefix("tuple") {
        let components = param.get("components").and_then(Value::as_array)?;
        let inner = components
            .iter()
            .map(abi_param_type)
            .collect::<Option<Vec<_>>>()?
            .join(",");
        return Some(format!("({inner}){suffix}"));
    }
    Some(ty.to_string())
}

async fn lookup_selectors(
    client: &reqwest::Client,
    selectors: &[[u8; 4]],
) -> HashMap<[u8; 4], String> {
    let mut names = HashMap::new();
    for chunk in selectors.chunks(20) {
        let functions = chunk.iter().map(selector_hex).collect::<Vec<_>>().join(",");
        let response = client
            .get("https://api.openchain.xyz/signature-database/v1/lookup")
            .query(&[("function", functions.as_str()), ("filter", "true")])
            .send()
            .await;
        let Ok(response) = response else {
            continue;
        };
        let Ok(response) = response.error_for_status() else {
            continue;
        };
        let Ok(body) = response.json::<Value>().await else {
            continue;
        };
        names.extend(parse_openchain_lookup(&body));
    }
    names
}

fn parse_openchain_lookup(body: &Value) -> HashMap<[u8; 4], String> {
    let mut names = HashMap::new();
    let Some(functions) = body
        .get("result")
        .and_then(|value| value.get("function"))
        .and_then(Value::as_object)
    else {
        return names;
    };
    for (selector, entries) in functions {
        let Some(name) = entries
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.get("name"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Some(selector) = parse_selector_hex(selector) else {
            continue;
        };
        names.insert(selector, name.to_string());
    }
    names
}

fn selector_bytes(signature: &str) -> [u8; 4] {
    let hash = keccak256(signature.as_bytes());
    let mut selector = [0u8; 4];
    selector.copy_from_slice(&hash[..4]);
    selector
}

fn parse_selector_hex(selector: &str) -> Option<[u8; 4]> {
    let bytes = hex::decode(selector.strip_prefix("0x")?).ok()?;
    if bytes.len() != 4 {
        return None;
    }
    let mut parsed = [0u8; 4];
    parsed.copy_from_slice(&bytes);
    Some(parsed)
}

fn selector_hex(selector: &[u8; 4]) -> String {
    format!("0x{}", hex::encode(selector))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_signature_handles_tuple_inputs() {
        let entry = serde_json::json!({
            "type": "function",
            "name": "swap",
            "inputs": [
                {"type": "address"},
                {"type": "tuple[]", "components": [{"type": "uint256"}, {"type": "address"}]}
            ]
        });
        assert_eq!(
            function_signature(&entry).as_deref(),
            Some("swap(address,(uint256,address)[])")
        );
    }

    #[test]
    fn parse_openchain_lookup_extracts_first_name() {
        let body = serde_json::json!({
            "ok": true,
            "result": {
                "function": {
                    "0x38ed1739": [
                        {"name": "swapExactTokensForTokens(uint256,uint256,address[],address,address,uint256)"},
                        {"name": "ignored()"}
                    ],
                    "0xdeadbeef": []
                }
            }
        });
        let names = parse_openchain_lookup(&body);
        assert_eq!(
            names.get(&[0x38, 0xed, 0x17, 0x39]).map(String::as_str),
            Some("swapExactTokensForTokens(uint256,uint256,address[],address,address,uint256)")
        );
        assert!(!names.contains_key(&[0xde, 0xad, 0xbe, 0xef]));
    }
}
