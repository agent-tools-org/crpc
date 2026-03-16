// crpc logs <chain> <address> [--event <sig>] [--from <block>] [--to <block>] [--limit N]
// Query and decode event logs from a contract

use alloy::dyn_abi::{DynSolReturns, DynSolType};
use alloy::primitives::{Address, keccak256};
use alloy::rpc::types::{Filter, Log};
use eyre::{eyre, Result};
use hex::encode as hex_encode;
use serde_json::{json, Map as JsonMap, Value as JsonValue};

use crate::abi::{dyn_values_to_decoded, DecodedValue};
use crate::commands::block::parse_block_number;

pub async fn run(
    chain: &str,
    address: &str,
    event: Option<&str>,
    topic0: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
    blocks: Option<u64>,
    limit: usize,
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
    if limit == 0 {
        return Ok(());
    }
    let target_address = address
        .parse::<Address>()
        .map_err(|err| eyre!("invalid address: {err}"))?;
    let parsed_event = if let Some(signature) = event {
        Some(parse_event_signature(signature)?)
    } else {
        None
    };
    // Resolve block range: --blocks N takes priority over --from/--to
    let (from_block, to_block) = if let Some(n) = blocks {
        let latest = crate::rpc::get_block_number(&rpc_url).await?;
        (Some(latest.saturating_sub(n)), Some(latest))
    } else {
        (parse_block_number(from)?, parse_block_number(to)?)
    };
    let raw_topic0 = parse_topic0(topic0)?;
    let filter = build_filter(target_address, parsed_event.as_ref(), raw_topic0.as_ref(), from_block, to_block);
    let logs = crate::rpc::get_logs(&rpc_url, filter).await?;
    let logs = logs.into_iter().take(limit).collect::<Vec<_>>();
    if json {
        let entries = logs
            .iter()
            .map(|log| render_log_json(log, parsed_event.as_ref()))
            .collect::<Vec<_>>();
        println!("{}", JsonValue::Array(entries));
        return Ok(());
    }
    for log in logs {
        render_log_text(&log, parsed_event.as_ref());
    }
    Ok(())
}

fn parse_topic0(raw: Option<&str>) -> Result<Option<[u8; 32]>> {
    let Some(raw) = raw else { return Ok(None) };
    let hex_str = raw.strip_prefix("0x").unwrap_or(raw);
    if hex_str.len() != 64 {
        return Err(eyre!("topic0 must be 32 bytes (64 hex chars), got {}", hex_str.len()));
    }
    let bytes = hex::decode(hex_str).map_err(|e| eyre!("invalid topic0 hex: {e}"))?;
    let mut topic = [0u8; 32];
    topic.copy_from_slice(&bytes);
    Ok(Some(topic))
}

fn build_filter(
    address: Address,
    event: Option<&ParsedEvent>,
    raw_topic0: Option<&[u8; 32]>,
    from_block: Option<u64>,
    to_block: Option<u64>,
) -> Filter {
    let mut filter = Filter::new().address(address);
    if let Some(topic) = raw_topic0 {
        // Raw topic0 hash takes priority (for unknown/custom events)
        filter = filter.event_signature(*topic);
    } else if let Some(event) = event {
        let topic_hash = keccak256(event.signature.as_bytes());
        let mut topic = [0u8; 32];
        topic.copy_from_slice(topic_hash.as_ref());
        filter = filter.event_signature(topic);
    }
    if let Some(from) = from_block {
        filter = filter.from_block(from);
    }
    if let Some(to) = to_block {
        filter = filter.to_block(to);
    }
    filter
}

fn render_log_json(log: &Log, event: Option<&ParsedEvent>) -> JsonValue {
    let mut entry = JsonMap::new();
    entry.insert("block".to_string(), json!(log.block_number));
    entry.insert(
        "tx".to_string(),
        json!(log.transaction_hash.as_ref().map(|hash| hex_prefixed(hash.as_ref()))),
    );
    let topics = log
        .topics()
        .iter()
        .map(|topic| hex_prefixed(topic.as_ref()))
        .collect::<Vec<_>>();
    entry.insert("topics".to_string(), json!(topics));
    entry.insert("data".to_string(), json!(hex_prefixed(log.data().data.as_ref())));
    if let Some(event) = event {
        entry.insert("event".to_string(), json!(event.signature));
        let decoded_params = decode_event_params(log, event);
        let decoded = decoded_params
            .iter()
            .enumerate()
            .map(|(idx, param)| {
                json!({
                    "index": idx,
                    "type": param.ty,
                    "value": param.value,
                    "indexed": param.indexed,
                })
            })
            .collect::<Vec<_>>();
        entry.insert("decoded".to_string(), json!(decoded));
    }
    JsonValue::Object(entry)
}

