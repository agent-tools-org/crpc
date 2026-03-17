# crpc

Chain-aware RPC CLI — cast but better.

Auto ABI encode/decode, 2600+ chains via chainlist.org, Etherscan V2 integration with RPC fallback, built-in token registry, batch Multicall3, event logs with block range shortcuts, trace, bytecode inspection, and formatted output.

## Install

```bash
cargo install --path .
```

## Commands

| Command | Description |
|---------|-------------|
| `call` | eth_call with auto ABI encode/decode |
| `balance` | ERC-20 token balance query |
| `allowance` | ERC-20 allowance check |
| `abi` | Fetch contract ABI from Etherscan (`--raw` for JSON) |
| `verify` | Batch verify contract addresses (code on-chain) |
| `pools` | Discover pools from factory contract events |
| `code` | Get contract bytecode size/existence |
| `slot` | Read contract storage slot |
| `block` | Get block info |
| `tx` | Transaction details + receipt + decoded logs |
| `logs` | Query and decode event logs |
| `trace` | Debug trace a transaction (requires archive node) |
| `multi` | Batch calls via Multicall3 |
| `diff` | Compare on-chain values across blocks |
| `gas` | Current gas prices (Etherscan + RPC fallback) |
| `history` | Transaction history (Etherscan + RPC fallback) |
| `transfers` | ERC-20 token transfer history |
| `chains` | List or search 2600+ chains from chainlist.org |
| `encode` | ABI-encode a function call (offline) |
| `decode` | ABI-decode hex data (offline) |
| `config` | Manage persistent configuration (`set`/`get`) |

### Global flags

```
--rpc <URL>            Override RPC URL (highest priority)
--provider <NAME>      Select named provider from config
--json                 Output as JSON
```

## Usage

### `crpc call` — eth_call with auto decode

```bash
crpc call base 0x4200000000000000000000000000000000000006 "name()(string)"
# [0] string = Wrapped Ether

crpc call base 0x4200000000000000000000000000000000000006 "decimals()(uint8)"
# [0] uint8 = 18

# Negative int arg
crpc call base 0xABCD...pool "getTick(int32)(int24)" -- -30

# Raw hex output
crpc call base 0xABCD...pool "getState()(uint256,int24,uint256)" --raw

# Historical query at specific block
crpc call eth 0x... "balanceOf(address)(uint256)" 0x... --block 19000000
```

### `crpc balance` — ERC-20 balance

```bash
crpc balance base WETH 0xYourAddress
# Balance: 871000000000000000
# Human: 0.871 WETH

# Token address directly
crpc balance arb 0xaf88d065e77c8cC2239327C5EDb3A432268e5831 0xYourAddress
```

### `crpc allowance` — ERC-20 allowance check

```bash
crpc allowance base USDC 0xOwner 0xSpender
# Token:     USDC
# Owner:     0xOwner
# Spender:   0xSpender
# Allowance: unlimited (max uint256)

# With token address
crpc allowance arb 0xaf88d065e77c8cC2239327C5EDb3A432268e5831 0xOwner 0xSpender
```

### `crpc code` — Contract bytecode check

```bash
crpc code base 0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913
# Address: 0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913
# Type:    Contract
# Size:    1852 bytes
# Code:    0x60806040...060c0033

crpc code eth 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045
# Type:    EOA (no bytecode)
```

Useful for verifying if an address is a contract or EOA during DEX investigation.

### `crpc abi` — Fetch contract ABI

```bash
crpc abi eth 0xdAC17F958D2ee523a2206206994597C13D831ec7
# transfer(address,uint256) -> (bool)
# approve(address,uint256) -> (bool)
# event Transfer(address indexed,address indexed,uint256)
# ...

# Raw JSON ABI output
crpc abi eth 0xdAC17F958D2ee523a2206206994597C13D831ec7 --raw
# [{ "type": "function", "name": "transfer", ... }]

# Also works with --json global flag
crpc --json abi eth 0xdAC17F958D2ee523a2206206994597C13D831ec7
```

Requires `ETHERSCAN_API_KEY` (via env var or `crpc config set`).

### `crpc verify` — Batch verify addresses

```bash
crpc verify eth 0xdAC17F958D2ee523a2206206994597C13D831ec7 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045
# Address                                    Status    Size
# 0xdAC17F958D2ee523a2206206994597C13D831ec7 CONTRACT  10.8 KB
# 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045 EOA       -

crpc verify eth 0xAddress1 0xAddress2 --json
# [{"address":"0x...","is_contract":true,"code_size":11075}, ...]
```

