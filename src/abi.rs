// ABI encoding/decoding from function signature strings
// Exports encode_call/decode_response plus DecodedValue
// Depends on alloy dyn-abi helpers and eyre for errors

use alloy::dyn_abi::{DynSolCall, DynSolReturns, DynSolType, DynSolValue};
use alloy::primitives::{Bytes, keccak256, Selector};
use eyre::{eyre, Result};
use hex;

/// Encode a function call from its signature and string arguments.
/// e.g. encode_call("getTick(int32)", &["-30"]) -> calldata bytes
pub fn encode_call(sig: &str, args: &[String]) -> Result<Bytes> {
    let SignatureParts { name, params, .. } = parse_signature(sig)?;
    let param_types = parse_types(&params)?;
    if param_types.len() != args.len() {
        return Err(eyre!("expected {} args, got {}", param_types.len(), args.len()));
    }
    let values = coerce_args(&param_types, args)?;
    let selector = build_selector(&name, &param_types);
    let call = DynSolCall::new(selector, param_types, Some(name.clone()), DynSolReturns::new(vec![]));
    Ok(Bytes::from(call.abi_encode_input(&values)?))
}

/// Decode calldata payload using the function's parameter types.
/// e.g. for sig "swap(address,bool,int256,uint160)", decode the 4 input params.
pub fn decode_input(sig: &str, data: &[u8]) -> Result<Vec<DecodedValue>> {
    let SignatureParts { params, .. } = parse_signature(sig)?;
    let param_types = parse_types(&params)?;
    if param_types.is_empty() {
        return Ok(vec![]);
    }
    let returns = DynSolReturns::new(param_types.clone());
    let values = returns.abi_decode_output(data)?;
    Ok(dyn_values_to_decoded(param_types, values))
}

/// Decode raw response bytes according to a function signature's return types.
/// e.g. for sig "getTick(int32)(int24)", decode the return value
pub fn decode_response(sig: &str, data: &[u8]) -> Result<Vec<DecodedValue>> {
    let SignatureParts { returns, .. } = parse_signature(sig)?;
    if let Some(return_names) = returns {
        let return_types = parse_types(&return_names)?;
        let returns = DynSolReturns::new(return_types.clone());
        let values = returns.abi_decode_output(data)?;
        Ok(dyn_values_to_decoded(return_types, values))
    } else {
        decode_raw_words(data)
    }
}

pub fn extract_selectors_from_bytecode(bytecode: &[u8]) -> Vec<[u8; 4]> {
    let mut selectors = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut i = 0;
    while i < bytecode.len() {
        if bytecode[i] == 0x63 && i + 4 < bytecode.len() {
            let mut selector = [0u8; 4];
            selector.copy_from_slice(&bytecode[i + 1..i + 5]);
            if seen.insert(selector) {
                selectors.push(selector);
            }
            i += 5;
        } else {
            i += 1;
        }
    }
    selectors
}

/// A decoded ABI value with type information
#[derive(Debug, Clone)]
pub struct DecodedValue {
    pub ty: String,
    pub value: String,
    pub raw: Vec<u8>,
}

struct SignatureParts {
    name: String,
    params: Vec<String>,
    returns: Option<Vec<String>>,
}

fn parse_signature(sig: &str) -> Result<SignatureParts> {
    let sig = sig.trim();
    let args_start = sig.find('(').ok_or_else(|| eyre!("missing argument list"))?;
    let args_end = find_matching_paren(sig, args_start)?;
    let name = sig[..args_start].trim();
    if name.is_empty() {
        return Err(eyre!("missing function name"));
    }
    let params = split_type_list(&sig[args_start + 1..args_end])?;
    let mut idx = args_end + 1;
    while idx < sig.len() && sig.as_bytes()[idx].is_ascii_whitespace() {
        idx += 1;
    }
    let returns = if idx < sig.len() && sig.as_bytes()[idx] == b'(' {
        let ret_end = find_matching_paren(sig, idx)?;
        let ret_list = split_type_list(&sig[idx + 1..ret_end])?;
        idx = ret_end + 1;
        Some(ret_list)
    } else {
        None
    };
    while idx < sig.len() {
        if !sig.as_bytes()[idx].is_ascii_whitespace() {
            return Err(eyre!("unexpected characters after signature"));
        }
        idx += 1;
    }
    Ok(SignatureParts { name: name.to_string(), params, returns })
}

