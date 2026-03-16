// crpc abi <chain> <contract> — fetch ABI from Etherscan V2 unified API
// Supports 60+ chains via single endpoint
// Exports run entrypoint returning formatted function signatures

use eyre::Result;
use serde_json::Value;

pub async fn run(chain: &str, contract: &str) -> Result<()> {
    let chain_id = crate::config::resolve_chain_id(chain)?;
    let client = crate::etherscan::EtherscanClient::new();
    let entries = client.get_abi(chain_id, contract).await?;
    for entry in &entries {
        if entry.get("type").and_then(Value::as_str) != Some("function") {
            continue;
        }
        let name = match entry.get("name").and_then(Value::as_str) {
            Some(name) if !name.is_empty() => name,
            _ => continue,
        };
        let inputs = join_types(entry.get("inputs"));
        let outputs = join_types(entry.get("outputs"));
        println!("{name}({inputs}) -> ({outputs})");
    }
    Ok(())
}

fn join_types(field: Option<&Value>) -> String {
    if let Some(Value::Array(items)) = field {
        let parts: Vec<&str> = items
            .iter()
            .filter_map(|item| item.get("type").and_then(Value::as_str))
            .collect();
        return parts.join(",");
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_types_formats_correctly() {
        let input = serde_json::json!([
            {"type": "address", "name": "to"},
            {"type": "uint256", "name": "amount"},
        ]);
        assert_eq!(join_types(Some(&input)), "address,uint256");
    }

    #[test]
    fn join_types_empty_array() {
        let input = serde_json::json!([]);
        assert_eq!(join_types(Some(&input)), "");
    }
}
