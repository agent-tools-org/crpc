// Built-in token address registry per chain
// Provides WETH, USDC, USDT, etc. addresses without config lookup

use alloy::primitives::Address;
use std::collections::HashMap;

/// Resolve a token symbol to its address on a given chain.
/// Falls back to config file tokens if not built-in.
pub fn resolve_token(
    chain: &str,
    symbol_or_addr: &str,
    config_tokens: Option<&HashMap<String, HashMap<String, String>>>,
) -> Option<Address> {
    let symbol = symbol_or_addr.trim();
    if let Some(addr) = parse_address(symbol) {
        return Some(addr);
    }
    if symbol.is_empty() {
        return None;
    }
    if let Some(entry) = find_builtin_chain(chain) {
        for (sym, addr_str) in entry.tokens {
            if sym.eq_ignore_ascii_case(symbol) {
                return parse_address(addr_str);
            }
        }
    }
    if let Some(map) = config_tokens {
        if let Some(chain_map) = match_config_tokens(chain, map) {
            if let Some((_, addr)) = chain_map.iter().find(|(key, _)| key.eq_ignore_ascii_case(symbol)) {
                return parse_address(addr);
            }
        }
    }
    None
}

/// Resolve a token address to a known symbol on a given chain.
pub fn lookup_symbol(
    chain: &str,
    address: Address,
    config_tokens: Option<&HashMap<String, HashMap<String, String>>>,
) -> Option<String> {
    if let Some(entry) = find_builtin_chain(chain) {
        if let Some((symbol, _)) = entry.tokens.iter().find(|(_, value)| parse_address(value) == Some(address)) {
            return Some((*symbol).to_string());
        }
    }
    let chain_map = match_config_tokens(chain, config_tokens?)?;
    chain_map
        .iter()
        .find(|(_, value)| parse_address(value) == Some(address))
        .map(|(symbol, _)| symbol.clone())
}

struct TokenChain {
    key: &'static str,
    aliases: &'static [&'static str],
    chain_id: u64,
    tokens: &'static [(&'static str, &'static str)],
}

const TOKEN_CHAINS: &[TokenChain] = &[
    TokenChain { key: "eth", aliases: &["ethereum"], chain_id: 1, tokens: &[
        ("WETH", "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
        ("USDC", "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        ("USDT", "0xdAC17F958D2ee523a2206206994597C13D831ec7"),
        ("DAI", "0x6B175474E89094C44Da98b954EedeAC495271d0F"),
        ("WBTC", "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599"),
    ] },
    TokenChain { key: "base", aliases: &["base-mainnet"], chain_id: 8453, tokens: &[
        ("WETH", "0x4200000000000000000000000000000000000006"),
        ("USDC", "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
        ("USDbC", "0xd9aAEc86B65D86f6A7B5B1b0c42FFA531710b6CA"),
        ("cbETH", "0x2Ae3F1Ec7F1F5012CFEab0185bfc7aa3cf0DEc22"),
    ] },
    TokenChain { key: "arb", aliases: &["arbitrum", "arb1"], chain_id: 42161, tokens: &[
        ("WETH", "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
        ("USDC", "0xaf88d065e77c8cC2239327C5EDb3A432268e5831"),
        ("USDC.e", "0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8"),
        ("USDT", "0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9"),
        ("WBTC", "0x2f2a2543B76A4166549F7aaB2e75Bef0aefC5B0f"),
        ("ARB", "0x912CE59144191C1204E64559FE8253a0e49E6548"),
    ] },
];

fn find_builtin_chain(chain: &str) -> Option<&'static TokenChain> {
    let normalized = chain.trim();
    if normalized.is_empty() {
        return None;
    }
    let parsed_id = normalized.parse::<u64>().ok();
    for entry in TOKEN_CHAINS {
        if entry.key.eq_ignore_ascii_case(normalized)
            || entry.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(normalized))
        {
            return Some(entry);
        }
        if let Some(id) = parsed_id {
            if entry.chain_id == id {
                return Some(entry);
            }
        }
    }
    None
}

fn match_config_tokens<'a>(chain: &str, config_tokens: &'a HashMap<String, HashMap<String, String>>) -> Option<&'a HashMap<String, String>> {
    if let Some(map) = config_tokens.get(chain) {
        return Some(map);
    }
    let normalized = chain.trim();
    if normalized.is_empty() {
        return None;
    }
    if let Some((_, value)) = config_tokens.iter().find(|(key, _)| key.eq_ignore_ascii_case(normalized)) {
        return Some(value);
    }
    find_builtin_chain(chain)
        .and_then(|entry| config_tokens.get(entry.key))
}

fn parse_address(value: &str) -> Option<Address> {
    let trimmed = value.trim();
    if !trimmed.starts_with("0x") || trimmed.len() != 42 {
        return None;
    }
    trimmed.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn parses_direct_address() {
        let expected: Address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse().unwrap();
        assert_eq!(resolve_token("eth", "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", None), Some(expected));
    }

    #[test]
    fn finds_built_in_symbol() {
        let addr = resolve_token("ethereum", "usdc", None).unwrap();
        assert_eq!(addr.to_string().to_ascii_lowercase(), "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");
        let arb = resolve_token("arb", "USDC.e", None).unwrap();
        assert_eq!(arb.to_string().to_ascii_lowercase(), "0xff970a61a04b1ca14834a43f5de4533ebddb5cc8");
    }

    #[test]
    fn uses_config_tokens() {
        let mut config = HashMap::new();
        let mut tokens = HashMap::new();
        tokens.insert("special".to_string(), "0x1111111111111111111111111111111111111111".to_string());
        config.insert("base".to_string(), tokens);
        let addr = resolve_token("base-mainnet", "special", Some(&config)).unwrap();
        assert_eq!(addr, "0x1111111111111111111111111111111111111111".parse::<Address>().unwrap());
    }

    #[test]
    fn looks_up_symbol_by_address() {
        let built_in = lookup_symbol(
            "ethereum",
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".parse().unwrap(),
            None,
        );
        assert_eq!(built_in.as_deref(), Some("USDC"));
        let mut config = HashMap::new();
        let mut tokens = HashMap::new();
        tokens.insert("special".to_string(), "0x1111111111111111111111111111111111111111".to_string());
        config.insert("base".to_string(), tokens);
        let custom = lookup_symbol(
            "base-mainnet",
            "0x1111111111111111111111111111111111111111".parse().unwrap(),
            Some(&config),
        );
        assert_eq!(custom.as_deref(), Some("special"));
    }
}
