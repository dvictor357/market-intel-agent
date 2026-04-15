use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};

/// Client for Tenzro Cloud AI inference.
/// Endpoint: POST https://api.cloud.tenzro.com/cloud/ai/infer
/// Docs: https://docs.cloud.tenzro.com/authentication
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
            "provider": self.provider,
            "model":    self.model,
            "prompt":   prompt,
            "temperature": 0.2,
            "max_tokens":  600
        });

        if !self.project_id.is_empty() {
            body["projectId"] = json!(self.project_id);
        }

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

        if !status.is_success() {
            return Err(anyhow!("Tenzro API {status}: {json}"));
        }

        // Extract text — Tenzro actual shape: data.responseText
        let content = json["data"]["responseText"].as_str()
            .or_else(|| json["data"]["responseData"]["text"].as_str())
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