fn render_log_text(log: &Log, event: Option<&ParsedEvent>) {
    let block_label = log
        .block_number
        .map(|n| n.to_string())
        .unwrap_or_else(|| "pending".to_string());
    let tx_label = log
        .transaction_hash
        .map(|hash| hex_prefixed(hash.as_ref()))
        .unwrap_or_else(|| "<unknown>".to_string());
    println!("Block {block_label} | Tx {tx_label}");
    if let Some(event) = event {
        println!("  {}", event.signature);
        let decoded_params = decode_event_params(log, event);
        for (idx, param) in decoded_params.iter().enumerate() {
            println!("  [{idx}] {}", param.value);
        }
    } else {
        let topics = log
            .topics()
            .iter()
            .map(|topic| hex_prefixed(topic.as_ref()))
            .collect::<Vec<_>>();
        let topic_line = if topics.is_empty() {
            "<none>".to_string()
        } else {
            topics.join(" ")
        };
        println!("  Topics: {topic_line}");
        println!("  Data: {}", hex_prefixed(log.data().data.as_ref()));
    }
}

fn decode_event_params(log: &Log, event: &ParsedEvent) -> Vec<DecodedParam> {
    let non_indexed_types = event
        .params
        .iter()
        .filter(|param| !param.indexed)
        .map(|param| param.ty.clone())
        .collect::<Vec<_>>();
    let decoded_data = decode_data_values(&non_indexed_types, log.data().data.as_ref()).unwrap_or_default();
    let mut non_indexed_iter = decoded_data.into_iter();
    let mut topic_cursor = 1;
    let mut result = Vec::with_capacity(event.params.len());
    for param in &event.params {
        if param.indexed {
            let value = if let Some(topic) = log.topics().get(topic_cursor) {
                match decode_topic_value(&param.ty, topic.as_ref()) {
                    Ok(value) => value,
                    Err(_) => hex_prefixed(topic.as_ref()),
                }
            } else {
                "<missing topic>".to_string()
            };
            topic_cursor += 1;
            result.push(DecodedParam {
                ty: param.type_name.clone(),
                value,
                indexed: true,
            });
        } else {
            let value = non_indexed_iter
                .next()
                .map(|decoded| decoded.value)
                .unwrap_or_else(|| "<missing data>".to_string());
            result.push(DecodedParam {
                ty: param.type_name.clone(),
                value,
                indexed: false,
            });
        }
    }
    result
}

fn decode_data_values(types: &[DynSolType], payload: &[u8]) -> Result<Vec<DecodedValue>> {
    if types.is_empty() {
        return Ok(vec![]);
    }
    let cloned_types = types.to_vec();
    let returns = DynSolReturns::new(cloned_types.clone());
    let values = returns.abi_decode_output(payload)?;
    Ok(dyn_values_to_decoded(cloned_types, values))
}

fn decode_topic_value(ty: &DynSolType, topic: &[u8]) -> Result<String> {
    let types = vec![ty.clone()];
    let returns = DynSolReturns::new(types.clone());
    let values = returns.abi_decode_output(topic)?;
    let decoded = dyn_values_to_decoded(types, values);
    decoded
        .into_iter()
        .next()
        .map(|value| value.value)
        .ok_or_else(|| eyre!("topic decode produced no values"))
}

fn hex_prefixed(bytes: &[u8]) -> String {
    format!("0x{}", hex_encode(bytes))
}

struct DecodedParam {
    ty: String,
    value: String,
    indexed: bool,
}

struct ParsedEvent {
    signature: String,
    params: Vec<EventParam>,
}

