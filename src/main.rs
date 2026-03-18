// crpc CLI entry point — chain-aware RPC tool
// Parses CLI args via clap, dispatches to command handlers

mod abi;
mod chainlist;
mod commands;
mod config;
mod etherscan;
mod format;
mod rpc;
#[allow(dead_code)]
mod cache;
mod tokens;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "crpc", version, about = "Chain-aware RPC CLI — cast but better")]
pub struct Cli {
    /// Override RPC URL directly (highest priority)
    #[arg(long, global = true)]
    pub rpc: Option<String>,
    /// Select a named provider from config
    #[arg(long, global = true)]
    pub provider: Option<String>,
    /// Output as JSON
    #[arg(long, global = true)]
    pub json: bool,
    /// Enable local caching for immutable RPC data
    #[arg(long, global = true)]
    pub cache: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// eth_call with auto ABI encode/decode
    Call {
        /// Chain alias (base, arb, eth) or chain ID
        chain: String,
        /// Contract address
        contract: String,
        /// Function signature, e.g. "getTick(int32)"
        sig: String,
        /// Function arguments
        args: Vec<String>,
        /// Output raw hex
        #[arg(long)]
        raw: bool,
        /// Human-readable output (auto-format token amounts)
        #[arg(long)]
        human: bool,
        /// Block number for historical query (default: latest)
        #[arg(long)]
        block: Option<String>,
    },
    /// ERC20 token balance query
    Balance {
        /// Chain alias or chain ID
        chain: String,
        /// Token symbol (WETH, USDC) or address
        token: String,
        /// Holder address
        holder: String,
        /// Output raw hex
        #[arg(long)]
        raw: bool,
        /// Block number for historical query (default: latest)
        #[arg(long)]
        block: Option<String>,
    },
    /// Fetch contract ABI from block explorer
    Abi {
        /// Chain alias or chain ID
        chain: String,
        /// Contract address
        contract: String,
        /// Output raw JSON ABI
        #[arg(long)]
        raw: bool,
    },
    /// Read contract storage slot
    Slot {
        /// Chain alias or chain ID
        chain: String,
        /// Contract address
        contract: String,
        /// Storage slot (hex or decimal)
        slot: String,
        /// Block number for historical query (default: latest)
        #[arg(long)]
        block: Option<String>,
    },
    /// Get block info
    Block {
        /// Chain alias or chain ID
        chain: String,
        /// Block number or "latest" (default: latest)
        number: Option<String>,
    },
    /// Transaction details + receipt + decoded logs
    Tx {
        /// Chain alias or chain ID
        chain: String,
        /// Transaction hash
        hash: String,
    },
    /// Query event logs
    Logs {
        /// Chain alias or chain ID
        chain: String,
        /// Contract address
        address: String,
        /// Event signature to filter/decode, e.g. "Transfer(address,address,uint256)"
        #[arg(long)]
        event: Option<String>,
        /// Filter by raw topic0 hash (0x-prefixed, 32 bytes)
        #[arg(long)]
        topic0: Option<String>,
        /// Starting block number
        #[arg(long)]
        from: Option<String>,
        /// Ending block number (default: latest)
        #[arg(long)]
        to: Option<String>,
        /// Query last N blocks from latest (shorthand for --from/--to)
        #[arg(long)]
        blocks: Option<u64>,
        /// Max logs to display
        #[arg(long, default_value = "50")]
        limit: usize,
    },
    /// Discover pools from factory contract events
    Pools {
        /// Chain alias or chain ID
        chain: String,
        /// Factory contract address
        factory: String,
        /// Event signature (default: PairCreated for Uniswap V2)
        #[arg(long)]
        event: Option<String>,
        /// Starting block number
        #[arg(long)]
        from: Option<String>,
        /// Ending block number
        #[arg(long)]
        to: Option<String>,
        /// Max pools to display
        #[arg(long, default_value = "50")]
        limit: usize,
    },
    /// Debug trace a transaction (requires archive node)
    Trace {
        /// Chain alias or chain ID
        chain: String,
        /// Transaction hash
        hash: String,
        /// Max call depth for formatting (default: 10)
        #[arg(long, default_value = "10")]
        depth: usize,
    },
    /// Batch calls via Multicall3
    Multi {
        /// Chain alias or chain ID
        chain: String,
        /// JSON file with call specs
        file: String,
    },
    /// Compare local cache vs on-chain value
    Diff {
        /// Chain alias or chain ID
        chain: String,
        /// Contract address
        contract: String,
        /// Function signature
        sig: String,
        /// Function arguments
        args: Vec<String>,
        /// Starting block number (required)
        #[arg(long)]
        from: String,
        /// Ending block number (default: latest)
        #[arg(long, default_value = "latest")]
        to: String,
    },
    /// Show current gas prices (Etherscan + RPC fallback)
    Gas {
        /// Chain alias or chain ID
        chain: String,
    },
    /// Get contract bytecode size/existence
    Code {
        /// Chain alias or chain ID
        chain: String,
        /// Contract address
        address: String,
    },
    /// Extract function selectors from contract bytecode
    Selectors {
        /// Chain alias or chain ID
        chain: String,
        /// Contract address
        contract: String,
        /// Skip online lookup (only show raw selectors)
        #[arg(long)]
        offline: bool,
    },
    /// Batch verify contract addresses (code exists on-chain)
    Verify {
        /// Chain alias or chain ID
        chain: String,
        /// Contract addresses to verify
        addresses: Vec<String>,
    },
    /// ERC20 allowance check
    Allowance {
        /// Chain alias or chain ID
        chain: String,
        /// Token symbol (WETH, USDC) or address
        token: String,
        /// Owner address
        owner: String,
        /// Spender address
        spender: String,
        /// Block number for historical query (default: latest)
        #[arg(long)]
        block: Option<String>,
    },
    /// Transaction history for an address
    History {
        /// Chain alias or chain ID
        chain: String,
        /// Address to query
        address: String,
        /// Starting block number
        #[arg(long)]
        from: Option<String>,
        /// Ending block number
        #[arg(long)]
        to: Option<String>,
        /// Max transactions to display
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// ERC-20 token transfer history
    Transfers {
        /// Chain alias or chain ID
        chain: String,
        /// Address to query
        address: String,
        /// Filter by token contract address
        #[arg(long)]
        token: Option<String>,
        /// Starting block number
        #[arg(long)]
        from: Option<String>,
        /// Ending block number
        #[arg(long)]
        to: Option<String>,
        /// Max transfers to display
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// List or search chains from chainlist.org
    Chains {
        /// Search query (name, symbol, or chain ID)
        query: Option<String>,
    },
    /// ABI-encode a function call (no RPC)
    Encode {
        /// Function signature, e.g. "transfer(address,uint256)"
        sig: String,
        /// Arguments to encode
        args: Vec<String>,
    },
    /// ABI-decode hex data (no RPC)
    Decode {
        /// Type signature for decoding, e.g. "(uint256,address)" or "transfer(address,uint256)"
        sig: String,
        /// Hex data to decode (0x-prefixed)
        data: String,
    },
    /// Manage crpc configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Interactive setup — configure RPC providers and chains
    Init {
        /// Write default config without prompts
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Set a config value
    Set { key: String, value: String },
    /// Get a config value
    Get { key: String },
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let cli = Cli::parse();
    let rpc = cli.rpc.as_deref();
    let provider = cli.provider.as_deref();
    match cli.command {
        Commands::Call { chain, contract, sig, args, raw, human, block } => {
            commands::call::run(&chain, &contract, &sig, &args, raw, human, block.as_deref(), rpc, provider, cli.json).await
        }
        Commands::Balance { chain, token, holder, raw, block } => {
            commands::balance::run(&chain, &token, &holder, raw, block.as_deref(), rpc, provider, cli.json).await
        }
        Commands::Abi { chain, contract, raw } => {
            commands::abi::run(&chain, &contract, raw, cli.json).await
        }
        Commands::Slot { chain, contract, slot, block } => {
            commands::storage::run(&chain, &contract, &slot, block.as_deref(), rpc, provider, cli.json).await
        }
        Commands::Block { chain, number } => {
            commands::block::run(&chain, number.as_deref(), rpc, provider, cli.json).await
        }
        Commands::Tx { chain, hash } => {
            commands::tx::run(&chain, &hash, rpc, provider, cli.json).await
        }
        Commands::Logs { chain, address, event, topic0, from, to, blocks, limit } => {
            commands::logs::run(&chain, &address, event.as_deref(), topic0.as_deref(), from.as_deref(), to.as_deref(), blocks, limit, rpc, provider, cli.json).await
        }
        Commands::Pools { chain, factory, event, from, to, limit } => {
            commands::pools::run(&chain, &factory, event.as_deref(), from.as_deref(), to.as_deref(), limit, cli.json, rpc, provider).await
        }
        Commands::Trace { chain, hash, depth } => {
            commands::trace::run(&chain, &hash, depth, rpc, provider).await
        }
        Commands::Multi { chain, file } => {
            commands::batch::run(&chain, &file, rpc, provider, cli.json).await
        }
        Commands::Diff { chain, contract, sig, args, from, to } => {
            commands::diff::run(&chain, &contract, &sig, &args, &from, &to, rpc, provider, cli.json).await
        }
        Commands::Gas { chain } => {
            commands::gas::run(&chain, rpc, provider, cli.json).await
        }
        Commands::History { chain, address, from, to, limit } => {
            commands::history::run(&chain, &address, from.as_deref(), to.as_deref(), limit, rpc, provider, cli.json).await
        }
        Commands::Code { chain, address } => {
            commands::code::run(&chain, &address, rpc, provider, cli.json).await
        }
        Commands::Selectors { chain, contract, offline } => {
            commands::selectors::run(&chain, &contract, offline, rpc, provider, cli.json).await
        }
        Commands::Verify { chain, addresses } => {
            commands::verify::run(&chain, &addresses, cli.json, rpc, provider).await
        }
        Commands::Allowance { chain, token, owner, spender, block } => {
            commands::allowance::run(&chain, &token, &owner, &spender, block.as_deref(), rpc, provider, cli.json).await
        }
        Commands::Transfers { chain, address, token, from, to, limit } => {
            commands::transfers::run(&chain, &address, token.as_deref(), from.as_deref(), to.as_deref(), limit, cli.json).await
        }
        Commands::Chains { query } => {
            commands::chains::run(query.as_deref()).await
        }
        Commands::Encode { sig, args } => commands::encode::run(&sig, &args).await,
        Commands::Decode { sig, data } => commands::decode::run(&sig, &data).await,
        Commands::Config { action } => match action {
            ConfigAction::Set { key, value } => commands::config_cmd::run_set(&key, &value),
            ConfigAction::Get { key } => commands::config_cmd::run_get(&key),
        },
        Commands::Init { force } => {
            if force {
                commands::init::run_default()
            } else {
                commands::init::run()
            }
        }
    }
}
