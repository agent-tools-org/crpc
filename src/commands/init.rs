// Interactive config generator for ~/.crpc.toml
// Walks user through provider keys, chain selection, and connectivity test

use dialoguer::{Confirm, MultiSelect, Select};
use eyre::{Result, WrapErr};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Known RPC providers with URL templates per chain
struct Provider {
    name: &'static str,
    env_var: &'static str,
    /// (chain_key, chain_id, url_template with `${ENV_VAR}`)
    chains: &'static [(&'static str, u64, &'static str)],
}

const PROVIDERS: &[Provider] = &[
    Provider {
        name: "Alchemy",
        env_var: "ALCHEMY_API_KEY",
        chains: &[
            ("eth", 1, "https://eth-mainnet.g.alchemy.com/v2/${ALCHEMY_API_KEY}"),
            ("base", 8453, "https://base-mainnet.g.alchemy.com/v2/${ALCHEMY_API_KEY}"),
            ("arb", 42161, "https://arb-mainnet.g.alchemy.com/v2/${ALCHEMY_API_KEY}"),
            ("op", 10, "https://opt-mainnet.g.alchemy.com/v2/${ALCHEMY_API_KEY}"),
            ("polygon", 137, "https://polygon-mainnet.g.alchemy.com/v2/${ALCHEMY_API_KEY}"),
            ("zksync", 324, "https://zksync-mainnet.g.alchemy.com/v2/${ALCHEMY_API_KEY}"),
        ],
    },
    Provider {
        name: "Infura",
        env_var: "INFURA_API_KEY",
        chains: &[
            ("eth", 1, "https://mainnet.infura.io/v3/${INFURA_API_KEY}"),
            ("arb", 42161, "https://arbitrum-mainnet.infura.io/v3/${INFURA_API_KEY}"),
            ("op", 10, "https://optimism-mainnet.infura.io/v3/${INFURA_API_KEY}"),
            ("polygon", 137, "https://polygon-mainnet.infura.io/v3/${INFURA_API_KEY}"),
            ("linea", 59144, "https://linea-mainnet.infura.io/v3/${INFURA_API_KEY}"),
            ("base", 8453, "https://base-mainnet.infura.io/v3/${INFURA_API_KEY}"),
            ("avax", 43114, "https://avalanche-mainnet.infura.io/v3/${INFURA_API_KEY}"),
        ],
    },
    Provider {
        name: "Ankr",
        env_var: "ANKR_API_KEY",
        chains: &[
            ("eth", 1, "https://rpc.ankr.com/eth/${ANKR_API_KEY}"),
            ("base", 8453, "https://rpc.ankr.com/base/${ANKR_API_KEY}"),
            ("arb", 42161, "https://rpc.ankr.com/arbitrum/${ANKR_API_KEY}"),
            ("op", 10, "https://rpc.ankr.com/optimism/${ANKR_API_KEY}"),
            ("polygon", 137, "https://rpc.ankr.com/polygon/${ANKR_API_KEY}"),
            ("bsc", 56, "https://rpc.ankr.com/bsc/${ANKR_API_KEY}"),
            ("avax", 43114, "https://rpc.ankr.com/avalanche/${ANKR_API_KEY}"),
            ("scroll", 534352, "https://rpc.ankr.com/scroll/${ANKR_API_KEY}"),
        ],
    },
];

struct ChainOption {
    key: &'static str,
    label: &'static str,
    chain_id: u64,
    free_rpcs: &'static [(&'static str, &'static str)],
}

const CHAIN_OPTIONS: &[ChainOption] = &[
    ChainOption {
        key: "eth",
        label: "Ethereum (1)",
        chain_id: 1,
        free_rpcs: &[
            ("llamarpc", "https://eth.llamarpc.com"),
            ("ankr", "https://rpc.ankr.com/eth"),
        ],
    },
    ChainOption {
        key: "base",
        label: "Base (8453)",
        chain_id: 8453,
        free_rpcs: &[
            ("base", "https://mainnet.base.org"),
            ("llamarpc", "https://base.llamarpc.com"),
        ],
    },
    ChainOption {
        key: "arb",
        label: "Arbitrum (42161)",
        chain_id: 42161,
        free_rpcs: &[
            ("arbitrum", "https://arb1.arbitrum.io/rpc"),
            ("llamarpc", "https://arbitrum.llamarpc.com"),
        ],
    },
    ChainOption {
        key: "op",
        label: "Optimism (10)",
        chain_id: 10,
        free_rpcs: &[("optimism", "https://mainnet.optimism.io")],
    },
    ChainOption {
        key: "polygon",
        label: "Polygon (137)",
        chain_id: 137,
        free_rpcs: &[("polygon", "https://polygon-rpc.com")],
    },
    ChainOption {
        key: "bsc",
        label: "BSC (56)",
        chain_id: 56,
        free_rpcs: &[("binance", "https://bsc-dataseed.binance.org")],
    },
    ChainOption {
        key: "avax",
        label: "Avalanche (43114)",
        chain_id: 43114,
        free_rpcs: &[("avalanche", "https://api.avax.network/ext/bc/C/rpc")],
    },
    ChainOption {
        key: "linea",
        label: "Linea (59144)",
        chain_id: 59144,
        free_rpcs: &[("linea", "https://rpc.linea.build")],
    },
    ChainOption {
        key: "scroll",
        label: "Scroll (534352)",
        chain_id: 534352,
        free_rpcs: &[("scroll", "https://rpc.scroll.io")],
    },
    ChainOption {
        key: "zksync",
        label: "zkSync Era (324)",
        chain_id: 324,
        free_rpcs: &[("zksync", "https://mainnet.era.zksync.io")],
    },
];

