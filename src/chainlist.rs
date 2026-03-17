// Chainlist RPC catalog loader and query helpers.
// Exports ChainEntry/RpcEntry descriptors plus loader/search utilities for chainlist.org.
// Depends on serde, reqwest, eyre, and std fs/time/env helpers.

#![allow(dead_code)]

use eyre::{Context, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const CHAINLIST_URL: &str = "https://chainlist.org/rpcs.json";
const CACHE_DIR: &str = ".crpc";
const CACHE_FILE: &str = "chainlist.json";
const CACHE_TTL_SECS: u64 = 24 * 60 * 60;

/// Load chainlist data from cache only (no network). Returns error if no cache exists.
pub fn load_cached() -> Result<Vec<ChainEntry>> {
    let cache_path = cache_file_path()?;
    read_cached_entries(&cache_path)
}

/// Load chainlist data (from cache or network). Uses cached file if < 24h old.
pub async fn load() -> Result<Vec<ChainEntry>> {
    let cache_path = cache_file_path()?;
    let now = SystemTime::now();
    let mut stale_entries = None;
    if let Ok(meta) = fs::metadata(&cache_path) {
        if let Ok(entries) = read_cached_entries(&cache_path) {
            if is_fresh(&meta, now) {
                return Ok(entries);
            }
            stale_entries = Some(entries);
        }
    }
    match fetch_chainlist().await {
        Ok((entries, body)) => {
            if let Some(parent) = cache_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&cache_path, body);
            Ok(entries)
        }
        Err(err) => {
            if let Some(entries) = stale_entries {
                Ok(entries)
            } else {
                Err(err)
            }
        }
    }
}

/// Look up a chain by chain ID
pub fn lookup_by_id(entries: &[ChainEntry], chain_id: u64) -> Option<&ChainEntry> {
    entries.iter().find(|entry| entry.chain_id == chain_id)
}

/// Look up a chain by name or short name (case-insensitive)
pub fn lookup_by_name<'a>(entries: &'a [ChainEntry], name: &str) -> Option<&'a ChainEntry> {
    let normalized = name.trim();
    if normalized.is_empty() {
        return None;
    }
    entries.iter().find(|entry| {
        entry
            .name
            .eq_ignore_ascii_case(normalized)
            || entry.short_name.eq_ignore_ascii_case(normalized)
    })
}

/// Search chains by query (matches name, shortName, chain symbol, or chain ID)
pub fn search<'a>(entries: &'a [ChainEntry], query: &str) -> Vec<&'a ChainEntry> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let normalized = trimmed.to_ascii_lowercase();
    let parsed_id = trimmed.parse::<u64>().ok();
    entries
        .iter()
        .filter(|entry| {
            entry
                .name
                .to_ascii_lowercase()
                .contains(&normalized)
                || entry
                    .short_name
                    .to_ascii_lowercase()
                    .contains(&normalized)
                || entry.chain.to_ascii_lowercase().contains(&normalized)
                || parsed_id.map_or(false, |id| entry.chain_id == id)
        })
        .collect()
}

/// Get filtered RPC URLs for a chain entry (skip bad URLs, prefer no-tracking)
pub fn filtered_rpcs(entry: &ChainEntry) -> Vec<String> {
    let mut scored: Vec<(bool, String)> = entry
        .rpc
        .iter()
        .filter_map(|rpc| {
            let url = rpc.url.trim();
            let lowered = url.to_ascii_lowercase();
            if !(lowered.starts_with("http://") || lowered.starts_with("https://")) {
                return None;
            }
            if url.contains("${") {
                return None;
            }
            if lowered.contains("api_key") || lowered.contains("apikey") {
                return None;
            }
            let tracking_none = rpc
                .tracking
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case("none"))
                .unwrap_or(false);
            Some((tracking_none, url.to_string()))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, url)| url).collect()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainEntry {
    pub chain_id: u64,
    pub name: String,
    #[serde(default)]
    pub short_name: String,
    #[serde(default)]
    pub chain: String,
    #[serde(default)]
    pub rpc: Vec<RpcEntry>,
    #[serde(default)]
    pub native_currency: Option<NativeCurrency>,
    #[serde(default)]
    pub is_testnet: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcEntry {
    pub url: String,
    #[serde(default)]
    pub tracking: Option<String>,
    #[serde(default)]
    pub is_open_source: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeCurrency {
    pub name: String,
    pub symbol: String,
    pub decimals: u32,
}

fn cache_file_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME env missing for chainlist cache")?;
    Ok(PathBuf::from(home).join(CACHE_DIR).join(CACHE_FILE))
}

fn read_cached_entries(path: &Path) -> Result<Vec<ChainEntry>> {
    let data =
        fs::read(path).with_context(|| format!("reading cached chainlist at {}", path.display()))?;
    parse_chainlist(&data)
}

async fn fetch_chainlist() -> Result<(Vec<ChainEntry>, String)> {
    let response = reqwest::get(CHAINLIST_URL)
        .await
        .context("fetching chainlist rpc directory")?
        .error_for_status()
        .context("chainlist returned error status")?;
    let body = response
        .text()
        .await
        .context("reading chainlist response body")?;
    let entries = parse_chainlist(body.as_bytes())?;
    Ok((entries, body))
}

fn parse_chainlist(data: &[u8]) -> Result<Vec<ChainEntry>> {
    let entries: Vec<ChainEntry> =
        serde_json::from_slice(data).context("parsing chainlist JSON payload")?;
    Ok(entries
        .into_iter()
        .filter(|entry| !entry.is_testnet.unwrap_or(false))
        .collect())
}

fn is_fresh(metadata: &fs::Metadata, now: SystemTime) -> bool {
    metadata
        .modified()
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .map(|age| age.as_secs() < CACHE_TTL_SECS)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    include!("chainlist_tests.rs");
}
