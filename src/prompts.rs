use anyhow::Result;
use std::path::PathBuf;

/// Load the system prompt from the user's config directory if present,
/// otherwise fall back to the bundled prompt in `prompts/system_prompt.md`.
pub async fn load_system_prompt() -> Result<String> {
    let file_name = "system_prompt.md";

    // Resolve XDG config or fallback to $HOME/.config
    let config_base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(PathBuf::from)
                .map(|p| p.join(".config"))
        });

    if let Some(mut cfg) = config_base {
        cfg.push("market-intel-agent");
        cfg.push("prompts");
        cfg.push(file_name);
        if cfg.exists() {
            match tokio::fs::read_to_string(&cfg).await {
                Ok(s) => return Ok(s),
                Err(e) => {
                    tracing::warn!(path = %cfg.display(), error = %e, "failed reading prompt, using bundled default")
                }
            }
        }
    }

    // Bundled default (compile-time include)
    Ok(include_str!("../prompts/system_prompt.md").to_string())
}
