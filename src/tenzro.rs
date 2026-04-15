use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Instant;

/// Client for Tenzro Cloud AI inference.
/// Endpoint: POST https://api.cloud.tenzro.com/cloud/ai/infer
pub struct TenzroClient {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    project_id: String,
    provider: String,
}

impl TenzroClient {
    pub fn new(
        api_key: String,
        base_url: String,
        model: String,
        project_id: String,
        provider: String,
    ) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(90))
                .build()
                .expect("failed to build reqwest client"),
            api_key,
            base_url,
            model,
            project_id,
            provider,
        }
    }

    pub async fn suggest(&self, market_context: &str) -> Result<String> {
        let prompt = format!("{SYSTEM_PROMPT}\n\n{market_context}");

        let mut body = json!({
            "provider":    self.provider,
            "model":       self.model,
            "prompt":      prompt,
            "temperature": 0.2,
            "max_tokens":  600
        });

        if !self.project_id.is_empty() {
            body["projectId"] = json!(self.project_id);
        }

        tracing::debug!(
            model    = %self.model,
            provider = %self.provider,
            prompt_chars = prompt.len(),
            "sending inference request"
        );

        let t = Instant::now();

        let resp = self
            .client
            .post(format!("{}/infer", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let json: Value = resp.json().await?;
        let round_trip_ms = t.elapsed().as_millis();

        if !status.is_success() {
            tracing::error!(status = %status, body = %json, "Tenzro API error");
            return Err(anyhow!("Tenzro API {status}: {json}"));
        }

        // Log usage metadata from Tenzro response
        let data = &json["data"];
        let input_tokens  = data["inputTokens"].as_u64().unwrap_or(0);
        let output_tokens = data["outputTokens"].as_u64().unwrap_or(0);
        let cost_micro    = data["estimatedCostMicrodollars"].as_u64().unwrap_or(0);
        let tenzro_ms     = data["latencyMs"].as_u64().unwrap_or(0);
        let inference_id  = data["inferenceId"].as_str().unwrap_or("-");

        tracing::info!(
            inference_id,
            model            = %self.model,
            input_tokens,
            output_tokens,
            tenzro_latency_ms = tenzro_ms,
            round_trip_ms,
            cost_usd         = format!("${:.6}", cost_micro as f64 / 1_000_000.0),
            "tenzro inference complete"
        );

        // Extract text — Tenzro shape: data.responseText
        let content = data["responseText"].as_str()
            .or_else(|| data["responseData"]["text"].as_str())
            .or_else(|| json["result"].as_str())
            .or_else(|| json["text"].as_str())
            .or_else(|| json["message"]["content"].as_str())
            .or_else(|| json["choices"][0]["message"]["content"].as_str())
            .ok_or_else(|| anyhow!("unexpected Tenzro response shape: {json}"))?;

        Ok(content.to_string())
    }
}

const SYSTEM_PROMPT: &str = "\
You are an expert cryptocurrency derivatives trader specializing in Smart Money Concepts (SMC) \
and institutional order flow analysis on Hyperliquid perpetuals. \
Given market data, produce a concise, actionable trading brief with:
1. Directional bias (Bullish / Bearish / Neutral)
2. Suggested entry zone with reason
3. Stop-loss level (below OB / above OB / swing point)
4. Take-profit targets (TP1 near FVG fill, TP2 at next liquidity)
5. Key risk factors to watch
Be specific about price levels. Do not hedge excessively. Think like an institution.";
