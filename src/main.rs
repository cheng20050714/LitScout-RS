mod agent;
mod cache;
mod checkpoint;
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
mod run_policy;
mod server;
mod session;
mod sources;
mod trace;
mod workflow;
mod workflow_state;

use clap::Parser;
use tracing::warn;

use crate::cli::Cli;
use crate::config::{AppConfig, LlmConfig};
use crate::error::AppError;

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
    let mut llm_config = LlmConfig::from_cli(&cli);
    if cli.serve {
        llm_config.enabled = true;
    } else if !llm_config.enabled {
        return Err(AppError::InvalidConfig(
            "LitScout-RS 当前主线需要启用 LLM：请添加 --llm 并配置 DEEPSEEK_API_KEY。".to_string(),
        )
        .into());
    }

    if app_config.github_token.is_none() {
        warn!("No GitHub token provided; using unauthenticated GitHub API access.");
    }

    if cli.serve {
        server::serve(app_config, llm_config, cli.port).await?;
        return Ok(());
    }

    let output_path = workflow::run(cli.into_query()?, app_config, llm_config).await?;
    println!("Report written to {}", output_path.display());

    Ok(())
}
