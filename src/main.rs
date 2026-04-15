mod http_server;
mod market;
mod mcp_server;
mod smc;
mod tenzro;
mod types;

use types::AgentConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    // Logs go to stderr — stdout is reserved for MCP JSON-RPC
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "market_intel_agent=info".to_string()),
        )
        .init();

    let config = AgentConfig::default();
    log_startup(&config);

    let args: Vec<String> = std::env::args().collect();
    let http_port = args.windows(2).find_map(|w| {
        if w[0] == "--http" { w[1].parse::<u16>().ok() } else { None }
    });

    if let Some(port) = http_port {
        tracing::info!(mode = "http", port, "server starting");
        http_server::run(port).await
    } else {
        tracing::info!(mode = "mcp_stdio", "server starting");
        let server = mcp_server::McpServer::new(&config);
        server.run().await
    }
}

fn log_startup(cfg: &AgentConfig) {
    let masked_key = if cfg.tenzro_api_key.len() > 8 {
        format!("{}***{}", &cfg.tenzro_api_key[..6], &cfg.tenzro_api_key[cfg.tenzro_api_key.len()-4..])
    } else if cfg.tenzro_api_key.is_empty() {
        "(not set — AI suggestions disabled)".to_string()
    } else {
        "***".to_string()
    };

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        model   = %cfg.tenzro_model,
        provider = %cfg.tenzro_provider,
        api_key = %masked_key,
        "market-intel-agent"
    );
}
