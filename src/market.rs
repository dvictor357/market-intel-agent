use crate::types::{Candle, FundingRate, WhaleTrade};
use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};

pub struct HyperliquidClient {
    client: Client,
    url: String,
}

impl HyperliquidClient {
    pub fn new(url: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("failed to build reqwest client"),
            url,
        }
    }

    async fn post(&self, body: Value) -> Result<Value> {
        let resp = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Hyperliquid HTTP {}", resp.status()));
        }

        Ok(resp.json().await?)
    }

    // ── Candles ───────────────────────────────────────────────────────────────

    pub async fn get_candles(&self, coin: &str, interval: &str, limit: usize) -> Result<Vec<Candle>> {
        let end_ms = chrono::Utc::now().timestamp_millis();
        let start_ms = end_ms - (limit as i64 * interval_to_ms(interval));

        let resp = self.post(json!({
            "type": "candleSnapshot",
            "req": {
                "coin": coin,
                "interval": interval,
                "startTime": start_ms,
                "endTime": end_ms
            }
        })).await?;

        let arr = resp.as_array().ok_or_else(|| anyhow!("expected array from candleSnapshot"))?;
        let candles = arr.iter().filter_map(parse_candle).collect();
        Ok(candles)
    }

    // ── Current mid price ─────────────────────────────────────────────────────

    pub async fn get_price(&self, coin: &str) -> Result<f64> {
        let resp = self.post(json!({"type": "allMids"})).await?;
        resp[coin]
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow!("price not found for {coin}"))
    }

    // ── Recent trades → whale filter ──────────────────────────────────────────

    pub async fn get_whale_trades(&self, coin: &str, min_usd: f64) -> Result<Vec<WhaleTrade>> {
        let resp = self.post(json!({
            "type": "recentTrades",
            "coin": coin
        })).await?;

        let arr = resp.as_array().ok_or_else(|| anyhow!("expected array from recentTrades"))?;

        let whales = arr
            .iter()
            .filter_map(|t| {
                let px: f64 = t["px"].as_str()?.parse().ok()?;
                let sz: f64 = t["sz"].as_str()?.parse().ok()?;
                let value = px * sz;
                if value < min_usd {
                    return None;
                }
                Some(WhaleTrade {
                    pair: coin.to_string(),
                    side: t["side"].as_str().unwrap_or("?").to_string(),
                    size: sz,
                    price: px,
                    value_usd: value,
                    timestamp_ms: t["time"].as_i64().unwrap_or(0),
                })
            })
            .collect();

        Ok(whales)
    }

    // ── Funding rate ──────────────────────────────────────────────────────────

    pub async fn get_funding_rate(&self, coin: &str) -> Result<Option<FundingRate>> {
        // metaAndAssetCtxs returns [meta, [ctx_per_asset...]]
        let resp = self.post(json!({"type": "metaAndAssetCtxs"})).await?;

        let universe = resp[0]["universe"]
            .as_array()
            .ok_or_else(|| anyhow!("no universe in meta"))?;

        let asset_ctxs = resp[1]
            .as_array()
            .ok_or_else(|| anyhow!("no asset ctxs"))?;

        for (i, asset) in universe.iter().enumerate() {
            if asset["name"].as_str() == Some(coin) {
                let ctx = &asset_ctxs[i];
                let rate: f64 = ctx["funding"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                let premium: f64 = ctx["premium"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);

                return Ok(Some(FundingRate {
                    pair: coin.to_string(),
                    rate,
                    premium,
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                }));
            }
        }

        Ok(None)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_candle(v: &Value) -> Option<Candle> {
    // Hyperliquid returns OHLCV as strings
    Some(Candle {
        open_time: v["t"].as_i64()?,
        open: parse_f64(&v["o"])?,
        high: parse_f64(&v["h"])?,
        low: parse_f64(&v["l"])?,
        close: parse_f64(&v["c"])?,
        volume: parse_f64(&v["v"])?,
    })
}

fn parse_f64(v: &Value) -> Option<f64> {
    // Accept both string and number representations
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

fn interval_to_ms(interval: &str) -> i64 {
    match interval {
        "1m"  => 60_000,
        "3m"  => 180_000,
        "5m"  => 300_000,
        "15m" => 900_000,
        "30m" => 1_800_000,
        "1h"  => 3_600_000,
        "2h"  => 7_200_000,
        "4h"  => 14_400_000,
        "8h"  => 28_800_000,
        "12h" => 43_200_000,
        "1d"  => 86_400_000,
        _     => 3_600_000,
    }
}
