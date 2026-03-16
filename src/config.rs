// Chain registry and config file loading
// Loads ~/.crpc.toml, resolves chain aliases to RPC URLs

use eyre::{ContextCompat, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;

/// Chain configuration with RPC endpoints
#[derive(Debug, Clone, Deserialize)]
pub struct ChainConfig {
    pub chain_id: u64,
    #[serde(default)]
    pub priority: Option<Vec<String>>,
    #[serde(default)]
    pub rpc: HashMap<String, String>,
}

/// Top-level config file structure (~/.crpc.toml)
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub default_provider: Option<String>,
    #[serde(default)]
    pub chains: HashMap<String, ChainConfig>,
    #[serde(default)]
    pub tokens: HashMap<String, HashMap<String, String>>,
}

/// RPC resolution overrides
#[derive(Debug, Clone, Default)]
pub struct RpcOpts {
    pub rpc: Option<String>,
    pub provider: Option<String>,
}

impl Config {
    /// Load config from ~/.crpc.toml, merging with built-in defaults
    pub fn load() -> Result<Self> {
        let mut config = Config {
            default_provider: None,
            chains: built_in_chains(),
            tokens: HashMap::new(),
        };
        if let Some(path) = config_file_path() {
            if path.exists() {
                let raw = fs::read_to_string(&path)?;
                let mut file_config: Config = toml::from_str(&raw)?;
                let file_default = file_config.default_provider.take();
                config.chains.extend(file_config.chains);
                config.tokens = file_config.tokens;
                if let Some(provider) = file_default {
                    config.default_provider = Some(provider);
                }
            }
        }
        Ok(config)
    }

    /// Resolve a chain alias (e.g. "base") to its first available RPC URL
    #[allow(dead_code)]
    pub fn resolve_rpc(&self, chain: &str, opts: &RpcOpts) -> Result<String> {
        self.resolve_rpc_all(chain, opts)?
            .into_iter()
            .next()
            .ok_or_else(|| eyre::eyre!("no RPC endpoints configured for {chain}"))
    }

    /// Return all available RPC URLs for a chain, in priority order.
    /// Skips providers with unresolvable env vars.
    pub fn resolve_rpc_all(&self, chain: &str, opts: &RpcOpts) -> Result<Vec<String>> {
        let key = match self.resolve_chain_key(chain) {
            Ok(k) => k,
            Err(_) => {
                // Chain not in config/builtins — try chainlist.org
                if opts.rpc.is_some() {
                    return Ok(vec![opts.rpc.clone().unwrap()]);
                }
                if let Some(rpcs) = Self::resolve_from_chainlist(chain) {
                    return Ok(rpcs);
                }
                return Err(eyre::eyre!("unknown chain {chain}"));
            }
        };
        if let Some(rpc) = opts.rpc.as_ref() {
            return Ok(vec![rpc.clone()]);
        }
        if let Some(url) = rpc_from_env(&key) {
            return Ok(vec![url]);
        }
        let chain_cfg = self
            .chains
            .get(&key)
            .with_context(|| eyre::eyre!("chain {chain} resolved to {key} but missing config"))?;
        if let Some(provider_name) = opts.provider.as_deref() {
            let url = chain_cfg.rpc.get(provider_name).with_context(|| {
                eyre::eyre!("provider {provider_name} not configured for {chain}")
            })?;
            return expand_env_vars(url).with_context(|| {
                eyre::eyre!("provider {provider_name} for {chain} requires unresolved environment variables")
            })
            .map(|expanded| vec![expanded]);
        }
        let mut urls = Vec::new();
        let mut seen = HashSet::new();
        if let Some(default_provider) = self.default_provider.as_deref() {
            if let Some(url) = chain_cfg.rpc.get(default_provider) {
                if let Some(expanded) = expand_env_vars(url) {
                    seen.insert(default_provider.to_string());
                    urls.push(expanded);
                }
            }
        }
        if let Some(priority) = &chain_cfg.priority {
            for provider_name in priority {
                if seen.contains(provider_name) {
                    continue;
                }
                if let Some(url) = chain_cfg.rpc.get(provider_name) {
                    if let Some(expanded) = expand_env_vars(url) {
                        seen.insert(provider_name.clone());
                        urls.push(expanded);
                    }
                }
            }
        } else {
            for (provider_name, url) in &chain_cfg.rpc {
                if seen.contains(provider_name) {
                    continue;
                }
                if let Some(expanded) = expand_env_vars(url) {
                    seen.insert(provider_name.clone());
                    urls.push(expanded);
                }
            }
        }
        if urls.is_empty() {
            Err(eyre::eyre!("no RPC endpoints configured for {chain}"))
        } else {
            Ok(urls)
        }
    }

    fn resolve_chain_key(&self, chain: &str) -> Result<String> {
        find_chain_key(chain, &self.chains).ok_or_else(|| eyre::eyre!("unknown chain {chain}"))
    }

