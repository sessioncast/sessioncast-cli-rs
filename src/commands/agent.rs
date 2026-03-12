use crate::agent::AgentRunner;
use crate::config::AgentConfig;
use anyhow::Result;
use colored::Colorize;

/// Start the SessionCast agent
pub async fn run_agent(config_path: Option<&str>, debug: bool) -> Result<()> {
    if debug {
        tracing::info!("{}", "[DEBUG] Debug mode enabled".yellow());
    }

    let config = AgentConfig::load(config_path).await?;
    let mut runner = AgentRunner::new(config);
    runner.start().await?;

    Ok(())
}
