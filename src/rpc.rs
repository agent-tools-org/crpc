// RPC client wrapper around alloy Provider
// Thin layer providing typed methods for common RPC calls

#![allow(dead_code)]

use alloy::dyn_abi::{DynSolValue, FunctionExt, JsonAbiExt};
use alloy::eips::{BlockId, BlockNumberOrTag};
use alloy::json_abi::Function;
use alloy::primitives::{Address, B256, Bytes, U256};
use alloy::providers::{Provider, ProviderBuilder, RootProvider};
use alloy::rpc::types::transaction::{TransactionInput, TransactionRequest};
use alloy::rpc::types::{Block, Filter, Log, Transaction, TransactionReceipt};
use eyre::{Report, Result, eyre};
use reqwest::Url;
use std::fmt;
use std::future::Future;

const MULTICALL3_SIGNATURE: &str = "aggregate3((address,bool,bytes)[])(tuple(bool,bytes)[])";

fn multicall3_address() -> Address {
    Address::parse_checksummed("0xcA11bde05977b3631167028862bE2a173976CA11", None)
        .expect("Multicall3 address is valid")
}

fn build_provider(rpc_url: &str) -> Result<RootProvider> {
    let url = Url::parse(rpc_url)?;
    Ok(ProviderBuilder::new()
        .disable_recommended_fillers()
        .connect_http(url))
}

#[derive(Debug)]
pub struct RevertError {
    pub data: Bytes,
}

impl fmt::Display for RevertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "eth_call reverted with {} bytes of data",
            self.data.len()
        )
    }
}

impl std::error::Error for RevertError {}

/// Create an alloy HTTP provider for the given RPC URL
pub fn make_provider(rpc_url: &str) -> Result<RootProvider> {
    build_provider(rpc_url)
}

/// Execute eth_call and return raw response bytes
pub async fn eth_call(
    rpc_url: &str,
    to: Address,
    calldata: Bytes,
    block: Option<u64>,
    from: Option<Address>,
) -> Result<Bytes> {
    let provider = make_provider(rpc_url)?;
    let mut tx = TransactionRequest::default()
        .to(to)
        .input(TransactionInput::new(calldata));
    if let Some(sender) = from {
        tx = tx.from(sender);
    }
    let block_id = block.map(BlockId::number).unwrap_or_else(BlockId::latest);
    match provider.call(tx).block(block_id).await {
        Ok(response) => Ok(response),
        Err(err) => {
            if let Some(payload) = err.as_error_resp() {
                if let Some(revert_data) = payload.as_revert_data() {
                    return Err(Report::new(RevertError { data: revert_data }));
                }
            }
            Err(err.into())
        }
    }
}

/// Execute eth_call with automatic fallback across multiple RPC URLs
pub async fn eth_call_with_fallback(
    rpc_urls: &[String],
    to: Address,
    calldata: Bytes,
    block: Option<u64>,
    from: Option<Address>,
) -> Result<Bytes> {
    with_fallback(rpc_urls, move |rpc_url| {
        let calldata = calldata.clone();
        async move { eth_call(&rpc_url, to, calldata, block, from).await }
    })
    .await
}

/// Read a storage slot
pub async fn get_storage_at(
    rpc_url: &str,
    address: Address,
    slot: U256,
    block: Option<u64>,
) -> Result<B256> {
    let provider = make_provider(rpc_url)?;
    let block_id = block.map(BlockId::number).unwrap_or_else(BlockId::latest);
    let value = provider
        .get_storage_at(address, slot)
        .block_id(block_id)
        .await?;
    Ok(value.into())
}

/// Read a storage slot with automatic fallback across multiple RPC URLs
pub async fn get_storage_at_with_fallback(
    rpc_urls: &[String],
    address: Address,
    slot: U256,
    block: Option<u64>,
) -> Result<B256> {
    with_fallback(rpc_urls, move |rpc_url| {
        let slot = slot.clone();
        async move { get_storage_at(&rpc_url, address, slot, block).await }
    })
    .await
}