pub fn run() -> Result<()> {
    println!();
    println!("  crpc init — configure your RPC endpoints");
    println!("  ─────────────────────────────────────────");
    println!();

    let config_path = config_path()?;

    // Check existing config
    if config_path.exists() {
        println!("  Found existing config: {}", config_path.display());
        let existing = fs::read_to_string(&config_path)?;
        let line_count = existing.lines().count();
        println!("  ({line_count} lines)");
        println!();
        if !Confirm::new()
            .with_prompt("  Overwrite existing config?")
            .default(false)
            .interact()?
        {
            println!();
            println!("  Keeping existing config. Use `crpc init --force` or edit ~/.crpc.toml manually.");
            return Ok(());
        }
        println!();
    }

    // Step 1: Provider API keys
    println!("  Step 1/4 — RPC Providers");
    println!("  Paid providers (Alchemy, Infura, Ankr) give faster, more reliable RPCs.");
    println!("  API keys are stored as env var references (${{VAR}}), not plaintext.");
    println!();

    let mut configured_providers: Vec<(usize, bool)> = Vec::new(); // (provider_idx, has_key_in_env)

    for (idx, provider) in PROVIDERS.iter().enumerate() {
        let env_set = env::var(provider.env_var).is_ok();
        let status = if env_set { " (detected in env)" } else { "" };

        let use_it = Confirm::new()
            .with_prompt(format!("  Configure {}?{status}", provider.name))
            .default(env_set)
            .interact()?;

        if use_it {
            if !env_set {
                println!("    → Set ${} in your shell profile to enable.", provider.env_var);
                println!("    → Config will use ${{{}}} template (works once env is set).", provider.env_var);
            } else {
                println!("    → ${} found, will use it.", provider.env_var);
            }
            configured_providers.push((idx, env_set));
        }
        println!();
    }

    // Step 2: Chain selection
    println!("  Step 2/4 — Chains");
    println!("  Select which chains you work with (free public RPCs always included).");
    println!("  crpc also supports 2600+ chains via chainlist.org — no config needed.");
    println!();

    let chain_labels: Vec<&str> = CHAIN_OPTIONS.iter().map(|c| c.label).collect();
    // Default: select eth, base, arb
    let defaults: Vec<bool> = CHAIN_OPTIONS
        .iter()
        .map(|c| matches!(c.key, "eth" | "base" | "arb"))
        .collect();

    let selected_chains = MultiSelect::new()
        .with_prompt("  Select chains")
        .items(&chain_labels)
        .defaults(&defaults)
        .interact()?;

    if selected_chains.is_empty() {
        println!();
        println!("  No chains selected. crpc will still work using chainlist.org for any chain.");
        println!("  Writing minimal config...");
        write_config(&config_path, None, &BTreeMap::new())?;
        print_done(&config_path);
        return Ok(());
    }

    println!();

    // Step 3: Default provider
    println!("  Step 3/4 — Default Provider");
    println!("  When multiple providers are configured, which should be tried first?");
    println!();

    let mut provider_names: Vec<&str> = configured_providers
        .iter()
        .map(|(idx, _)| PROVIDERS[*idx].name)
        .collect();
    provider_names.push("None (use free RPCs first)");

    let default_provider = if provider_names.len() > 1 {
        let choice = Select::new()
            .with_prompt("  Default provider")
            .items(&provider_names)
            .default(0)
            .interact()?;
        if choice < configured_providers.len() {
            Some(PROVIDERS[configured_providers[choice].0].name)
        } else {
            None
        }
    } else {
        None
    };

    println!();

    // Step 4: Etherscan API key
    println!("  Step 4/4 — Etherscan API Key (optional)");
    println!("  Used for `crpc abi`, `crpc history`, `crpc transfers`, and `crpc gas`.");
    println!("  Get a free key at https://etherscan.io/myapikey");
    println!();

    let etherscan_key_set = env::var("ETHERSCAN_API_KEY").is_ok();
    let etherscan_status = if etherscan_key_set {
        " (detected in env)"
    } else {
        ""
    };

    let configure_etherscan = Confirm::new()
        .with_prompt(format!(
            "  Configure Etherscan V2 API key?{etherscan_status}"
        ))
        .default(etherscan_key_set)
        .interact()?;

    if configure_etherscan && !etherscan_key_set {
        println!("    → Set $ETHERSCAN_API_KEY in your shell profile.");
        println!("    → One key works for all chains (Etherscan V2 unified API).");
    }
    println!();

    // Build config
    let mut chains: BTreeMap<String, ChainEntry> = BTreeMap::new();

    for &idx in &selected_chains {
        let chain = &CHAIN_OPTIONS[idx];
        let mut rpcs: BTreeMap<String, String> = BTreeMap::new();
        let mut priority: Vec<String> = Vec::new();

        // Add paid provider RPCs first
        for &(provider_idx, _) in &configured_providers {
            let provider = &PROVIDERS[provider_idx];
            if let Some((_, _, url)) = provider.chains.iter().find(|(k, _, _)| *k == chain.key) {
                let name = provider.name.to_lowercase();
                rpcs.insert(name.clone(), url.to_string());
                priority.push(name);
            }
        }

        // Add free RPCs
        for (name, url) in chain.free_rpcs {
            if !rpcs.contains_key(*name) {
                rpcs.insert(name.to_string(), url.to_string());
                priority.push(name.to_string());
            }
        }

        chains.insert(
            chain.key.to_string(),
            ChainEntry {
                chain_id: chain.chain_id,
                priority,
                rpc: rpcs,
            },
        );
    }

    write_config(
        &config_path,
        default_provider.map(|p| p.to_lowercase()),
        &chains,
    )?;

    // Connectivity test
    println!();
    let test_chain = &CHAIN_OPTIONS[selected_chains[0]];
    if Confirm::new()
        .with_prompt(format!(
            "  Test connectivity to {}?",
            test_chain.label
        ))
        .default(true)
        .interact()?
    {
        print!("  Testing {}... ", test_chain.key);
        std::io::stdout().flush()?;
        match test_rpc(test_chain.free_rpcs[0].1) {
            Ok(chain_id) => println!("OK (chain_id: {chain_id})"),
            Err(e) => println!("FAILED: {e}"),
        }
    }

    print_done(&config_path);
    Ok(())
}

