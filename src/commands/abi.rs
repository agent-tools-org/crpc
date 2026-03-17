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
    use serde_json::Value;

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
    fn render_supports_human_readable_functions_and_events() {
        let entries = serde_json::json!([
            {"type": "function", "name": "transfer", "inputs": [{"type": "address"}, {"type": "uint256"}], "outputs": [{"type": "bool"}]},
            {"type": "event", "name": "Transfer", "inputs": [{"type": "address", "indexed": true}, {"type": "address", "indexed": true}, {"type": "uint256"}]},
        ]);
        assert_eq!(render(entries.as_array().unwrap(), false, false).unwrap(), "transfer(address,uint256) -> (bool)\nevent Transfer(address indexed,address indexed,uint256)");
    }

    #[test]
    fn render_raw_and_json_modes_return_valid_json_arrays() {
        let entries = serde_json::json!([
            {"type": "function", "name": "approve", "inputs": [{"type": "address"}, {"type": "uint256"}], "outputs": [{"type": "bool"}]},
            {"type": "event", "name": "Approval", "inputs": [{"type": "address", "indexed": true}, {"type": "address", "indexed": true}, {"type": "uint256"}]},
        ]);
        for output in [
            render(entries.as_array().unwrap(), true, false).unwrap(),
            render(entries.as_array().unwrap(), false, true).unwrap(),
        ] {
            let parsed: Value = serde_json::from_str(&output).unwrap();
            assert!(parsed.is_array());
            assert_eq!(parsed, entries);
        }
    }

    #[test]
    fn render_handles_empty_function_only_and_event_only_entries() {
        assert_eq!(render(&[], false, false).unwrap(), "");

        let function_only = serde_json::json!([
            {"type": "function", "name": "totalSupply", "inputs": [], "outputs": [{"type": "uint256"}]},
        ]);
        assert_eq!(
            render(function_only.as_array().unwrap(), false, false).unwrap(),
            "totalSupply() -> (uint256)"
        );

        let event_only = serde_json::json!([
            {"type": "event", "name": "Paused", "inputs": []},
        ]);
        assert_eq!(
            render(event_only.as_array().unwrap(), false, false).unwrap(),
            "event Paused()"
        );
    }
}
