// rpc multi <chain> <file> — batch calls via Multicall3
use alloy::primitives::Address;
use eyre::Result;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct BatchEntry {
    target: String,
    sig: String,
    #[serde(default)]
    args: Vec<String>,
}

pub async fn run(
    chain: &str,
    file: &str,
    rpc_override: Option<&str>,
    provider: Option<&str>,
    json: bool,
) -> Result<()> {
    let config = crate::config::Config::load()?;
    let opts = crate::config::RpcOpts {
        rpc: rpc_override.map(String::from),
        provider: provider.map(String::from),
    };
    let rpc_urls = config.resolve_rpc_all(chain, &opts)?;
    let contents = std::fs::read_to_string(file)?;
    let entries: Vec<BatchEntry> = serde_json::from_str(&contents)?;
    let calls = entries
        .iter()
        .map(|entry| {
            let target = entry.target.parse::<Address>()?;
            let calldata = crate::abi::encode_call(&entry.sig, &entry.args)?;
            Ok((target, calldata))
        })
        .collect::<Result<Vec<_>>>()?;
    let responses = crate::rpc::eth_call_batch_with_fallback(&rpc_urls, calls, None).await?;
    if json {
        let mut array = Vec::with_capacity(entries.len());
        for (entry, response) in entries.iter().zip(responses.iter()) {
            let decoded = crate::abi::decode_response(&entry.sig, response)?;
            let values = decoded
                .iter()
                .map(|decoded| json!({ "type": decoded.ty, "value": decoded.value }))
                .collect::<Vec<_>>();
            array.push(json!({
                "target": entry.target,
                "sig": entry.sig,
                "values": values,
            }));
        }
        println!("{}", Value::Array(array));
        return Ok(());
    }
    for (entry, response) in entries.iter().zip(responses.iter()) {
        let decoded = crate::abi::decode_response(&entry.sig, response)?;
        println!("Call {} →", entry.sig);
        println!("{}", crate::format::format_values(&decoded, crate::format::FormatMode::Decode));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rejects_unknown_chain() {
        let tmp = std::env::temp_dir().join("crpc-batch-test.json");
        let path = tmp.to_string_lossy().to_string();
        let _ = std::fs::write(&tmp, "[]");
        assert!(run("missing", &path, None, None, false).await.is_err());
        let _ = std::fs::remove_file(&tmp);
    }
}
