// Chainlist module tests for RPC filtering and lookup helpers.
// Covers public helpers plus cache-loading behavior.
// Depends on eyre and std env/fs/time utilities.

use super::*;
use eyre::Result;
use std::{
    env,
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

fn make_entry(id: u64, name: &str, short_name: &str, chain: &str) -> ChainEntry {
    ChainEntry {
        chain_id: id,
        name: name.to_string(),
        short_name: short_name.to_string(),
        chain: chain.to_string(),
        rpc: Vec::new(),
        native_currency: None,
        is_testnet: None,
    }
}

fn rpc(url: &str, tracking: Option<&str>) -> RpcEntry {
    RpcEntry {
        url: url.to_string(),
        tracking: tracking.map(str::to_string),
        is_open_source: None,
    }
}

#[test]
fn filtered_rpcs_filters_bad_urls_and_prefers_tracking_none() {
    let entry = ChainEntry {
        chain_id: 1,
        name: "Filtered".to_string(),
        short_name: "filt".to_string(),
        chain: "FLT".to_string(),
        rpc: vec![
            rpc("https://should-stay.example", Some("None")),
            rpc("wss://socket.example", None),
            rpc("https://need-key.example/${API_KEY}", None),
            rpc("https://alts.example", Some("analytics")),
        ],
        native_currency: None,
        is_testnet: None,
    };
    let urls = filtered_rpcs(&entry);
    assert_eq!(
        urls,
        vec![
            "https://should-stay.example",
            "https://alts.example"
        ]
    );
}

#[test]
fn lookup_by_id_honors_chain_id() {
    let entries = vec![make_entry(1, "Alpha", "a", "ALPHA"), make_entry(2, "Bravo", "b", "BRVO")];
    assert_eq!(lookup_by_id(&entries, 2).unwrap().name, "Bravo");
}

#[test]
fn lookup_by_name_matches_short_name_case_insensitive() {
    let entries = vec![make_entry(1, "Alpha", "eth", "ETH"), make_entry(2, "Beta", "base", "BASE")];
    assert_eq!(lookup_by_name(&entries, "ETH").unwrap().chain, "ETH");
}

#[test]
fn search_matches_name_fragment_and_chain_id() {
    let entries = vec![
        make_entry(1, "Ethereum Mainnet", "eth", "ETH"),
        make_entry(2, "Optimism", "op", "OP"),
    ];
    let by_name = search(&entries, "main");
    assert_eq!(by_name.len(), 1);
    assert_eq!(by_name[0].chain_id, 1);
    let by_id = search(&entries, "2");
    assert_eq!(by_id.len(), 1);
    assert_eq!(by_id[0].short_name, "op");
}

#[test]
fn load_prefers_fresh_cache_over_network() -> Result<()> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_nanos();
    let home = std::env::temp_dir().join(format!("crpc-chainlist-{}", timestamp));
    let _guard = HomeGuard::set(&home);
    let cache_dir = home.join(CACHE_DIR);
    fs::create_dir_all(&cache_dir)?;
    let cache_file = cache_dir.join(CACHE_FILE);
    let raw = r#"[{"chainId":999,"name":"Temp Chain","shortName":"temp","chain":"TMP","rpc":[],"isTestnet":false}]"#;
    fs::write(&cache_file, raw)?;
    let entries = load_cached()?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].chain_id, 999);
    Ok(())
}

struct HomeGuard {
    prev: Option<String>,
}

impl HomeGuard {
    fn set(path: impl AsRef<Path>) -> Self {
        let prev = env::var("HOME").ok();
        unsafe { env::set_var("HOME", path.as_ref()) };
        HomeGuard { prev }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        if let Some(ref prev) = self.prev {
            unsafe { env::set_var("HOME", prev) };
        } else {
            unsafe { env::remove_var("HOME") };
        }
    }
}
