/// Thin HTTP wrapper around the same tool logic — for testing via HTTPie/curl.
/// Start with:  ./market-intel-agent --http 8080
///
/// Endpoints:
///   POST /analyze   {"pair":"BTC","interval":"1h","limit":50}
///   POST /whale     {"pair":"BTC","min_usd":100000}
///   POST /funding   {"pair":"BTC"}
///   POST /summary   {"pairs":["BTC","ETH","SOL"]}
use crate::{mcp_server::McpServer, types::AgentConfig};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::post,
    Router,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub async fn run(port: u16) -> anyhow::Result<()> {
    let config = AgentConfig::default();
    let server = Arc::new(McpServer::new(&config));

    let app = Router::new()
        .route("/analyze", post(handle_analyze))
        .route("/whale", post(handle_whale))
        .route("/funding", post(handle_funding))
        .route("/summary", post(handle_summary))
        .with_state(server);

    let addr = format!("0.0.0.0:{port}");
    tracing::info!("HTTP test server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn handle_analyze(State(srv): State<Arc<McpServer>>, Json(body): Json<Value>) -> impl IntoResponse {
    run_tool(srv, "analyze_pair", body).await
}

async fn handle_whale(State(srv): State<Arc<McpServer>>, Json(body): Json<Value>) -> impl IntoResponse {
    run_tool(srv, "get_whale_activity", body).await
}

async fn handle_funding(State(srv): State<Arc<McpServer>>, Json(body): Json<Value>) -> impl IntoResponse {
    run_tool(srv, "get_funding_rate", body).await
}

async fn handle_summary(State(srv): State<Arc<McpServer>>, Json(body): Json<Value>) -> impl IntoResponse {
    run_tool(srv, "get_market_summary", body).await
}

async fn run_tool(srv: Arc<McpServer>, name: &str, args: Value) -> (StatusCode, Json<Value>) {
    match srv.call_tool(name, &args).await {
        Ok(text) => {
            let value: Value = serde_json::from_str(&text).unwrap_or(json!({"result": text}));
            (StatusCode::OK, Json(value))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}
