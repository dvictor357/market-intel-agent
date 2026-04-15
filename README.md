# market-intel-agent

Rust MCP server for Hyperliquid perpetuals market intelligence using Smart Money Concepts (SMC) and Tenzro Cloud AI.

Exposes trading analysis tools via the [Model Context Protocol](https://modelcontextprotocol.io) — callable from Claude Code, Cursor, or any MCP-compatible client.

## Features

- **Smart Money Concepts engine** — Order Blocks, Fair Value Gaps, Break of Structure, Change of Character, Liquidity Zones, Smart Money flow detection
- **Whale activity tracking** — filter large trades by notional value, net buy/sell breakdown
- **Funding rate analysis** — sentiment bias from perpetual funding
- **Multi-pair market summary** — overview across multiple coins with AI narrative
- **Tenzro Cloud AI** — AI-powered trading suggestion (entry, SL, TP) for every analysis
- **Dual mode** — MCP stdio server (for AI clients) or HTTP server (for testing)

## Prerequisites

- Rust 1.85+
- [Tenzro Cloud](https://cloud.tenzro.com) account with an AI Inference endpoint created
- Hyperliquid access (public API, no auth needed)

## Setup

```bash
git clone https://github.com/your-username/market-intel-agent
cd market-intel-agent
cargo build --release
```

### Environment variables

Copy and fill in your credentials:

```bash
cp .env.example .env
```

`.env`:
```env
TENZRO_API_KEY=sk_...
TENZRO_MODEL=gemini-2.5-flash
TENZRO_PROVIDER=google
```

Optional overrides:
```env
TENZRO_PROJECT_ID=        # leave empty unless required
RUST_LOG=info             # log level
```

### Tenzro Cloud setup

1. Sign up at [cloud.tenzro.com](https://cloud.tenzro.com)
2. Create a project → Add Resource → **AI & Compute**
3. Go to **Inference** tab → **Add Endpoint**
4. Select provider (Google) and model (`gemini-2.5-flash`)
5. Set system prompt (optional, code includes one by default)
6. Copy your API key from the dashboard

## Usage

### As MCP server (Claude Code / Cursor)

Add to your `.claude/settings.json` or MCP client config:

```json
{
  "mcpServers": {
    "market-intel": {
      "command": "/path/to/market-intel-agent/target/release/market-intel-agent",
      "env": {
        "TENZRO_API_KEY": "sk_...",
        "TENZRO_MODEL": "gemini-2.5-flash",
        "TENZRO_PROVIDER": "google"
      }
    }
  }
}
```

Then ask Claude: *"Analyze BTC on the 1h timeframe using SMC"*

### As HTTP server (testing)

```bash
./target/release/market-intel-agent --http 8080
```

Test with [HTTPie](https://httpie.io):

```bash
# Full SMC analysis + AI suggestion
http POST localhost:8080/analyze pair=BTC interval=1h limit:=50

# Whale activity (trades > $500K)
http POST localhost:8080/whale pair=BTC min_usd:=500000

# Funding rate sentiment
http POST localhost:8080/funding pair=ETH

# Multi-pair market overview
http POST localhost:8080/summary pairs:='["BTC","ETH","SOL","HYPE"]'
```

## MCP Tools

| Tool | Description | Required args |
|------|-------------|---------------|
| `analyze_pair` | Full SMC analysis + AI trading brief | `pair` |
| `get_whale_activity` | Large trade tracking | `pair` |
| `get_funding_rate` | Funding rate + sentiment | `pair` |
| `get_market_summary` | Multi-pair overview + AI narrative | `pairs` |

### `analyze_pair` parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `pair` | string | — | Coin name: `BTC`, `ETH`, `SOL`, `HYPE`, etc. |
| `interval` | string | `1h` | Timeframe: `1m` `5m` `15m` `1h` `4h` `1d` |
| `limit` | integer | `100` | Number of candles to analyze |

### SMC signals detected

- **Order Block (OB)** — last opposing candle before a decisive structural move; institutional demand/supply zone
- **Fair Value Gap (FVG)** — 3-candle imbalance; price tends to retrace and fill before continuing
- **Break of Structure (BOS)** — close beyond previous swing high/low; trend continuation signal
- **Change of Character (CHoCH)** — structural reversal; first sign of trend change
- **Liquidity Zone** — clusters of equal highs/lows; stop orders resting, likely to be swept
- **Smart Money Flow** — volume-weighted accumulation or distribution in recent candles

## Architecture

```
src/
├── main.rs          # Entry point — MCP stdio or HTTP mode
├── mcp_server.rs    # MCP JSON-RPC 2.0 handler + tool logic
├── http_server.rs   # Thin HTTP wrapper (testing only)
├── market.rs        # Hyperliquid REST client
├── smc.rs           # SMC analysis engine
├── tenzro.rs        # Tenzro Cloud AI client
└── types.rs         # Shared types
```

**Data flow:**

```
MCP Client / HTTP
      │
      ▼
  mcp_server
      │
      ├── market.rs  →  Hyperliquid API (candles, trades, funding)
      ├── smc.rs     →  SMC signal detection
      └── tenzro.rs  →  Tenzro Cloud AI  →  trading suggestion
```

## Supported pairs

Any perpetual listed on Hyperliquid. Common ones: `BTC`, `ETH`, `SOL`, `ARB`, `HYPE`, `WIF`, `BONK`, `JTO`, `TIA`, `DOGE`.

## License

MIT
