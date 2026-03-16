// Offline ABI decoding — decode calldata or raw ABI data
// With function sig: decodes calldata params. With type tuple: decodes raw data.

use crate::format::{format_raw_words, format_values, FormatMode};
use crate::abi::{decode_input, dyn_values_to_decoded, parse_type_list, DecodedValue};
use alloy::dyn_abi::DynSolReturns;
use eyre::{eyre, Result};
use hex;

pub async fn run(sig: &str, data: &str) -> Result<()> {
    let input = parse_hex_data(data)?;
    let (decoded, payload) = decode_bytes(sig, &input)?;
    println!("Decoded:\n{}", format_values(&decoded, FormatMode::Decode));
    println!("Raw words:\n{}", format_raw_words(payload));
    Ok(())
}

fn parse_hex_data(input: &str) -> Result<Vec<u8>> {
    let trimmed = input.trim();
    let payload = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    if payload.len() % 2 != 0 {
        return Err(eyre!("hex data must have an even number of characters"));
    }
    hex::decode(payload).map_err(|err| eyre!("invalid hex data: {err}"))
}

fn looks_like_function_sig(sig: &str) -> bool {
    let trimmed = sig.trim();
    if let Some(idx) = trimmed.find('(') {
        let name = trimmed[..idx].trim();
        !name.is_empty()
    } else {
        false
    }
}

fn decode_bytes<'a>(sig: &str, data: &'a [u8]) -> Result<(Vec<DecodedValue>, &'a [u8])> {
    if looks_like_function_sig(sig) {
        if data.len() < 4 {
            return Err(eyre!("data too short to contain selector"));
        }
        let payload = &data[4..];
        // Decode using parameter types (input data), not return types
        let decoded = decode_input(sig, payload)?;
        Ok((decoded, payload))
    } else {
        let types = parse_type_list(sig)?;
        let returns = DynSolReturns::new(types.clone());
        let values = returns.abi_decode_output(data)?;
        Ok((dyn_values_to_decoded(types, values), data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_uint256_decodes_to_value() {
        let raw = hex::decode("000000000000000000000000000000000000000000000000000000000000002a").unwrap();
        let (decoded, payload) = decode_bytes("(uint256)", &raw).unwrap();
        assert_eq!(payload, raw.as_slice());
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].ty, "uint256");
        assert_eq!(decoded[0].value, "42");
    }

    #[test]
    fn function_sig_decodes_params() {
        // swap(address,bool,int256,uint160) calldata
        let selector = hex::decode("04e45aaf").unwrap(); // arbitrary selector
        let addr = hex::decode("000000000000000000000000abcdefabcdefabcdefabcdefabcdefabcdefabcd").unwrap();
        let bool_true = hex::decode("0000000000000000000000000000000000000000000000000000000000000001").unwrap();
        let int_val = hex::decode("00000000000000000000000000000000000000000000000000000000000f4240").unwrap(); // 1000000
        let uint_val = hex::decode("0000000000000000000000000000000000000000000000000000000000000064").unwrap(); // 100
        let mut data = selector;
        data.extend_from_slice(&addr);
        data.extend_from_slice(&bool_true);
        data.extend_from_slice(&int_val);
        data.extend_from_slice(&uint_val);

        let (decoded, _) = decode_bytes("swap(address,bool,int256,uint160)", &data).unwrap();
        assert_eq!(decoded.len(), 4);
        assert_eq!(decoded[0].ty, "address");
        assert_eq!(decoded[1].ty, "bool");
        assert_eq!(decoded[1].value, "true");
        assert_eq!(decoded[2].ty, "int256");
        assert_eq!(decoded[2].value, "1000000");
        assert_eq!(decoded[3].ty, "uint160");
        assert_eq!(decoded[3].value, "100");
    }
}