struct ChainEntry {
    chain_id: u64,
    priority: Vec<String>,
    rpc: BTreeMap<String, String>,
}

fn write_config(
    path: &PathBuf,
    default_provider: Option<String>,
    chains: &BTreeMap<String, ChainEntry>,
) -> Result<()> {
    let mut out = String::new();
    out.push_str("# crpc configuration — generated by `crpc init`\n");
    out.push_str("# Docs: https://github.com/agent-tools-org/crpc\n\n");

    if let Some(ref provider) = default_provider {
        out.push_str(&format!("default_provider = \"{provider}\"\n\n"));
    }

    for (key, entry) in chains {
        out.push_str(&format!("[chains.{key}]\n"));
        out.push_str(&format!("chain_id = {}\n", entry.chain_id));
        let priority_str: Vec<String> = entry.priority.iter().map(|p| format!("\"{p}\"")).collect();
        out.push_str(&format!("priority = [{}]\n\n", priority_str.join(", ")));

        out.push_str(&format!("[chains.{key}.rpc]\n"));
        for (name, url) in &entry.rpc {
            out.push_str(&format!("{name} = \"{url}\"\n"));
        }
        out.push('\n');
    }

    // Tokens section (commented example)
    out.push_str("# Custom token addresses (optional)\n");
    out.push_str("# [tokens.base]\n");
    out.push_str("# MYTOKEN = \"0x...\"\n");

    fs::write(path, &out).wrap_err_with(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn config_path() -> Result<PathBuf> {
    let home = env::var("HOME").wrap_err("HOME not set")?;
    Ok(PathBuf::from(home).join(".crpc.toml"))
}

fn test_rpc(url: &str) -> Result<u64> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let resp: serde_json::Value = client
        .post(url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_chainId",
            "params": [],
            "id": 1
        }))
        .send()?
        .json()?;
    let hex = resp["result"]
        .as_str()
        .ok_or_else(|| eyre::eyre!("no result in response"))?;
    let id = u64::from_str_radix(hex.trim_start_matches("0x"), 16)?;
    Ok(id)
}

fn print_done(path: &std::path::Path) {
    println!();
    println!("  Config written to {}", path.display());
    println!();
    println!("  Next steps:");
    println!("    • Set API key env vars in your shell profile (~/.zshrc or ~/.bashrc)");
    println!("    • Try: crpc call eth 0xdAC17F958D2ee523a2206206994597C13D831ec7 \"name()\"");
    println!("    • Try: crpc balance base USDC 0x...");
    println!("    • Try: crpc chains solana");
    println!();
}

/// Non-interactive init for `--force` flag (write defaults without prompts)
pub fn run_default() -> Result<()> {
    let config_path = config_path()?;
    let chains = BTreeMap::new();
    write_config(&config_path, None, &chains)?;
    println!("  Wrote default config to {}", config_path.display());
    Ok(())
}
