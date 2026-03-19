// Trace command for debug_traceTransaction
// Exports `run` to render callTracer responses as a call tree
// Depends on crate::config, crate::rpc, selectors, abi, serde_json, and alloy primitives
use alloy::primitives::{B256, U256};
use eyre::{eyre, Result};
use serde_json::Value;
use std::collections::HashMap;

type SelectorMap = HashMap<[u8; 4], String>;

pub async fn run(
    chain: &str,
    hash: &str,
    depth: usize,
    rpc_override: Option<&str>,
    provider: Option<&str>,
    json: bool,
) -> Result<()> {
    if !is_valid_tx_hash(hash) {
        return Err(eyre!(
            "Error: invalid transaction hash: expected 0x + 64 hex chars, got \"{hash}\""
        ));
    }

    let config = crate::config::Config::load()?;
    let opts = crate::config::RpcOpts {
        rpc: rpc_override.map(String::from),
        provider: provider.map(String::from),
    };
    let rpc_url = config.resolve_rpc(chain, &opts)?;
    let tx_hash = hash.parse::<B256>()?;
    let trace = crate::rpc::debug_trace_transaction(&rpc_url, tx_hash)
        .await
        .map_err(|err| {
            let message = err.to_string().to_lowercase();
            if message.contains("method not found") {
                eyre!("Error: debug_traceTransaction not supported (requires archive/debug node)")
            } else if message.contains("not found") {
                eyre!("Error: transaction not found")
            } else {
                err
            }
        })?;

    if json {
        println!("{}", serde_json::to_string_pretty(&trace)?);
        return Ok(());
    }

    // Collect unique selectors per target address, resolve in batch
    let mut addr_selectors: HashMap<String, Vec<[u8; 4]>> = HashMap::new();
    collect_selectors(&trace, &mut addr_selectors);
    let names = resolve_all_selectors(chain, &addr_selectors).await;

    for line in collect_call_lines(&trace, 0, depth, &names) {
        println!("{line}");
    }
    Ok(())
}

/// Walk the trace tree and collect unique (address, selector) pairs
fn collect_selectors(call: &Value, map: &mut HashMap<String, Vec<[u8; 4]>>) {
    if let (Some(to), Some(input)) = (
        call.get("to").and_then(Value::as_str),
        call.get("input").and_then(Value::as_str),
    ) {
        if let Some(selector) = parse_selector(input) {
            let entry = map.entry(to.to_lowercase()).or_default();
            if !entry.contains(&selector) {
                entry.push(selector);
            }
        }
    }
    if let Some(children) = call.get("calls").and_then(Value::as_array) {
        for child in children {
            collect_selectors(child, map);
        }
    }
}

fn parse_selector(input: &str) -> Option<[u8; 4]> {
    let hex = input.strip_prefix("0x").unwrap_or(input);
    if hex.len() < 8 {
        return None;
    }
    let bytes = hex::decode(&hex[..8]).ok()?;
    let mut sel = [0u8; 4];
    sel.copy_from_slice(&bytes);
    Some(sel)
}

/// Resolve selectors for all addresses via Etherscan ABI + OpenChain (best-effort)
async fn resolve_all_selectors(
    chain: &str,
    addr_selectors: &HashMap<String, Vec<[u8; 4]>>,
) -> SelectorMap {
    let mut names = SelectorMap::new();
    for (addr, selectors) in addr_selectors {
        let resolved =
            super::selectors::resolve_selector_names(chain, addr, selectors).await;
        names.extend(resolved);
    }
    names
}

fn is_valid_tx_hash(hash: &str) -> bool {
    hash.starts_with("0x")
        && hash.len() == 66
        && hash.as_bytes()[2..]
            .iter()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F'))
}

fn collect_call_lines(
    call: &Value,
    indent: usize,
    max_depth: usize,
    names: &SelectorMap,
) -> Vec<String> {
    if indent > max_depth {
        return Vec::new();
    }
    let indent_str = "  ".repeat(indent);
    let call_type = call.get("type").and_then(Value::as_str).unwrap_or("CALL");
    let from = call.get("from").and_then(Value::as_str).unwrap_or("<unknown>");
    let to = call.get("to").and_then(Value::as_str).unwrap_or("<unknown>");
    let value_hex = call.get("value").and_then(Value::as_str).unwrap_or("0x0");
    let input = call.get("input").and_then(Value::as_str).unwrap_or("");

    // Function name from selector
    let func = parse_selector(input)
        .map(|sel| {
            names
                .get(&sel)
                .map(String::as_str)
                .unwrap_or_else(|| selector_hex_str(input))
        })
        .unwrap_or("");

    // Gas used
    let gas = call
        .get("gasUsed")
        .and_then(Value::as_str)
        .map(format_hex_u64)
        .unwrap_or_default();

    // Build main line: TYPE from -> to  funcName  value: N  gas: N
    let mut main = format!("{indent_str}{call_type} {from} -> {to}");
    if !func.is_empty() {
        main.push_str(&format!("  {func}"));
    }
    main.push_str(&format!("  value: {}", format_value_decimal(value_hex)));
    if !gas.is_empty() {
        main.push_str(&format!("  gas: {gas}"));
    }
    let mut lines = vec![main];

    // Input calldata
    if !input.is_empty() && input != "0x" {
        lines.push(format!(
            "{indent_str}  input: {input} ({} bytes)",
            hex_bytes_len(input)
        ));
    }

    // Output or revert
    let error = call.get("error").and_then(Value::as_str);
    let output = call.get("output").and_then(Value::as_str).unwrap_or("");

    if let Some(err_msg) = error {
        let reason = if !output.is_empty() && output != "0x" {
            decode_revert_hex(output)
        } else {
            err_msg.to_string()
        };
        lines.push(format!("{indent_str}  \u{21a9} revert: {reason}"));
    } else if !output.is_empty() && output != "0x" {
        lines.push(format!(
            "{indent_str}  output: {output} ({} bytes)",
            hex_bytes_len(output)
        ));
    }

    if indent >= max_depth {
        return lines;
    }
    if let Some(children) = call.get("calls").and_then(Value::as_array) {
        for child in children {
            lines.extend(collect_call_lines(child, indent + 1, max_depth, names));
        }
    }
    lines
}