### `crpc pools` — Discover pools from factory events

```bash
# Uniswap V2 factory (default: PairCreated event)
crpc pools eth 0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f --limit 5
# Pool                                       Token0                                     Token1
# 0xDe05cb...b744                            0xC02aaA...6Cc2 (WETH)                     0xF385fc...049a

# Uniswap V3 factory with custom event
crpc pools eth 0x1F98431c8aD98523631AE4a59f267346ea31F984 \
  --event "PoolCreated(address indexed,address indexed,uint24,int24,address)" \
  --from 12369621 --limit 10

# JSON output
crpc pools eth 0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f --json --limit 3
```

### `crpc slot` — Storage slot read

```bash
crpc slot base 0x4200000000000000000000000000000000000006 0
# Slot 0: 0x577261707065642045746865720000000000000000000000000000000000001a
# As uint256: 3955331089...
```

### `crpc block` — Block info

```bash
crpc block base
# Number: 43432032
# Hash: 0xe5a982...
# Timestamp: 1773653411
# Gas used: 51663520
# Transactions: 190

crpc block eth 19000000
```

### `crpc tx` — Transaction + receipt

```bash
crpc tx base 0xTxHash
# From: 0x...
# To: 0x...
# Value: 0
# Status: true
# Gas used: 21000
# Log count: 3
```

### `crpc logs` — Query event logs

```bash
# Last 1000 blocks from a contract (no --from/--to needed)
crpc logs base 0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913 --blocks 1000

# Filter and decode Transfer events
crpc logs eth 0xdAC17F958D2ee523a2206206994597C13D831ec7 \
  --event "Transfer(address indexed,address indexed,uint256)" \
  --blocks 100 --limit 10

# Filter by raw topic0 hash (for unknown/custom events)
crpc logs base 0xPoolAddress \
  --topic0 0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef \
  --blocks 500

# Explicit block range
crpc logs eth 0xAddress --from 19000000 --to 19000100
```

### `crpc trace` — Debug trace a transaction

```bash
crpc trace eth 0xTxHash
# CALL 0xFrom -> 0xTo  value: 0
#   STATICCALL 0xTo -> 0xContract1
#     output: 0x... (32 bytes)

crpc trace eth 0xTxHash --depth 3
```

Requires an archive/debug node (Alchemy, QuickNode, etc).

### `crpc diff` — Compare values across blocks

```bash
crpc diff base 0xPool "slot0()(uint160,int24,uint16)" --from 43000000 --to 43001000
```

### `crpc gas` — Gas price oracle

```bash
crpc gas eth
# Gas Prices (Gwei):
#   Safe:     20
#   Standard: 25
#   Fast:     30
#   Base fee: 18.5

# Works on L2s too — falls back to RPC when Etherscan unavailable
crpc gas base
# Gas Price (via RPC):
#   Gas price: 0.006 Gwei
#   Priority:  0.001 Gwei
```

Uses Etherscan gas oracle when available, falls back to `eth_gasPrice` / `eth_maxPriorityFeePerGas` RPC calls on chains where Etherscan requires a paid plan (Base, Arbitrum, etc).

### `crpc history` — Transaction history

```bash
crpc history eth 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045 --limit 5

# Works on L2s — falls back to recent block scanning via RPC
crpc history base 0xAddress --limit 10
```

Uses Etherscan when available, falls back to scanning recent blocks via RPC.

### `crpc transfers` — Token transfer history

```bash
# All ERC-20 transfers
crpc transfers eth 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045

# Filter by token contract
crpc transfers eth 0xAddress --token 0xdAC17F958D2ee523a2206206994597C13D831ec7
```

Requires `ETHERSCAN_API_KEY`.

### `crpc chains` — Browse 2600+ chains

```bash
crpc chains              # Top 30 by RPC availability
crpc chains fantom       # Search by name
crpc chains 250          # Search by chain ID
```

Any chain from chainlist.org works automatically — no config needed:

```bash
crpc block fantom          # Fantom Opera (chain 250)
crpc block gnosis          # Gnosis Chain (chain 100)
crpc block moonbeam        # Moonbeam (chain 1284)
crpc block 250             # By chain ID directly
```

### `crpc encode` / `crpc decode` — Offline ABI tools

