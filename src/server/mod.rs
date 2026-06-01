pub mod dto;
pub mod routes;
pub mod state;

use crate::config::{AppConfig, LlmConfig};
use crate::error::Result;

pub async fn serve(app_config: AppConfig, llm_config: LlmConfig, port: u16) -> Result<()> {
    let state = state::AppState::new(app_config, llm_config);
    let router = routes::build_router(state);
    let addr = format!("0.0.0.0:{port}");
    println!("LitScout-RS workbench: http://127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}
