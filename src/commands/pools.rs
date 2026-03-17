// crpc pools <chain> <factory> [--event <sig>] [--from <block>] [--to <block>] [--limit N]
// Scan factory creation events and extract pool/token addresses

use alloy::dyn_abi::{DynSolReturns, DynSolType};
use alloy::primitives::{Address, B256, keccak256};
use alloy::rpc::types::Filter;
use eyre::{eyre, Result};
use serde_json::json;

use crate::commands::block::parse_block_number;

const DEFAULT_EVENT: &str = "PairCreated(address indexed token0, address indexed token1, address pair, uint256)";
const DEFAULT_BLOCKS: u64 = 10_000;
const PAGE_SIZE: u64 = 2_000;

pub async fn run(chain: &str, factory: &str, event: Option<&str>, from: Option<&str>, to: Option<&str>, limit: usize, json: bool, rpc: Option<&str>, provider: Option<&str>) -> Result<()> {
    if limit == 0 { return Ok(()); }
    let config = crate::config::Config::load()?;
    let rpc_url = config.resolve_rpc(chain, &crate::config::RpcOpts { rpc: rpc.map(String::from), provider: provider.map(String::from) })?;
    let factory = factory.parse::<Address>().map_err(|err| eyre!("invalid address: {err}"))?;
    let event = parse_event_signature(event.unwrap_or(DEFAULT_EVENT))?;
    let latest = crate::rpc::get_block_number(&rpc_url).await?;
    let to_block = parse_block_number(to)?.unwrap_or(latest);
    let from_block = parse_block_number(from)?.unwrap_or(to_block.saturating_sub(DEFAULT_BLOCKS));
    let mut pools = Vec::new();
    let mut start = from_block;
    while start <= to_block && pools.len() < limit {
        let end = start.saturating_add(PAGE_SIZE - 1).min(to_block);
        let filter = Filter::new().address(factory).event_signature(topic0(&event)).from_block(start).to_block(end);
        for log in crate::rpc::get_logs(&rpc_url, filter).await? {
            pools.push(extract_pool(log.topics(), log.data().data.as_ref(), log.block_number, &event)?);
            if pools.len() == limit { break; }
        }
        start = end.saturating_add(1);
    }
    if json {
        println!("{}", json!(pools.iter().map(|pool| json!({"pool": pool.pool.to_string(), "token0": pool.token0.to_string(), "token1": pool.token1.to_string(), "block": pool.block})).collect::<Vec<_>>()));
        return Ok(());
    }
    println!("{:<42} {:<42} {}", "Pool", "Token0", "Token1");
    for pool in pools {
        println!("{:<42} {:<42} {}", short(pool.pool), token_label(chain, pool.token0, &config), token_label(chain, pool.token1, &config));
    }
    Ok(())
}

fn extract_pool(topics: &[B256], data: &[u8], block: Option<u64>, event: &ParsedEvent) -> Result<PoolRow> {
    let indexed = topics.len().saturating_sub(1).min(event.params.len());
    let decoded = DynSolReturns::new(event.params[indexed..].to_vec()).abi_decode_output(data)?;
    let mut addresses = Vec::new();
    for (idx, ty) in event.params.iter().enumerate() {
        if !matches!(ty, DynSolType::Address) { continue; }
        let address = if idx < indexed {
            let values = DynSolReturns::new(vec![DynSolType::Address]).abi_decode_output(topics[idx + 1].as_ref())?;
            values[0].as_address()
        } else {
            decoded[idx - indexed].as_address()
        }
        .ok_or_else(|| eyre!("expected address"))?;
        addresses.push(address);
    }
    if addresses.len() < 3 { return Err(eyre!("event must expose token0, token1, and pool/pair addresses")); }
    Ok(PoolRow { pool: *addresses.last().unwrap(), token0: addresses[0], token1: addresses[1], block })
}

fn token_label(chain: &str, token: Address, config: &crate::config::Config) -> String {
    match crate::tokens::lookup_symbol(chain, token, Some(&config.tokens)) {
        Some(symbol) => format!("{} ({symbol})", short(token)),
        None => short(token),
    }
}

fn short(address: Address) -> String {
    let full = address.to_string();
    format!("{}...{}", &full[..8], &full[38..])
}

fn topic0(event: &ParsedEvent) -> B256 {
    keccak256(event.signature.as_bytes())
}

struct ParsedEvent {
    signature: String,
    params: Vec<DynSolType>,
}

struct PoolRow {
    pool: Address,
    token0: Address,
    token1: Address,
    block: Option<u64>,
}

fn parse_event_signature(raw: &str) -> Result<ParsedEvent> {
    let trimmed = raw.trim();
    let start = trimmed.find('(').ok_or_else(|| eyre!("event signature missing parameter list"))?;
    let end = trimmed.rfind(')').ok_or_else(|| eyre!("event signature missing closing ')'"))?;
    let name = trimmed[..start].trim();
    let params = trimmed[start + 1..end].split(',').filter(|part| !part.trim().is_empty()).map(parse_param_type).collect::<Result<Vec<_>>>()?;
    let canonical = params.iter().map(|ty| ty.sol_type_name().into_owned()).collect::<Vec<_>>().join(",");
    Ok(ParsedEvent { signature: format!("{name}({canonical})"), params })
}

