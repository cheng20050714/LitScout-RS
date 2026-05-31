use std::path::PathBuf;

use clap::Parser;

use crate::model::SearchQuery;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "litscout-rs",
    version,
    about = "Scout GitHub repositories and arXiv papers for a research topic"
)]
pub struct Cli {
    #[arg(help = "Research topic to scout")]
    pub topic: String,

    #[arg(long, default_value_t = 10, help = "Maximum GitHub repositories")]
    pub github_limit: usize,

    #[arg(long, default_value_t = 10, help = "Maximum arXiv papers")]
    pub arxiv_limit: usize,

    #[arg(
        long,
        value_name = "PATH",
        default_value = "reports",
        help = "Output report file or directory"
    )]
    pub output: PathBuf,

    #[arg(
        long,
        value_name = "DIR",
        default_value = ".litscout-cache",
        help = "Cache directory"
    )]
    pub cache_dir: PathBuf,

    #[arg(long, help = "Disable local cache")]
    pub no_cache: bool,

    #[arg(
        long,
        value_name = "DIR",
        default_value = "sessions",
        help = "Session JSON directory"
    )]
    pub session_dir: PathBuf,

    #[arg(
        long,
        value_name = "PATH",
        help = "Optional TOML classification tag dictionary"
    )]
    pub tags_file: Option<PathBuf>,

    #[arg(
        long,
        default_value_t = 24,
        value_name = "HOURS",
        help = "Cache TTL in hours"
    )]
    pub cache_ttl_hours: u64,

    #[arg(long, default_value_t = 30, help = "HTTP timeout in seconds")]
    pub timeout: u64,

    #[arg(long, help = "Enable optional LLM-assisted synthesis")]
    pub llm: bool,

    #[arg(
        long,
        value_name = "KEY",
        hide_env_values = true,
        help = "DeepSeek API key. Prefer DEEPSEEK_API_KEY for normal use"
    )]
    pub deepseek_api_key: Option<String>,

    #[arg(
        long,
        value_name = "URL",
        help = "DeepSeek OpenAI-compatible API base URL"
    )]
    pub deepseek_base_url: Option<String>,

    #[arg(long, value_name = "MODEL", help = "DeepSeek main analysis model")]
    pub deepseek_model: Option<String>,

    #[arg(
        long,
        value_name = "MODEL",
        help = "DeepSeek side model reserved for lightweight LLM tasks"
    )]
    pub deepseek_side_model: Option<String>,

    #[arg(long, value_name = "SECONDS", help = "LLM request timeout in seconds")]
    pub llm_timeout: Option<u64>,

    #[arg(long, value_name = "TOKENS", help = "Maximum LLM output tokens")]
    pub llm_max_tokens: Option<usize>,

    #[arg(long, help = "Enable optional GitHub enrichment in later stages")]
    pub enrich: bool,

    #[arg(long, env = "GITHUB_TOKEN", hide_env_values = true)]
    pub github_token: Option<String>,
}

impl Cli {
    pub fn into_query(self) -> SearchQuery {
        SearchQuery {
            topic: self.topic,
            github_limit: self.github_limit,
            arxiv_limit: self.arxiv_limit,
        }
    }
}
