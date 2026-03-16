// File cache for immutable RPC responses.
// Exports get/put helpers plus cache-key builders.
// Dependencies: eyre, serde_json, std.

use eyre::{eyre, Result};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// Get a cached value. Returns None if not cached or on error.
pub fn get(method: &str, chain_id: u64, params: &str) -> Option<Value> {
    let key = cache_key(method, chain_id, params);
    let path = cache_path(&key).ok()?;
    if !path.exists() {
        return None;
    }
    let raw = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Store a value in cache.
pub fn put(method: &str, chain_id: u64, params: &str, value: &Value) -> Result<()> {
    let key = cache_key(method, chain_id, params);
    let path = cache_path(&key)?;
    let body = serde_json::to_string(value)?;
    fs::write(path, body)?;
    Ok(())
}

fn cache_key(method: &str, chain_id: u64, params: &str) -> String {
    format!("{}:{}:{}", method, chain_id, params_hash(params))
}

fn cache_path(key: &str) -> Result<PathBuf> {
    let home = env::var("HOME").map_err(|_| eyre!("HOME environment variable is not set"))?;
    let cache_dir = PathBuf::from(home).join(".crpc").join("cache");
    fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir.join(format!("{key}.json")))
}

fn params_hash(params: &str) -> String {
    let mut hasher = DefaultHasher::new();
    params.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Returns true if the RPC method produces immutable data suitable for permanent caching.
pub fn is_cacheable(method: &str, block_param: Option<&str>) -> bool {
    let has_numeric_block = block_param.filter(|value| *value != "latest").is_some();
    match method {
        "block" => has_numeric_block,
        "tx" | "receipt" | "trace" => true,
        "logs" => has_numeric_block,
        "call" | "storage" => has_numeric_block,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn params_hash_is_consistent() {
        let first = params_hash("same");
        let second = params_hash("same");
        assert_eq!(first, second);
    }

    #[test]
    fn params_hash_differs_for_different_input() {
        assert_ne!(params_hash("foo"), params_hash("bar"));
    }

    #[test]
    fn cache_key_includes_parts() {
        let key = cache_key("block", 1337, "payload");
        let expected = format!("block:1337:{}", params_hash("payload"));
        assert_eq!(key, expected);
    }

    #[test]
    fn is_cacheable_behavior() {
        assert!(is_cacheable("tx", None));
        assert!(is_cacheable("receipt", None));
        assert!(is_cacheable("trace", None));
        assert!(is_cacheable("block", Some("123")));
        assert!(!is_cacheable("block", Some("latest")));
        assert!(is_cacheable("logs", Some("100")));
        assert!(!is_cacheable("logs", None));
        assert!(is_cacheable("call", Some("200")));
        assert!(!is_cacheable("call", Some("latest")));
        assert!(is_cacheable("storage", Some("300")));
        assert!(!is_cacheable("storage", None));
        assert!(!is_cacheable("unknown", None));
    }

    #[test]
    fn get_returns_none_for_missing_key() {
        // Non-existent cache entry should return None
        assert!(get("nonexistent", 0, "nope").is_none());
    }
}