    /// Try to resolve RPCs from cached chainlist data (no network)
    fn resolve_from_chainlist(chain: &str) -> Option<Vec<String>> {
        let entries = crate::chainlist::load_cached().ok()?;
        let entry = chain
            .parse::<u64>()
            .ok()
            .and_then(|id| crate::chainlist::lookup_by_id(&entries, id))
            .or_else(|| crate::chainlist::lookup_by_name(&entries, chain))
            .or_else(|| {
                // Fuzzy: pick first search result
                crate::chainlist::search(&entries, chain).into_iter().next()
            })?;
        let rpcs = crate::chainlist::filtered_rpcs(entry);
        if rpcs.is_empty() { None } else { Some(rpcs) }
    }
}

struct BuiltInChain {
    key: &'static str,
    aliases: &'static [&'static str],
    chain_id: u64,
    providers: &'static [(&'static str, &'static str)],
}

const BUILT_IN_CHAINS: &[BuiltInChain] = &[
    BuiltInChain {
        key: "eth",
        aliases: &["ethereum"],
        chain_id: 1,
        providers: &[
            ("llamarpc", "https://eth.llamarpc.com"),
            ("ankr", "https://rpc.ankr.com/eth"),
        ],
    },
    BuiltInChain {
        key: "base",
        aliases: &["base-mainnet"],
        chain_id: 8453,
        providers: &[
            ("base", "https://mainnet.base.org"),
            ("llamarpc", "https://base.llamarpc.com"),
        ],
    },
    BuiltInChain {
        key: "arb",
        aliases: &["arbitrum", "arb1"],
        chain_id: 42161,
        providers: &[
            ("arbitrum", "https://arb1.arbitrum.io/rpc"),
            ("llamarpc", "https://arbitrum.llamarpc.com"),
        ],
    },
    BuiltInChain {
        key: "bsc",
        aliases: &[],
        chain_id: 56,
        providers: &[("binance", "https://bsc-dataseed.binance.org")],
    },
    BuiltInChain {
        key: "polygon",
        aliases: &["matic"],
        chain_id: 137,
        providers: &[("polygon", "https://polygon-rpc.com")],
    },
    BuiltInChain {
        key: "op",
        aliases: &["optimism"],
        chain_id: 10,
        providers: &[("optimism", "https://mainnet.optimism.io")],
    },
    BuiltInChain {
        key: "avax",
        aliases: &["avalanche"],
        chain_id: 43114,
        providers: &[("avalanche", "https://api.avax.network/ext/bc/C/rpc")],
    },
    BuiltInChain {
        key: "linea",
        aliases: &[],
        chain_id: 59144,
        providers: &[("linea", "https://rpc.linea.build")],
    },
    BuiltInChain {
        key: "scroll",
        aliases: &[],
        chain_id: 534352,
        providers: &[("scroll", "https://rpc.scroll.io")],
    },
    BuiltInChain {
        key: "zksync",
        aliases: &["zksync-era"],
        chain_id: 324,
        providers: &[("zksync", "https://mainnet.era.zksync.io")],
    },
];

fn built_in_chains() -> HashMap<String, ChainConfig> {
    BUILT_IN_CHAINS
        .iter()
        .map(|entry| {
            let mut rpc = HashMap::new();
            let mut priority = Vec::new();
            for (name, url) in entry.providers {
                rpc.insert(name.to_string(), url.to_string());
                priority.push(name.to_string());
            }
            (
                entry.key.to_string(),
                ChainConfig {
                    chain_id: entry.chain_id,
                    priority: Some(priority),
                    rpc,
                },
            )
        })
        .collect()
}

fn config_file_path() -> Option<PathBuf> {
    env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".crpc.toml"))
}

fn rpc_from_env(chain_key: &str) -> Option<String> {
    let var = format!("CRPC_{}_RPC", chain_key.to_ascii_uppercase());
    env::var(&var).ok().and_then(|value| {
        value
            .split(|c: char| c == ',' || c == ';' || c.is_ascii_whitespace())
            .map(str::trim)
            .find(|part: &&str| !part.is_empty())
            .map(ToString::to_string)
    })
}

fn expand_env_vars(url: &str) -> Option<String> {
    let mut result = String::with_capacity(url.len());
    let mut rest = url;
    while let Some(start) = rest.find("${") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find('}') {
            let var_name = &after[..end];
            if var_name.is_empty() {
                return None;
            }
            let value = env::var(var_name).ok()?;
            result.push_str(&value);
            rest = &after[end + 1..];
        } else {
            return None;
        }
    }
    result.push_str(rest);
    Some(result)
}

