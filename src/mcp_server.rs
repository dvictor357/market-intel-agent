/// MCP stdio server — JSON-RPC 2.0 transport.
///
/// Protocol flow (client = any MCP host like Claude Code / Cursor):
///   client → server : initialize
///   server → client : initialize result (capabilities)
///   client → server : notifications/initialized   (no response)
///   client → server : tools/list
///   server → client : tools/list result
///   client → server : tools/call  { name, arguments }
///   server → client : tools/call result { content: [{type,text}] }
///
/// Important: logging MUST go to stderr; stdout is reserved for MCP.
use crate::{
    market::HyperliquidClient,
    smc::SmcEngine,
    tenzro::TenzroClient,
    types::{AgentConfig, Bias, MarketAnalysis, SignalKind, SmcSignal},
};
use anyhow::Result;
use serde_json::{json, Value};
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct McpServer {
    market: HyperliquidClient,
    smc: SmcEngine,
    tenzro: Option<TenzroClient>,
}

impl McpServer {
    pub fn new(config: &AgentConfig) -> Self {
        let tenzro = if config.tenzro_api_key.is_empty() {
            tracing::warn!("TENZRO_API_KEY not set — AI suggestions disabled");
            None
        } else {
            Some(TenzroClient::new(
                config.tenzro_api_key.clone(),
                config.tenzro_base_url.clone(),
                config.tenzro_model.clone(),
                config.tenzro_endpoint_id.clone(),
                config.tenzro_provider.clone(),
            ))
        };

        Self {
            market: HyperliquidClient::new(config.hyperliquid_url.clone()),
            smc: SmcEngine::new(),
            tenzro,
        }
    }