struct EventParam {
    ty: DynSolType,
    type_name: String,
    indexed: bool,
}

fn parse_event_signature(raw: &str) -> Result<ParsedEvent> {
    let trimmed = raw.trim();
    let start = trimmed
        .find('(')
        .ok_or_else(|| eyre!("event signature missing parameter list"))?;
    let end = find_matching_paren(trimmed, start)?;
    let name = trimmed[..start].trim();
    if name.is_empty() {
        return Err(eyre!("event signature missing name"));
    }
    let params_src = &trimmed[start + 1..end];
    let remainder = trimmed[end + 1..].trim();
    if !remainder.is_empty() {
        return Err(eyre!("unexpected characters after event signature"));
    }
    let params = if params_src.trim().is_empty() {
        Vec::new()
    } else {
        split_top_level(params_src, ',')
            .into_iter()
            .map(|segment| parse_event_param(segment))
            .collect::<Result<Vec<_>>>()?
    };
    let type_list = params
        .iter()
        .map(|param| param.type_name.clone())
        .collect::<Vec<_>>()
        .join(",");
    let signature = if type_list.is_empty() {
        format!("{}()", name)
    } else {
        format!("{}({})", name, type_list)
    };
    Ok(ParsedEvent {
        signature,
        params,
    })
}

fn parse_event_param(src: &str) -> Result<EventParam> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return Err(eyre!("empty event parameter"));
    }
    let tokens = split_whitespace_top_level(trimmed);
    let mut indexed = false;
    let mut type_token = None;
    for token in tokens {
        if token.eq_ignore_ascii_case("indexed") {
            indexed = true;
            continue;
        }
        if type_token.is_none() {
            type_token = Some(token);
        }
    }
    let type_str = type_token
        .ok_or_else(|| eyre!("event parameter missing type"))?
        .trim();
    if type_str.is_empty() {
        return Err(eyre!("event parameter missing type"));
    }
    let ty = DynSolType::parse(type_str)
        .map_err(|err| eyre!("failed to parse event parameter type {type_str}: {err}"))?;
    let type_name = ty.sol_type_name().into_owned();
    Ok(EventParam { ty, type_name, indexed })
}

fn split_top_level(src: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth: usize = 0;
    let mut start = 0;
    for (idx, ch) in src.char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            
            c if c == delimiter && depth == 0 => {
                parts.push(src[start..idx].trim());
                start = idx + c.len_utf8();
                continue;
            }
            _ => {}
        }
    }
    if start <= src.len() {
        parts.push(src[start..].trim());
    }
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

fn split_whitespace_top_level(src: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth: usize = 0;
    let mut start: Option<usize> = None;
    for (idx, ch) in src.char_indices() {
        if ch == '(' || ch == '[' {
            depth += 1;
        } else if ch == ')' || ch == ']' {
            depth = depth.saturating_sub(1);
        }
        if ch.is_whitespace() && depth == 0 {
            if let Some(begin) = start {
                let segment = src[begin..idx].trim();
                if !segment.is_empty() {
                    parts.push(segment);
                }
                start = None;
            }
            continue;
        }
        if start.is_none() {
            start = Some(idx);
        }
    }
    if let Some(begin) = start {
        let segment = src[begin..].trim();
        if !segment.is_empty() {
            parts.push(segment);
        }
    }
    parts
}

fn find_matching_paren(src: &str, start: usize) -> Result<usize> {
    let bytes = src.as_bytes();
    if start >= bytes.len() || bytes[start] != b'(' {
        return Err(eyre!("expected '(' at position {start}"));
    }
    let mut depth = 0;
    for (idx, &byte) in bytes.iter().enumerate().skip(start) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(idx);
                }
            }
            _ => {}
        }
    }
    Err(eyre!("unmatched '(' in signature"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex;

    #[test]
    fn computes_transfer_topic0() {
        let event = parse_event_signature("Transfer(address,address,uint256)").unwrap();
        assert_eq!(event.signature, "Transfer(address,address,uint256)");
        let topic = keccak256(event.signature.as_bytes());
        let encoded = hex::encode(topic.as_slice());
        assert_eq!(encoded, "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef");
    }
}
