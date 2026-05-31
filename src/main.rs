mod cache;
mod classify;
mod cli;
mod config;
mod dedup;
mod error;
mod llm;
mod model;
mod quality;
mod ranking;
mod report;
mod session;
mod sources;
mod workflow;

use clap::Parser;
use tracing::warn;

use crate::cli::Cli;
use crate::config::{AppConfig, LlmConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "litscout_rs=info,warn".into()),
        )
        .init();

    let cli = Cli::parse();
    let app_config = AppConfig::from_cli(&cli)?;
    let llm_config = LlmConfig::from_cli(&cli);

    if app_config.github_token.is_none() {
        warn!("No GitHub token provided; using unauthenticated GitHub API access.");
    }

    let output_path = workflow::run(cli.into_query(), app_config, llm_config).await?;
    println!("Report written to {}", output_path.display());

    Ok(())
}
