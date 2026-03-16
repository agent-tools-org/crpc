// Offline ABI calldata formatter for encode subcommand

use crate::abi::encode_call;
use eyre::{eyre, Result};
use hex;

pub async fn run(sig: &str, args: &[String]) -> Result<()> {
    let calldata = encode_call(sig, args)?;
    let bytes = calldata.as_ref();
    if bytes.len() < 4 {
        return Err(eyre!("calldata shorter than selector"));
    }
    let selector = &bytes[..4];
    println!("Selector: 0x{}", hex::encode(selector));
    println!("Calldata: 0x{}", hex::encode(bytes));
    println!("Args:");
    for (idx, chunk) in bytes[4..].chunks(32).enumerate() {
        println!("  [{idx}] 0x{}", hex::encode(chunk));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::encode_call;

    #[tokio::test]
    async fn transfer_selector_matches_expected() {
        let args = vec![
            "0x0000000000000000000000000000000000001234".to_string(),
            "100".to_string(),
        ];
        let calldata = encode_call("transfer(address,uint256)", &args).unwrap();
        assert_eq!(calldata.as_ref().len(), 4 + 32 * 2);
        assert_eq!(hex::encode(&calldata.as_ref()[..4]), "a9059cbb");
    }

    #[tokio::test]
    async fn run_handles_empty_arg_list() {
        let args: Vec<String> = vec![];
        assert!(run("balanceOf(address)", &args).await.is_err());
    }
}
