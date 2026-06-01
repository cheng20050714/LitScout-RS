use std::sync::Arc;

use crate::config::{AppConfig, LlmConfig};

#[derive(Clone)]
pub struct AppState {
    pub app_config: Arc<AppConfig>,
    pub llm_config: Arc<LlmConfig>,
}

impl AppState {
    pub fn new(app_config: AppConfig, llm_config: LlmConfig) -> Self {
        Self {
            app_config: Arc::new(app_config),
            llm_config: Arc::new(llm_config),
        }
    }
}
