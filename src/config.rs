use std::env;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::cli::Cli;
use crate::error::{AppError, Result};

pub const DEFAULT_LLM_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    pub github_token: Option<String>,
    pub output: PathBuf,
    pub cache_dir: PathBuf,
    pub session_dir: PathBuf,
    pub tags_file: Option<PathBuf>,
    pub use_cache: bool,
    pub cache_ttl_hours: u64,
    pub timeout_secs: u64,
    pub enrich: bool,
}

impl AppConfig {
    pub fn from_cli(cli: &Cli) -> Result<Self> {
        if cli.github_limit == 0 || cli.arxiv_limit == 0 {
            return Err(AppError::InvalidConfig(
                "github-limit and arxiv-limit must be greater than 0".to_string(),
            ));
        }

        Ok(Self {
            github_token: cli.github_token.clone(),
            output: cli.output.clone(),
            cache_dir: cli.cache_dir.clone(),
            session_dir: cli.session_dir.clone(),
            tags_file: cli.tags_file.clone(),
            use_cache: !cli.no_cache,
            cache_ttl_hours: cli.cache_ttl_hours,
            timeout_secs: cli.timeout,
            enrich: cli.enrich,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmConfig {
    pub enabled: bool,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub main_model: String,
    pub side_model: Option<String>,
    pub max_tokens: usize,
    pub timeout_secs: u64,
}

impl LlmConfig {
    pub fn from_cli(cli: &Cli) -> Self {
        if !cli.llm {
            return Self::disabled(llm_timeout_from_cli(cli));
        }

        let api_key = cli
            .deepseek_api_key
            .clone()
            .or_else(|| read_env_first(&["DEEPSEEK_API_KEY", "LLM_API_KEY"]));
        let base_url = cli
            .deepseek_base_url
            .clone()
            .or_else(|| read_env_first(&["DEEPSEEK_BASE_URL", "LLM_BASE_URL"]))
            .unwrap_or_else(|| "https://api.deepseek.com".to_string());
        let main_model = cli
            .deepseek_model
            .clone()
            .or_else(|| read_env_first(&["DEEPSEEK_MODEL", "LLM_MODEL"]))
            .unwrap_or_else(|| "deepseek-v4-pro".to_string());
        let side_model = cli
            .deepseek_side_model
            .clone()
            .or_else(|| read_env_first(&["DEEPSEEK_SIDE_MODEL", "LLM_SIDE_MODEL"]))
            .unwrap_or_else(|| "deepseek-v4-flash".to_string());
        let max_tokens = cli
            .llm_max_tokens
            .or_else(|| read_env_usize(&["DEEPSEEK_MAX_TOKENS", "LLM_MAX_TOKENS"]))
            .unwrap_or(4096);
        let timeout_secs = llm_timeout_from_cli(cli);

        Self {
            enabled: true,
            api_key,
            base_url: Some(base_url),
            main_model,
            side_model: Some(side_model),
            max_tokens,
            timeout_secs,
        }
    }

    #[cfg(test)]
    pub fn from_env(enabled: bool, timeout_secs: u64) -> Self {
        if !enabled {
            return Self::disabled(timeout_secs);
        }

        let api_key = read_env_first(&["DEEPSEEK_API_KEY", "LLM_API_KEY"]);
        let base_url = read_env_first(&["DEEPSEEK_BASE_URL", "LLM_BASE_URL"])
            .unwrap_or_else(|| "https://api.deepseek.com".to_string());
        let main_model = read_env_first(&["DEEPSEEK_MODEL", "LLM_MODEL"])
            .unwrap_or_else(|| "deepseek-v4-pro".to_string());
        let side_model = read_env_first(&["DEEPSEEK_SIDE_MODEL", "LLM_SIDE_MODEL"])
            .or_else(|| Some("deepseek-v4-flash".to_string()));

        Self {
            enabled,
            api_key,
            base_url: Some(base_url),
            main_model,
            side_model,
            max_tokens: 4096,
            timeout_secs,
        }
    }

    fn disabled(timeout_secs: u64) -> Self {
        Self {
            enabled: false,
            api_key: None,
            base_url: Some("https://api.deepseek.com".to_string()),
            main_model: "deepseek-v4-pro".to_string(),
            side_model: Some("deepseek-v4-flash".to_string()),
            max_tokens: 4096,
            timeout_secs,
        }
    }
}

fn read_env_first(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| env::var(key).ok())
        .filter(|value| !value.trim().is_empty())
}

fn read_env_usize(keys: &[&str]) -> Option<usize> {
    read_env_first(keys).and_then(|value| value.parse().ok())
}

fn read_env_u64(keys: &[&str]) -> Option<u64> {
    read_env_first(keys).and_then(|value| value.parse().ok())
}

fn llm_timeout_from_cli(cli: &Cli) -> u64 {
    cli.llm_timeout
        .or_else(|| read_env_u64(&["DEEPSEEK_TIMEOUT_SECS", "LLM_TIMEOUT_SECS"]))
        .unwrap_or(DEFAULT_LLM_TIMEOUT_SECS)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{AppConfig, LlmConfig, DEFAULT_LLM_TIMEOUT_SECS};
    use crate::cli::Cli;

    #[test]
    fn rejects_zero_limits() {
        let cli = Cli::try_parse_from(["litscout-rs", "rust agent", "--github-limit", "0"])
            .expect("CLI parsing should accept numeric zero");

        let err = AppConfig::from_cli(&cli).expect_err("zero limits must be rejected");
        assert!(err.to_string().contains("must be greater than 0"));
    }

    #[test]
    fn llm_config_disabled_skips_env() {
        let config = LlmConfig::from_env(false, 30);

        assert!(!config.enabled);
        assert!(config.api_key.is_none());
        assert_eq!(config.main_model, "deepseek-v4-pro");
        assert_eq!(config.side_model.as_deref(), Some("deepseek-v4-flash"));
    }

    #[test]
    fn serve_mode_disabled_llm_config_keeps_llm_timeout_default() {
        let cli =
            Cli::try_parse_from(["litscout-rs", "--serve"]).expect("CLI should parse serve mode");

        let config = LlmConfig::from_cli(&cli);

        assert!(!config.enabled);
        assert_eq!(config.timeout_secs, DEFAULT_LLM_TIMEOUT_SECS);
    }

    #[test]
    fn llm_config_uses_cli_values_before_defaults() {
        let cli = Cli::try_parse_from([
            "litscout-rs",
            "rust agent",
            "--llm",
            "--deepseek-api-key",
            "sk-test",
            "--deepseek-base-url",
            "https://example.test",
            "--deepseek-model",
            "deepseek-v4-pro",
            "--deepseek-side-model",
            "deepseek-v4-flash",
            "--llm-timeout",
            "45",
            "--llm-max-tokens",
            "2048",
        ])
        .expect("CLI should parse DeepSeek options");

        let config = LlmConfig::from_cli(&cli);

        assert!(config.enabled);
        assert_eq!(config.api_key.as_deref(), Some("sk-test"));
        assert_eq!(config.base_url.as_deref(), Some("https://example.test"));
        assert_eq!(config.main_model, "deepseek-v4-pro");
        assert_eq!(config.side_model.as_deref(), Some("deepseek-v4-flash"));
        assert_eq!(config.timeout_secs, 45);
        assert_eq!(config.max_tokens, 2048);
    }
}