    /// Main loop: read lines from stdin, write responses to stdout.
    pub async fn run(self) -> Result<()> {
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        let stdout = tokio::io::stdout();
        let mut out = tokio::io::BufWriter::new(stdout);

        tracing::info!("MCP server ready — waiting for messages on stdin");

        while let Some(line) = lines.next_line().await? {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }

            let msg: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!("JSON parse error: {e}");
                    continue;
                }
            };

            if let Some(resp) = self.dispatch(&msg).await {
                let mut payload = serde_json::to_string(&resp)?;
                payload.push('\n');
                out.write_all(payload.as_bytes()).await?;
                out.flush().await?;
            }
        }

        Ok(())
    }

    // ── Dispatcher ────────────────────────────────────────────────────────────

    async fn dispatch(&self, msg: &Value) -> Option<Value> {
        let method = msg["method"].as_str()?;
        let id = msg.get("id").cloned();

        match method {
            "initialize" => Some(self.handle_initialize(id)),
            "notifications/initialized" => None, // notification — no response
            "tools/list" => Some(self.handle_tools_list(id)),
            "tools/call" => {
                let name = msg["params"]["name"].as_str().unwrap_or("").to_string();
                let args = msg["params"]["arguments"].clone();
                let result = self.call_tool(&name, &args).await;
                Some(self.wrap_tool_result(id, result))
            }
            _ => {
                // Only respond if it has an id (requests, not notifications)
                id.map(|id| {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": format!("Method not found: {method}") }
                    })
                })
            }
        }
    }

    // ── Protocol handlers ─────────────────────────────────────────────────────

    fn handle_initialize(&self, id: Option<Value>) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "market-intel-agent",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        })
    }

    fn handle_tools_list(&self, id: Option<Value>) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": TOOLS_SCHEMA.clone() }
        })
    }

    fn wrap_tool_result(&self, id: Option<Value>, result: Result<String, anyhow::Error>) -> Value {
        match result {
            Ok(text) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{ "type": "text", "text": text }],
                    "isError": false
                }
            }),
            Err(e) => {
                tracing::error!("Tool error: {e}");
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": format!("Error: {e}") }],
                        "isError": true
                    }
                })
            }
        }
    }

    // ── Tool router ───────────────────────────────────────────────────────────

    pub async fn call_tool(&self, name: &str, args: &Value) -> Result<String> {
        match name {
            "analyze_pair"       => self.tool_analyze_pair(args).await,
            "get_whale_activity" => self.tool_whale_activity(args).await,
            "get_funding_rate"   => self.tool_funding_rate(args).await,
            "get_market_summary" => self.tool_market_summary(args).await,
            other => Err(anyhow::anyhow!("Unknown tool: {other}")),
        }
    }

    // ── Tool: analyze_pair ────────────────────────────────────────────────────

    async fn tool_analyze_pair(&self, args: &Value) -> Result<String> {
        let pair     = args["pair"].as_str().unwrap_or("BTC").to_uppercase();
        let interval = args["interval"].as_str().unwrap_or("1h");
        let limit    = args["limit"].as_u64().unwrap_or(100) as usize;
        let t_total  = Instant::now();

        tracing::info!(pair = %pair, interval, candles = limit, "analyze_pair started");

        // ── Fetch market data
        let t = Instant::now();
        let candles = self.market.get_candles(&pair, interval, limit).await?;
        let price   = self.market.get_price(&pair).await
            .unwrap_or_else(|_| candles.last().map(|c| c.close).unwrap_or(0.0));
        let funding = self.market.get_funding_rate(&pair).await.ok().flatten();
        tracing::debug!(
            candles_fetched = candles.len(),
            price,
            funding_rate = funding.as_ref().map(|f| f.rate).unwrap_or(0.0),
            elapsed_ms = t.elapsed().as_millis(),
            "hyperliquid fetch done"
        );

        // ── SMC analysis
        let t = Instant::now();
        let signals = self.smc.analyze(&candles);
        let bias    = self.smc.overall_bias(&candles, &signals);
        log_signal_summary(&pair, &signals, t.elapsed().as_millis());

        // ── Tenzro AI
        let ai_suggestion = if let Some(tenzro) = &self.tenzro {
            let ctx = build_ai_context(&pair, interval, price, &bias, &funding, &signals);
            match tenzro.suggest(&ctx).await {
                Ok(s)  => Some(s),
                Err(e) => {
                    tracing::warn!(error = %e, "Tenzro suggestion failed");
                    Some(format!("AI unavailable: {e}"))
                }
            }
        } else {
            None
        };

        tracing::info!(
            pair = %pair,
            bias = %bias,
            price,
            signals = signals.len(),
            ai = ai_suggestion.is_some(),
            total_ms = t_total.elapsed().as_millis(),
            "analyze_pair done"
        );

        let analysis = MarketAnalysis {
            pair,
            interval: interval.to_string(),
            current_price: price,
            overall_bias: bias,
            funding_rate: funding,
            signals,
            ai_suggestion,
            analyzed_at: chrono::Utc::now(),
        };

        Ok(serde_json::to_string_pretty(&analysis)?)
    }

    // ── Tool: get_whale_activity ──────────────────────────────────────────────

    async fn tool_whale_activity(&self, args: &Value) -> Result<String> {
        let pair    = args["pair"].as_str().unwrap_or("BTC").to_uppercase();
        let min_usd = args["min_usd"].as_f64().unwrap_or(100_000.0);
        let t = Instant::now();

        tracing::info!(pair = %pair, min_usd, "get_whale_activity started");

        let whales = self.market.get_whale_trades(&pair, min_usd).await?;

        if whales.is_empty() {
            return Ok(format!(
                "No whale trades (>${:.0}K USD) found in recent trades for {pair}",
                min_usd / 1000.0
            ));
        }

        let buy_vol: f64  = whales.iter().filter(|t| t.side == "B").map(|t| t.value_usd).sum();
        let sell_vol: f64 = whales.iter().filter(|t| t.side == "A").map(|t| t.value_usd).sum();
        let ratio = if sell_vol > 0.0 { buy_vol / sell_vol } else { f64::INFINITY };

        let bias = if buy_vol > sell_vol * 1.3 {
            "Bullish — whales net buying"
        } else if sell_vol > buy_vol * 1.3 {
            "Bearish — whales net selling"
        } else {
            "Neutral — mixed whale activity"
        };

        tracing::info!(
            pair = %pair,
            whale_trades = whales.len(),
            buy_usd  = format!("${:.0}", buy_vol),
            sell_usd = format!("${:.0}", sell_vol),
            bias,
            elapsed_ms = t.elapsed().as_millis(),
            "get_whale_activity done"
        );

        let result = json!({
            "pair": pair,
            "threshold_usd": min_usd,
            "whale_trades_found": whales.len(),
            "total_buy_usd": buy_vol,
            "total_sell_usd": sell_vol,
            "buy_sell_ratio": ratio,
            "bias": bias,
            "top_10_trades": whales.iter().take(10).collect::<Vec<_>>()
        });

        Ok(serde_json::to_string_pretty(&result)?)
    }

    // ── Tool: get_funding_rate ────────────────────────────────────────────────

    async fn tool_funding_rate(&self, args: &Value) -> Result<String> {
        let pair = args["pair"].as_str().unwrap_or("BTC").to_uppercase();

        tracing::info!(pair = %pair, "get_funding_rate started");

        let Some(f) = self.market.get_funding_rate(&pair).await? else {
            return Ok(format!("Funding rate not available for {pair}"));
        };

        let annualized = f.rate * 3.0 * 365.0 * 100.0;
        let sentiment = match f.rate {
            r if r >  0.0005 => "Extremely bullish — overheated, long squeeze risk",
            r if r >  0.0001 => "Bullish — longs dominant, slight bearish pressure from fees",
            r if r < -0.0005 => "Extremely bearish — overheated, short squeeze risk",
            r if r < -0.0001 => "Bearish — shorts dominant, slight bullish pressure from fees",
            _                 => "Neutral — balanced funding",
        };

        let result = json!({
            "pair": pair,
            "funding_rate_8h": f.rate,
            "funding_rate_8h_pct": f.rate * 100.0,
            "premium": f.premium,
            "annualized_pct": annualized,
            "sentiment": sentiment
        });

        Ok(serde_json::to_string_pretty(&result)?)
    }

    // ── Tool: get_market_summary ──────────────────────────────────────────────

    async fn tool_market_summary(&self, args: &Value) -> Result<String> {
        let pairs: Vec<String> = args["pairs"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_uppercase()))
                    .collect()
            })
            .unwrap_or_else(|| vec!["BTC".into(), "ETH".into(), "SOL".into()]);

        tracing::info!("get_market_summary pairs={pairs:?}");

        let mut per_pair = Vec::new();
        let mut narrative_parts = Vec::new();

        for pair in &pairs {
            match self.market.get_candles(pair, "1h", 50).await {
                Ok(candles) if !candles.is_empty() => {
                    let price   = self.market.get_price(pair).await.unwrap_or(0.0);
                    let signals = self.smc.analyze(&candles);
                    let bias    = self.smc.overall_bias(&candles, &signals);
                    let top_sig = signals.first().map(|s| s.description.as_str()).unwrap_or("—");

                    narrative_parts.push(format!(
                        "{pair}: ${price:.2}, {bias} bias, top signal: {top_sig}"
                    ));

                    per_pair.push(json!({
                        "pair": pair,
                        "price": price,
                        "bias": bias.to_string(),
                        "signal_count": signals.len(),
                        "top_signal": top_sig
                    }));
                }
                Err(e) => {
                    per_pair.push(json!({ "pair": pair, "error": e.to_string() }));
                }
                _ => {
                    per_pair.push(json!({ "pair": pair, "error": "no candle data" }));
                }
            }
        }

        let ai_overview = if let Some(tenzro) = &self.tenzro {
            let prompt = format!(
                "Hyperliquid perpetuals market snapshot:\n{}\n\nGive: (1) overall crypto market bias, (2) best opportunity pair with reasoning, (3) key macro risk to watch.",
                narrative_parts.join("\n")
            );
            match tenzro.suggest(&prompt).await {
                Ok(s) => s,
                Err(e) => format!("AI unavailable: {e}"),
            }
        } else {
            "Tenzro AI disabled — set TENZRO_API_KEY to enable.".to_string()
        };

        let result = json!({
            "analyzed_at": chrono::Utc::now().to_rfc3339(),
            "pairs": per_pair,
            "ai_market_overview": ai_overview
        });

        Ok(serde_json::to_string_pretty(&result)?)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn build_ai_context(
    pair: &str,
    interval: &str,
    price: f64,
    bias: &Bias,
    funding: &Option<crate::types::FundingRate>,
    signals: &[SmcSignal],
) -> String {
    let funding_str = funding
        .as_ref()
        .map(|f| format!("{:.4}% / 8h ({})", f.rate * 100.0, if f.rate > 0.0 { "longs pay" } else { "shorts pay" }))
        .unwrap_or_else(|| "N/A".to_string());

    let top_signals: String = signals
        .iter()
        .take(6)
        .map(|s| format!("  • [{} {:?}] {}", s.bias, s.kind, s.description))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "Pair: {pair} | Interval: {interval} | Price: ${price:.4}\n\
         Overall Bias: {bias}\n\
         Funding Rate: {funding_str}\n\n\
         Top SMC Signals:\n{top_signals}\n\n\
         Provide specific entry zone, SL, TP1, TP2, and risk note."
    )
}

