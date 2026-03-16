// rpc tx <chain> <hash> — transaction + receipt + decoded logs
use alloy_consensus::transaction::Transaction as ConsensusTransaction;
use eyre::{eyre, Result};

pub async fn run(
    chain: &str,
    hash: &str,
    rpc_override: Option<&str>,
    provider: Option<&str>,
    json: bool,
) -> Result<()> {
    let is_valid_hash = hash.starts_with("0x")
        && hash.len() == 66
        && hash.as_bytes()[2..]
            .iter()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F'));
    if !is_valid_hash {
        return Err(eyre!(
            "Error: invalid transaction hash: expected 0x + 64 hex chars, got \"{hash}\""
        ));
    }

    let config = crate::config::Config::load()?;
    let opts = crate::config::RpcOpts {
        rpc: rpc_override.map(String::from),
        provider: provider.map(String::from),
    };
    let rpc_urls = config.resolve_rpc_all(chain, &opts)?;
    let tx_hash = hash.parse::<alloy::primitives::B256>()?;
    let tx = crate::rpc::get_transaction_with_fallback(&rpc_urls, tx_hash).await?;
    let tx = match tx {
        Some(tx) => tx,
        None => {
            return Err(eyre!(
                "Error: transaction not found: {hash}\n\nPossible reasons:\n  - Transaction is pending (not yet mined)\n  - Hash is incorrect\n  - Transaction is on a different chain"
            ))
        }
    };
    let receipt = crate::rpc::get_receipt_with_fallback(&rpc_urls, tx_hash).await?;
    let receipt = match receipt {
        Some(receipt) => receipt,
        None => {
            println!("Transaction found but receipt not available (may be pending)");
            return Err(eyre!("receipt for {hash} not available yet"));
        }
    };
    let from = tx.inner.signer();
    let to = tx.inner.to();
    let gas_price = tx.inner.gas_price().unwrap_or(tx.inner.max_fee_per_gas());
    let from_bytes: &[u8] = from.as_ref();
    let from_repr = format!("0x{}", hex::encode(from_bytes));
    let to_repr = match to {
        Some(addr) => {
            let bytes: &[u8] = addr.as_ref();
            format!("0x{}", hex::encode(bytes))
        }
        None => "<contract creation>".to_string(),
    };
    let value_repr = tx.inner.value().to_string();
    let gas_price_repr = gas_price.to_string();
    let log_count = receipt.inner.logs().len();
    if json {
        println!(
            "{}",
            crate::format::format_tx_json(
                &from_repr,
                &to_repr,
                &value_repr,
                &gas_price_repr,
                receipt.status(),
                receipt.gas_used,
                log_count,
            )
        );
        return Ok(());
    }
    println!("From: {}", from_repr);
    println!("To: {}", to_repr);
    println!("Value: {}", value_repr);
    println!("Gas price: {gas_price_repr}");
    println!("Input length: {}", tx.inner.input().len());
    println!("Status: {}", receipt.status());
    println!("Gas used: {}", receipt.gas_used);
    println!("Log count: {}", log_count);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_invalid_hash_format() {
        let err = run("missing", "0x123", None, None, false).await.unwrap_err();
        assert_eq!(
            err.to_string(),
            "Error: invalid transaction hash: expected 0x + 64 hex chars, got \"0x123\""
        );
    }

    #[tokio::test]
    async fn rejects_unknown_chain() {
        let err = run(
            "missing",
            "0x0000000000000000000000000000000000000000000000000000000000000000",
            None,
            None,
            false,
        )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown chain missing"));
    }
}