/// Resolve a chain alias to its numeric chain ID.
/// Checks: direct parse → config chains → built-in chains → chainlist cache.
pub fn resolve_chain_id(chain: &str) -> Result<u64> {
    if let Ok(id) = chain.parse::<u64>() {
        return Ok(id);
    }
    if let Ok(config) = Config::load() {
        for (key, cfg) in &config.chains {
            if key.eq_ignore_ascii_case(chain) {
                return Ok(cfg.chain_id);
            }
        }
    }
    for entry in BUILT_IN_CHAINS {
        if entry.key.eq_ignore_ascii_case(chain)
            || entry.aliases.iter().any(|a| a.eq_ignore_ascii_case(chain))
        {
            return Ok(entry.chain_id);
        }
    }
    if let Ok(entries) = crate::chainlist::load_cached() {
        if let Some(entry) = crate::chainlist::lookup_by_name(&entries, chain) {
            return Ok(entry.chain_id);
        }
        // Fuzzy search
        if let Some(entry) = crate::chainlist::search(&entries, chain).into_iter().next() {
            return Ok(entry.chain_id);
        }
    }
    Err(eyre::eyre!("unknown chain: {chain}"))
}

fn find_chain_key(chain: &str, chains: &HashMap<String, ChainConfig>) -> Option<String> {
    let normalized = chain.trim();
    if normalized.is_empty() {
        return None;
    }
    for key in chains.keys() {
        if key.eq_ignore_ascii_case(normalized) {
            return Some(key.clone());
        }
    }
    if let Ok(id) = normalized.parse::<u64>() {
        if let Some((key, _)) = chains.iter().find(|(_, cfg)| cfg.chain_id == id) {
            return Some(key.clone());
        }
        if let Some(key) = builtin_chain_key_for_id(id) {
            return Some(key.to_string());
        }
    }
    builtin_chain_key_for_alias(normalized).map(ToString::to_string)
}

fn builtin_chain_key_for_alias(name: &str) -> Option<&'static str> {
    for entry in BUILT_IN_CHAINS {
        if entry.key.eq_ignore_ascii_case(name) {
            return Some(entry.key);
        }
        if entry
            .aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(name))
        {
            return Some(entry.key);
        }
    }
    None
}

