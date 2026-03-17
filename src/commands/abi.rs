// crpc abi <chain> <contract> — fetch ABI from Etherscan V2 unified API
// Supports 60+ chains via single endpoint
// Exports run entrypoint returning formatted function signatures

use eyre::Result;
use serde_json::Value;

pub async fn run(chain: &str, contract: &str, raw: bool, json: bool) -> Result<()> {
    let chain_id = crate::config::resolve_chain_id(chain)?;
    let client = crate::etherscan::EtherscanClient::new();
    let entries = client.get_abi(chain_id, contract).await?;
    println!("{}", render(&entries, raw, json)?);
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
fn render(entries: &[Value], raw: bool, json: bool) -> Result<String> {
    if raw || json {
        return Ok(serde_json::to_string_pretty(entries)?);
    }
    Ok(entries.iter().filter_map(|entry| {
        let kind = entry.get("type").and_then(Value::as_str)?;
        let name = entry.get("name").and_then(Value::as_str).filter(|name| !name.is_empty())?;
        match kind {
            "function" => Some(format!("{name}({}) -> ({})", join_types(entry.get("inputs")), join_types(entry.get("outputs")))),
            "event" => Some(format!("event {name}({})", join_event_types(entry.get("inputs")))),
            _ => None,
        }
    }).collect::<Vec<_>>().join("\n"))
}
fn join_event_types(field: Option<&Value>) -> String {
    if let Some(Value::Array(items)) = field {
        return items.iter().filter_map(|item| {
            let ty = item.get("type").and_then(Value::as_str)?;
            Some(if item.get("indexed").and_then(Value::as_bool) == Some(true) { format!("{ty} indexed") } else { ty.to_string() })
        }).collect::<Vec<_>>().join(",");
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

    #[test]
    fn render_supports_json_and_events() {
        let entries = serde_json::json!([
            {"type": "function", "name": "transfer", "inputs": [{"type": "address"}, {"type": "uint256"}], "outputs": [{"type": "bool"}]},
            {"type": "event", "name": "Transfer", "inputs": [{"type": "address", "indexed": true}, {"type": "address", "indexed": true}, {"type": "uint256"}]},
        ]);
        assert_eq!(render(entries.as_array().unwrap(), false, false).unwrap(), "transfer(address,uint256) -> (bool)\nevent Transfer(address indexed,address indexed,uint256)");
        assert_eq!(render(entries.as_array().unwrap(), true, false).unwrap(), serde_json::to_string_pretty(&entries).unwrap());
    }
}