fn find_matching_paren(data: &str, start: usize) -> Result<usize> {
    let bytes = data.as_bytes();
    if start >= bytes.len() || bytes[start] != b'(' {
        return Err(eyre!("expected '(' at position {}", start));
    }
    let mut depth = 0;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            _ => {}
        }
    }
    Err(eyre!("unmatched '(' in signature"))
}

fn split_type_list(src: &str) -> Result<Vec<String>> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    let mut result = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (idx, ch) in trimmed.char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                if depth == 0 {
                    return Err(eyre!("unmatched closing delimiter"));
                }
                depth -= 1;
            }
            ',' if depth == 0 => {
                let piece = trimmed[start..idx].trim();
                if piece.is_empty() {
                    return Err(eyre!("empty type"));
                }
                result.push(piece.to_string());
                start = idx + 1;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(eyre!("unmatched '(' in type list"));
    }
    let tail = trimmed[start..].trim();
    if !tail.is_empty() {
        result.push(tail.to_string());
    }
    Ok(result)
}

fn parse_types(names: &[String]) -> Result<Vec<DynSolType>> {
    names
        .iter()
        .map(|name| DynSolType::parse(name).map_err(|err| eyre!("type parse failed: {err}")))
        .collect()
}

pub fn parse_type_list(sig: &str) -> Result<Vec<DynSolType>> {
    fn strip_enclosing_parens(src: &str) -> &str {
        let trimmed = src.trim();
        if let Some(stripped) = trimmed.strip_prefix('(').and_then(|v| v.strip_suffix(')')) {
            stripped
        } else {
            trimmed
        }
    }

    let inner = strip_enclosing_parens(sig);
    if inner.trim().is_empty() {
        return Ok(vec![]);
    }
    let type_names = split_type_list(inner)?;
    parse_types(&type_names)
}

fn coerce_args(types: &[DynSolType], args: &[String]) -> Result<Vec<DynSolValue>> {
    let mut values = Vec::with_capacity(args.len());
    for (ty, arg) in types.iter().zip(args.iter()) {
        values.push(ty.coerce_str(arg)?);
    }
    Ok(values)
}

fn canonical_signature(name: &str, types: &[DynSolType]) -> String {
    let params = types
        .iter()
        .map(|ty| ty.sol_type_name().into_owned())
        .collect::<Vec<_>>()
        .join(",");
    format!("{}({})", name, params)
}

fn build_selector(name: &str, types: &[DynSolType]) -> Selector {
    let signature = canonical_signature(name, types);
    let hash = keccak256(signature.as_bytes());
    let mut selector = [0u8; 4];
    let bytes: &[u8] = hash.as_ref();
    selector.copy_from_slice(&bytes[..4]);
    Selector::from(selector)
}

fn decode_raw_words(data: &[u8]) -> Result<Vec<DecodedValue>> {
    if data.is_empty() {
        return Ok(vec![]);
    }
    if data.len() % 32 != 0 {
        return Err(eyre!("response length must be multiple of 32 bytes"));
    }
    let chunks = data.chunks_exact(32);
    let mut result = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        let raw = chunk.to_vec();
        result.push(DecodedValue {
            ty: "bytes32".to_string(),
            value: hex_prefixed(&raw),
            raw,
        });
    }
    Ok(result)
}