/// Get a block by number (or latest when `None`)
pub async fn get_block(rpc_url: &str, number: Option<u64>) -> Result<Option<Block>> {
    let provider = make_provider(rpc_url)?;
    let tag = number.map(BlockNumberOrTag::from).unwrap_or_default();
    Ok(provider.get_block_by_number(tag).await?)
}

/// Get block data with fallback across multiple RPC URLs
pub async fn get_block_with_fallback(
    rpc_urls: &[String],
    number: Option<u64>,
) -> Result<Option<Block>> {
    with_fallback(rpc_urls, move |rpc_url| async move {
        get_block(&rpc_url, number).await
    })
    .await
}

/// Get a transaction by hash
pub async fn get_transaction(rpc_url: &str, hash: B256) -> Result<Option<Transaction>> {
    let provider = make_provider(rpc_url)?;
    Ok(provider.get_transaction_by_hash(hash).await?)
}

/// Get transaction data with fallback across multiple RPC URLs
pub async fn get_transaction_with_fallback(
    rpc_urls: &[String],
    hash: B256,
) -> Result<Option<Transaction>> {
    with_fallback(rpc_urls, move |rpc_url| async move {
        get_transaction(&rpc_url, hash).await
    })
    .await
}

/// Get a receipt for a transaction
pub async fn get_receipt(rpc_url: &str, hash: B256) -> Result<Option<TransactionReceipt>> {
    let provider = make_provider(rpc_url)?;
    Ok(provider.get_transaction_receipt(hash).await?)
}

/// Call debug_traceTransaction with callTracer
pub async fn debug_trace_transaction(rpc_url: &str, hash: B256) -> Result<serde_json::Value> {
    let provider = make_provider(rpc_url)?;
    let result: serde_json::Value = provider
        .raw_request(
            "debug_traceTransaction".into(),
            (hash, serde_json::json!({"tracer": "callTracer"})),
        )
        .await?;
    Ok(result)
}

/// Call debug_traceTransaction with fallback across multiple RPC URLs
pub async fn debug_trace_transaction_with_fallback(
    rpc_urls: &[String],
    hash: B256,
) -> Result<serde_json::Value> {
    with_fallback(rpc_urls, move |rpc_url| async move {
        debug_trace_transaction(&rpc_url, hash).await
    })
    .await
}

/// Get transaction receipt with fallback across multiple RPC URLs
pub async fn get_receipt_with_fallback(
    rpc_urls: &[String],
    hash: B256,
) -> Result<Option<TransactionReceipt>> {
    with_fallback(rpc_urls, move |rpc_url| async move {
        get_receipt(&rpc_url, hash).await
    })
    .await
}

/// Get current gas price (wei)
pub async fn get_gas_price(rpc_url: &str) -> Result<u128> {
    let provider = make_provider(rpc_url)?;
    Ok(provider.get_gas_price().await?)
}

/// Get current gas price with fallback across multiple RPC URLs
pub async fn get_gas_price_with_fallback(rpc_urls: &[String]) -> Result<u128> {
    with_fallback(rpc_urls, move |rpc_url| async move {
        get_gas_price(&rpc_url).await
    })
    .await
}

/// Get max priority fee per gas (wei), returns None if unsupported
pub async fn get_max_priority_fee(rpc_url: &str) -> Result<Option<u128>> {
    let provider = make_provider(rpc_url)?;
    match provider.get_max_priority_fee_per_gas().await {
        Ok(fee) => Ok(Some(fee)),
        Err(_) => Ok(None),
    }
}

/// Get max priority fee with fallback across multiple RPC URLs
pub async fn get_max_priority_fee_with_fallback(rpc_urls: &[String]) -> Result<Option<u128>> {
    with_fallback(rpc_urls, move |rpc_url| async move {
        get_max_priority_fee(&rpc_url).await
    })
    .await
}

/// Get latest block number
pub async fn get_block_number(rpc_url: &str) -> Result<u64> {
    let provider = make_provider(rpc_url)?;
    Ok(provider.get_block_number().await?)
}

