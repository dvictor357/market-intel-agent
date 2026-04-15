use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub hyperliquid_url: String,
    pub tenzro_api_key: String,
    pub tenzro_base_url: String,
    pub tenzro_model: String,
    /// Tenzro inference endpoint UUID (from dashboard → Inference tab)
    pub tenzro_endpoint_id: String,
    /// AI provider: "anthropic" | "google" | "openai"
    pub tenzro_provider: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            hyperliquid_url: "https://api.hyperliquid.xyz/info".to_string(),
            tenzro_api_key: std::env::var("TENZRO_API_KEY").unwrap_or_default(),
            // Correct endpoint from Tenzro docs
            tenzro_base_url: "https://api.cloud.tenzro.com/cloud/ai".to_string(),
            tenzro_model: std::env::var("TENZRO_MODEL")
                .unwrap_or_else(|_| "claude-sonnet-4-6".to_string()),
            tenzro_endpoint_id: std::env::var("TENZRO_ENDPOINT_ID").unwrap_or_default(),
            tenzro_provider: std::env::var("TENZRO_PROVIDER")
                .unwrap_or_else(|_| "anthropic".to_string()),
        }
    }
}

// ── Market data primitives ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub open_time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhaleTrade {
    pub pair: String,
    /// "B" = buy (aggressive), "A" = sell (aggressive)
    pub side: String,
    pub size: f64,
    pub price: f64,
    pub value_usd: f64,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingRate {
    pub pair: String,
    /// Per-8h rate as a decimal (e.g. 0.0001 = 0.01%)
    pub rate: f64,
    pub premium: f64,
    pub timestamp_ms: i64,
}

// ── SMC signal types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    OrderBlock,
    FairValueGap,
    BreakOfStructure,
    ChangeOfCharacter,
    LiquidityZone,
    SmartMoneyAccumulation,
    SmartMoneyDistribution,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Bias {
    Bullish,
    Bearish,
    Neutral,
}

impl std::fmt::Display for Bias {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Bias::Bullish => write!(f, "Bullish"),
            Bias::Bearish => write!(f, "Bearish"),
            Bias::Neutral => write!(f, "Neutral"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmcSignal {
    pub kind: SignalKind,
    pub bias: Bias,
    /// Key price level (midpoint of OB, FVG zone, BOS level, etc.)
    pub price_level: f64,
    /// 0.0–1.0 confidence score
    pub strength: f64,
    pub description: String,
    pub detected_at: DateTime<Utc>,
}

// ── Aggregated analysis result ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketAnalysis {
    pub pair: String,
    pub interval: String,
    pub current_price: f64,
    pub overall_bias: Bias,
    pub funding_rate: Option<FundingRate>,
    pub signals: Vec<SmcSignal>,
    pub ai_suggestion: Option<String>,
    pub analyzed_at: DateTime<Utc>,
}