/// Return the first 10 chars of hex input as selector label (e.g. "0xdeadc0de")
fn selector_hex_str(input: &str) -> &str {
    if input.starts_with("0x") && input.len() >= 10 {
        &input[..10]
    } else {
        input
    }
}

fn decode_revert_hex(hex_output: &str) -> String {
    let hex = hex_output.strip_prefix("0x").unwrap_or(hex_output);
    match hex::decode(hex) {
        Ok(bytes) => crate::abi::decode_revert(&bytes),
        Err(_) => hex_output.to_string(),
    }
}

fn hex_bytes_len(value: &str) -> usize {
    let trimmed = value.strip_prefix("0x").unwrap_or(value);
    (trimmed.len() + 1) / 2
}

fn format_value_decimal(value: &str) -> String {
    let trimmed = value.strip_prefix("0x").unwrap_or(value);
    if trimmed.is_empty() {
        return "0".to_string();
    }
    U256::from_str_radix(trimmed, 16)
        .map(|val| val.to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn format_hex_u64(value: &str) -> String {
    let trimmed = value.strip_prefix("0x").unwrap_or(value);
    if trimmed.is_empty() {
        return "0".to_string();
    }
    u64::from_str_radix(trimmed, 16)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_hash() {
        assert!(!is_valid_tx_hash("0x123"));
        assert!(is_valid_tx_hash(
            "0x0000000000000000000000000000000000000000000000000000000000000000"
        ));
    }

    #[test]
    fn parse_selector_extracts_4_bytes() {
        assert_eq!(
            parse_selector("0xa9059cbb000000000000000000000000"),
            Some([0xa9, 0x05, 0x9c, 0xbb])
        );
        assert_eq!(parse_selector("0x"), None);
        assert_eq!(parse_selector("0xab"), None);
    }

    #[test]
    fn formats_nested_calls_with_gas_and_selector() {
        let mut names = SelectorMap::new();
        names.insert([0xde, 0xad, 0xc0, 0xde], "doSomething()".to_string());

        let payload = serde_json::json!({
            "type": "CALL",
            "from": "0xdeadbeef",
            "to": "0xfeedface",
            "value": "0x0",
            "gas": "0x5208",
            "gasUsed": "0x1388",
            "input": "0xdeadc0de",
            "output": "0xbeef",
            "calls": [
                {
                    "type": "STATICCALL",
                    "from": "0xdeadbeef",
                    "to": "0xabcdef",
                    "value": "0x1",
                    "gasUsed": "0x64",
                    "input": "0x10",
                    "output": "0x20",
                    "calls": []
                }
            ]
        });
        let lines = collect_call_lines(&payload, 0, 2, &names);
        assert_eq!(lines[0], "CALL 0xdeadbeef -> 0xfeedface  doSomething()  value: 0  gas: 5000");
        assert_eq!(lines[1], "  input: 0xdeadc0de (4 bytes)");
        assert_eq!(lines[2], "  output: 0xbeef (2 bytes)");
        // Child has short input (< 4 bytes), no selector
        assert!(lines[3].contains("STATICCALL"));
        assert!(lines[3].contains("gas: 100"));
    }

    #[test]
    fn formats_reverted_call() {
        let names = SelectorMap::new();
        // Error(string) revert with "insufficient balance"
        let revert_data = format!(
            "0x08c379a0{}",
            hex::encode(
                alloy::dyn_abi::DynSolReturns::new(vec![alloy::dyn_abi::DynSolType::String])
                    .abi_encode_output(&[alloy::dyn_abi::DynSolValue::String(
                        "insufficient balance".to_string()
                    )])
                    .unwrap()
            )
        );
        let payload = serde_json::json!({
            "type": "CALL",
            "from": "0xaaa",
            "to": "0xbbb",
            "value": "0x0",
            "gasUsed": "0x100",
            "input": "0x",
            "output": revert_data,
            "error": "execution reverted"
        });
        let lines = collect_call_lines(&payload, 0, 2, &names);
        assert!(lines.iter().any(|l| l.contains("revert: insufficient balance")));
    }

    #[test]
    fn format_hex_u64_works() {
        assert_eq!(format_hex_u64("0x1388"), "5000");
        assert_eq!(format_hex_u64("0x0"), "0");
        assert_eq!(format_hex_u64("0x"), "0");
    }

    #[test]
    fn collect_selectors_deduplicates() {
        let trace = serde_json::json!({
            "to": "0xAAA",
            "input": "0xa9059cbb00000000",
            "calls": [
                {"to": "0xAAA", "input": "0xa9059cbb00000000", "calls": []},
                {"to": "0xBBB", "input": "0x095ea7b300000000", "calls": []}
            ]
        });
        let mut map = HashMap::new();
        collect_selectors(&trace, &mut map);
        assert_eq!(map.get("0xaaa").unwrap().len(), 1);
        assert_eq!(map.get("0xbbb").unwrap().len(), 1);
    }
}