/// Get latest block number with fallback across multiple RPC URLs
pub async fn get_block_number_with_fallback(rpc_urls: &[String]) -> Result<u64> {
    with_fallback(rpc_urls, move |rpc_url| async move {
        get_block_number(&rpc_url).await
    })
    .await
}

/// Get contract bytecode at address
pub async fn get_code(rpc_url: &str, address: Address) -> Result<Bytes> {
    let provider = make_provider(rpc_url)?;
    Ok(provider.get_code_at(address).await?)
}

/// Get contract bytecode with fallback across multiple RPC URLs
pub async fn get_code_with_fallback(rpc_urls: &[String], address: Address) -> Result<Bytes> {
    with_fallback(rpc_urls, move |rpc_url| async move {
        get_code(&rpc_url, address).await
    })
    .await
}

/// Query event logs with filters
pub async fn get_logs(rpc_url: &str, filter: Filter) -> Result<Vec<Log>> {
    let provider = make_provider(rpc_url)?;
    Ok(provider.get_logs(&filter).await?)
}

/// Query event logs with fallback across multiple RPC URLs
pub async fn get_logs_with_fallback(rpc_urls: &[String], filter: Filter) -> Result<Vec<Log>> {
    with_fallback(rpc_urls, move |rpc_url| {
        let filter = filter.clone();
        async move { get_logs(&rpc_url, filter).await }
    })
    .await
}

/// Execute multiple calls through Multicall3 aggregate3
pub async fn eth_call_batch(
    rpc_url: &str,
    calls: Vec<(Address, Bytes)>,
    block: Option<u64>,
) -> Result<Vec<Bytes>> {
    let provider = make_provider(rpc_url)?;
    let aggregate = multicall3_function()?;
    let input = encode_multicall3(&aggregate, &calls)?;
    let tx = TransactionRequest::default()
        .to(multicall3_address())
        .input(TransactionInput::new(input));
    let block_id = block.map(BlockId::number).unwrap_or_else(BlockId::latest);
    let response = provider.call(tx).block(block_id).await?;
    decode_multicall3(&aggregate, response.as_ref())
}

/// Execute multiple calls through Multicall3 via fallback RPCs
pub async fn eth_call_batch_with_fallback(
    rpc_urls: &[String],
    calls: Vec<(Address, Bytes)>,
    block: Option<u64>,
) -> Result<Vec<Bytes>> {
    with_fallback(rpc_urls, move |rpc_url| {
        let calls = calls.clone();
        async move { eth_call_batch(&rpc_url, calls, block).await }
    })
    .await
}

fn multicall3_function() -> Result<Function> {
    Function::parse(MULTICALL3_SIGNATURE).map_err(Into::into)
}

fn encode_multicall3(function: &Function, calls: &[(Address, Bytes)]) -> Result<Bytes> {
    let tuples: Vec<DynSolValue> = calls
        .iter()
        .map(|(to, calldata)| {
            DynSolValue::Tuple(vec![
                DynSolValue::Address(*to),
                DynSolValue::Bool(true),
                DynSolValue::Bytes(calldata.as_ref().to_vec()),
            ])
        })
        .collect();

    let payload = DynSolValue::Array(tuples);
    let data = function.abi_encode_input(&[payload])?;
    Ok(Bytes::from(data))
}

fn decode_multicall3(function: &Function, response: &[u8]) -> Result<Vec<Bytes>> {
    let items = function.abi_decode_output(response)?;
    let array = items
        .get(0)
        .and_then(|value| match value {
            DynSolValue::Array(items) => Some(items),
            _ => None,
        })
        .ok_or_else(|| eyre!("invalid multicall3 return structure"))?;

    let mut outs = Vec::with_capacity(array.len());
    for (idx, entry) in array.iter().enumerate() {
        let (success, data) = match entry {
            DynSolValue::Tuple(fields) if fields.len() == 2 => {
                let success = match &fields[0] {
                    DynSolValue::Bool(f) => *f,
                    other => {
                        return Err(eyre!("unexpected wire type for success flag: {other:?}"));
                    }
                };
                let data = match &fields[1] {
                    DynSolValue::Bytes(bytes) => bytes.clone(),
                    other => {
                        return Err(eyre!("unexpected wire type for return data: {other:?}"));
                    }
                };
                (success, data)
            }
            other => return Err(eyre!("unexpected multicall3 tuple: {other:?}")),
        };

        if !success {
            return Err(eyre!("multicall3 call {idx} failed"));
        }

        outs.push(Bytes::from(data));
    }

    Ok(outs)
}