fn parse_param_type(raw: &str) -> Result<DynSolType> {
    let ty = raw.split_whitespace().find(|part| *part != "indexed").ok_or_else(|| eyre!("event parameter missing type"))?;
    let mut types = crate::abi::parse_type_list(&format!("({ty})"))?;
    types.pop().ok_or_else(|| eyre!("event parameter missing type"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_event_signature() {
        let event = parse_event_signature("PoolCreated(address token0, address token1, uint24 fee, int24 tickSpacing, address pool)").unwrap();
        assert_eq!(event.signature, "PoolCreated(address,address,uint24,int24,address)");
    }

    #[test]
    fn parse_event_signature_handles_edge_cases() {
        let no_params = parse_event_signature("Paused()").unwrap();
        assert_eq!(no_params.signature, "Paused()");
        assert!(no_params.params.is_empty());

        let single_param = parse_event_signature("OwnerChanged(address owner)").unwrap();
        assert_eq!(single_param.signature, "OwnerChanged(address)");
        assert_eq!(single_param.params.len(), 1);

        let indexed = parse_event_signature(
            "PairCreated(address indexed token0, address indexed token1, address pair)",
        )
        .unwrap();
        assert_eq!(indexed.signature, "PairCreated(address,address,address)");
        assert_eq!(indexed.params.len(), 3);
    }

    #[test]
    fn extracts_pair_created_addresses() {
        let token0: Address = "0x0000000000000000000000000000000000000001".parse().unwrap();
        let token1: Address = "0x0000000000000000000000000000000000000002".parse().unwrap();
        let pair: Address = "0x0000000000000000000000000000000000000003".parse().unwrap();
        let event = parse_event_signature(DEFAULT_EVENT).unwrap();
        let topics = vec![topic0(&event), topic(token0), topic(token1)];
        let mut data = vec![0u8; 64];
        data[12..32].copy_from_slice(pair.as_slice());
        data[63] = 1;
        let row = extract_pool(&topics, &data, Some(42), &event).unwrap();
        assert_eq!(row.pool, pair);
        assert_eq!(row.token0, token0);
        assert_eq!(row.token1, token1);
        assert_eq!(row.block, Some(42));
    }

    #[test]
    fn extract_pool_supports_events_with_three_and_four_address_params() {
        let token0: Address = "0x0000000000000000000000000000000000000001".parse().unwrap();
        let token1: Address = "0x0000000000000000000000000000000000000002".parse().unwrap();
        let fee_recipient: Address = "0x0000000000000000000000000000000000000003".parse().unwrap();
        let pool: Address = "0x0000000000000000000000000000000000000004".parse().unwrap();

        let event_three =
            parse_event_signature("PoolCreated(address indexed token0, address indexed token1, address pool)")
                .unwrap();
        let topics_three = vec![topic0(&event_three), topic(token0), topic(token1)];
        let data_three = encode_addresses(&[pool]);
        let row_three = extract_pool(&topics_three, &data_three, Some(7), &event_three).unwrap();
        assert_eq!(row_three.token0, token0);
        assert_eq!(row_three.token1, token1);
        assert_eq!(row_three.pool, pool);

        let event_four = parse_event_signature(
            "PoolCreated(address indexed token0, address indexed token1, address feeRecipient, address pool)",
        )
        .unwrap();
        let topics_four = vec![topic0(&event_four), topic(token0), topic(token1)];
        let data_four = encode_addresses(&[fee_recipient, pool]);
        let row_four = extract_pool(&topics_four, &data_four, Some(8), &event_four).unwrap();
        assert_eq!(row_four.token0, token0);
        assert_eq!(row_four.token1, token1);
        assert_eq!(row_four.pool, pool);
    }

    #[test]
    fn extract_pool_reports_malformed_event_data() {
        let event = parse_event_signature(DEFAULT_EVENT).unwrap();
        let topics = vec![
            topic0(&event),
            topic("0x0000000000000000000000000000000000000001".parse().unwrap()),
            topic("0x0000000000000000000000000000000000000002".parse().unwrap()),
        ];
        let err = match extract_pool(&topics, &[0u8; 16], None, &event) {
            Ok(_) => panic!("expected malformed event data to fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("buffer overrun"));
    }

    fn topic(address: Address) -> B256 {
        let mut word = [0u8; 32];
        word[12..32].copy_from_slice(address.as_slice());
        B256::from(word)
    }

    fn encode_addresses(addresses: &[Address]) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(addresses.len() * 32);
        for address in addresses {
            encoded.extend_from_slice(&[0u8; 12]);
            encoded.extend_from_slice(address.as_slice());
        }
        encoded
    }
}