```bash
crpc encode "transfer(address,uint256)" 0xRecipient 1000000
# 0xa9059cbb000000000000000000000000...00000000000000000000000000000f4240

# Decode with function signature — typed output
crpc decode "swap(address,bool,int256,uint160)" 0x04e45aaf000000...
# [0] address = 0x833589fcd6edb6e08f4c7c32d4f71b54bda02913
# [1] bool = true
# [2] int256 = 100
# [3] uint160 = 200

# Decode raw tuple
crpc decode "(uint256,address)" 0x000000...
```

### `crpc multi` — Batch calls via Multicall3

```bash
crpc multi base calls.json
```

`calls.json`:
```json
[
  {"target": "0x4200..0006", "sig": "name()(string)", "args": []},
  {"target": "0x4200..0006", "sig": "decimals()(uint8)", "args": []},
  {"target": "0x8335..2913", "sig": "symbol()(string)", "args": []}
]
```

All calls execute in a single RPC request.

## Chain Support

### Built-in aliases

| Alias | Chain ID |
|-------|----------|
| `eth` / `ethereum` | 1 |
| `base` | 8453 |
| `arb` / `arbitrum` | 42161 |
| `bsc` | 56 |
| `polygon` / `matic` | 137 |
| `op` / `optimism` | 10 |
| `avax` / `avalanche` | 43114 |
| `linea` | 59144 |
| `scroll` | 534352 |
| `zksync` / `zksync-era` | 324 |

You can use chain IDs directly: `crpc block 8453`.

Beyond built-in chains, crpc supports **2600+ chains** via [chainlist.org](https://chainlist.org). Run `crpc chains` to browse.

### Built-in tokens

| Chain | Tokens |
|-------|--------|
| eth | WETH, USDC, USDT, DAI, WBTC |
| base | WETH, USDC, USDbC, cbETH |
| arb | WETH, USDC, USDC.e, USDT, WBTC, ARB |

## Etherscan Integration

Commands `abi`, `gas`, `history`, and `transfers` use the [Etherscan V2 API](https://docs.etherscan.io/etherscan-v2) — a single unified endpoint supporting 60+ chains. On L2 chains where Etherscan requires a paid plan, `gas` and `history` automatically fall back to RPC methods.

```bash
# Option 1: persistent config (recommended)
crpc config set etherscan_api_key your_key_here

# Option 2: environment variable
export ETHERSCAN_API_KEY=your_key_here  # free at etherscan.io/apis
```

## JSON Output

All commands support `--json` for machine-readable output:

```bash
crpc --json call base 0x... "name()(string)"
crpc --json block eth 19000000
crpc --json gas eth
crpc --json code base 0x...
```

## RPC Provider Selection

### Resolution priority

1. `--rpc <url>` flag — use directly
2. `CRPC_{CHAIN}_RPC` env var (e.g. `CRPC_ETH_RPC=https://...`)
3. `--provider <name>` flag — named provider from config
4. `default_provider` from config file
5. `priority` list — first provider whose env vars resolve
6. Chainlist.org public RPCs
7. Built-in fallback RPCs

When a chain has multiple providers, crpc automatically tries the next on transport errors (timeout, connection refused). Contract-level errors (reverts) do not trigger fallback.

### `crpc config` — Manage persistent configuration

```bash
crpc config set etherscan_api_key YOUR_KEY
crpc config set default_provider alchemy

crpc config get etherscan_api_key
# YOUR_KEY
```

Settings are stored in `~/.crpc.toml`. Environment variables override config values.

## Configuration

`~/.crpc.toml`:

```toml
default_provider = "alchemy"

[chains.eth]
chain_id = 1
priority = ["alchemy", "llamarpc"]

[chains.eth.rpc]
alchemy = "https://eth-mainnet.g.alchemy.com/v2/${ALCHEMY_API_KEY}"
llamarpc = "https://eth.llamarpc.com"

[chains.base]
chain_id = 8453
priority = ["alchemy", "base"]

[chains.base.rpc]
alchemy = "https://base-mainnet.g.alchemy.com/v2/${ALCHEMY_API_KEY}"
base = "https://mainnet.base.org"

[tokens.base]
DEGEN = "0x4ed4E862860beD51a9570b96d89aF5E1B0Efefed"
```

URLs support `${VAR_NAME}` environment variable expansion. Providers with unresolvable variables are automatically skipped.

## Function Signature Format

```
functionName(paramTypes)(returnTypes)
```

Examples: `balanceOf(address)(uint256)`, `name()(string)`, `transfer(address,uint256)(bool)`

Return types are optional — omit for raw hex word output.
