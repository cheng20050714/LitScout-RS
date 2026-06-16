mod agent;
mod checkpoint;
mod classify;
mod cli;
mod config;
mod dedup;
mod error;
mod llm;
mod model;
mod ranking;
mod reading;
mod run_policy;
mod server;
mod sources;
mod trace;
mod workflow_state;

use clap::Parser;
use tracing::warn;

use crate::agent::orchestrator;
use crate::cli::Cli;
use crate::config::{AppConfig, LlmConfig};
use crate::error::AppError;
use crate::run_policy::RunPolicy;

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

    let academic_extra_enabled = cli.academic_extra;
    let academic_budget = cli.academic_limit;
    let query = cli.into_query()?;
    let run = orchestrator::create_run(
        query.topic,
        app_config.clone(),
        llm_config.clone(),
        RunPolicy {
            github_budget: query.github_limit,
            arxiv_budget: query.arxiv_limit,
            academic_extra_enabled,
            academic_budget,
            allow_github_enrich: true,
            ..RunPolicy::default()
        },
    )
    .await?;
    let run = orchestrator::continue_run(&run.run_id, app_config, llm_config, None).await?;
    if let Some(output_path) = run.output_report {
        println!("Report written to {output_path}");
    } else {
        println!("Report generated for run {}", run.run_id);
    }

    Ok(())
}
