// Persistent config subcommands for ~/.crpc.toml.
// Exports run_set and run_get; depends on crate::config and basic_toml.

use crate::config::Keys;
use eyre::{bail, eyre, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct StoredConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    keys: Option<Keys>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    chains: HashMap<String, StoredChainConfig>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    tokens: HashMap<String, HashMap<String, String>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct StoredChainConfig {
    chain_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    priority: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    rpc: HashMap<String, String>,
}

pub fn run_set(key: &str, value: &str) -> Result<()> {
    let path = config_path()?;
    let mut config = load_config(&path)?;
    set_value(&mut config, key, value)?;
    save_config(&path, &config)
}

pub fn run_get(key: &str) -> Result<()> {
    let path = config_path()?;
    let config = load_config(&path)?;
    println!("{}", get_value(&config, key)?);
    Ok(())
}

fn config_path() -> Result<PathBuf> {
    crate::config::config_file_path().ok_or_else(|| eyre!("HOME is not set"))
}

fn load_config(path: &Path) -> Result<StoredConfig> {
    if !path.exists() {
        return Ok(StoredConfig::default());
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(StoredConfig::default());
    }
    Ok(basic_toml::from_str(&raw)?)
}

fn save_config(path: &Path, config: &StoredConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, basic_toml::to_string(config)?)?;
    Ok(())
}

fn set_value(config: &mut StoredConfig, key: &str, value: &str) -> Result<()> {
    match key {
        "etherscan_api_key" => {
            let keys = config.keys.get_or_insert_with(Keys::default);
            keys.etherscan = Some(value.to_string());
            Ok(())
        }
        "default_provider" => {
            config.default_provider = Some(value.to_string());
            Ok(())
        }
        _ => bail!("unsupported config key: {key}"),
    }
}

fn get_value(config: &StoredConfig, key: &str) -> Result<String> {
    match key {
        "etherscan_api_key" => config
            .keys
            .as_ref()
            .and_then(|keys| keys.etherscan.clone())
            .ok_or_else(|| eyre!("config key is not set: {key}")),
        "default_provider" => config
            .default_provider
            .clone()
            .ok_or_else(|| eyre!("config key is not set: {key}")),
        _ => bail!("unsupported config key: {key}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eyre::Result;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_config_path() -> Result<PathBuf> {
        let dir = std::env::temp_dir().join(format!(
            "crpc-config-cmd-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir_all(&dir)?;
        Ok(dir.join(".crpc.toml"))
    }

    #[test]
    fn set_value_rejects_unknown_keys() {
        let mut config = StoredConfig::default();
        let err = set_value(&mut config, "unknown_key", "value").unwrap_err();
        assert_eq!(err.to_string(), "unsupported config key: unknown_key");
    }

    #[test]
    fn empty_config_file_round_trips() -> Result<()> {
        let path = temp_config_path()?;
        fs::write(&path, "")?;

        let config = load_config(&path)?;
        assert!(config.default_provider.is_none());
        assert!(config.keys.is_none());
        assert!(config.chains.is_empty());
        assert!(config.tokens.is_empty());

        save_config(&path, &config)?;
        let reloaded = load_config(&path)?;
        assert!(reloaded.default_provider.is_none());
        assert!(reloaded.keys.is_none());
        assert!(reloaded.chains.is_empty());
        assert!(reloaded.tokens.is_empty());

        fs::remove_dir_all(path.parent().unwrap())?;
        Ok(())
    }

    #[test]
    fn multiple_set_operations_do_not_clobber_existing_values() -> Result<()> {
        let path = temp_config_path()?;
        let mut config = StoredConfig::default();
        config.chains.insert(
            "base".to_string(),
            StoredChainConfig {
                chain_id: 8453,
                priority: Some(vec!["base".to_string()]),
                rpc: HashMap::from([(
                    "base".to_string(),
                    "https://mainnet.base.org".to_string(),
                )]),
            },
        );
        set_value(&mut config, "default_provider", "alchemy")?;
        save_config(&path, &config)?;

        let mut updated = load_config(&path)?;
        set_value(&mut updated, "etherscan_api_key", "secret-key")?;
        save_config(&path, &updated)?;

        let reloaded = load_config(&path)?;
        assert_eq!(get_value(&reloaded, "default_provider")?, "alchemy");
        assert_eq!(get_value(&reloaded, "etherscan_api_key")?, "secret-key");
        assert_eq!(reloaded.chains["base"].chain_id, 8453);
        assert_eq!(
            reloaded.chains["base"].rpc.get("base").map(String::as_str),
            Some("https://mainnet.base.org")
        );

        fs::remove_dir_all(path.parent().unwrap())?;
        Ok(())
    }
}