fn format_dyn_value(value: &DynSolValue) -> String {
    match value {
        DynSolValue::Bool(b) => b.to_string(),
        DynSolValue::Int(i, _) => i.to_string(),
        DynSolValue::Uint(u, _) => u.to_string(),
        DynSolValue::FixedBytes(word, size) => {
            let bytes: &[u8] = word.as_ref();
            hex_prefixed(&bytes[..*size])
        }
        DynSolValue::Address(addr) => hex_prefixed(addr.as_ref()),
        DynSolValue::Function(func) => hex_prefixed(func.as_ref()),
        DynSolValue::Bytes(bytes) => hex_prefixed(bytes),
        DynSolValue::String(text) => text.clone(),
        DynSolValue::Array(values) => format_sequence(values, '[', ']'),
        DynSolValue::FixedArray(values) => format_sequence(values, '[', ']'),
        DynSolValue::Tuple(values) => format_sequence(values, '(', ')'),
    }
}

fn format_sequence(values: &[DynSolValue], open: char, close: char) -> String {
    let parts = values.iter().map(format_dyn_value).collect::<Vec<_>>().join(", ");
    format!("{open}{parts}{close}")
}

fn hex_prefixed(data: &[u8]) -> String {
    format!("0x{}", hex::encode(data))
}

pub(crate) fn dyn_values_to_decoded(
    types: Vec<DynSolType>,
    values: Vec<DynSolValue>,
) -> Vec<DecodedValue> {
    values
        .into_iter()
        .zip(types.into_iter())
        .map(|(value, ty)| DecodedValue {
            ty: ty.sol_type_name().into_owned(),
            value: format_dyn_value(&value),
            raw: value.abi_encode(),
        })
        .collect()
}

const ERROR_SELECTOR: [u8; 4] = [0x08, 0xc3, 0x79, 0xa0];
const PANIC_SELECTOR: [u8; 4] = [0x4e, 0x48, 0x7b, 0x71];

/// Decode raw revert data that came from eth_call/eth_sendTransaction errors.
pub fn decode_revert(data: &[u8]) -> String {
    if data.len() < 4 {
        return "reverted without reason".to_string();
    }

    let mut selector = [0u8; 4];
    selector.copy_from_slice(&data[..4]);
    let payload = &data[4..];

    match selector {
        ERROR_SELECTOR => decode_error_payload(payload)
            .unwrap_or_else(|| format_custom_error(&selector, payload)),
        PANIC_SELECTOR => decode_panic_payload(payload)
            .unwrap_or_else(|| format_custom_error(&selector, payload)),
        _ => format_custom_error(&selector, payload),
    }
}

fn decode_error_payload(payload: &[u8]) -> Option<String> {
    let returns = DynSolReturns::new(vec![DynSolType::String]);
    let values = returns.abi_decode_output(payload).ok()?;
    match values.into_iter().next()? {
        DynSolValue::String(message) => Some(message),
        _ => None,
    }
}

fn decode_panic_payload(payload: &[u8]) -> Option<String> {
    let returns = DynSolReturns::new(vec![DynSolType::Uint(256)]);
    let values = returns.abi_decode_output(payload).ok()?;
    let value = match values.into_iter().next()? {
        DynSolValue::Uint(code, _) => code,
        _ => return None,
    };
    let code = u64::try_from(&value).ok()?;
    Some(format!("panic: {} (0x{:x})", panic_description(code), code))
}

fn panic_description(code: u64) -> &'static str {
    match code {
        0x01 => "assert",
        0x11 => "overflow",
        0x12 => "div-by-zero",
        0x21 => "enum",
        0x22 => "storage",
        0x31 => "pop",
        0x32 => "index",
        0x41 => "memory",
        0x51 => "zero-fn",
        _ => "unknown panic",
    }
}

