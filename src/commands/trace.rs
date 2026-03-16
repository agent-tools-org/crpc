// Trace command for debug_traceTransaction
// Exports `run` to render callTracer responses as a call tree
// Depends on crate::config, crate::rpc, serde_json, and alloy primitives
use alloy::primitives::{B256, U256};
use eyre::{eyre, Result};
use serde_json::Value;

pub async fn run(
    chain: &str,
    hash: &str,
    depth: usize,
    rpc_override: Option<&str>,
    provider: Option<&str>,
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
    print_call(&trace, 0, depth);
    Ok(())
}
fn is_valid_tx_hash(hash: &str) -> bool {
    hash.starts_with("0x")
        && hash.len() == 66
        && hash.as_bytes()[2..]
            .iter()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F'))
}
fn print_call(call: &Value, indent: usize, max_depth: usize) {
    for line in collect_call_lines(call, indent, max_depth) {
        println!("{line}");
    }
}
fn collect_call_lines(call: &Value, indent: usize, max_depth: usize) -> Vec<String> {
    if indent > max_depth {
        return Vec::new();
    }
    let indent_str = "  ".repeat(indent);
    let call_type = call.get("type").and_then(|value| value.as_str()).unwrap_or("CALL");
    let from = call.get("from").and_then(|value| value.as_str()).unwrap_or("<unknown>");
    let to = call.get("to").and_then(|value| value.as_str()).unwrap_or("<unknown>");
    let value = call.get("value").and_then(|value| value.as_str()).unwrap_or("0x0");
    let mut lines = vec![format!(
        "{indent_str}{call_type} {from} -> {to}  value: {}",
        format_value_decimal(value)
    )];
    if let Some(input) = call.get("input").and_then(|value| value.as_str()) {
        if !input.is_empty() {
            lines.push(format!(
                "{indent_str}  input: {input} ({} bytes)",
                hex_bytes_len(input)
            ));
        }
    }
    if let Some(output) = call.get("output").and_then(|value| value.as_str()) {
        if !output.is_empty() {
            lines.push(format!(
                "{indent_str}  output: {output} ({} bytes)",
                hex_bytes_len(output)
            ));
        }
    }
    if indent >= max_depth {
        return lines;
    }
    if let Some(children) = call.get("calls").and_then(|value| value.as_array()) {
        for child in children {
            lines.extend(collect_call_lines(child, indent + 1, max_depth));
        }
    }
    lines
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
    fn formats_nested_calls() {
        let payload = serde_json::json!({
            "type": "CALL",
            "from": "0xdeadbeef",
            "to": "0xfeedface",
            "value": "0x0",
            "input": "0xdeadc0de",
            "output": "0xbeef",
            "calls": [
                {"type": "STATICCALL", "from": "0xdeadbeef", "to": "0xabcdef", "value": "0x1", "input": "0x10", "output": "0x20", "calls": []}
            ]
        });
        let lines = collect_call_lines(&payload, 0, 2);
        assert_eq!(
            lines,
            vec![
                "CALL 0xdeadbeef -> 0xfeedface  value: 0".to_string(),
                "  input: 0xdeadc0de (4 bytes)".to_string(),
                "  output: 0xbeef (2 bytes)".to_string(),
                "  STATICCALL 0xdeadbeef -> 0xabcdef  value: 1".to_string(),
                "    input: 0x10 (1 bytes)".to_string(),
                "    output: 0x20 (1 bytes)".to_string(),
            ]
        );
    }
}