fn builtin_chain_key_for_id(id: u64) -> Option<&'static str> {
    BUILT_IN_CHAINS
        .iter()
        .find(|entry| entry.chain_id == id)
        .map(|entry| entry.key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eyre::Result;
    use std::env;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }
    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prev = env::var(key).ok();
            unsafe { env::set_var(key, value) };
            EnvGuard { key, prev }
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(ref prev) = self.prev {
                unsafe { env::set_var(self.key, prev) };
            } else {
                unsafe { env::remove_var(self.key) };
            }
        }
    }

    struct HomeGuard {
        prev: Option<String>,
    }
    impl HomeGuard {
        fn set(path: impl AsRef<std::path::Path>) -> Self {
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

    fn test_chain(
        chain_id: u64,
        providers: &[(&str, &str)],
        priority: Option<&[&str]>,
    ) -> ChainConfig {
        let rpc = providers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        ChainConfig {
            chain_id,
            priority: priority.map(|p| p.iter().map(|s| s.to_string()).collect()),
            rpc,
        }
    }

    fn test_config(key: &str, chain: ChainConfig, default_provider: Option<&str>) -> Config {
        let mut chains = HashMap::new();
        chains.insert(key.to_string(), chain);
        Config {
            default_provider: default_provider.map(String::from),
            chains,
            tokens: HashMap::new(),
        }
    }

    #[test]
    fn built_in_defaults_resolve() -> Result<()> {
        let _home = HomeGuard::set("/tmp/crpc-test-no-config");
        let config = Config::load()?;
        assert_eq!(
            config.resolve_rpc("eth", &RpcOpts::default())?,
            "https://eth.llamarpc.com"
        );
        Ok(())
    }

    #[test]
    fn resolve_rpc_uses_env() -> Result<()> {
        let _home = HomeGuard::set("/tmp/crpc-test-no-config");
        let _guard = EnvGuard::set("CRPC_ETH_RPC", "https://env.rpc");
        let config = Config::load()?;
        assert_eq!(
            config.resolve_rpc("eth", &RpcOpts::default())?,
            "https://env.rpc"
        );
        Ok(())
    }

    #[test]
    fn config_file_overrides_builtin() -> Result<()> {
        let tmp = env::temp_dir().join(format!(
            "crpc_test_{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir_all(&tmp)?;
        fs::write(
            tmp.join(".crpc.toml"),
            "[chains.base]\nchain_id = 8453\npriority = [\"custom\"]\n\n[chains.base.rpc]\ncustom = \"https://custom.base\"\n",
        )?;
        let _home = HomeGuard::set(&tmp);
        let config = Config::load()?;
        assert_eq!(
            config.resolve_rpc("base", &RpcOpts::default())?,
            "https://custom.base"
        );
        fs::remove_dir_all(&tmp)?;
        Ok(())
    }

    #[test]
    fn rpc_override_wins() -> Result<()> {
        let config = test_config(
            "foo",
            test_chain(1, &[("a", "https://a")], Some(&["a"])),
            None,
        );
        let opts = RpcOpts {
            rpc: Some("https://override".into()),
            provider: None,
        };
        assert_eq!(config.resolve_rpc("foo", &opts)?, "https://override");
        Ok(())
    }

    #[test]
    fn provider_selects_named() -> Result<()> {
        let config = test_config(
            "foo",
            test_chain(
                1,
                &[("alpha", "https://alpha"), ("beta", "https://beta")],
                None,
            ),
            None,
        );
        let opts = RpcOpts {
            rpc: None,
            provider: Some("beta".into()),
        };
        assert_eq!(config.resolve_rpc("foo", &opts)?, "https://beta");
        Ok(())
    }

    #[test]
    fn env_var_expansion() -> Result<()> {
        let _guard = EnvGuard::set("CRPC_TEST_KEY", "mykey123");
        let config = test_config(
            "foo",
            test_chain(
                1,
                &[("api", "https://rpc.example.com/${CRPC_TEST_KEY}")],
                Some(&["api"]),
            ),
            None,
        );
        assert_eq!(
            config.resolve_rpc("foo", &RpcOpts::default())?,
            "https://rpc.example.com/mykey123"
        );
        Ok(())
    }

    #[test]
    fn env_var_missing_skips_provider() -> Result<()> {
        let config = test_config(
            "foo",
            test_chain(
                1,
                &[
                    ("bad", "https://${MISSING_VAR_XYZ}"),
                    ("good", "https://good"),
                ],
                Some(&["bad", "good"]),
            ),
            None,
        );
        assert_eq!(
            config.resolve_rpc("foo", &RpcOpts::default())?,
            "https://good"
        );
        Ok(())
    }

    #[test]
    fn priority_ordering() -> Result<()> {
        let config = test_config(
            "foo",
            test_chain(
                1,
                &[("alpha", "https://alpha"), ("beta", "https://beta")],
                Some(&["beta", "alpha"]),
            ),
            None,
        );
        assert_eq!(
            config.resolve_rpc("foo", &RpcOpts::default())?,
            "https://beta"
        );
        Ok(())
    }

    #[test]
    fn resolve_rpc_all_returns_all_urls_in_priority_order() -> Result<()> {
        let config = test_config(
            "foo",
            test_chain(
                1,
                &[
                    ("default", "https://default"),
                    ("alpha", "https://alpha"),
                    ("beta", "https://beta"),
                ],
                Some(&["alpha", "beta"]),
            ),
            Some("default"),
        );
        let urls = config.resolve_rpc_all("foo", &RpcOpts::default())?;
        assert_eq!(
            urls,
            vec![
                "https://default".to_string(),
                "https://alpha".to_string(),
                "https://beta".to_string(),
            ]
        );
        Ok(())
    }

    #[test]
    fn resolve_rpc_all_respects_rpc_override() -> Result<()> {
        let config = test_config(
            "foo",
            test_chain(1, &[("alpha", "https://alpha")], Some(&["alpha"])),
            None,
        );
        let opts = RpcOpts {
            rpc: Some("https://override".into()),
            provider: None,
        };
        assert_eq!(
            config.resolve_rpc_all("foo", &opts)?,
            vec!["https://override".to_string()]
        );
        Ok(())
    }

    #[test]
    fn default_provider_used() -> Result<()> {
        let config = test_config(
            "foo",
            test_chain(
                1,
                &[
                    ("special", "https://special"),
                    ("fallback", "https://fallback"),
                ],
                Some(&["fallback"]),
            ),
            Some("special"),
        );
        assert_eq!(
            config.resolve_rpc("foo", &RpcOpts::default())?,
            "https://special"
        );
        Ok(())
    }

    #[test]
    fn expand_env_vars_no_vars() {
        assert_eq!(
            expand_env_vars("https://plain.url"),
            Some("https://plain.url".into())
        );
    }

    #[test]
    fn expand_env_vars_unclosed_brace() {
        assert_eq!(expand_env_vars("https://${BROKEN"), None);
    }

    #[test]
    fn expand_env_vars_empty_name() {
        assert_eq!(expand_env_vars("https://${}"), None);
    }

    #[test]
    fn resolve_chain_id_builtins() -> Result<()> {
        assert_eq!(resolve_chain_id("1")?, 1);
        assert_eq!(resolve_chain_id("eth")?, 1);
        assert_eq!(resolve_chain_id("base")?, 8453);
        assert_eq!(resolve_chain_id("arb")?, 42161);
        assert_eq!(resolve_chain_id("arbitrum")?, 42161);
        Ok(())
    }
}