fn format_custom_error(selector: &[u8; 4], payload: &[u8]) -> String {
    format!(
        "custom error: 0x{} 0x{}",
        hex::encode(selector),
        hex::encode(payload)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::dyn_abi::{DynSolReturns, DynSolType, DynSolValue};
    use alloy::primitives::U256;
    use hex;

    #[test]
    fn encode_balance_of_address() {
        let args = vec!["0x000000000000000000000000000000000000abcd".to_string()];
        let calldata = encode_call("balanceOf(address)", &args).unwrap();
        let expected = hex::decode("70a08231000000000000000000000000000000000000000000000000000000000000abcd").unwrap();
        assert_eq!(calldata.as_ref(), expected.as_slice());
    }

    #[test]
    fn encode_get_tick_int32() {
        let args = vec!["-30".to_string()];
        let calldata = encode_call("getTick(int32)", &args).unwrap();
        assert_eq!(calldata.len(), 36);
        let payload = &calldata.as_ref()[4..];
        let expected = hex::decode("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe2").unwrap();
        assert_eq!(payload, expected.as_slice());
    }

    #[test]
    fn decode_uint256_response() {
        let raw = hex::decode("000000000000000000000000000000000000000000000000000000000000002a").unwrap();
        let decoded = decode_response("foo()(uint256)", &raw).unwrap();
        assert_eq!(decoded.len(), 1);
        let value = &decoded[0];
        assert_eq!(value.ty, "uint256");
        assert_eq!(value.value, "42");
        assert_eq!(value.raw, raw);
    }

    #[test]
    fn parse_signature_variants() {
        let parsed = parse_signature("foo(uint256)(bool,uint256)").unwrap();
        assert_eq!(parsed.name, "foo");
        assert_eq!(parsed.params, vec!["uint256".to_string()]);
        assert_eq!(parsed.returns.unwrap(), vec!["bool".to_string(), "uint256".to_string()]);
        let parsed = parse_signature("bar(address)").unwrap();
        assert_eq!(parsed.name, "bar");
        assert!(parsed.returns.is_none());
    }

    #[test]
    fn decode_revert_error_string() {
        let data = build_error_revert("insufficient balance");
        assert_eq!(decode_revert(&data), "insufficient balance");
    }

    #[test]
    fn decode_revert_panic_overflow() {
        let data = build_panic_revert(0x11);
        assert_eq!(decode_revert(&data), "panic: overflow (0x11)");
    }

    #[test]
    fn decode_revert_custom_error() {
        let mut data = vec![0xde, 0xad, 0xbe, 0xef];
        data.extend_from_slice(&[0x01, 0x02, 0x03]);
        assert_eq!(decode_revert(&data), "custom error: 0xdeadbeef 0x010203");
    }

    #[test]
    fn decode_revert_empty_payload() {
        assert_eq!(decode_revert(&[]), "reverted without reason");
    }

    #[test]
    fn extract_selectors_from_bytecode_deduplicates_push4_values() {
        let bytecode = vec![
            0x60, 0x00,
            0x63, 0xde, 0xad, 0xbe, 0xef,
            0x61, 0x12, 0x34,
            0x63, 0xde, 0xad, 0xbe, 0xef,
            0x63, 0xca, 0xfe, 0xba, 0xbe,
            0x63, 0xaa, 0xbb,
        ];
        assert_eq!(
            extract_selectors_from_bytecode(&bytecode),
            vec![[0xde, 0xad, 0xbe, 0xef], [0xca, 0xfe, 0xba, 0xbe]]
        );
    }

    fn build_error_revert(message: &str) -> Vec<u8> {
        let returns = DynSolReturns::new(vec![DynSolType::String]);
        let payload = returns
            .abi_encode_output(&[DynSolValue::String(message.to_string())])
            .unwrap();
        let mut data = hex::decode("08c379a0").unwrap();
        data.extend(payload);
        data
    }

    fn build_panic_revert(code: u64) -> Vec<u8> {
        let returns = DynSolReturns::new(vec![DynSolType::Uint(256)]);
        let payload = returns
            .abi_encode_output(&[DynSolValue::Uint(U256::from(code), 256)])
            .unwrap();
        let mut data = hex::decode("4e487b71").unwrap();
        data.extend(payload);
        data
    }
}