pub async fn with_fallback<F, Fut, T>(rpc_urls: &[String], mut f: F) -> Result<T>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut failures: Vec<(String, Report)> = Vec::new();
    for rpc_url in rpc_urls.iter().cloned() {
        match f(rpc_url.clone()).await {
            Ok(value) => return Ok(value),
            Err(err) => {
                if err.downcast_ref::<RevertError>().is_some() {
                    return Err(err);
                }
                failures.push((rpc_url, err));
            }
        }
    }
    if failures.len() <= 1 {
        return Err(failures
            .pop()
            .map(|(_, err)| err)
            .unwrap_or_else(|| eyre!("no RPC URLs provided")));
    }
    let lines = failures
        .iter()
        .map(|(url, err)| format!("  {}: {err}", rpc_host(url)))
        .collect::<Vec<_>>()
        .join("\n");
    Err(eyre!("all {} RPCs failed:\n{lines}", failures.len()))
}

/// Host part of an RPC URL only: paths and query strings often carry API keys.
fn rpc_host(rpc_url: &str) -> String {
    Url::parse(rpc_url)
        .ok()
        .and_then(|url| url.host_str().map(String::from))
        .unwrap_or_else(|| "<invalid url>".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::Bytes;
    use eyre::Result as EyreResult;

    #[tokio::test]
    async fn with_fallback_reports_every_failure_without_url_paths() {
        let urls = vec![
            "https://a.example/v2/SECRETKEY".to_string(),
            "https://b.example/rpc".to_string(),
        ];
        let err = with_fallback(&urls, |url| async move {
            Err::<(), _>(eyre!("boom from {}", rpc_host(&url)))
        })
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("all 2 RPCs failed"));
        assert!(err.contains("a.example: boom from a.example"));
        assert!(err.contains("b.example: boom from b.example"));
        assert!(!err.contains("SECRETKEY"));
    }

    #[tokio::test]
    async fn with_fallback_single_url_keeps_original_error() {
        let urls = vec!["https://only.example".to_string()];
        let err = with_fallback(&urls, |_| async { Err::<(), _>(eyre!("original")) })
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "original");
    }

    #[test]
    fn make_provider_rejects_invalid_urls() {
        assert!(make_provider("not-a-url").is_err());
    }

    #[test]
    fn multicall3_encoding_roundtrip() -> EyreResult<()> {
        let aggregate = multicall3_function()?;
        let calls = vec![
            (
                Address::parse_checksummed("0x0000000000000000000000000000000000000001", None)?,
                Bytes::new(),
            ),
            (
                Address::parse_checksummed("0x0000000000000000000000000000000000000002", None)?,
                Bytes::from(vec![1, 2, 3]),
            ),
        ];
        let payload = encode_multicall3(&aggregate, &calls)?;
        assert_eq!(&payload.as_ref()[..4], aggregate.selector().as_slice());

        let results = vec![
            DynSolValue::Tuple(vec![
                DynSolValue::Bool(true),
                DynSolValue::Bytes(vec![0x01, 0x02]),
            ]),
            DynSolValue::Tuple(vec![
                DynSolValue::Bool(true),
                DynSolValue::Bytes(vec![0x03]),
            ]),
        ];
        let encoded = aggregate.abi_encode_output(&[DynSolValue::Array(results)])?;
        let decoded = decode_multicall3(&aggregate, &encoded)?;
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0], Bytes::from(vec![0x01, 0x02]));
        assert_eq!(decoded[1], Bytes::from(vec![0x03]));
        Ok(())
    }
}
