// Output formatting — raw hex, decoded typed, human-readable
// Handles --raw, --decode (default), --human flags
use std::convert::TryInto;
use alloy::primitives::U256;
use crate::abi::DecodedValue;
use serde_json::json;

/// Format mode selected by CLI flags
#[derive(Debug, Clone, Copy)]
pub enum FormatMode {
    /// Raw hex output
    Raw,
    /// Decoded with type annotations and word indices
    Decode,
    /// Human-readable (e.g. token amounts with decimals)
    Human,
}

impl FormatMode {
    pub fn from_flags(raw: bool, human: bool) -> Self {
        if raw {
            FormatMode::Raw
        } else if human {
            FormatMode::Human
        } else {
            FormatMode::Decode
        }
    }
}

/// Format decoded values according to the selected mode
pub fn format_values(values: &[DecodedValue], mode: FormatMode) -> String {
    match mode {
        FormatMode::Raw => values
            .iter()
            .map(|decoded| format_raw_words(&decoded.raw))
            .collect::<Vec<_>>()
            .join("\n"),
        FormatMode::Decode => values
            .iter()
            .enumerate()
            .map(|(idx, decoded)| format!("[{idx}] {} = {}", decoded.ty, decoded.value))
            .collect::<Vec<_>>()
            .join("\n"),
        FormatMode::Human => values
            .iter()
            .map(|decoded| decoded.value.clone())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub fn format_json(values: &[DecodedValue]) -> String {
    let entries = values
        .iter()
        .map(|decoded| json!({ "type": decoded.ty, "value": decoded.value }))
        .collect::<Vec<_>>();
    json!({ "values": entries }).to_string()
}

/// Format a raw hex response (for --raw mode)
pub fn format_raw(data: &[u8]) -> String {
    format!("0x{}", hex::encode(data))
}

/// Format raw hex response showing 32-byte words with indices
pub fn format_raw_words(data: &[u8]) -> String {
    if data.is_empty() {
        return "0x (empty)".to_string();
    }
    let mut lines = vec![format!("0x{} ({} bytes)", hex::encode(data), data.len())];
    for (i, chunk) in data.chunks(32).enumerate() {
        lines.push(format!("  [{i:>2}] 0x{}", hex::encode(chunk)));
        if chunk.len() == 32 {
            let mut word = [0u8; 32];
            word.copy_from_slice(chunk);
            for note in annotate_word(&word) {
                lines.push(format!("    → {note}"));
            }
        }
    }
    lines.join("\n")
}

pub fn format_block_json(
    number: u64,
    hash: &[u8],
    timestamp: u64,
    gas_used: u64,
    gas_limit: u64,
    transactions: usize,
    base_fee: Option<String>,
) -> String {
    json!({
        "number": number,
        "hash": format!("0x{}", hex::encode(hash)),
        "timestamp": timestamp,
        "gas_used": gas_used,
        "gas_limit": gas_limit,
        "transactions": transactions,
        "base_fee": base_fee,
    })
    .to_string()
}

pub fn format_tx_json(
    from: &str,
    to: &str,
    value: &str,
    gas_price: &str,
    status: bool,
    gas_used: u64,
    log_count: usize,
) -> String {
    json!({
        "from": from,
        "to": to,
        "value": value,
        "gas_price": gas_price,
        "status": status,
        "gas_used": gas_used,
        "log_count": log_count,
    })
    .to_string()
}

pub fn format_diff(
    old: &[DecodedValue],
    new: &[DecodedValue],
    from_block: &str,
    to_block: &str,
) -> String {
    let mut lines = vec![format!("Diff block {from_block} → {to_block}:")];
    let max_len = old.len().max(new.len());
    for idx in 0..max_len {
        match (old.get(idx), new.get(idx)) {
            (Some(old_val), Some(new_val)) => {
                let ty = if old_val.ty == new_val.ty { &old_val.ty } else { &new_val.ty };
                if old_val.ty == new_val.ty && old_val.value == new_val.value {
                    lines.push(format!("[{idx}] {ty}: = {} (unchanged)", old_val.value));
                    continue;
                }
                lines.push(format!("[{idx}] {ty}:"));
                lines.push(format!("  - {}", old_val.value));
                lines.push(format!("  + {}", new_val.value));
                if let Some(delta) = compute_numeric_delta(&old_val.value, &new_val.value) {
                    lines.push(format!("  Δ {delta}"));
                }
            }
            (Some(old_val), None) => {
                lines.push(format!("[{idx}] {}: - {} (removed)", old_val.ty, old_val.value));
            }
            (None, Some(new_val)) => {
                lines.push(format!("[{idx}] {}: + {} (added)", new_val.ty, new_val.value));
            }
            (None, None) => unreachable!(),
        }
    }
    lines.join("\n")
}

fn compute_numeric_delta(old_str: &str, new_str: &str) -> Option<String> {
    if let (Ok(old), Ok(new)) = (old_str.parse::<i128>(), new_str.parse::<i128>()) {
        if let Some(delta) = new.checked_sub(old) {
            let formatted = if delta >= 0 { format!("+{}", delta) } else { delta.to_string() };
            return Some(formatted);
        }
    }
    if let (Ok(old), Ok(new)) = (old_str.parse::<u128>(), new_str.parse::<u128>()) {
        let formatted = if new >= old {
            format!("+{}", new - old)
        } else {
            format!("-{}", old - new)
        };
        return Some(formatted);
    }
    None
}

fn annotate_word(word: &[u8; 32]) -> Vec<String> {
    let mut annotations = Vec::new();
    let uint_value = U256::from_be_bytes(*word);
    if uint_value.is_zero() {
        annotations.push("bool: false".to_string());
    } else if uint_value == U256::from(1u64) {
        annotations.push("bool: true".to_string());
    }
    if word[..12].iter().all(|&b| b == 0) && word[12..].iter().any(|&b| b != 0) {
        annotations.push(format!("address: 0x{}", hex::encode(&word[12..])));
    }
    if word[..24].iter().all(|&b| b == 0) {
        let seconds = u64::from_be_bytes(word[24..32].try_into().unwrap());
        if (1_000_000_000..=2_000_000_000).contains(&seconds) {
            annotations.push(format!("timestamp: {}", format_timestamp(seconds)));
        }
    }
    if word[0] & 0x80 != 0 {
        let magnitude = (!uint_value).wrapping_add(U256::from(1u64));
        annotations.push(format!("int256: -{}", magnitude));
    }
    if is_sign_extended(word, 28) && word[28..32].iter().any(|&b| b != 0) {
        let int32 = i32::from_be_bytes(word[28..32].try_into().unwrap());
        annotations.push(format!("int32: {int32}"));
    }
    if is_sign_extended(word, 29) && word[29..32].iter().any(|&b| b != 0) {
        let raw: [u8; 3] = word[29..32].try_into().unwrap();
        let mut value = ((raw[0] as i32) << 16) | ((raw[1] as i32) << 8) | raw[2] as i32;
        if raw[0] & 0x80 != 0 {
            value -= 1 << 24;
        }
        annotations.push(format!("int24: {value}"));
    }
    let decimal = uint_value.to_string();
    if decimal.len() <= 70 { annotations.push(format!("uint256: {decimal}")); }
    annotations
}

fn is_sign_extended(word: &[u8; 32], prefix_len: usize) -> bool {
    let first = word[0];
    if first != 0 && first != 0xff {
        return false;
    }
    word[..prefix_len].iter().all(|&b| b == first)
}

fn format_timestamp(seconds: u64) -> String {
    const SECONDS_PER_DAY: u64 = 86_400;
    let days = (seconds / SECONDS_PER_DAY) as i64;
    let secs_of_day = seconds % SECONDS_PER_DAY;
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z / 146_097 } else { (z - 146_096) / 146_097 };
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    (year as i32, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    fn sample_value() -> DecodedValue {
        DecodedValue { ty: "uint256".to_string(), value: "42".to_string(), raw: vec![0u8; 32] }
    }
    fn assert_contains(hex_data: &str, expected: &[&str]) {
        let data = hex::decode(hex_data).unwrap();
        let formatted = format_raw_words(&data);
        for want in expected {
            assert!(formatted.contains(want), "{want} missing from {formatted}");
        }
    }
    fn decoded_value(ty: &str, value: &str) -> DecodedValue {
        DecodedValue { ty: ty.to_string(), value: value.to_string(), raw: Vec::new() }
    }
    #[test]
    fn decode_mode_lists_types() {
        let values = vec![sample_value()];
        let formatted = format_values(&values, FormatMode::Decode);
        assert!(formatted.contains("uint256"));
        assert!(formatted.contains("42"));
    }
    #[test]
    fn human_mode_prints_values_only() {
        let values = vec![sample_value()];
        assert_eq!(format_values(&values, FormatMode::Human), "42");
    }
    #[test]
    fn raw_words_chunks() {
        let data = vec![0x11u8; 64];
        let formatted = format_raw_words(&data);
        assert!(formatted.contains("0x1111"));
    }
    #[test]
    fn raw_words_annotations() {
        assert_contains("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe2", &["int256: -30", "int32: -30"]);
        assert_contains("0000000000000000000000004200000000000000000000000000000000000006", &["address: 0x4200000000000000000000000000000000000006"]);
        assert_contains("000000000000000000000000000000000000000000000000000000006787a1c3", &["timestamp: 2025-01-15 11:53:39 UTC", "uint256: 1736942019"]);
        assert_contains("0000000000000000000000000000000000000000000000000000000000000000", &["uint256: 0", "bool: false"]);
        assert_contains("0000000000000000000000000000000000000000000000000000000000000001", &["bool: true", "uint256: 1"]);
    }
    #[test]
    fn diff_identical_values_shows_unchanged() {
        let value = decoded_value("uint256", "42");
        let output = format_diff(&[value.clone()], &[value], "0xabc", "0xdef");
        assert!(output.contains("= 42 (unchanged)"));
    }
    #[test]
    fn diff_uint256_value_shows_delta() {
        let old = decoded_value("uint256", "10");
        let new = decoded_value("uint256", "25");
        let output = format_diff(&[old], &[new], "100", "101");
        assert!(output.contains("Δ +15"));
    }
    #[test]
    fn diff_address_value_does_not_show_delta() {
        let old = decoded_value("address", "0xaaa");
        let new = decoded_value("address", "0xbbb");
        let output = format_diff(&[old], &[new], "10", "11");
        assert!(output.contains("- 0xaaa"));
        assert!(output.contains("+ 0xbbb"));
        assert!(!output.contains("Δ "));
    }
    #[test]
    fn diff_mismatched_lengths_reports_added_and_removed() {
        let removed_output = format_diff(
            &[decoded_value("uint256", "1"), decoded_value("bool", "true")],
            &[decoded_value("uint256", "1")],
            "a",
            "b",
        );
        assert!(removed_output.contains("(removed)"));
        let added_output = format_diff(
            &[decoded_value("uint256", "1")],
            &[
                decoded_value("uint256", "1"),
                decoded_value("address", "0xabc"),
            ],
            "c",
            "d",
        );
        assert!(added_output.contains("(added)"));
    }

    #[test]
    fn format_json_outputs_values() {
        let values = vec![decoded_value("uint256", "10"), decoded_value("address", "0x00")];
        let parsed: Value = serde_json::from_str(&format_json(&values)).unwrap();
        assert_eq!(parsed["values"][0]["type"], "uint256");
        assert_eq!(parsed["values"][0]["value"], "10");
        assert_eq!(parsed["values"][1]["type"], "address");
    }

    #[test]
    fn format_block_json_includes_fields() {
        let hash = [0u8; 32];
        let parsed: Value = serde_json::from_str(&format_block_json(
            12,
            &hash,
            13,
            14,
            15,
            2,
            Some("99".to_string()),
        ))
        .unwrap();
        assert_eq!(parsed["number"], 12);
        assert_eq!(parsed["hash"], format!("0x{}", hex::encode(&hash)));
        assert_eq!(parsed["transactions"], 2);
        assert_eq!(parsed["base_fee"].as_str(), Some("99"));
    }

    #[test]
    fn format_tx_json_includes_fields() {
        let parsed: Value = serde_json::from_str(&format_tx_json("0x1", "0x2", "0", "3", true, 4, 5)).unwrap();
        assert_eq!(parsed["from"], "0x1");
        assert_eq!(parsed["to"], "0x2");
        assert_eq!(parsed["gas_price"], "3");
        assert_eq!(parsed["status"], true);
        assert_eq!(parsed["log_count"], 5);
    }
}