fn log_signal_summary(pair: &str, signals: &[SmcSignal], elapsed_ms: u128) {
    let ob   = signals.iter().filter(|s| s.kind == SignalKind::OrderBlock).count();
    let fvg  = signals.iter().filter(|s| s.kind == SignalKind::FairValueGap).count();
    let bos  = signals.iter().filter(|s| s.kind == SignalKind::BreakOfStructure).count();
    let chch = signals.iter().filter(|s| s.kind == SignalKind::ChangeOfCharacter).count();
    let liq  = signals.iter().filter(|s| s.kind == SignalKind::LiquidityZone).count();
    let flow = signals.iter().filter(|s| {
        s.kind == SignalKind::SmartMoneyAccumulation || s.kind == SignalKind::SmartMoneyDistribution
    }).count();

    tracing::info!(
        pair,
        total   = signals.len(),
        ob, fvg, bos, choch = chch, liquidity = liq, smart_flow = flow,
        elapsed_ms,
        "smc analysis done"
    );
}

// ── Static tool schema (MCP tools/list response) ──────────────────────────────

static TOOLS_SCHEMA: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| {
    json!([
        {
            "name": "analyze_pair",
            "description": "Full Smart Money Concept (SMC) analysis for a Hyperliquid perpetual pair. Detects Order Blocks, Fair Value Gaps, Break of Structure, Change of Character, Liquidity Zones, and Smart Money flow. Optionally enriched with Tenzro AI trading suggestion (entry, SL, TP).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pair": {
                        "type": "string",
                        "description": "Coin name as used on Hyperliquid, e.g. 'BTC', 'ETH', 'SOL', 'ARB'"
                    },
                    "interval": {
                        "type": "string",
                        "description": "Candle timeframe: 1m | 3m | 5m | 15m | 30m | 1h | 4h | 1d",
                        "default": "1h"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Number of candles to fetch and analyze (default 100, max 500)",
                        "default": 100
                    }
                },
                "required": ["pair"]
            }
        },
        {
            "name": "get_whale_activity",
            "description": "Scan recent Hyperliquid trades for large positions (whale activity). Returns buy/sell volume breakdown, bias signal, and top trades list. Useful for confirming Smart Money direction.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pair": {
                        "type": "string",
                        "description": "Coin name, e.g. 'BTC', 'ETH'"
                    },
                    "min_usd": {
                        "type": "number",
                        "description": "Minimum notional value (USD) to classify a trade as whale (default: 100000)",
                        "default": 100000
                    }
                },
                "required": ["pair"]
            }
        },
        {
            "name": "get_funding_rate",
            "description": "Get current perpetual funding rate on Hyperliquid for a coin. Funding rate reveals market sentiment: positive = longs paying = overcrowded long = bearish pressure; negative = shorts paying = overcrowded short = bullish pressure.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pair": {
                        "type": "string",
                        "description": "Coin name, e.g. 'BTC', 'ETH'"
                    }
                },
                "required": ["pair"]
            }
        },
        {
            "name": "get_market_summary",
            "description": "Get a multi-pair Hyperliquid market overview with per-pair SMC bias and an AI-powered narrative summary using Tenzro Cloud. Good for macro session planning.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pairs": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of coin names to summarize, e.g. ['BTC', 'ETH', 'SOL']"
                    }
                },
                "required": ["pairs"]
            }
        }
    ])
});
