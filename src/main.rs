mod http_server;
mod market;
mod mcp_server;
mod smc;
mod tenzro;
mod types;

use types::AgentConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env if present (silently skip if missing)
    let _ = dotenvy::dotenv();

    // Logs go to stderr — stdout is reserved for MCP JSON-RPC
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "market_intel_agent=info".to_string()),
        )
        .init();

    // Arg parsing: `--http PORT` starts HTTP test server, otherwise MCP stdio
    let args: Vec<String> = std::env::args().collect();
    let http_port = args.windows(2).find_map(|w| {
        if w[0] == "--http" { w[1].parse::<u16>().ok() } else { None }
    });

    let config = AgentConfig::default();

    if let Some(port) = http_port {
        tracing::info!("Starting HTTP test server on port {port}");
        http_server::run(port).await
    } else {
        tracing::info!(
            "market-intel-agent v{} starting as MCP stdio server",
            env!("CARGO_PKG_VERSION")
        );
        let server = mcp_server::McpServer::new(&config);
        server.run().await
    }
}
